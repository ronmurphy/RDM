use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    delegate_noop, event_created_child,
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_output::WlOutput, wl_registry, wl_seat},
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

/// Represents a toplevel window's current state
#[derive(Debug, Clone)]
pub struct ToplevelInfo {
    pub title: String,
    pub app_id: String,
    pub is_activated: bool,
    pub is_maximized: bool,
    pub is_minimized: bool,
    pub is_fullscreen: bool,
}

/// Actions the GTK side can request on a toplevel
pub enum ToplevelAction {
    Activate(u32),
    Close(u32),
}

/// Snapshot of all toplevels for the GTK side to consume
pub type ToplevelMap = HashMap<u32, ToplevelInfo>;

/// Shared state between the Wayland thread and the GTK thread
pub struct SharedState {
    pub toplevels: ToplevelMap,
    pub generation: u64,
}

/// Internal state for the Wayland event loop
struct WaylandState {
    shared: Arc<Mutex<SharedState>>,
    handles: HashMap<u32, HandleState>,
    next_id: u32,
    obj_to_id: HashMap<wayland_client::backend::ObjectId, u32>,
    seat: Option<WlSeat>,
    action_rx: std::sync::mpsc::Receiver<ToplevelAction>,
    id_to_handle: HashMap<u32, ZwlrForeignToplevelHandleV1>,
}

struct HandleState {
    title: String,
    app_id: String,
    states: Vec<u8>,
}

impl HandleState {
    fn has_state(&self, state: zwlr_foreign_toplevel_handle_v1::State) -> bool {
        let val = state as u32;
        // States are sent as a byte array of u32 LE values
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
    fn flush_to_shared(&mut self) {
        let mut shared = self.shared.lock().unwrap();
        shared.toplevels.clear();
        for (&id, handle) in &self.handles {
            shared.toplevels.insert(id, handle.to_info());
        }
        shared.generation += 1;
    }

    fn process_actions(&mut self) {
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                ToplevelAction::Activate(id) => {
                    if let (Some(handle), Some(seat)) = (self.id_to_handle.get(&id), &self.seat) {
                        handle.activate(seat);
                    }
                }
                ToplevelAction::Close(id) => {
                    if let Some(handle) = self.id_to_handle.get(&id) {
                        handle.close();
                    }
                }
            }
        }
    }
}

// --- Dispatch implementations ---

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
                let obj_id = toplevel.id();
                state.obj_to_id.insert(obj_id, id);
                state.id_to_handle.insert(id, toplevel);
                state.handles.insert(
                    id,
                    HandleState {
                        title: String::new(),
                        app_id: String::new(),
                        states: Vec::new(),
                    },
                );
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
        let Some(&id) = state.obj_to_id.get(&obj_id) else {
            return;
        };

        match event {
            zwlr_foreign_toplevel_handle_v1::Event::Title { title } => {
                if let Some(h) = state.handles.get_mut(&id) {
                    h.title = title;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::AppId { app_id } => {
                if let Some(h) = state.handles.get_mut(&id) {
                    h.app_id = app_id;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::State { state: new_state } => {
                if let Some(h) = state.handles.get_mut(&id) {
                    h.states = new_state;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                state.flush_to_shared();
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.handles.remove(&id);
                state.obj_to_id.remove(&obj_id);
                state.id_to_handle.remove(&id);
                proxy.destroy();
                state.flush_to_shared();
            }
            _ => {}
        }
    }
}

delegate_noop!(WaylandState: ignore WlOutput);

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

/// Start the Wayland toplevel tracking thread.
/// Returns the shared state and an action sender for the GTK side.
pub fn start_toplevel_tracker() -> (Arc<Mutex<SharedState>>, std::sync::mpsc::Sender<ToplevelAction>)
{
    let shared = Arc::new(Mutex::new(SharedState {
        toplevels: HashMap::new(),
        generation: 0,
    }));

    let (action_tx, action_rx) = std::sync::mpsc::channel();

    let shared_clone = shared.clone();
    std::thread::spawn(move || {
        use std::io::Write;
        let log_write = |msg: &str| {
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("/tmp/rdm-taskbar.log")
                .and_then(|mut f| writeln!(f, "{}", msg));
        };

        log_write("toplevel thread spawned");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_wayland_loop(shared_clone, action_rx)
        }));

        match result {
            Ok(Ok(())) => log_write("toplevel loop exited normally"),
            Ok(Err(e)) => {
                let msg = format!("toplevel tracker error: {}", e);
                log_write(&msg);
                log::error!("{}", msg);
            }
            Err(panic_info) => {
                let msg = if let Some(s) = panic_info.downcast_ref::<&str>() {
                    format!("toplevel tracker PANICKED: {}", s)
                } else if let Some(s) = panic_info.downcast_ref::<String>() {
                    format!("toplevel tracker PANICKED: {}", s)
                } else {
                    "toplevel tracker PANICKED (unknown payload)".to_string()
                };
                log_write(&msg);
                log::error!("{}", msg);
            }
        }
    });

    (shared, action_tx)
}

fn run_wayland_loop(
    shared: Arc<Mutex<SharedState>>,
    action_rx: std::sync::mpsc::Receiver<ToplevelAction>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut dbg = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/rdm-taskbar.log")
        .ok();
    macro_rules! dbglog {
        ($($arg:tt)*) => {
            if let Some(ref mut f) = dbg { let _ = writeln!(f, $($arg)*); }
        }
    }

    dbglog!("toplevel: connecting to wayland...");
    let conn = Connection::connect_to_env()?;
    dbglog!("toplevel: connected, initializing registry...");
    let (globals, mut event_queue): (_, EventQueue<WaylandState>) =
        registry_queue_init(&conn)?;

    let qh = event_queue.handle();

    dbglog!("toplevel: binding foreign toplevel manager...");
    // Bind the foreign toplevel manager
    let _manager = match globals.bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ()) {
        Ok(m) => { dbglog!("toplevel: bound OK"); m }
        Err(e) => { dbglog!("toplevel: BIND FAILED: {}", e); return Err(e.into()); }
    };

    // Bind a seat for activate requests
    let seat: Option<WlSeat> = globals.bind::<WlSeat, _, _>(&qh, 1..=9, ()).ok();
    dbglog!("toplevel: seat bound: {}", seat.is_some());

    let mut state = WaylandState {
        shared,
        handles: HashMap::new(),
        next_id: 1,
        obj_to_id: HashMap::new(),
        seat,
        action_rx,
        id_to_handle: HashMap::new(),
    };

    dbglog!("toplevel: entering event loop");
    log::info!("Foreign toplevel manager bound — tracking windows");

    loop {
        // Process any pending actions from GTK
        state.process_actions();

        // Blocking dispatch with a short timeout so we can also check actions
        event_queue.roundtrip(&mut state)?;

        // Brief sleep to avoid busy-spinning
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
