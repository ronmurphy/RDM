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

struct WidgetState {
    widget: TaskbarWidget,
    cached_title: String,
    cached_app_id: String,
    cached_icon_name: String,
    cached_activated: bool,
    cached_minimized: bool,
}

struct TaskbarState {
    widgets: HashMap<u32, WidgetState>,
    last_generation: u64,
    color_cache: HashMap<String, Option<(f64, f64, f64)>>,
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
        color_cache: HashMap::new(),
    }));

    let container = container.clone();
    let action_tx = action_tx.clone();
    let shared = shared.clone();

    // Poll the shared state every 250ms
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(250), move || {
        update_taskbar(&container, &shared, &state, &action_tx, mode);
        gtk4::glib::ControlFlow::Continue
    });

    log::info!(
        "Taskbar initialized (mode: {:?})",
        match mode {
            TaskbarMode::Text => "text",
            TaskbarMode::Icons => "icons",
            TaskbarMode::Nerd => "nerd",
        }
    );
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
        if let Some(ws) = tb.widgets.remove(&id) {
            container.remove(ws.widget.button());
        }
    }

    // Add/update widgets for current toplevels
    for (&id, info) in &shared_data.toplevels {
        if info.title.is_empty() {
            continue;
        }

        if let Some(ws) = tb.widgets.get_mut(&id) {
            update_existing_widget(ws, info, mode);
        } else {
            let w = create_widget(info, mode, id, action_tx, &mut tb.color_cache);
            container.append(w.button());
            let icon_name = if mode == TaskbarMode::Icons {
                resolve_icon_name(&info.app_id)
            } else {
                String::new()
            };
            tb.widgets.insert(
                id,
                WidgetState {
                    widget: w,
                    cached_title: info.title.clone(),
                    cached_app_id: info.app_id.clone(),
                    cached_icon_name: icon_name,
                    cached_activated: info.is_activated,
                    cached_minimized: info.is_minimized,
                },
            );
        }
    }
}

fn update_existing_widget(ws: &mut WidgetState, info: &ToplevelInfo, mode: TaskbarMode) {
    let btn = ws.widget.button();

    // Only update label/icon when the underlying data actually changed
    match (mode, &ws.widget) {
        (TaskbarMode::Text, TaskbarWidget::TextButton(b)) => {
            if ws.cached_title != info.title {
                b.set_label(&truncate_title(&info.title, 25));
                ws.cached_title = info.title.clone();
            }
        }
        (TaskbarMode::Icons, TaskbarWidget::IconButton(_, img)) => {
            if ws.cached_app_id != info.app_id {
                let icon_name = resolve_icon_name(&info.app_id);
                img.set_icon_name(Some(&icon_name));
                ws.cached_icon_name = icon_name;
                ws.cached_app_id = info.app_id.clone();
            }
        }
        (TaskbarMode::Nerd, TaskbarWidget::NerdButton(b)) => {
            if ws.cached_app_id != info.app_id {
                b.set_label(&nerd_glyph_for(&info.app_id));
                ws.cached_app_id = info.app_id.clone();
            }
        }
        _ => {}
    }

    // Tooltip: only update when title changed
    if mode != TaskbarMode::Text && ws.cached_title != info.title {
        btn.set_tooltip_text(Some(&info.title));
        ws.cached_title = info.title.clone();
    }

    // State CSS classes: only toggle when state actually changed
    if ws.cached_activated != info.is_activated {
        if info.is_activated {
            btn.add_css_class("active");
        } else {
            btn.remove_css_class("active");
        }
        ws.cached_activated = info.is_activated;
    }
    if ws.cached_minimized != info.is_minimized {
        if info.is_minimized {
            btn.add_css_class("minimized");
        } else {
            btn.remove_css_class("minimized");
        }
        ws.cached_minimized = info.is_minimized;
    }
}

fn create_widget(
    info: &ToplevelInfo,
    mode: TaskbarMode,
    id: u32,
    action_tx: &Rc<std::sync::mpsc::Sender<ToplevelAction>>,
    color_cache: &mut HashMap<String, Option<(f64, f64, f64)>>,
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
            let label = gtk4::Label::new(Some(&glyph));
            label.add_css_class("nerd-icon");

            // Apply icon-derived color to the label (cached)
            let icon_name = resolve_icon_name(&info.app_id);
            let color = color_cache.entry(icon_name.clone()).or_insert_with(|| {
                let c = extract_icon_color(&icon_name);
                log::info!("Icon color for '{}' ({}): {:?}", info.app_id, icon_name, c);
                c
            });
            if let Some((r, g, b)) = *color {
                apply_color_to_label(&label, r, g, b);
            }

            let btn = gtk4::Button::new();
            btn.set_child(Some(&label));
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
        s if s.contains("terminal")
            || s.contains("foot")
            || s.contains("kitty")
            || s.contains("alacritty") =>
        {
            "utilities-terminal"
        }
        s if s.contains("thunar")
            || s.contains("nautilus")
            || s.contains("dolphin")
            || s.contains("files") =>
        {
            "system-file-manager"
        }
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
        s if s.contains("firefox") => "\u{f269}",  //
        s if s.contains("chrome") => "\u{f268}",   //
        s if s.contains("chromium") => "\u{f268}", //
        s if s.contains("brave") => "\u{f39f}",    //
        // Terminals
        s if s.contains("foot") => "\u{f489}",      //
        s if s.contains("kitty") => "\u{f489}",     //
        s if s.contains("alacritty") => "\u{f489}", //
        s if s.contains("terminal") => "\u{f489}",  //
        s if s.contains("wezterm") => "\u{f489}",   //
        s if s.contains("konsole") => "\u{f489}",   //
        // Editors / IDEs
        s if s.contains("code") || s.contains("vscode") => "\u{e70c}", //
        s if s.contains("neovim") || s.contains("nvim") => "\u{e62b}", //
        s if s.contains("vim") => "\u{e62b}",                          //
        s if s.contains("emacs") => "\u{e632}",                        //
        s if s.contains("sublime") => "\u{e7aa}",                      //
        // Files
        s if s.contains("thunar")
            || s.contains("nautilus")
            || s.contains("dolphin")
            || s.contains("files")
            || s.contains("pcmanfm") =>
        {
            "\u{f413}"
        } //
        // Media
        s if s.contains("spotify") => "\u{f1bc}", //
        s if s.contains("vlc") => "\u{f40a}",     //
        s if s.contains("mpv") => "\u{f40a}",     //
        // Communication
        s if s.contains("discord") => "\u{f392}",  //
        s if s.contains("telegram") => "\u{f2c6}", //
        s if s.contains("slack") => "\u{f198}",    //
        s if s.contains("signal") => "\u{f086}",   //
        // Games / Creative
        s if s.contains("steam") => "\u{f1b6}",    //
        s if s.contains("gimp") => "\u{e69e}",     //
        s if s.contains("inkscape") => "\u{e69e}", //
        s if s.contains("blender") => "\u{e69e}",  //
        s if s.contains("obs") => "\u{f03d}",      //
        // System
        s if s.contains("settings") || s.contains("control") => "\u{f013}", //
        s if s.contains("monitor") || s.contains("htop") || s.contains("btop") => "\u{f080}", //
        // Fallback
        _ => "\u{f2d0}", //
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

// ─── Icon color extraction ────────────────────────────────────────

/// Extract the dominant saturated color from an app icon via the icon theme.
fn extract_icon_color(icon_name: &str) -> Option<(f64, f64, f64)> {
    let display = gtk4::gdk::Display::default()?;
    let theme = gtk4::IconTheme::for_display(&display);

    let paintable = theme.lookup_icon(
        icon_name,
        &[],
        16,
        1,
        gtk4::TextDirection::None,
        gtk4::IconLookupFlags::empty(),
    );

    let file = paintable.file()?;
    let path = file.path()?;

    let pixbuf = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(&path, 16, 16, true).ok()?;

    let pixels = pixbuf.pixel_bytes()?;
    let data = pixels.as_ref();
    let n_channels = pixbuf.n_channels() as usize;
    let has_alpha = pixbuf.has_alpha();

    let mut best_sat = 0.0f64;
    let mut best_color = None;

    let stride = n_channels;
    let mut i = 0;
    while i + stride <= data.len() {
        let r = data[i] as f64 / 255.0;
        let g = data[i + 1] as f64 / 255.0;
        let b = data[i + 2] as f64 / 255.0;

        // Skip transparent pixels
        if has_alpha && n_channels >= 4 && data[i + 3] < 128 {
            i += stride;
            continue;
        }

        // Skip very dark or very light pixels
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let lightness = (max + min) / 2.0;
        if lightness < 0.1 || lightness > 0.9 {
            i += stride;
            continue;
        }

        // Compute saturation (HSL)
        let delta = max - min;
        let sat = if delta < 0.001 {
            0.0
        } else {
            delta / (1.0 - (2.0 * lightness - 1.0).abs())
        };

        if sat > best_sat {
            best_sat = sat;
            best_color = Some((r, g, b));
        }

        i += stride;
    }

    if best_sat > 0.1 {
        best_color
    } else {
        None
    }
}

/// Apply an RGB color to a label via an inline CSS provider.
fn apply_color_to_label(label: &gtk4::Label, r: f64, g: f64, b: f64) {
    let css = format!(
        "* {{ color: rgb({},{},{}); }}",
        (r * 255.0) as u8,
        (g * 255.0) as u8,
        (b * 255.0) as u8,
    );
    let provider = gtk4::CssProvider::new();
    provider.load_from_data(&css);
    label
        .style_context()
        .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_USER + 2);
}
