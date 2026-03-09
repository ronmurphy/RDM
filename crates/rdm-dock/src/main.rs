mod dock;
mod toplevel;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use rdm_common::config::RdmConfig;
use std::cell::RefCell;
use std::env;
use std::fs;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("rdm_dock=info"))
        .init();
    log::info!("Starting RDM Dock");

    if !is_rdm_session() {
        log::warn!("Not starting rdm-dock: non-RDM desktop detected");
        return;
    }
    if !has_rdm_session_ancestor() {
        log::warn!("Not starting rdm-dock: parent chain does not include rdm-session");
        return;
    }
    if !toplevel::can_bind_foreign_toplevel_manager() {
        log::warn!(
            "Not starting rdm-dock: compositor does not expose zwlr_foreign_toplevel_manager_v1"
        );
        return;
    }

    let config = RdmConfig::load();

    if !config.dock.enabled {
        log::info!("Dock disabled in config, exiting");
        return;
    }

    let app = Application::builder()
        .application_id("org.rdm.dock")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    let cfg = config.clone();
    app.connect_activate(move |app| build_dock(app, &cfg));
    app.run();
}

fn build_dock(app: &Application, config: &RdmConfig) {
    let display = gtk4::gdk::Display::default().expect("No display");
    let monitors = display.monitors();

    let (shared_state, action_tx) = toplevel::start_toplevel_tracker();
    let action_tx = Rc::new(action_tx);

    for i in 0..monitors.n_items() {
        if let Some(obj) = monitors.item(i) {
            if let Ok(monitor) = obj.downcast::<gtk4::gdk::Monitor>() {
                let connector = monitor
                    .connector()
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| format!("unknown-{}", i));
                log::info!("Creating dock for monitor: {}", connector);
                build_dock_window(app, config, &monitor, &shared_state, &action_tx);
            }
        }
    }

    load_css();
    log::info!("Dock initialized");
}

fn build_dock_window(
    app: &Application,
    config: &RdmConfig,
    monitor: &gtk4::gdk::Monitor,
    shared_state: &Arc<Mutex<toplevel::SharedState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<toplevel::ToplevelAction>>,
) -> ApplicationWindow {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Dock")
        .build();

    window.add_css_class("rdm-dock");

    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_namespace("rdm-dock");
    window.set_monitor(monitor);

    // Anchor only to bottom — centers the dock horizontally on this monitor
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Left, false);
    window.set_anchor(Edge::Right, false);

    // Small gap from screen edge — never changes
    window.set_margin(Edge::Bottom, 8);

    // No exclusive zone — dock floats above windows like macOS
    window.set_exclusive_zone(0);

    // Horizontal bar of dock icons
    let bar = gtk4::Box::new(Orientation::Horizontal, 0);
    bar.add_css_class("dock-bar");
    bar.set_halign(gtk4::Align::Center);
    bar.set_valign(gtk4::Align::End);

    let mode = dock::DockMode::from_str(&config.panel.taskbar_mode);
    dock::build_dock(&bar, &config.dock, mode, shared_state, action_tx);

    window.set_child(Some(&bar));
    window.present();

    setup_autohide(&window, &bar);

    window
}

// ─── Auto-hide ────────────────────────────────────────────────────
//
// Strategy: never move the window off-screen (avoids multi-monitor edge
// issues where the compositor clips at output boundaries).  Instead:
//
//   Visible   — bar at natural height, window opacity = 1.0
//   Hiding    — bar shrinks toward HIDDEN_PX, opacity fades to 0.0
//   Hidden    — bar at HIDDEN_PX, opacity = 0.0
//               The surface is still present and receives pointer events,
//               giving a 16-px invisible hit-zone flush with the monitor edge.
//   Showing   — bar grows back, opacity fades to 1.0
//
// No sliding, no activation strip, no negative margins.

/// Height (px) the bar collapses to when hidden.  Invisible but still
/// receives pointer-enter events.
const HIDDEN_PX: i32 = 16;

/// Frames to complete a full show/hide transition at ~60 fps.
const ANIM_FRAMES: f64 = 18.0;

struct AutoHideState {
    /// 0.0 = fully visible, 1.0 = fully hidden.
    position: f64,
    hovered: bool,
    last_leave: Option<Instant>,
    hide_delay: Duration,
    /// Natural (full) height of the bar — captured once the surface is realised.
    natural_height: i32,
}

impl AutoHideState {
    /// Bar height_request for the current animated position.
    fn bar_height(&self) -> i32 {
        let full = self.natural_height as f64;
        let small = HIDDEN_PX as f64;
        (full + (small - full) * self.position).round() as i32
    }

    /// Window opacity for the current animated position.
    fn opacity(&self) -> f64 {
        // Fade out quickly (leading half of the travel), so the dock
        // disappears before it finishes shrinking.
        (1.0 - self.position * 2.0).clamp(0.0, 1.0)
    }
}

fn setup_autohide(window: &ApplicationWindow, bar: &gtk4::Box) {
    let state: Rc<RefCell<AutoHideState>> = Rc::new(RefCell::new(AutoHideState {
        position: 0.0,
        hovered: false,
        last_leave: None,
        hide_delay: Duration::from_millis(2000),
        natural_height: 0, // filled in on first tick after realise
    }));

    // ── Pointer tracking ──────────────────────────────────────────
    {
        let s = state.clone();
        let motion = gtk4::EventControllerMotion::new();
        motion.connect_enter(move |_, _, _| {
            let mut st = s.borrow_mut();
            st.hovered = true;
            st.last_leave = None;
        });
        let s2 = state.clone();
        motion.connect_leave(move |_| {
            let mut st = s2.borrow_mut();
            st.hovered = false;
            st.last_leave = Some(Instant::now());
        });
        window.add_controller(motion);
    }

    // ── Animation tick ────────────────────────────────────────────
    let win_weak = window.downgrade();
    let bar_weak = bar.downgrade();

    gtk4::glib::timeout_add_local(Duration::from_millis(16), move || {
        let (Some(win), Some(bar)) = (win_weak.upgrade(), bar_weak.upgrade()) else {
            return gtk4::glib::ControlFlow::Break;
        };

        let mut s = state.borrow_mut();

        // Capture natural height once the widget is laid out
        if s.natural_height == 0 {
            let h = bar.height();
            if h > HIDDEN_PX {
                s.natural_height = h;
            } else {
                // Not yet realised — nothing to animate yet
                return gtk4::glib::ControlFlow::Continue;
            }
        }

        // Decide direction
        let should_hide = !s.hovered
            && s.last_leave
                .map(|t| t.elapsed() >= s.hide_delay)
                .unwrap_or(false);
        let target: f64 = if should_hide { 1.0 } else { 0.0 };

        let delta = target - s.position;
        let step = 1.0 / ANIM_FRAMES;
        if delta.abs() <= step {
            s.position = target;
        } else {
            s.position += delta.signum() * step;
        }

        bar.set_height_request(s.bar_height());
        win.set_opacity(s.opacity());

        gtk4::glib::ControlFlow::Continue
    });
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(&rdm_common::theme::load_theme_css());
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}

// ─── Session guards (mirrors rdm-panel) ───────────────────────────

fn is_rdm_session() -> bool {
    let has_session_marker = env::var("RDM_SESSION")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let is_wayland = env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|v| v.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false);

    let has_rdm_desktop = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
        .iter()
        .any(|name| {
            env::var(name)
                .ok()
                .map(|v| v.split(':').any(|p| p.trim().eq_ignore_ascii_case("rdm")))
                .unwrap_or(false)
        });

    has_session_marker && is_wayland && has_rdm_desktop
}

fn has_rdm_session_ancestor() -> bool {
    parent_chain(16)
        .iter()
        .skip(1)
        .any(|(_, comm)| comm == "rdm-session")
}

fn parent_chain(max_depth: usize) -> Vec<(u32, String)> {
    let mut chain = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..max_depth {
        let Some(comm) = read_proc_comm(pid) else { break };
        chain.push((pid, comm));
        let Some(ppid) = read_proc_ppid(pid) else { break };
        if ppid == 0 || ppid == 1 || ppid == pid {
            break;
        }
        pid = ppid;
    }
    chain
}

fn read_proc_comm(pid: u32) -> Option<String> {
    fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
}

fn read_proc_ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, rest) = stat.split_once(") ")?;
    let mut fields = rest.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse::<u32>().ok()
}
