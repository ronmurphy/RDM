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
    master_ratio: f64,
    inner_gap: i32,
    outer_gap: i32,
}

impl TilingState {
    fn new(config: &RdmConfig) -> Self {
        Self {
            outputs: HashMap::new(),
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
    fn remove_window(&mut self, id: u32) -> Option<String> {
        for (name, ot) in &mut self.outputs {
            if let Some(pos) = ot.tiled.iter().position(|&w| w == id) {
                ot.tiled.remove(pos);
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
}

pub fn run_daemon() {
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
            // Group non-minimized windows by their output
            for (&id, info) in &state.toplevels {
                if info.is_minimized { continue; }
                let out_name = info.output_name.as_deref()
                    .or_else(|| outputs.first().map(|o| o.name.as_str()));
                if let Some(name) = out_name {
                    // Remove from any other output first
                    for (_, ot) in &mut tiling.outputs {
                        ot.tiled.retain(|&w| w != id);
                    }
                    let ot = tiling.get_output(name);
                    if !ot.tiled.contains(&id) {
                        ot.tiled.push(id);
                    }
                }
            }
            drop(state);
            // Apply layout on each output that has tiled windows
            let output_names: Vec<String> = tiling.outputs.keys().cloned().collect();
            for out_name in &output_names {
                if let Some((x, y, w, h)) = find_output(&outputs, out_name) {
                    log::info!("Tiling {} windows on output {}",
                        tiling.outputs.get(out_name).map(|o| o.tiled.len()).unwrap_or(0),
                        out_name);
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
