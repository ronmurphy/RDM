use crate::ipc::{self, SnapCommand, Direction};
use crate::layout;
use crate::position;
use crate::wayland::{self, OutputInfo, SharedState, ToplevelAction};
use rdm_common::config::RdmConfig;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::sync::{Arc, Mutex, mpsc::Sender};

/// Per-output tiling state.
struct OutputTiling {
    /// Window IDs in tiling order (first = master).
    tiled: Vec<u32>,
}

/// The daemon's tiling state across all outputs.
struct TilingState {
    /// Per-output tiling, keyed by output name.
    outputs: HashMap<String, OutputTiling>,
    /// Remember the last output a window was tiled on, so after float+re-tile
    /// it goes back to the same monitor instead of wherever focus drifted.
    last_output: HashMap<u32, String>,
    master_ratio: f64,
    inner_gap: i32,
    outer_gap: i32,
}

impl TilingState {
    fn new(config: &RdmConfig) -> Self {
        Self {
            outputs: HashMap::new(),
            last_output: HashMap::new(),
            master_ratio: config.snap.master_ratio,
            inner_gap: config.snap.inner_gap,
            outer_gap: config.snap.outer_gap,
        }
    }

    fn get_output(&mut self, name: &str) -> &mut OutputTiling {
        self.outputs.entry(name.to_string()).or_insert_with(|| OutputTiling {
            tiled: Vec::new(),
        })
    }

    /// Remove a window from whichever output it's tiled on.
    /// Remembers the output so re-tiling can restore it.
    fn remove_window(&mut self, id: u32) -> Option<String> {
        for (name, ot) in &mut self.outputs {
            if let Some(pos) = ot.tiled.iter().position(|&w| w == id) {
                ot.tiled.remove(pos);
                self.last_output.insert(id, name.clone());
                return Some(name.clone());
            }
        }
        None
    }

    /// Find which output a window is tiled on.
    fn find_window(&self, id: u32) -> Option<&str> {
        for (name, ot) in &self.outputs {
            if ot.tiled.contains(&id) {
                return Some(name);
            }
        }
        None
    }

    /// Get the best output for a window: remembered output first, then Wayland-reported, then fallback.
    fn best_output_for(&self, id: u32, wayland_output: Option<&str>, fallback: Option<&str>) -> Option<String> {
        // 1. If we remember where it was last tiled, use that
        if let Some(name) = self.last_output.get(&id) {
            return Some(name.clone());
        }
        // 2. Use the output Wayland says it's on
        if let Some(name) = wayland_output {
            return Some(name.to_string());
        }
        // 3. Fallback to first output
        fallback.map(|s| s.to_string())
    }
}

pub fn run_daemon() {
    // Single-instance guard: prevent duplicate daemons from stacking up.
    let _lock = match acquire_daemon_lock() {
        Some(lock) => lock,
        None => {
            eprintln!("rdm-snap: another daemon is already running");
            return;
        }
    };

    let config = RdmConfig::load();
    log::info!(
        "rdm-snap daemon starting (master_ratio={}, gaps={}+{})",
        config.snap.master_ratio,
        config.snap.inner_gap,
        config.snap.outer_gap,
    );

    // Ensure rc.xml has our action markers for the F24 dynamic keybind
    position::ensure_markers_in_rc_xml();

    // Start Wayland tracking thread
    let (shared, action_tx) = wayland::start_wayland_tracker();

    // Give Wayland a moment to enumerate outputs + toplevels
    std::thread::sleep(std::time::Duration::from_millis(500));

    {
        let s = shared.lock().unwrap();
        log::info!(
            "Initial state: {} toplevels, {} outputs",
            s.toplevels.len(),
            s.outputs.len(),
        );
        for out in &s.outputs {
            log::info!("  output: {} ({}x{} @ {},{})", out.name, out.width, out.height, out.x, out.y);
        }
    }

    let mut tiling = TilingState::new(&config);

    // Set up Unix socket listener
    let sock_path = ipc::socket_path();
    let _ = std::fs::remove_file(&sock_path);
    let listener = match UnixListener::bind(&sock_path) {
        Ok(l) => l,
        Err(e) => {
            log::error!("Failed to bind socket {:?}: {}", sock_path, e);
            return;
        }
    };
    log::info!("Listening on {:?}", sock_path);

    // Accept connections sequentially (tiling commands are fast and serial)
    for stream in listener.incoming() {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => { log::warn!("Accept error: {}", e); continue; }
        };

        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() { continue; }

        let Some(cmd) = SnapCommand::from_wire(&line) else {
            let _ = writeln!(stream, "error: unknown command");
            continue;
        };

        log::info!("Command: {:?}", cmd);

        let response = handle_command(
            &cmd,
            &mut tiling,
            &shared,
            &action_tx,
        );

        let _ = writeln!(stream, "{}", response);

        if matches!(cmd, SnapCommand::Quit) {
            log::info!("Quit command received, shutting down");
            break;
        }
    }

    // Clean up
    let _ = std::fs::remove_file(&sock_path);
    position::remove_markers();
    log::info!("rdm-snap daemon stopped");
}

/// Look up an output's geometry by name.
fn find_output(outputs: &[OutputInfo], name: &str) -> Option<(i32, i32, i32, i32)> {
    outputs.iter()
        .find(|o| o.name == name)
        .map(|o| (o.x, o.y, o.width, o.height))
}

fn handle_command(
    cmd: &SnapCommand,
    tiling: &mut TilingState,
    shared: &Arc<Mutex<SharedState>>,
    action_tx: &Sender<ToplevelAction>,
) -> String {
    let state = shared.lock().unwrap();

    // Find the currently focused window and its output
    let focused = state.toplevels.iter()
        .find(|(_, info)| info.is_activated)
        .map(|(&id, info)| (id, info.output_name.clone()));

    let focused_id = focused.as_ref().map(|(id, _)| *id);
    let focused_output = focused.and_then(|(_, out)| out);

    // Snapshot outputs for geometry lookups
    let outputs: Vec<OutputInfo> = state.outputs.clone();

    drop(state); // Release lock before doing any positioning work

    // Resolve the output geometry for the focused window
    let resolve_output = |output_name: &Option<String>| -> Option<(String, i32, i32, i32, i32)> {
        if let Some(name) = output_name {
            if let Some((x, y, w, h)) = find_output(&outputs, name) {
                return Some((name.clone(), x, y, w, h));
            }
        }
        // Fallback to first output
        outputs.first().map(|o| (o.name.clone(), o.x, o.y, o.width, o.height))
    };

    match cmd {
        SnapCommand::Tile => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            let Some((out_name, out_x, out_y, out_w, out_h)) = resolve_output(&focused_output) else {
                return "error: no output found".into();
            };
            // Remove from any previous output first
            tiling.remove_window(id);
            let ot = tiling.get_output(&out_name);
            if !ot.tiled.contains(&id) {
                ot.tiled.push(id);
            }
            log::info!("Tiling window {} on output {}", id, out_name);
            apply_layout_for_output(tiling, &out_name, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::Float => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            if let Some(out_name) = tiling.remove_window(id) {
                if let Some((x, y, w, h)) = find_output(&outputs, &out_name) {
                    log::info!("Floating window {} from output {}", id, out_name);
                    apply_layout_for_output(tiling, &out_name, x, y, w, h, action_tx, focused_id);
                }
            }
            "ok".into()
        }
        SnapCommand::TileAll => {
            let state = shared.lock().unwrap();
            let fallback = outputs.first().map(|o| o.name.as_str());
            // Only assign windows that are NOT already tiled on some output.
            // For previously-floated windows, prefer their remembered output
            // over the Wayland-reported one (which may have drifted due to
            // focus changes during the activate/F24 cycle).
            for (&id, info) in &state.toplevels {
                if info.is_minimized { continue; }
                // Already tiled somewhere? Keep it there.
                if tiling.find_window(id).is_some() { continue; }
                let best = tiling.best_output_for(
                    id,
                    info.output_name.as_deref(),
                    fallback,
                );
                if let Some(name) = best {
                    log::info!("Assigning window {} to output {} (remembered: {})",
                        id, name, tiling.last_output.contains_key(&id));
                    let ot = tiling.get_output(&name);
                    ot.tiled.push(id);
                }
            }
            // Also prune dead windows (closed since last tile)
            let live_ids: Vec<u32> = state.toplevels.keys().copied().collect();
            drop(state);
            for (_, ot) in &mut tiling.outputs {
                ot.tiled.retain(|id| live_ids.contains(id));
            }
            // Apply layout on each output that has tiled windows
            let output_names: Vec<String> = tiling.outputs.keys().cloned().collect();
            for out_name in &output_names {
                if let Some((x, y, w, h)) = find_output(&outputs, out_name) {
                    let count = tiling.outputs.get(out_name).map(|o| o.tiled.len()).unwrap_or(0);
                    if count == 0 { continue; }
                    log::info!("Tiling {} windows on output {}", count, out_name);
                    apply_layout_for_output(tiling, out_name, x, y, w, h, action_tx, None);
                }
            }
            // Restore focus
            if let Some(focus_id) = focused_id {
                let _ = action_tx.send(ToplevelAction::Activate(focus_id));
            }
            "ok".into()
        }
        SnapCommand::SwapMaster => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            let Some(out_name) = tiling.find_window(id).map(|s| s.to_string()) else {
                return "error: focused window is not tiled".into();
            };
            let ot = tiling.get_output(&out_name);
            if let Some(pos) = ot.tiled.iter().position(|&w| w == id) {
                if pos > 0 {
                    ot.tiled.swap(0, pos);
                    if let Some((x, y, w, h)) = find_output(&outputs, &out_name) {
                        apply_layout_for_output(tiling, &out_name, x, y, w, h, action_tx, focused_id);
                    }
                }
            }
            "ok".into()
        }
        SnapCommand::GrowMaster => {
            tiling.master_ratio = (tiling.master_ratio + 0.05).min(0.8);
            apply_all_outputs(tiling, &outputs, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::ShrinkMaster => {
            tiling.master_ratio = (tiling.master_ratio - 0.05).max(0.2);
            apply_all_outputs(tiling, &outputs, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::Focus(dir) => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            let Some(out_name) = tiling.find_window(id).map(|s| s.to_string()) else {
                return "error: focused window is not tiled".into();
            };
            let ot = tiling.get_output(&out_name);
            let tiled = &ot.tiled;
            let Some(pos) = tiled.iter().position(|&w| w == id) else {
                return "error: window not in tiling list".into();
            };
            let new_pos = match dir {
                Direction::Left | Direction::Up => {
                    if pos > 0 { pos - 1 } else { tiled.len() - 1 }
                }
                Direction::Right | Direction::Down => {
                    if pos + 1 < tiled.len() { pos + 1 } else { 0 }
                }
            };
            let target = tiled[new_pos];
            let _ = action_tx.send(ToplevelAction::Activate(target));
            "ok".into()
        }
        SnapCommand::Reset => {
            let state = shared.lock().unwrap();
            let live_ids: Vec<u32> = state.toplevels.keys().copied().collect();
            drop(state);
            for (_, ot) in &mut tiling.outputs {
                ot.tiled.retain(|id| live_ids.contains(id));
            }
            apply_all_outputs(tiling, &outputs, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::Quit => "ok".into(),
    }
}

fn apply_layout_for_output(
    tiling: &TilingState,
    output_name: &str,
    out_x: i32,
    out_y: i32,
    out_w: i32,
    out_h: i32,
    action_tx: &Sender<ToplevelAction>,
    restore_focus: Option<u32>,
) {
    let Some(ot) = tiling.outputs.get(output_name) else { return };

    let geometries = layout::master_stack(
        &ot.tiled,
        out_x, out_y, out_w, out_h,
        tiling.master_ratio,
        tiling.inner_gap,
        tiling.outer_gap,
    );

    if geometries.is_empty() {
        position::clear_actions();
        return;
    }

    for (wid, geo) in &geometries {
        log::info!("  [{}] window {} → {}x{} @ ({},{})", output_name, wid, geo.width, geo.height, geo.x, geo.y);
        // Activate the target window
        let _ = action_tx.send(ToplevelAction::Activate(*wid));
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Apply geometry via dynamic keybind trick
        if let Err(e) = position::apply_one(geo, output_name) {
            log::error!("Failed to position window {}: {}", wid, e);
        }
    }

    // Restore focus to the originally focused window
    if let Some(focus_id) = restore_focus {
        let _ = action_tx.send(ToplevelAction::Activate(focus_id));
    }

    // Clean up the temporary keybind
    position::clear_actions();
}

/// Re-apply layout on all outputs (used for grow/shrink master, reset).
fn apply_all_outputs(
    tiling: &TilingState,
    outputs: &[OutputInfo],
    action_tx: &Sender<ToplevelAction>,
    restore_focus: Option<u32>,
) {
    let output_names: Vec<String> = tiling.outputs.keys().cloned().collect();
    for out_name in &output_names {
        if let Some((x, y, w, h)) = find_output(outputs, out_name) {
            apply_layout_for_output(tiling, out_name, x, y, w, h, action_tx, None);
        }
    }
    if let Some(focus_id) = restore_focus {
        let _ = action_tx.send(ToplevelAction::Activate(focus_id));
    }
}

/// File-lock based single-instance guard for the daemon.
struct DaemonLock {
    _file: std::fs::File,
}

fn acquire_daemon_lock() -> Option<DaemonLock> {
    use std::os::unix::io::AsRawFd;

    let run_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    let lock_path = std::path::PathBuf::from(run_dir).join("rdm-snap.lock");

    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .ok()?;

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        return None;
    }

    use std::io::Write;
    let mut f = &file;
    let _ = f.write_all(format!("{}\n", std::process::id()).as_bytes());

    Some(DaemonLock { _file: file })
}
