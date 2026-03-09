use gtk4::prelude::*;
use gtk4::Orientation;
use rdm_common::config::{DockConfig, DockPin, RdmConfig};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::toplevel::{SharedState, ToplevelAction};

// ─── Display mode ─────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum DockMode {
    Icons,
    Text,
    Nerd,
}

impl DockMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "text" => Self::Text,
            "nerd" => Self::Nerd,
            _ => Self::Icons,
        }
    }
}

// ─── Data model ───────────────────────────────────────────────────

#[derive(Clone)]
struct DockSlot {
    name: String,
    exec: String,
    icon: String,
    pinned: bool,
}

struct DockState {
    pinned: Vec<DockPin>,
    last_generation: u64,
}

// ─── Public entry point ────────────────────────────────────────────

pub fn build_dock(
    bar: &gtk4::Box,
    config: &DockConfig,
    mode: DockMode,
    shared: &Arc<Mutex<SharedState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
) -> Box<dyn Fn()> {
    let icon_size = config.icon_size;

    // ── Permanent rdm-launcher pin (left of separator) ──
    let launcher_pin = build_launcher_pin(icon_size, mode);
    bar.append(&launcher_pin);

    // ── Vertical separator ──
    let sep = gtk4::Separator::new(Orientation::Vertical);
    sep.add_css_class("dock-separator");
    sep.set_margin_top(10);
    sep.set_margin_bottom(10);
    sep.set_margin_start(4);
    sep.set_margin_end(4);
    bar.append(&sep);

    // ── Dynamic slot zone — only this gets cleared each tick ──
    let slot_zone = gtk4::Box::new(Orientation::Horizontal, 0);
    slot_zone.add_css_class("dock-slots");
    bar.append(&slot_zone);

    let state = Rc::new(RefCell::new(DockState {
        pinned: config.pins.clone(),
        last_generation: u64::MAX,
    }));

    let shared = shared.clone();
    let action_tx = action_tx.clone();

    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        update_dock(&slot_zone, &shared, &state, &action_tx, icon_size, mode);
        gtk4::glib::ControlFlow::Continue
    });

    // ── Clock (right end) ──
    let sep2 = gtk4::Separator::new(Orientation::Vertical);
    sep2.add_css_class("dock-separator");
    sep2.set_margin_top(10);
    sep2.set_margin_bottom(10);
    sep2.set_margin_start(4);
    sep2.set_margin_end(4);
    bar.append(&sep2);

    build_clock(bar)
}

// ─── Permanent launcher pin ───────────────────────────────────────

fn build_launcher_pin(icon_size: i32, mode: DockMode) -> gtk4::Box {
    let root = gtk4::Box::new(Orientation::Vertical, 0);
    root.add_css_class("dock-item");
    root.add_css_class("dock-launcher-pin");
    root.set_halign(gtk4::Align::Center);

    let btn = gtk4::Button::new();
    btn.add_css_class("dock-btn");
    btn.set_tooltip_text(Some("Launcher"));

    match mode {
        DockMode::Icons => {
            btn.set_child(Some(&make_icon_image("rdm-launcher", icon_size)));
        }
        DockMode::Text => {
            btn.set_label("Launcher");
        }
        DockMode::Nerd => {
            let lbl = gtk4::Label::new(Some("\u{f135}"));
            lbl.add_css_class("nerd-icon");
            btn.set_child(Some(&lbl));
        }
    }

    btn.connect_clicked(|_| {
        let _ = std::process::Command::new("rdm-launcher")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });

    // Right-click: context menu with Quit Dock
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let btn_ref = btn.clone();
    gesture.connect_released(move |_, _, _, _| {
        let popover = gtk4::Popover::new();
        popover.set_parent(&btn_ref);
        popover.set_autohide(true);

        let quit_btn = gtk4::Button::with_label("Quit Dock");
        quit_btn.set_margin_top(4);
        quit_btn.set_margin_bottom(4);
        quit_btn.set_margin_start(8);
        quit_btn.set_margin_end(8);
        let popover_ref = popover.clone();
        quit_btn.connect_clicked(move |_| {
            popover_ref.popdown();
            std::process::exit(0);
        });

        popover.set_child(Some(&quit_btn));
        popover.popup();
    });
    btn.add_controller(gesture);

    root.append(&btn);
    // No dots row — the launcher pin isn't a tracked window
    root
}

// ─── Poll / update slot zone ──────────────────────────────────────

fn update_dock(
    slot_zone: &gtk4::Box,
    shared: &Arc<Mutex<SharedState>>,
    state: &Rc<RefCell<DockState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
    icon_size: i32,
    mode: DockMode,
) {
    let (gen, toplevels) = {
        let s = shared.lock().unwrap();
        if s.generation == state.borrow().last_generation {
            return;
        }
        (s.generation, s.toplevels.clone())
    };

    state.borrow_mut().last_generation = gen;

    // Build ordered slot list: pinned first, then transient running-only
    let pinned = state.borrow().pinned.clone();
    let mut slots: Vec<DockSlot> = pinned
        .iter()
        .map(|p| DockSlot {
            name: p.name.clone(),
            exec: p.exec.clone(),
            icon: p.icon.clone(),
            pinned: true,
        })
        .collect();

    for info in toplevels.values() {
        if info.app_id.is_empty() {
            continue;
        }
        let covered = slots.iter().any(|s| matches_app(&s.exec, &info.app_id));
        if !covered {
            slots.push(DockSlot {
                name: display_name_for(&info.app_id),
                exec: info.app_id.clone(),
                icon: resolve_icon_name(&info.app_id),
                pinned: false,
            });
        }
    }

    // Full rebuild of slot zone
    while let Some(child) = slot_zone.first_child() {
        slot_zone.remove(&child);
    }

    for slot in &slots {
        let running_ids: Vec<u32> = toplevels
            .iter()
            .filter(|(_, info)| matches_app(&slot.exec, &info.app_id))
            .map(|(&id, _)| id)
            .collect();

        let is_running = !running_ids.is_empty();
        let is_activated = running_ids
            .iter()
            .any(|id| toplevels.get(id).map(|i| i.is_activated).unwrap_or(false));

        let widget = create_slot_widget(
            slot,
            icon_size,
            mode,
            is_running,
            is_activated,
            running_ids.len().min(3),
            shared,
            action_tx,
            state,
        );
        slot_zone.append(&widget);
    }
}

// ─── Slot widget ──────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn create_slot_widget(
    slot: &DockSlot,
    icon_size: i32,
    mode: DockMode,
    is_running: bool,
    is_activated: bool,
    dot_count: usize,
    shared: &Arc<Mutex<SharedState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
    state: &Rc<RefCell<DockState>>,
) -> gtk4::Box {
    let root = gtk4::Box::new(Orientation::Vertical, 0);
    root.add_css_class("dock-item");
    root.set_halign(gtk4::Align::Center);

    let btn = gtk4::Button::new();
    btn.add_css_class("dock-btn");
    btn.set_tooltip_text(Some(&slot.name));

    // Render according to mode
    match mode {
        DockMode::Icons => {
            btn.set_child(Some(&make_icon_image(&slot.icon, icon_size)));
        }
        DockMode::Text => {
            btn.set_label(&slot.name);
        }
        DockMode::Nerd => {
            let glyph = nerd_glyph_for(&slot.exec);
            let lbl = gtk4::Label::new(Some(&glyph));
            lbl.add_css_class("nerd-icon");
            btn.set_child(Some(&lbl));
        }
    }

    if is_running {
        btn.add_css_class("running");
    }
    if is_activated {
        btn.add_css_class("active-window");
    }

    // Dots row
    let dots = gtk4::Box::new(Orientation::Horizontal, 0);
    dots.add_css_class("dock-dots");
    dots.set_halign(gtk4::Align::Center);
    for _ in 0..dot_count {
        let dot = gtk4::Box::new(Orientation::Horizontal, 0);
        dot.add_css_class("dock-dot");
        dots.append(&dot);
    }

    root.append(&btn);
    root.append(&dots);

    // ── Left click: launch or toggle ──
    let exec = slot.exec.clone();
    let shared_click = shared.clone();
    let tx = action_tx.clone();
    btn.connect_clicked(move |_| {
        let running: Vec<u32> = {
            let s = shared_click.lock().unwrap();
            s.toplevels
                .iter()
                .filter(|(_, info)| matches_app(&exec, &info.app_id))
                .map(|(&id, _)| id)
                .collect()
        };
        if running.is_empty() {
            let _ = std::process::Command::new(&exec)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        } else {
            let _ = tx.send(ToplevelAction::Toggle(running[0]));
        }
    });

    // ── Right click: pin / unpin ──
    let is_pinned = slot.pinned;
    let exec_rc = slot.exec.clone();
    let name_rc = slot.name.clone();
    let icon_rc = slot.icon.clone();
    let state_rc = state.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let btn_ref = btn.clone();
    gesture.connect_released(move |_, _, _, _| {
        show_context_menu(&btn_ref, is_pinned, &exec_rc, &name_rc, &icon_rc, &state_rc);
    });
    btn.add_controller(gesture);

    root
}

// ─── Context menu ─────────────────────────────────────────────────

fn show_context_menu(
    parent: &gtk4::Button,
    is_pinned: bool,
    exec: &str,
    name: &str,
    icon: &str,
    state: &Rc<RefCell<DockState>>,
) {
    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    popover.set_autohide(true);

    let vbox = gtk4::Box::new(Orientation::Vertical, 2);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);

    let launch_btn = gtk4::Button::with_label("Launch");
    let exec_launch = exec.to_string();
    launch_btn.connect_clicked(move |_| {
        let _ = std::process::Command::new(&exec_launch)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });
    vbox.append(&launch_btn);

    let pin_label = if is_pinned { "Unpin from Dock" } else { "Pin to Dock" };
    let pin_btn = gtk4::Button::with_label(pin_label);

    let exec_pin = exec.to_string();
    let name_pin = name.to_string();
    let icon_pin = icon.to_string();
    let state_pin = state.clone();
    let popover_ref = popover.clone();
    pin_btn.connect_clicked(move |_| {
        popover_ref.popdown();
        toggle_pin(&exec_pin, &name_pin, &icon_pin, is_pinned, &state_pin);
    });
    vbox.append(&pin_btn);

    popover.set_child(Some(&vbox));
    popover.popup();
}

fn toggle_pin(
    exec: &str,
    name: &str,
    icon: &str,
    currently_pinned: bool,
    state: &Rc<RefCell<DockState>>,
) {
    let mut config = RdmConfig::load();

    if currently_pinned {
        config.dock.pins.retain(|p| p.exec != exec);
        log::info!("Unpinned '{}' from dock", name);
    } else {
        config.dock.pins.push(DockPin {
            name: name.to_string(),
            exec: exec.to_string(),
            icon: icon.to_string(),
        });
        log::info!("Pinned '{}' to dock", name);
    }

    if let Err(e) = config.save() {
        log::error!("Failed to save dock pins: {}", e);
        return;
    }

    let mut ds = state.borrow_mut();
    ds.pinned = config.dock.pins.clone();
    ds.last_generation = u64::MAX;
}

// ─── Nerd font glyphs ─────────────────────────────────────────────

fn nerd_glyph_for(app_id: &str) -> String {
    let s = app_id.to_lowercase();
    let glyph = match s.as_str() {
        s if s.contains("firefox")  => "\u{f269}",
        s if s.contains("chrome") || s.contains("chromium") => "\u{f268}",
        s if s.contains("brave")    => "\u{f39f}",
        s if s.contains("foot") || s.contains("kitty") || s.contains("alacritty")
          || s.contains("terminal") || s.contains("wezterm") || s.contains("konsole") => "\u{f489}",
        s if s.contains("code") || s.contains("vscode")  => "\u{e70c}",
        s if s.contains("neovim") || s.contains("nvim") || s.contains("vim") => "\u{e62b}",
        s if s.contains("emacs")    => "\u{e632}",
        s if s.contains("thunar") || s.contains("nautilus") || s.contains("dolphin")
          || s.contains("files") || s.contains("pcmanfm") => "\u{f413}",
        s if s.contains("spotify")  => "\u{f1bc}",
        s if s.contains("vlc") || s.contains("mpv") => "\u{f40a}",
        s if s.contains("discord")  => "\u{f392}",
        s if s.contains("telegram") => "\u{f2c6}",
        s if s.contains("slack")    => "\u{f198}",
        s if s.contains("steam")    => "\u{f1b6}",
        s if s.contains("gimp") || s.contains("inkscape") || s.contains("blender") => "\u{e69e}",
        s if s.contains("obs")      => "\u{f03d}",
        s if s.contains("settings") || s.contains("control") => "\u{f013}",
        _ => "\u{f2d0}",
    };
    glyph.to_string()
}

// ─── Icon loading ─────────────────────────────────────────────────

/// Create an Image widget for the given icon name at the given pixel size.
/// Tries (in order):
///   1. SVG file in /usr/share/icons/hicolor/scalable/apps/<name>.svg
///   2. SVG file in /usr/local/share/icons/hicolor/scalable/apps/<name>.svg
///   3. GTK icon theme lookup by name
///   4. Generic fallback icon
fn make_icon_image(icon_name: &str, size: i32) -> gtk4::Image {
    let svg_dirs = [
        "/usr/share/icons/hicolor/scalable/apps",
        "/usr/local/share/icons/hicolor/scalable/apps",
    ];
    for dir in &svg_dirs {
        let path = format!("{}/{}.svg", dir, icon_name);
        if std::path::Path::new(&path).exists() {
            let img = gtk4::Image::from_file(&path);
            img.set_pixel_size(size);
            return img;
        }
    }

    // Theme lookup
    let img = gtk4::Image::from_icon_name(icon_name);
    img.set_pixel_size(size);
    img
}

// ─── Helpers ──────────────────────────────────────────────────────

fn matches_app(exec: &str, app_id: &str) -> bool {
    if exec.is_empty() || app_id.is_empty() {
        return false;
    }
    let e = exec.to_lowercase();
    let a = app_id.to_lowercase();
    let a_base = a.rsplit('.').next().unwrap_or(&a);
    let e_base = e.rsplit('/').last().unwrap_or(&e);
    e_base == a_base || a_base.contains(e_base) || e_base.contains(a_base)
}

fn resolve_icon_name(app_id: &str) -> String {
    let display = gtk4::gdk::Display::default();
    let theme = display
        .as_ref()
        .map(gtk4::IconTheme::for_display)
        .unwrap_or_else(gtk4::IconTheme::new);

    let base = app_id.rsplit('.').next().unwrap_or(app_id);
    let candidates = [
        app_id.to_string(),
        app_id.to_lowercase(),
        base.to_string(),
        base.to_lowercase(),
        format!("rdm-{}", base.to_lowercase()),
    ];

    for name in &candidates {
        if theme.has_icon(name) {
            return name.clone();
        }
    }

    let mapped = match app_id.to_lowercase().as_str() {
        s if s.contains("firefox")  => "firefox",
        s if s.contains("chrome") || s.contains("chromium") => "chromium",
        s if s.contains("code") || s.contains("vscode") => "visual-studio-code",
        s if s.contains("terminal") || s.contains("foot")
          || s.contains("kitty") || s.contains("alacritty") => "utilities-terminal",
        s if s.contains("thunar") || s.contains("nautilus")
          || s.contains("dolphin") || s.contains("files") => "system-file-manager",
        s if s.contains("discord") => "discord",
        s if s.contains("spotify") => "spotify",
        s if s.contains("steam")   => "steam",
        s if s.contains("gimp")    => "gimp",
        s if s.contains("vlc")     => "vlc",
        _ => "application-x-executable",
    };

    if theme.has_icon(mapped) {
        mapped.to_string()
    } else {
        "application-x-executable".to_string()
    }
}

// ─── Clock ────────────────────────────────────────────────────────

fn build_clock(bar: &gtk4::Box) -> Box<dyn Fn()> {
    let time_label = gtk4::Label::new(None);
    time_label.add_css_class("dock-clock-time");

    let date_label = gtk4::Label::new(None);
    date_label.add_css_class("dock-clock-date");

    let vbox = gtk4::Box::new(Orientation::Vertical, 0);
    vbox.add_css_class("dock-clock");
    vbox.set_valign(gtk4::Align::Center);
    vbox.set_margin_start(8);
    vbox.set_margin_end(8);
    vbox.append(&time_label);
    vbox.append(&date_label);
    bar.append(&vbox);

    let use_24h = Rc::new(Cell::new(false));

    update_clock(&time_label, &date_label, use_24h.get());

    // Clone before the gesture closure moves use_24h — both Rcs point to the
    // same Cell, so a mode change via the menu is immediately visible here too.
    let use_24h_refresh = use_24h.clone();
    let time_refresh = time_label.clone();
    let date_refresh = date_label.clone();

    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let time_click = time_label.clone();
    let date_click = date_label.clone();
    let vbox_click = vbox.clone();
    gesture.connect_released(move |_, _, _, _| {
        show_clock_menu(&vbox_click, &time_click, &date_click, &use_24h);
    });
    vbox.add_controller(gesture);

    Box::new(move || update_clock(&time_refresh, &date_refresh, use_24h_refresh.get()))
}

fn update_clock(time_label: &gtk4::Label, date_label: &gtk4::Label, use_24h: bool) {
    let now = match gtk4::glib::DateTime::now_local() {
        Ok(dt) => dt,
        Err(_) => return,
    };
    let time_fmt = if use_24h { "%H:%M" } else { "%l:%M %p" };
    if let Ok(t) = now.format(time_fmt) {
        time_label.set_text(t.trim());
    }
    if let Ok(d) = now.format("%b %e") {
        date_label.set_text(d.trim());
    }
}

fn show_clock_menu(
    parent: &gtk4::Box,
    time_label: &gtk4::Label,
    date_label: &gtk4::Label,
    use_24h: &Rc<Cell<bool>>,
) {
    let popover = gtk4::Popover::new();
    popover.set_parent(parent);
    popover.set_autohide(true);

    let vbox = gtk4::Box::new(Orientation::Vertical, 2);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);

    let btn_12h = gtk4::Button::with_label("12-hour");
    let btn_24h = gtk4::Button::with_label("24-hour");

    let use_24h_12 = use_24h.clone();
    let time_12 = time_label.clone();
    let date_12 = date_label.clone();
    let pop_12 = popover.clone();
    btn_12h.connect_clicked(move |_| {
        use_24h_12.set(false);
        update_clock(&time_12, &date_12, false);
        pop_12.popdown();
    });

    let use_24h_24 = use_24h.clone();
    let time_24 = time_label.clone();
    let date_24 = date_label.clone();
    let pop_24 = popover.clone();
    btn_24h.connect_clicked(move |_| {
        use_24h_24.set(true);
        update_clock(&time_24, &date_24, true);
        pop_24.popdown();
    });

    vbox.append(&btn_12h);
    vbox.append(&btn_24h);
    popover.set_child(Some(&vbox));
    popover.popup();
}

fn display_name_for(app_id: &str) -> String {
    let base = app_id.rsplit('.').next().unwrap_or(app_id);
    let mut chars = base.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}
