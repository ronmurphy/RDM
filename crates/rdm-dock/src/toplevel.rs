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

#[derive(Debug, Clone)]
pub struct ToplevelInfo {
    #[allow(dead_code)]
    pub title: String,
    pub app_id: String,
    pub is_activated: bool,
    #[allow(dead_code)]
    pub is_minimized: bool,
}

pub enum ToplevelAction {
    Toggle(u32),
}

pub type ToplevelMap = HashMap<u32, ToplevelInfo>;

pub struct SharedState {
    pub toplevels: ToplevelMap,
    pub generation: u64,
}

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
        self.states
            .chunks_exact(4)
            .any(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) == val)
    }

    fn to_info(&self) -> ToplevelInfo {
        ToplevelInfo {
            title: self.title.clone(),
            app_id: self.app_id.clone(),
            is_activated: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Activated),
            is_minimized: self.has_state(zwlr_foreign_toplevel_handle_v1::State::Minimized),
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
                ToplevelAction::Toggle(id) => {
                    if let Some(handle_state) = self.handles.get(&id) {
                        let is_activated = handle_state
                            .has_state(zwlr_foreign_toplevel_handle_v1::State::Activated);
                        let is_minimized = handle_state
                            .has_state(zwlr_foreign_toplevel_handle_v1::State::Minimized);
                        if let Some(handle) = self.id_to_handle.get(&id) {
                            if is_activated && !is_minimized {
                                handle.set_minimized();
                            } else {
                                handle.unset_minimized();
                                if let Some(seat) = &self.seat {
                                    handle.activate(seat);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

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

pub fn start_toplevel_tracker() -> (
    Arc<Mutex<SharedState>>,
    std::sync::mpsc::Sender<ToplevelAction>,
) {
    let shared = Arc::new(Mutex::new(SharedState {
        toplevels: HashMap::new(),
        generation: 0,
    }));

    let (action_tx, action_rx) = std::sync::mpsc::channel();
    let shared_clone = shared.clone();

    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_wayland_loop(shared_clone, action_rx)
        }));
        match result {
            Ok(Ok(())) => log::info!("dock toplevel loop exited normally"),
            Ok(Err(e)) => log::error!("dock toplevel tracker error: {}", e),
            Err(_) => log::error!("dock toplevel tracker PANICKED"),
        }
    });

    (shared, action_tx)
}

pub fn can_bind_foreign_toplevel_manager() -> bool {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let (globals, mut event_queue): (_, EventQueue<WaylandState>) =
        match registry_queue_init(&conn) {
            Ok(v) => v,
            Err(_) => return false,
        };
    let qh = event_queue.handle();
    if globals
        .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
        .is_err()
    {
        return false;
    }
    let (_tx, action_rx) = std::sync::mpsc::channel();
    let mut state = WaylandState {
        shared: Arc::new(Mutex::new(SharedState {
            toplevels: HashMap::new(),
            generation: 0,
        })),
        handles: HashMap::new(),
        next_id: 1,
        obj_to_id: HashMap::new(),
        seat: None,
        action_rx,
        id_to_handle: HashMap::new(),
    };
    event_queue.roundtrip(&mut state).is_ok()
}

fn run_wayland_loop(
    shared: Arc<Mutex<SharedState>>,
    action_rx: std::sync::mpsc::Receiver<ToplevelAction>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut event_queue): (_, EventQueue<WaylandState>) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let _manager = globals
        .bind::<ZwlrForeignToplevelManagerV1, _, _>(&qh, 1..=3, ())
        .map_err(|e| format!("bind foreign toplevel manager: {}", e))?;

    let seat: Option<WlSeat> = globals.bind::<WlSeat, _, _>(&qh, 1..=9, ()).ok();

    let mut state = WaylandState {
        shared,
        handles: HashMap::new(),
        next_id: 1,
        obj_to_id: HashMap::new(),
        seat,
        action_rx,
        id_to_handle: HashMap::new(),
    };

    loop {
        state.process_actions();
        event_queue.roundtrip(&mut state)?;
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}
