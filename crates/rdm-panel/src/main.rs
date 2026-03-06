mod clock;
mod taskbar;
mod toplevel;
mod tray;
mod wifi;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use rdm_common::config::RdmConfig;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn main() {
    env_logger::init();
    log::info!("Starting RDM Panel");

    let config = RdmConfig::load();

    let app = Application::builder()
        .application_id("org.rdm.panel")
        .build();

    let cfg = config.clone();
    app.connect_activate(move |app| build_panel(app, &cfg));
    app.run();
}

fn build_panel(app: &Application, config: &RdmConfig) {
    let display = gtk4::gdk::Display::default().expect("No display");
    let monitors = display.monitors();

    // Start toplevel tracker ONCE, shared across all panel windows
    let (shared_state, action_tx) = toplevel::start_toplevel_tracker();
    let action_tx = Rc::new(action_tx);

    // Track active panel windows by connector name
    let windows: Rc<std::cell::RefCell<HashMap<String, ApplicationWindow>>> =
        Rc::new(std::cell::RefCell::new(HashMap::new()));

    // Create panel for each connected monitor
    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gtk4::gdk::Monitor>() {
                let connector = monitor
                    .connector()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("unknown-{}", i));
                log::info!("Creating panel for monitor: {}", connector);
                let win =
                    build_panel_window(app, config, &monitor, &shared_state, &action_tx);
                windows.borrow_mut().insert(connector, win);
            }
        }
    }

    // Load CSS once for all windows
    load_css();

    log::info!(
        "Panel initialized with {} monitor(s)",
        windows.borrow().len()
    );
}

fn build_panel_window(
    app: &Application,
    config: &RdmConfig,
    monitor: &gtk4::gdk::Monitor,
    shared_state: &Arc<Mutex<toplevel::SharedState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<toplevel::ToplevelAction>>,
) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Panel")
        .default_width(0)
        .default_height(config.panel.height)
        .build();

    // Set up layer shell
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_namespace("rdm-panel");

    // Pin this window to the specific monitor
    window.set_monitor(monitor);

    // Anchor to edges based on position
    let at_top = config.panel.position == "top";
    window.set_anchor(Edge::Left, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, at_top);
    window.set_anchor(Edge::Bottom, !at_top);

    window.auto_exclusive_zone_enable();

    // Main horizontal layout
    let hbox = gtk4::Box::new(Orientation::Horizontal, 0);
    hbox.add_css_class("panel");

    // Left: launcher button → spawns rdm-launcher
    let launcher_btn = gtk4::Button::with_label("  Apps  ");
    launcher_btn.add_css_class("launcher-btn");
    launcher_btn.connect_clicked(|_| {
        match std::process::Command::new("rdm-launcher")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => log::error!("Failed to launch rdm-launcher: {}", e),
        }
    });
    hbox.append(&launcher_btn);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Vertical);
    hbox.append(&sep);

    // Center: taskbar (running windows) — uses shared toplevel tracker
    let taskbar_box = gtk4::Box::new(Orientation::Horizontal, 4);
    taskbar_box.set_hexpand(true);
    taskbar_box.set_halign(gtk4::Align::Start);
    taskbar_box.add_css_class("taskbar");
    let mode = taskbar::TaskbarMode::from_str(&config.panel.taskbar_mode);
    taskbar::setup_taskbar_with_shared(&taskbar_box, mode, shared_state, action_tx);
    hbox.append(&taskbar_box);

    // Right: clock (with calendar popover)
    if config.panel.show_clock {
        let clock_widget = clock::build_clock_widget(&config.panel.clock_format);
        hbox.append(&clock_widget);
    }

    // Right: system tray (battery + power menu)
    let tray = tray::setup_tray(app);
    hbox.append(&tray);

    window.set_child(Some(&hbox));

    window.present();
    window
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(&rdm_common::theme::load_theme_css());

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
