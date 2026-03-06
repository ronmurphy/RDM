mod clock;
mod sni;
mod taskbar;
mod toplevel;
mod tray;
mod wifi;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use rdm_common::config::RdmConfig;
use rdm_common::theme::ThemeLayout;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn main() {
    env_logger::init();
    log::info!("Starting RDM Panel");

    if !is_rdm_session() {
        log::warn!(
            "Not starting rdm-panel: non-RDM desktop detected (RDM_SESSION={:?}, XDG_SESSION_TYPE={:?}, XDG_CURRENT_DESKTOP={:?}, XDG_SESSION_DESKTOP={:?}, DESKTOP_SESSION={:?})",
            env::var("RDM_SESSION").ok(),
            env::var("XDG_SESSION_TYPE").ok(),
            env::var("XDG_CURRENT_DESKTOP").ok(),
            env::var("XDG_SESSION_DESKTOP").ok(),
            env::var("DESKTOP_SESSION").ok()
        );
        return;
    }
    if !has_rdm_session_ancestor() {
        log::warn!(
            "Not starting rdm-panel: parent chain does not include rdm-session ({})",
            parent_chain_summary(16)
        );
        return;
    }
    if !toplevel::can_bind_foreign_toplevel_manager() {
        log::warn!("Not starting rdm-panel: compositor does not expose zwlr_foreign_toplevel_manager_v1");
        return;
    }

    let config = RdmConfig::load();

    let app = Application::builder()
        .application_id("org.rdm.panel")
        .build();

    let cfg = config.clone();
    app.connect_activate(move |app| build_panel(app, &cfg));
    app.run();
}

fn is_rdm_session() -> bool {
    let has_session_marker = env::var("RDM_SESSION")
        .ok()
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let is_wayland = env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|value| value.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false);

    let has_rdm_desktop_marker = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
        .iter()
        .any(|name| {
        env::var(name)
            .ok()
            .map(|value| {
                value
                    .split(':')
                    .any(|part| part.trim().eq_ignore_ascii_case("rdm"))
            })
            .unwrap_or(false)
    });

    has_session_marker && is_wayland && has_rdm_desktop_marker
}

fn has_rdm_session_ancestor() -> bool {
    parent_chain(16)
        .iter()
        .skip(1)
        .any(|(_, comm)| comm == "rdm-session")
}

fn parent_chain_summary(max_depth: usize) -> String {
    let chain = parent_chain(max_depth);
    if chain.is_empty() {
        return "unavailable".to_string();
    }
    chain
        .into_iter()
        .map(|(pid, comm)| format!("{pid}:{comm}"))
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn parent_chain(max_depth: usize) -> Vec<(u32, String)> {
    let mut chain = Vec::new();
    let mut pid = std::process::id();

    for _ in 0..max_depth {
        let Some(comm) = read_proc_comm(pid) else {
            break;
        };
        chain.push((pid, comm));

        let Some(ppid) = read_proc_ppid(pid) else {
            break;
        };
        if ppid == 0 || ppid == 1 || ppid == pid {
            break;
        }
        pid = ppid;
    }

    chain
}

fn read_proc_comm(pid: u32) -> Option<String> {
    let path = format!("/proc/{pid}/comm");
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn read_proc_ppid(pid: u32) -> Option<u32> {
    let path = format!("/proc/{pid}/stat");
    let stat = fs::read_to_string(path).ok()?;
    let (_, rest) = stat.split_once(") ")?;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse::<u32>().ok()
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
                let win = build_panel_window(app, config, &monitor, &shared_state, &action_tx);
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
    let layout = rdm_common::theme::load_active_theme_layout();
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

    // Main horizontal layout split into left/center/right zones.
    let center_box = gtk4::CenterBox::new();
    center_box.add_css_class("panel");

    let left_zone = gtk4::Box::new(Orientation::Horizontal, 4);
    let center_zone = gtk4::Box::new(Orientation::Horizontal, 4);
    let right_zone = gtk4::Box::new(Orientation::Horizontal, 4);

    center_zone.set_halign(gtk4::Align::Center);
    center_zone.set_hexpand(true);
    right_zone.set_halign(gtk4::Align::End);

    center_box.set_start_widget(Some(&left_zone));
    center_box.set_center_widget(Some(&center_zone));
    center_box.set_end_widget(Some(&right_zone));

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
            Err(e) => {
                log::error!("Failed to launch rdm-launcher: {}", e);
                let _ = std::process::Command::new("dbus-send")
                    .args([
                        "--session",
                        "--dest=org.freedesktop.Notifications",
                        "--type=method_call",
                        "/org/freedesktop/Notifications",
                        "org.freedesktop.Notifications.Notify",
                        "string:rdm-panel",
                        "uint32:0",
                        "string:",
                        "string:Launcher failed",
                        &format!("string:{}", e),
                        "array:string:",
                        "dict:string:variant:",
                        "int32:4000",
                    ])
                    .status();
            }
        }
    });
    // Center: taskbar (running windows) — uses shared toplevel tracker
    let taskbar_box = gtk4::Box::new(Orientation::Horizontal, 4);
    taskbar_box.add_css_class("taskbar");
    taskbar_box.set_halign(gtk4::Align::Center);
    let mode = taskbar::TaskbarMode::from_str(&config.panel.taskbar_mode);
    taskbar::setup_taskbar_with_shared(&taskbar_box, mode, shared_state, action_tx);

    // Right: clock (with calendar popover), optional.
    let clock_widget = if config.panel.show_clock {
        Some(clock::build_clock_widget(&config.panel.clock_format))
    } else {
        None
    };

    // Right: unified tray area — SNI app icons on the left, battery/power on the right.
    let sni_tray = sni::setup_sni_tray();
    let tray = tray::setup_tray(app, mode);
    let tray_area = gtk4::Box::new(Orientation::Horizontal, 0);
    tray_area.add_css_class("tray-area");
    tray_area.append(&sni_tray);
    tray_area.append(&tray);

    append_panel_widget(
        &layout,
        "launcher",
        &launcher_btn,
        &left_zone,
        &center_zone,
        &right_zone,
    );
    append_panel_widget(
        &layout,
        "taskbar",
        &taskbar_box,
        &left_zone,
        &center_zone,
        &right_zone,
    );
    if let Some(clock_widget) = clock_widget.as_ref() {
        append_panel_widget(
            &layout,
            "clock",
            clock_widget,
            &left_zone,
            &center_zone,
            &right_zone,
        );
    }
    append_panel_widget(
        &layout,
        "tray",
        &tray_area,
        &left_zone,
        &center_zone,
        &right_zone,
    );

    window.set_child(Some(&center_box));

    window.present();
    window
}

fn append_panel_widget<W: IsA<gtk4::Widget>>(
    layout: &ThemeLayout,
    role: &str,
    widget: &W,
    left_zone: &gtk4::Box,
    center_zone: &gtk4::Box,
    right_zone: &gtk4::Box,
) {
    let target = match role {
        "launcher" => layout.panel.launcher.as_str(),
        "taskbar" => layout.panel.taskbar.as_str(),
        "clock" => layout.panel.clock.as_str(),
        "tray" => layout.panel.tray.as_str(),
        _ => "left",
    };
    match target {
        "center" => center_zone.append(widget),
        "right" => right_zone.append(widget),
        _ => left_zone.append(widget),
    }
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(&rdm_common::theme::load_theme_css());

    // Priority 801 beats the user's ~/.config/gtk-4.0/gtk.css (loaded at 800)
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}
