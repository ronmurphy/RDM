use std::collections::HashMap;
use std::os::unix::io::AsRawFd;
use std::sync::{Arc, Mutex};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output, wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ToplevelInfo {
    pub title: String,
    pub app_id: String,
    pub is_activated: bool,
    pub is_maximized: bool,
    pub is_minimized: bool,
    pub is_fullscreen: bool,
}

#[derive(Debug, Clone)]
pub struct OutputInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

pub type ToplevelMap = HashMap<u32, ToplevelInfo>;

/// Actions the tiling engine can request on a toplevel.
pub enum ToplevelAction {
    Activate(u32),
}

/// Shared state between the Wayland thread and the main thread.
pub struct SharedState {
    pub toplevels: ToplevelMap,
    pub outputs: Vec<OutputInfo>,
    pub generation: u64,
}

// ── Internal Wayland state ────────────────────────────────────────────────────

struct WaylandState {
    shared: Arc<Mutex<SharedState>>,
    handles: HashMap<u32, HandleState>,
    next_id: u32,
    obj_to_id: HashMap<wayland_client::backend::ObjectId, u32>,
    seat: Option<WlSeat>,
    action_rx: std::sync::mpsc::Receiver<ToplevelAction>,
    id_to_handle: HashMap<u32, ZwlrForeignToplevelHandleV1>,
    // Output tracking
    output_pending: HashMap<wayland_client::backend::ObjectId, OutputPending>,
}

struct HandleState {
    title: String,
    app_id: String,
    states: Vec<u8>,
}

#[derive(Default)]
struct OutputPending {
    name: String,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl HandleState {
    fn has_state(&self, state: zwlr_foreign_toplevel_handle_v1::State) -> bool {
        let val = state as u32;
        self.states
            .chunks_exact(4)
            .any(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) == val)
    }

    fn to_info(&self) -> ToplevelInfo {
        ToplevelInfo {
            title: self.title.clone(),
            app_id: self.app_id.clone(),
            is_activated: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Activated),
            is_maximized: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Maximized),
            is_minimized: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Minimized),
            is_fullscreen: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Fullscreen),
        }
    }
}

impl WaylandState {
    fn flush_toplevels(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.toplevels.clear();
        for (&id, handle) in &self.handles {
            shared.toplevels.insert(id, handle.to_info());
        }
        shared.generation += 1;
    }

    fn flush_outputs(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.outputs.clear();
        for (_, op) in &self.output_pending {
            if op.width > 0 && op.height > 0 {
                shared.outputs.push(OutputInfo {
                    name: op.name.clone(),
                    x: op.x,
                    y: op.y,
                    width: op.width,
                    height: op.height,
                });
            }
        }
    }

    fn process_actions(&mut self) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                ToplevelAction::Activate(id) => {
                    if let (Some(handle), Some(seat)) = (self.id_to_handle.get(&id), &self.seat) {
                        handle.activate(seat);
                    }
                }
            }
        }
    }
}

// ── Dispatch: registry ────────────────────────────────────────────────────────

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_registry::WlRegistry,
        _event: wl_registry::Event,
        _data: &GlobalListContents,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// ── Dispatch: foreign toplevel manager ────────────────────────────────────────

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                let id = state.next_id;
                state.next_id += 1;
                state.obj_to_id.insert(toplevel.id(), id);
                state.id_to_handle.insert(id, toplevel);
                state.handles.insert(id, HandleState {
                    title: String::new(),
                    app_id: String::new(),
                    states: Vec::new(),
                });
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => {
                log::warn!("Foreign toplevel manager finished");
            }
            _ => {}
        }
    }

    event_created_child!(WaylandState, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ()),
    ]);
}

// ── Dispatch: foreign toplevel handle ─────────────────────────────────────────

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let obj_id = proxy.id();
        let Some(&id) = state.obj_to_id.get(&obj_id) else { return };
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(h) = state.handles.get_mut(&id) { h.title = title; }
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(h) = state.handles.get_mut(&id) { h.app_id = app_id; }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: new_state } => {
                if let Some(h) = state.handles.get_mut(&id) { h.states = new_state; }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                state.flush_toplevels();
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.handles.remove(&id);
                state.obj_to_id.remove(&obj_id);
                state.id_to_handle.remove(&id);
                proxy.destroy();
                state.flush_toplevels();
            }
            _ => {}
        }
    }
}

// ── Dispatch: wl_output ───────────────────────────────────────────────────────

impl Dispatch<wl_output::WlOutput, ()> for WaylandState {
    fn event(
        state: &mut Self,
        proxy: &wl_output::WlOutput,
        event: wl_output::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        let obj_id = proxy.id();
        let op = state.output_pending.entry(obj_id).or_default();
        match event {
            wl_output::Event::Name { name } => { op.name = name; }
            wl_output::Event::Geometry { x, y, .. } => { op.x = x; op.y = y; }
            wl_output::Event::Mode { flags, width, height, .. } => {
                if let wayland_client::WEnum::Value(f) = flags {
                    if f.contains(wl_output::Mode::Current) {
                        op.width = width;
                        op.height = height;
                    }
                }
            }
            wl_output::Event::Done => {
                state.flush_outputs();
            }
            _ => {}
        }
    }
}

// ── Dispatch: wl_seat ─────────────────────────────────────────────────────────

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _proxy: &wl_seat::WlSeat,
        _event: wl_seat::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Start the Wayland tracking thread.
/// Returns shared state and an action sender.
pub fn start_wayland_tracker() -> (
    Arc<Mutex<SharedState>>,
    std::sync::mpsc::Sender<ToplevelAction>,
) {
    let shared = Arc::new(Mutex::new(SharedState {
        toplevels: HashMap::new(),
        outputs: Vec::new(),
        generation: 0,
    }));

    let (action_tx, action_rx) = std::sync::mpsc::channel();

    let shared_clone = shared.clone();
    std::thread::spawn(move || {
        if let Err(e) = run_wayland_loop(shared_clone, action_rx) {
            log::error!("Wayland event loop error: {}", e);
        }
    });

    (shared, action_tx)
}

fn run_wayland_loop(
    shared: Arc<Mutex<SharedState>>,
    action_rx: std::sync::mpsc::Receiver<ToplevelAction>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue): (_, EventQueue<WaylandState>) =
        registry_queue_init(&conn)?;

    let mut state = WaylandState {
        shared,
        handles: HashMap::new(),
        next_id: 1,
        obj_to_id: HashMap::new(),
        seat: None,
        action_rx,
        id_to_handle: HashMap::new(),
        output_pending: HashMap::new(),
    };

    // Bind globals
    let _manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(&event_queue.handle(), 1..=3, ())
        .map_err(|e| format!("Failed to bind foreign toplevel manager: {}", e))?;

    // Bind seat
    if let Ok(seat) = globals.bind::<WlSeat, _, _>(&event_queue.handle(), 1..=8, ()) {
        state.seat = Some(seat);
    }

    // Bind outputs
    globals.contents().with_list(|list| {
        for global in list {
            if global.interface == "wl_output" {
                let _output: wl_output::WlOutput = globals.registry()
                    .bind(global.name, global.version, &event_queue.handle(), ());
                // Output events will arrive in the dispatch loop
            }
        }
    });

    log::info!("Wayland connection established, entering event loop");

    // Use a polling approach: dispatch pending events, process actions, sleep briefly
    let fd = conn.prepare_read().unwrap().connection_fd().as_raw_fd();
    loop {
        // Flush outgoing
        let _ = conn.flush();

        // Poll the Wayland fd with a timeout so we can process actions
        let mut pollfd = [nix::poll::PollFd::new(
            unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) },
            nix::poll::PollFlags::POLLIN,
        )];
        let _ = nix::poll::poll(&mut pollfd, nix::poll::PollTimeout::from(100u16));

        // Dispatch any pending events
        if let Some(guard) = conn.prepare_read() {
            let _ = guard.read();
        }
        event_queue.dispatch_pending(&mut state)?;

        // Process actions from the main thread
        state.process_actions();
        let _ = conn.flush();
    }
}
