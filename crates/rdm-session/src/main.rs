use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use serde::Deserialize;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};

static RELOAD_REQUESTED: AtomicBool = AtomicBool::new(false);

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

    // Install SIGUSR1 handler for hot reload
    install_signal_handler();

    // Write our PID so rdm-reload can find us
    write_pid_file();

    // Apply display configuration from rdm.toml BEFORE starting panel/swaybg
    apply_display_settings();

    let config = load_session_config();
    let mut processes = start_all(&config);

    // Monitor loop
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Check for hot reload request
        if RELOAD_REQUESTED.swap(false, Ordering::SeqCst) {
            log::info!("=== HOT RELOAD REQUESTED ===");
            log::info!("Stopping all managed processes...");
            stop_all(&mut processes);

            // Small delay to let processes fully exit and release layer-shell surfaces
            tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;

            // Re-apply display config (may have changed via rdm-settings)
            apply_display_settings();

            // Small delay for display changes to settle before starting panel
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

            // Reload config in case it changed
            let new_config = load_session_config();
            log::info!("Restarting all processes with fresh binaries...");
            processes = start_all(&new_config);
            log::info!("=== HOT RELOAD COMPLETE ===");
            continue;
        }

        // Normal monitoring — restart crashed processes (with backoff)
        let now = std::time::Instant::now();
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
                    }
                }
            }
        }
    }
}

fn install_signal_handler() {
    // Use a simple signal-safe atomic flag
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
    }
    log::info!("SIGUSR1 handler installed — send SIGUSR1 to reload shell components");
}

extern "C" fn handle_sigusr1(_: libc::c_int) {
    RELOAD_REQUESTED.store(true, Ordering::SeqCst);
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
    // For swaybg, build args from rdm.toml wallpaper config instead of session.toml args
    let args: Vec<String> = if entry.command == "swaybg" {
        build_swaybg_args()
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

fn write_pid_file() {
    let run_dir = rdm_common::config::config_dir();
    let _ = std::fs::create_dir_all(&run_dir);
    let pid_path = run_dir.join("session.pid");
    if let Err(e) = std::fs::write(&pid_path, std::process::id().to_string()) {
        log::error!("Failed to write PID file: {}", e);
    } else {
        log::info!("PID file written to {:?}", pid_path);
    }
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
    match rdm_common::display::apply_display_config(&rdm_config.displays) {
        Ok(()) => log::info!("Display configuration applied"),
        Err(e) => log::error!("Failed to apply display config: {}", e),
    }
}
