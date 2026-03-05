use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::toplevel::{SharedState, ToplevelAction};

struct TaskbarState {
    buttons: HashMap<u32, gtk4::Button>,
    last_generation: u64,
}

/// Set up the taskbar: starts the Wayland toplevel tracker and polls it
/// from the GTK main loop to update buttons.
pub fn setup_taskbar(container: &gtk4::Box) {
    let (shared, action_tx) = crate::toplevel::start_toplevel_tracker();

    let state = Rc::new(RefCell::new(TaskbarState {
        buttons: HashMap::new(),
        last_generation: 0,
    }));

    let container = container.clone();
    let action_tx = Rc::new(action_tx);

    // Poll the shared state every 250ms
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        update_taskbar(&container, &shared, &state, &action_tx);
        gtk4::glib::ControlFlow::Continue
    });

    log::info!("Taskbar initialized with live toplevel tracking");
}

fn update_taskbar(
    container: &gtk4::Box,
    shared: &Arc<Mutex<SharedState>>,
    state: &Rc<RefCell<TaskbarState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
) {
    let shared_data = shared.lock().unwrap();
    let mut tb = state.borrow_mut();

    // Skip if nothing changed
    if shared_data.generation == tb.last_generation {
        return;
    }
    tb.last_generation = shared_data.generation;

    // Remove buttons for toplevels that no longer exist
    let stale_ids: Vec<u32> = tb
        .buttons
        .keys()
        .filter(|id| !shared_data.toplevels.contains_key(id))
        .cloned()
        .collect();
    for id in stale_ids {
        if let Some(btn) = tb.buttons.remove(&id) {
            container.remove(&btn);
        }
    }

    // Add/update buttons for current toplevels
    for (&id, info) in &shared_data.toplevels {
        // Skip toplevels with empty titles (not yet initialized)
        if info.title.is_empty() {
            continue;
        }

        let label = truncate_title(&info.title, 25);

        if let Some(btn) = tb.buttons.get(&id) {
            // Update existing button
            btn.set_label(&label);
            if info.is_activated {
                btn.add_css_class("active");
            } else {
                btn.remove_css_class("active");
            }
            if info.is_minimized {
                btn.add_css_class("minimized");
            } else {
                btn.remove_css_class("minimized");
            }
        } else {
            // Create new button
            let btn = gtk4::Button::with_label(&label);
            btn.add_css_class("taskbar-item");
            if info.is_activated {
                btn.add_css_class("active");
            }

            // Left click: activate
            let tx = action_tx.clone();
            btn.connect_clicked(move |_| {
                let _ = tx.send(ToplevelAction::Activate(id));
            });

            // Middle click: close
            let tx_close = action_tx.clone();
            let gesture = gtk4::GestureClick::new();
            gesture.set_button(2); // Middle mouse button
            gesture.connect_released(move |_, _, _, _| {
                let _ = tx_close.send(ToplevelAction::Close(id));
            });
            btn.add_controller(gesture);

            container.append(&btn);
            tb.buttons.insert(id, btn);
        }
    }
}

fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        let mut s: String = title.chars().take(max_len - 1).collect();
        s.push('\u{2026}'); // ellipsis
        s
    }
}
