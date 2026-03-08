use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::output::OutputPanel;

/// Events sent from the background thread to the GTK main loop.
enum RunEvent {
    Stdout(String),
    Stderr(String),
    Done { success: bool, exit_code: Option<i32> },
}

/// Manages the currently running child process.
#[derive(Clone)]
pub struct RunManager {
    stop_flag: Arc<AtomicBool>,
}

impl RunManager {
    pub fn new() -> Self {
        Self { stop_flag: Arc::new(AtomicBool::new(false)) }
    }

    /// Attempt to stop the current process.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        !self.stop_flag.load(Ordering::Relaxed)
    }

    /// Run the file at `path`, streaming output to `output`.
    /// `on_start` / `on_done` are called on the GTK main thread.
    pub fn run_file(
        &self,
        path: &Path,
        output: &OutputPanel,
        on_start: impl Fn() + 'static,
        on_done: impl Fn(bool) + 'static,
    ) {
        // Reset stop flag.
        self.stop_flag.store(false, Ordering::Relaxed);

        let Some((program, args, cwd)) = build_command(path) else {
            output.append_run_error(&format!(
                "Don't know how to run: {}",
                path.display()
            ));
            return;
        };

        output.clear_run();
        output.show_panel();
        output.switch_to_run();
        output.append_run_line(&format!(
            "▶ {} {}",
            program,
            args.join(" ")
        ));
        on_start();

        let stop_flag = self.stop_flag.clone();
        let (tx, rx) = async_channel::unbounded::<RunEvent>();

        // Spawn background thread to read child output.
        std::thread::spawn(move || {
            let mut child = match Command::new(&program)
                .args(&args)
                .current_dir(&cwd)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send_blocking(RunEvent::Stderr(format!(
                        "Failed to start '{}': {}",
                        program, e
                    )));
                    let _ = tx.send_blocking(RunEvent::Done { success: false, exit_code: None });
                    return;
                }
            };

            let stdout = child.stdout.take();
            let stderr_pipe = child.stderr.take();
            let tx2 = tx.clone();
            let flag2 = stop_flag.clone();

            // Read stderr in a separate thread.
            if let Some(err) = stderr_pipe {
                std::thread::spawn(move || {
                    for line in std::io::BufReader::new(err).lines() {
                        if flag2.load(Ordering::Relaxed) { break; }
                        if let Ok(l) = line {
                            let _ = tx2.send_blocking(RunEvent::Stderr(l));
                        }
                    }
                });
            }

            // Read stdout in this thread.
            if let Some(out) = stdout {
                for line in std::io::BufReader::new(out).lines() {
                    if stop_flag.load(Ordering::Relaxed) {
                        let _ = child.kill();
                        break;
                    }
                    if let Ok(l) = line {
                        let _ = tx.send_blocking(RunEvent::Stdout(l));
                    }
                }
            }

            let exit_code = child.wait().ok().and_then(|s| s.code());
            let success = exit_code == Some(0);
            let _ = tx.send_blocking(RunEvent::Done { success, exit_code });
        });

        // Consume events on the GTK main thread.
        let output = output.clone();
        gtk4::glib::spawn_future_local(async move {
            while let Ok(ev) = rx.recv().await {
                match ev {
                    RunEvent::Stdout(line) => output.append_run_line(&line),
                    RunEvent::Stderr(line) => output.append_run_error(&line),
                    RunEvent::Done { success, exit_code } => {
                        let msg = match exit_code {
                            Some(code) => format!(
                                "── Process exited with code {} ──",
                                code
                            ),
                            None => "── Process terminated ──".to_string(),
                        };
                        if success {
                            output.append_run_success(&msg);
                        } else {
                            output.append_run_error(&msg);
                        }
                        on_done(success);
                        break;
                    }
                }
            }
        });
    }

    /// Open an HTML/CSS file in the default browser.
    pub fn open_in_browser(path: &Path) {
        let url = format!("file://{}", path.display());
        let _ = Command::new("xdg-open").arg(&url).spawn();
    }
}

/// Build the shell command for the given file.
/// Returns (program, args, working_directory).
fn build_command(path: &Path) -> Option<(String, Vec<String>, PathBuf)> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let cwd = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let file_str = path.to_string_lossy().to_string();

    match ext {
        "py" => Some(("python3".into(), vec![file_str], cwd)),

        "js" | "mjs" => Some(("node".into(), vec![file_str], cwd)),

        "ts" => {
            // Try node --experimental-strip-types (Node ≥ 22), fall back to ts-node.
            if node_supports_strip_types() {
                Some(("node".into(), vec!["--experimental-strip-types".into(), file_str], cwd))
            } else {
                Some(("npx".into(), vec!["ts-node".into(), file_str], cwd))
            }
        }

        "html" | "htm" | "css" => {
            // These are opened in browser / live preview; not run as processes.
            // Return a no-op that just echoes.
            Some(("echo".into(), vec!["Opening in browser…".into()], cwd))
        }

        "rs" => {
            // Find the nearest Cargo.toml and run `cargo run` there.
            if let Some(cargo_dir) = find_cargo_toml(path) {
                Some(("cargo".into(), vec!["run".into()], cargo_dir))
            } else {
                // Compile single file with rustc.
                let out = format!(
                    "/tmp/rdm_editor_{}",
                    path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "out".into())
                );
                Some((
                    "sh".into(),
                    vec![
                        "-c".into(),
                        format!("rustc {} -o {} && {}", file_str, out, out),
                    ],
                    cwd,
                ))
            }
        }

        "sh" | "bash" => Some(("bash".into(), vec![file_str], cwd)),

        _ => None,
    }
}

/// Walk up from `path` to find a Cargo.toml and return its directory.
fn find_cargo_toml(path: &Path) -> Option<PathBuf> {
    let mut dir = path.parent()?.to_path_buf();
    loop {
        if dir.join("Cargo.toml").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

/// Quick check if the system `node` supports --experimental-strip-types (≥ v22).
fn node_supports_strip_types() -> bool {
    Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|v| {
            v.trim()
                .trim_start_matches('v')
                .split('.')
                .next()
                .and_then(|maj| maj.parse::<u32>().ok())
        })
        .map(|maj| maj >= 22)
        .unwrap_or(false)
}
