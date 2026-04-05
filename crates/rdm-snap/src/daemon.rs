use crate::ipc::{self, SnapCommand, Direction};
use crate::layout;
use crate::position;
use crate::wayland::{self, SharedState, ToplevelAction};
use rdm_common::config::RdmConfig;
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
    /// For v1: single output tiling. Multi-output keyed by output name comes later.
    default: OutputTiling,
    master_ratio: f64,
    inner_gap: i32,
    outer_gap: i32,
}

impl TilingState {
    fn new(config: &RdmConfig) -> Self {
        Self {
            default: OutputTiling { tiled: Vec::new() },
            master_ratio: config.snap.master_ratio,
            inner_gap: config.snap.inner_gap,
            outer_gap: config.snap.outer_gap,
        }
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

fn handle_command(
    cmd: &SnapCommand,
    tiling: &mut TilingState,
    shared: &Arc<Mutex<SharedState>>,
    action_tx: &Sender<ToplevelAction>,
) -> String {
    let state = shared.lock().unwrap();

    // Find the currently focused window
    let focused_id = state.toplevels.iter()
        .find(|(_, info)| info.is_activated)
        .map(|(&id, _)| id);

    // Get primary output geometry (first output, or fallback)
    let (out_x, out_y, out_w, out_h) = if let Some(out) = state.outputs.first() {
        (out.x, out.y, out.width, out.height)
    } else {
        log::warn!("No outputs detected, using 1920x1080 fallback");
        (0, 0, 1920, 1080)
    };

    drop(state); // Release lock before doing any positioning work

    match cmd {
        SnapCommand::Tile => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            if !tiling.default.tiled.contains(&id) {
                tiling.default.tiled.push(id);
                apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            }
            "ok".into()
        }
        SnapCommand::Float => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            tiling.default.tiled.retain(|&w| w != id);
            apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::TileAll => {
            let state = shared.lock().unwrap();
            for (&id, info) in &state.toplevels {
                if !info.is_minimized && !tiling.default.tiled.contains(&id) {
                    tiling.default.tiled.push(id);
                }
            }
            drop(state);
            apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::SwapMaster => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            if let Some(pos) = tiling.default.tiled.iter().position(|&w| w == id) {
                if pos > 0 {
                    tiling.default.tiled.swap(0, pos);
                    apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
                }
            }
            "ok".into()
        }
        SnapCommand::GrowMaster => {
            tiling.master_ratio = (tiling.master_ratio + 0.05).min(0.8);
            apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::ShrinkMaster => {
            tiling.master_ratio = (tiling.master_ratio - 0.05).max(0.2);
            apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::Focus(dir) => {
            let Some(id) = focused_id else { return "error: no focused window".into() };
            let tiled = &tiling.default.tiled;
            let Some(pos) = tiled.iter().position(|&w| w == id) else {
                return "error: focused window is not tiled".into();
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
            // Remove any tiled windows that no longer exist
            let state = shared.lock().unwrap();
            let live_ids: Vec<u32> = state.toplevels.keys().copied().collect();
            drop(state);
            tiling.default.tiled.retain(|id| live_ids.contains(id));
            apply_layout(tiling, out_x, out_y, out_w, out_h, action_tx, focused_id);
            "ok".into()
        }
        SnapCommand::Quit => "ok".into(),
    }
}

fn apply_layout(
    tiling: &TilingState,
    out_x: i32,
    out_y: i32,
    out_w: i32,
    out_h: i32,
    action_tx: &Sender<ToplevelAction>,
    restore_focus: Option<u32>,
) {
    let geometries = layout::master_stack(
        &tiling.default.tiled,
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
        log::info!("  window {} → {}x{} @ ({},{})", wid, geo.width, geo.height, geo.x, geo.y);
        // Activate the target window
        let _ = action_tx.send(ToplevelAction::Activate(*wid));
        std::thread::sleep(std::time::Duration::from_millis(50));
        // Apply geometry via dynamic keybind trick
        if let Err(e) = position::apply_one(geo) {
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
