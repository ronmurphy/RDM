use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::Deserialize;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);
static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize)]
struct SessionConfig {
    #[serde(default)]
    autostart: Vec<AutostartEntry>,
}

#[derive(Debug, Deserialize, Clone)]
struct AutostartEntry {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    restart: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            autostart: vec![
                AutostartEntry {
                    name: "rdm-watermark".into(),
                    command: "rdm-watermark".into(),
                    args: vec![],
                    restart: true,
                },
                AutostartEntry {
                    name: "rdm-panel".into(),
                    command: "rdm-panel".into(),
                    args: vec![],
                    restart: true,
                },
                AutostartEntry {
                    name: "rdm-notify".into(),
                    command: "rdm-notify".into(),
                    args: vec![],
                    restart: true,
                },
                AutostartEntry {
                    name: "rdm-dock".into(),
                    command: "rdm-dock".into(),
                    args: vec![],
                    restart: true,
                },
                AutostartEntry {
                    name: "swaybg".into(),
                    command: "swaybg".into(),
                    args: vec!["-c".into(), "#1a1b26".into()],
                    restart: false,
                },
            ],
        }
    }
}

struct ManagedProcess {
    entry: AutostartEntry,
    child: Option<Child>,
    /// When this process was last spawned (for fast-fail detection).
    last_start: Option<std::time::Instant>,
    /// How many consecutive fast-fails have occurred.
    consecutive_fast_fails: u32,
    /// Do not restart before this instant (backoff hold).
    restart_hold_until: Option<std::time::Instant>,
}

/// A process is a "fast fail" if it exits within this many seconds of starting.
const FAST_FAIL_SECS: u64 = 5;
/// Maximum backoff delay in seconds (caps exponential growth).
const MAX_BACKOFF_SECS: u64 = 60;

#[tokio::main]
async fn main() {
    env_logger::init();
    log::info!("Starting RDM Session Manager (pid={})", std::process::id());

    // Only run inside the RDM desktop environment
    if std::env::var("RDM_SESSION").as_deref() != Ok("1") {
        log::warn!("RDM_SESSION is not set — not running inside rdm-start. Exiting.");
        eprintln!("rdm-session: not inside RDM desktop (RDM_SESSION!=1), exiting.");
        std::process::exit(0);
    }

    // Single-instance guard: exit if another rdm-session is already running
    // under the same WAYLAND_DISPLAY (allows different compositor instances)
    if let Some(existing_pid) = check_existing_instance() {
        log::warn!(
            "Another rdm-session is already running (pid={}). Exiting.",
            existing_pid
        );
        eprintln!("rdm-session already running (pid={}), exiting.", existing_pid);
        std::process::exit(0);
    }

    // Install SIGUSR1 handler for hot reload
    install_signal_handler();

    // Write our PID so rdm-reload can find us
    write_pid_file();

    // Apply display configuration from rdm.toml BEFORE starting panel/swaybg
    apply_display_settings();

    let config = load_session_config();
    let mut processes = start_all(&config);

    // Track display apply timing for stabilization & crash detection
    let session_start = std::time::Instant::now();
    let mut last_display_apply = std::time::Instant::now();
    let mut stabilization_applied = false;

    // Monitor loop
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check for shutdown request (SIGTERM)
        if SHUTDOWN_REQUESTED.load(Ordering::SeqCst) {
            log::info!("SIGTERM received — shutting down session manager");
            stop_all(&mut processes);
            cleanup_pid_file();
            // Tell labwc to exit, completing the logout
            log::info!("Requesting labwc exit...");
            let _ = Command::new("labwc").arg("--exit").status();
            break;
        }

        // Compositor health check: if labwc is no longer running, exit gracefully.
        // This catches logout, compositor crashes, and any other scenario where
        // the Wayland session is gone but rdm-session is still alive.
        if !is_compositor_alive() {
            log::info!("Compositor (labwc) is no longer running — cleaning up and exiting");
            stop_all(&mut processes);
            cleanup_pid_file();
            break;
        }

        // Check for hot reload request
        if RELOAD_REQUESTED.swap(false, Ordering::SeqCst) {
            log::info!("=== HOT RELOAD REQUESTED ===");
            log::info!("Stopping all managed processes...");
            stop_all(&mut processes);

            // Small delay to let processes fully exit and release layer-shell surfaces
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

            // Re-apply display config (may have changed via rdm-settings)
            apply_display_settings();
            last_display_apply = std::time::Instant::now();
            stabilization_applied = true; // no need for stabilization after hot reload

            // Small delay for display changes to settle before starting panel
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Reload config in case it changed
            let new_config = load_session_config();
            log::info!("Restarting all processes with fresh binaries...");
            processes = start_all(&new_config);
            log::info!("=== HOT RELOAD COMPLETE ===");
            continue;
        }

        // Stabilization re-apply: ~20 seconds after startup, re-apply display config
        // to catch labwc output resets that happen shortly after initialization
        if !stabilization_applied && session_start.elapsed().as_secs() >= 20 {
            stabilization_applied = true;
            log::info!("Stabilization re-apply: re-applying display config 20s after startup");
            apply_display_settings();
            last_display_apply = std::time::Instant::now();
        }

        // Normal monitoring — restart crashed processes (with backoff)
        let now = std::time::Instant::now();
        let mut crashes_this_tick: u32 = 0;
        for proc in processes.iter_mut() {
            // If we're in a backoff hold, check whether we can restart yet.
            if proc.child.is_none() {
                if let Some(hold_until) = proc.restart_hold_until {
                    if now >= hold_until {
                        proc.restart_hold_until = None;
                        if proc.entry.restart {
                            log::info!("Restarting {} (backoff elapsed)", proc.entry.name);
                            proc.child = spawn_process(&proc.entry);
                            proc.last_start = Some(std::time::Instant::now());
                        }
                    }
                }
                continue;
            }

            if let Some(ref mut child) = proc.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        let uptime_secs = proc
                            .last_start
                            .map(|t| t.elapsed().as_secs())
                            .unwrap_or(u64::MAX);
                        log::warn!(
                            "{} exited with status: {} (uptime: {}s)",
                            proc.entry.name,
                            status,
                            uptime_secs
                        );
                        proc.child = None;
                        crashes_this_tick += 1;

                        if !proc.entry.restart {
                            continue;
                        }

                        if uptime_secs < FAST_FAIL_SECS {
                            proc.consecutive_fast_fails += 1;
                            let backoff_secs = std::cmp::min(
                                2u64.saturating_pow(proc.consecutive_fast_fails),
                                MAX_BACKOFF_SECS,
                            );
                            log::warn!(
                                "{} fast-failed {} time(s); backing off {}s before restart",
                                proc.entry.name,
                                proc.consecutive_fast_fails,
                                backoff_secs,
                            );
                            proc.restart_hold_until = Some(
                                std::time::Instant::now()
                                    + std::time::Duration::from_secs(backoff_secs),
                            );
                        } else {
                            // Healthy run — reset the fast-fail counter and restart immediately.
                            proc.consecutive_fast_fails = 0;
                            log::info!("Restarting: {}", proc.entry.name);
                            proc.child = spawn_process(&proc.entry);
                            proc.last_start = Some(std::time::Instant::now());
                        }
                    }
                    Ok(None) => {} // Still running
                    Err(e) => {
                        log::error!("Error checking {}: {}", proc.entry.name, e);
                        proc.child = None;
                        crashes_this_tick += 1;
                    }
                }
            }
        }

        // Mass-crash detection: if multiple processes died at once, the compositor
        // likely reset its outputs. Re-apply display config before restarts.
        if crashes_this_tick >= 2 && last_display_apply.elapsed().as_secs() >= 10 {
            log::info!(
                "Detected {} simultaneous crashes — compositor likely reset outputs. Re-applying display config.",
                crashes_this_tick
            );
            apply_display_settings();
            last_display_apply = std::time::Instant::now();
        }
    }
}

fn install_signal_handler() {
    // Use simple signal-safe atomic flags
    unsafe {
        signal::sigaction(
            Signal::SIGUSR1,
            &signal::SigAction::new(
                signal::SigHandler::Handler(handle_sigusr1),
                signal::SaFlags::SA_RESTART,
                signal::SigSet::empty(),
            ),
        )
        .expect("Failed to install SIGUSR1 handler");

        signal::sigaction(
            Signal::SIGTERM,
            &signal::SigAction::new(
                signal::SigHandler::Handler(handle_sigterm),
                signal::SaFlags::SA_RESTART,
                signal::SigSet::empty(),
            ),
        )
        .expect("Failed to install SIGTERM handler");
    }
    log::info!("Signal handlers installed (SIGUSR1=reload, SIGTERM=shutdown)");
}

extern "C" fn handle_sigusr1(_: libc::c_int) {
    RELOAD_REQUESTED.store(true, Ordering::SeqCst);
}

extern "C" fn handle_sigterm(_: libc::c_int) {
    SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
}

/// Check if the compositor (labwc) is still running.
/// If it's dead, there's no Wayland session to manage — rdm-session should exit.
fn is_compositor_alive() -> bool {
    Command::new("pgrep")
        .args(["-x", "labwc"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn start_all(config: &SessionConfig) -> Vec<ManagedProcess> {
    let mut processes = Vec::new();
    for entry in &config.autostart {
        log::info!("Starting: {} ({})", entry.name, entry.command);
        let child = spawn_process(entry);
        processes.push(ManagedProcess {
            entry: entry.clone(),
            child,
            last_start: Some(std::time::Instant::now()),
            consecutive_fast_fails: 0,
            restart_hold_until: None,
        });
    }
    processes
}

fn stop_all(processes: &mut Vec<ManagedProcess>) {
    for proc in processes.iter_mut() {
        if let Some(ref mut child) = proc.child {
            let pid = child.id();
            log::info!("Stopping {} (pid={})", proc.entry.name, pid);
            // Send SIGTERM first
            let _ = signal::kill(Pid::from_raw(pid as i32), Signal::SIGTERM);
        }
    }

    // Give processes time to exit gracefully
    std::thread::sleep(std::time::Duration::from_millis(300));

    // Force-kill any that didn't exit
    for proc in processes.iter_mut() {
        if let Some(ref mut child) = proc.child {
            match child.try_wait() {
                Ok(Some(_)) => {} // Already exited
                _ => {
                    log::warn!("Force-killing {}", proc.entry.name);
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        proc.child = None;
    }
    processes.clear();
}

fn spawn_process(entry: &AutostartEntry) -> Option<Child> {
    // For swaybg/swayidle, build args from rdm.toml config instead of session.toml args
    let args: Vec<String> = if entry.command == "swaybg" {
        build_swaybg_args()
    } else if entry.command == "swayidle" {
        let idle_args = build_swayidle_args();
        if idle_args.is_empty() {
            log::info!("Idle disabled, skipping swayidle launch");
            return None;
        }
        idle_args
    } else {
        entry.args.clone()
    };

    Command::new(&entry.command)
        .args(&args)
        // Keep a stable RDM marker on managed children even if the session manager
        // itself was started before newer env exports were introduced.
        .env("RDM_SESSION", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| log::error!("Failed to start {}: {}", entry.name, e))
        .ok()
}

/// Build swayidle arguments from the idle config in rdm.toml
///
/// swayidle syntax:
///   swayidle -w timeout <secs> '<cmd>' resume '<cmd>' [timeout <secs> '<cmd>' ...]
///
/// We use `wlopm` for DPMS control if available, otherwise fall back to
/// `swaymsg output '*' dpms off/on` (sway-only).  labwc ships with wlopm
/// support, so that's the primary path.
fn build_swayidle_args() -> Vec<String> {
    let config = rdm_common::config::RdmConfig::load();
    let idle = &config.idle;
    let mut args = Vec::new();

    if !idle.enabled {
        log::info!("Idle management disabled in config");
        return args;
    }

    // -w flag: also react to logind idle hints (lock screen, lid switch, etc.)
    args.push("-w".to_string());

    // Screen off: DPMS off after screen_off_secs, resume on input
    if idle.screen_off_secs > 0 {
        args.push("timeout".to_string());
        args.push(idle.screen_off_secs.to_string());
        args.push("wlopm --off '*'".to_string());
        args.push("resume".to_string());
        args.push("wlopm --on '*'".to_string());
    }

    log::info!("swayidle args: {:?}", args);
    args
}

/// Build swaybg arguments from the wallpaper config in rdm.toml
fn build_swaybg_args() -> Vec<String> {
    let config = rdm_common::config::RdmConfig::load();
    let wp = &config.wallpaper;
    let mut args = Vec::new();
    if !wp.path.is_empty() {
        args.push("-i".to_string());
        args.push(wp.path.clone());
        args.push("-m".to_string());
        args.push(wp.mode.clone());
    }
    args.push("-c".to_string());
    args.push(wp.color.clone());
    log::info!("swaybg args: {:?}", args);
    args
}

/// Check if another rdm-session is already running by reading the PID file
/// and verifying the process is alive. Returns `Some(pid)` if a live instance
/// exists, `None` otherwise (safe to proceed).
fn check_existing_instance() -> Option<u32> {
    let pid_path = rdm_common::config::config_dir().join("session.pid");
    let contents = std::fs::read_to_string(&pid_path).ok()?;

    // PID file format: "<pid>\n<WAYLAND_DISPLAY>" (second line optional)
    let mut lines = contents.lines();
    let pid: u32 = lines.next()?.trim().parse().ok()?;
    let stored_display = lines.next().unwrap_or("").trim().to_string();

    // Don't block on our own PID (shouldn't happen, but be safe)
    if pid == std::process::id() {
        return None;
    }

    // If the stored session was on a different WAYLAND_DISPLAY, it's stale
    let current_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    if !stored_display.is_empty() && !current_display.is_empty() && stored_display != current_display {
        log::info!(
            "Stale PID file (display was {}, now {}), ignoring",
            stored_display, current_display
        );
        return None;
    }

    // Check if the process is alive by sending signal 0
    match signal::kill(Pid::from_raw(pid as i32), None) {
        Ok(()) => {
            // Process exists — verify it's actually rdm-session, not a recycled PID
            let cmdline_path = format!("/proc/{}/cmdline", pid);
            if let Ok(cmdline) = std::fs::read_to_string(&cmdline_path) {
                if cmdline.contains("rdm-session") {
                    return Some(pid);
                }
                // PID was recycled for a different process — stale PID file
                log::info!("Stale PID file (pid={} is now {:?}), ignoring", pid, cmdline);
                return None;
            }
            // /proc not readable (unlikely on Linux) — assume it's ours to be safe
            Some(pid)
        }
        Err(_) => {
            // Process is dead — stale PID file
            log::info!("Stale PID file (pid={} is dead), ignoring", pid);
            None
        }
    }
}

fn write_pid_file() {
    let run_dir = rdm_common::config::config_dir();
    let _ = std::fs::create_dir_all(&run_dir);
    let pid_path = run_dir.join("session.pid");
    let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_default();
    let contents = format!("{}\n{}", std::process::id(), wayland_display);
    if let Err(e) = std::fs::write(&pid_path, contents) {
        log::error!("Failed to write PID file: {}", e);
    } else {
        log::info!("PID file written to {:?} (display={})", pid_path, wayland_display);
    }
}

fn cleanup_pid_file() {
    let pid_path = rdm_common::config::config_dir().join("session.pid");
    let _ = std::fs::remove_file(&pid_path);
    log::info!("PID file removed");
}

fn load_session_config() -> SessionConfig {
    let path = rdm_common::config::config_dir().join("session.toml");
    match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
        Err(_) => {
            log::info!("No session.toml found, using defaults");
            SessionConfig::default()
        }
    }
}

/// Apply display configuration from rdm.toml using wlr-randr.
/// Called before starting processes so the panel sees the correct monitor layout.
fn apply_display_settings() {
    let rdm_config = rdm_common::config::RdmConfig::load();
    if rdm_config.displays.is_empty() {
        log::info!("No display config in rdm.toml, using compositor defaults");
        return;
    }

    // Retry loop — labwc may not have finished initializing outputs yet
    let max_attempts = 10;
    for attempt in 1..=max_attempts {
        match rdm_common::display::apply_display_config(&rdm_config.displays) {
            Ok(()) => {
                log::info!("Display configuration applied (attempt {})", attempt);
                return;
            }
            Err(e) => {
                if attempt < max_attempts {
                    log::warn!(
                        "Display config attempt {}/{} failed: {} — retrying in 500ms",
                        attempt, max_attempts, e
                    );
                    std::thread::sleep(std::time::Duration::from_millis(500));
                } else {
                    log::error!(
                        "Failed to apply display config after {} attempts: {}",
                        max_attempts, e
                    );
                }
            }
        }
    }
}
