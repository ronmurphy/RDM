use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::toplevel::{SharedState, ToplevelAction, ToplevelInfo};

/// Taskbar display mode
#[derive(Clone, Copy, PartialEq)]
pub enum TaskbarMode {
    Text,
    Icons,
    Nerd,
}

impl TaskbarMode {
    pub fn from_str(s: &str) -> Self {
        match s {
            "icons" => Self::Icons,
            "nerd" => Self::Nerd,
            _ => Self::Text,
        }
    }
}

enum TaskbarWidget {
    TextButton(gtk4::Button),
    IconButton(gtk4::Button, gtk4::Image),
    NerdButton(gtk4::Button),
}

impl TaskbarWidget {
    fn button(&self) -> &gtk4::Button {
        match self {
            Self::TextButton(b) | Self::IconButton(b, _) | Self::NerdButton(b) => b,
        }
    }
}

struct TaskbarState {
    widgets: HashMap<u32, TaskbarWidget>,
    last_generation: u64,
}

/// Set up the taskbar: starts the Wayland toplevel tracker and polls it
/// from the GTK main loop to update buttons.
pub fn setup_taskbar(container: &gtk4::Box, mode: TaskbarMode) {
    let (shared, action_tx) = crate::toplevel::start_toplevel_tracker();
    setup_taskbar_with_shared(container, mode, &shared, &Rc::new(action_tx));
}

/// Set up taskbar using an externally-provided toplevel tracker.
/// This allows multiple panel windows to share one Wayland tracker thread.
pub fn setup_taskbar_with_shared(
    container: &gtk4::Box,
    mode: TaskbarMode,
    shared: &Arc<Mutex<SharedState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
) {
    let state = Rc::new(RefCell::new(TaskbarState {
        widgets: HashMap::new(),
        last_generation: 0,
    }));

    let container = container.clone();
    let action_tx = action_tx.clone();
    let shared = shared.clone();

    // Poll the shared state every 250ms
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        update_taskbar(&container, &shared, &state, &action_tx, mode);
        gtk4::glib::ControlFlow::Continue
    });

    log::info!("Taskbar initialized (mode: {:?})", match mode {
        TaskbarMode::Text => "text",
        TaskbarMode::Icons => "icons",
        TaskbarMode::Nerd => "nerd",
    });
}

fn update_taskbar(
    container: &gtk4::Box,
    shared: &Arc<Mutex<SharedState>>,
    state: &Rc<RefCell<TaskbarState>>,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
    mode: TaskbarMode,
) {
    let shared_data = shared.lock().unwrap();
    let mut tb = state.borrow_mut();

    // Skip if nothing changed
    if shared_data.generation == tb.last_generation {
        return;
    }
    tb.last_generation = shared_data.generation;

    // Remove widgets for toplevels that no longer exist
    let stale_ids: Vec<u32> = tb
        .widgets
        .keys()
        .filter(|id| !shared_data.toplevels.contains_key(id))
        .cloned()
        .collect();
    for id in stale_ids {
        if let Some(w) = tb.widgets.remove(&id) {
            container.remove(w.button());
        }
    }

    // Add/update widgets for current toplevels
    for (&id, info) in &shared_data.toplevels {
        if info.title.is_empty() {
            continue;
        }

        if let Some(w) = tb.widgets.get(&id) {
            update_existing_widget(w, info, mode);
        } else {
            let w = create_widget(info, mode, id, action_tx);
            container.append(w.button());
            tb.widgets.insert(id, w);
        }
    }
}

fn update_existing_widget(w: &TaskbarWidget, info: &ToplevelInfo, mode: TaskbarMode) {
    let btn = w.button();

    match (mode, w) {
        (TaskbarMode::Text, TaskbarWidget::TextButton(b)) => {
            b.set_label(&truncate_title(&info.title, 25));
        }
        (TaskbarMode::Icons, TaskbarWidget::IconButton(_, img)) => {
            let icon_name = resolve_icon_name(&info.app_id);
            img.set_icon_name(Some(&icon_name));
        }
        (TaskbarMode::Nerd, TaskbarWidget::NerdButton(b)) => {
            b.set_label(&nerd_glyph_for(&info.app_id));
        }
        _ => {}
    }

    // Tooltip: always show full title for icon/nerd modes
    if mode != TaskbarMode::Text {
        btn.set_tooltip_text(Some(&info.title));
    }

    // State CSS classes
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
}

fn create_widget(
    info: &ToplevelInfo,
    mode: TaskbarMode,
    id: u32,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
) -> TaskbarWidget {
    let w = match mode {
        TaskbarMode::Text => {
            let btn = gtk4::Button::with_label(&truncate_title(&info.title, 25));
            btn.add_css_class("taskbar-item");
            TaskbarWidget::TextButton(btn)
        }
        TaskbarMode::Icons => {
            let icon_name = resolve_icon_name(&info.app_id);
            let img = gtk4::Image::from_icon_name(&icon_name);
            img.set_pixel_size(20);
            let btn = gtk4::Button::new();
            btn.set_child(Some(&img));
            btn.add_css_class("taskbar-item");
            btn.add_css_class("taskbar-icon");
            btn.set_tooltip_text(Some(&info.title));
            TaskbarWidget::IconButton(btn, img)
        }
        TaskbarMode::Nerd => {
            let glyph = nerd_glyph_for(&info.app_id);
            let btn = gtk4::Button::with_label(&glyph);
            btn.add_css_class("taskbar-item");
            btn.add_css_class("taskbar-nerd");
            btn.set_tooltip_text(Some(&info.title));
            TaskbarWidget::NerdButton(btn)
        }
    };

    let btn = w.button();
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
    gesture.set_button(2);
    gesture.connect_released(move |_, _, _, _| {
        let _ = tx_close.send(ToplevelAction::Close(id));
    });
    btn.add_controller(gesture);

    w
}

/// Resolve an app_id to a freedesktop icon name.
/// Tries: exact app_id, lowercased, common aliases.
fn resolve_icon_name(app_id: &str) -> String {
    let display = gtk4::gdk::Display::default();
    let theme = display
        .as_ref()
        .map(gtk4::IconTheme::for_display)
        .unwrap_or_else(|| gtk4::IconTheme::new());

    // Try exact, then lowercase, then common mappings
    let candidates = [
        app_id.to_string(),
        app_id.to_lowercase(),
        // Strip org.foo.Bar → Bar → bar
        app_id.rsplit('.').next().unwrap_or(app_id).to_string(),
        app_id.rsplit('.').next().unwrap_or(app_id).to_lowercase(),
    ];

    for name in &candidates {
        if theme.has_icon(name) {
            return name.clone();
        }
    }

    // Hardcoded fallbacks for common apps
    let mapped = match app_id.to_lowercase().as_str() {
        s if s.contains("firefox") => "firefox",
        s if s.contains("chrome") || s.contains("chromium") => "chromium",
        s if s.contains("code") || s.contains("vscode") => "visual-studio-code",
        s if s.contains("terminal") || s.contains("foot") || s.contains("kitty") || s.contains("alacritty") => "utilities-terminal",
        s if s.contains("thunar") || s.contains("nautilus") || s.contains("dolphin") || s.contains("files") => "system-file-manager",
        s if s.contains("discord") => "discord",
        s if s.contains("spotify") => "spotify",
        s if s.contains("telegram") => "telegram",
        s if s.contains("steam") => "steam",
        s if s.contains("gimp") => "gimp",
        s if s.contains("inkscape") => "inkscape",
        s if s.contains("blender") => "blender",
        s if s.contains("obs") => "obs",
        s if s.contains("vlc") => "vlc",
        s if s.contains("mpv") => "mpv",
        _ => "application-x-executable",
    };

    if theme.has_icon(mapped) {
        mapped.to_string()
    } else {
        "application-x-executable".to_string()
    }
}

/// Map app_id to a Nerd Font glyph.
fn nerd_glyph_for(app_id: &str) -> String {
    let lower = app_id.to_lowercase();
    let glyph = match lower.as_str() {
        // Browsers
        s if s.contains("firefox") => "\u{f269}",       // 
        s if s.contains("chrome") => "\u{f268}",        // 
        s if s.contains("chromium") => "\u{f268}",      // 
        s if s.contains("brave") => "\u{f39f}",         // 
        // Terminals
        s if s.contains("foot") => "\u{f489}",          // 
        s if s.contains("kitty") => "\u{f489}",         // 
        s if s.contains("alacritty") => "\u{f489}",     // 
        s if s.contains("terminal") => "\u{f489}",      // 
        s if s.contains("wezterm") => "\u{f489}",       // 
        s if s.contains("konsole") => "\u{f489}",       // 
        // Editors / IDEs
        s if s.contains("code") || s.contains("vscode") => "\u{e70c}", // 
        s if s.contains("neovim") || s.contains("nvim") => "\u{e62b}", // 
        s if s.contains("vim") => "\u{e62b}",           // 
        s if s.contains("emacs") => "\u{e632}",         // 
        s if s.contains("sublime") => "\u{e7aa}",       // 
        // Files
        s if s.contains("thunar") || s.contains("nautilus") || s.contains("dolphin") || s.contains("files") || s.contains("pcmanfm") => "\u{f413}", // 
        // Media
        s if s.contains("spotify") => "\u{f1bc}",       // 
        s if s.contains("vlc") => "\u{f40a}",           // 
        s if s.contains("mpv") => "\u{f40a}",           // 
        // Communication
        s if s.contains("discord") => "\u{f392}",       // 
        s if s.contains("telegram") => "\u{f2c6}",      // 
        s if s.contains("slack") => "\u{f198}",         // 
        s if s.contains("signal") => "\u{f086}",        // 
        // Games / Creative
        s if s.contains("steam") => "\u{f1b6}",         // 
        s if s.contains("gimp") => "\u{e69e}",          // 
        s if s.contains("inkscape") => "\u{e69e}",      // 
        s if s.contains("blender") => "\u{e69e}",       // 
        s if s.contains("obs") => "\u{f03d}",           // 
        // System
        s if s.contains("settings") || s.contains("control") => "\u{f013}", // 
        s if s.contains("monitor") || s.contains("htop") || s.contains("btop") => "\u{f080}", // 
        // Fallback
        _ => "\u{f2d0}",                                // 
    };
    glyph.to_string()
}

fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        let mut s: String = title.chars().take(max_len - 1).collect();
        s.push('\u{2026}');
        s
    }
}
