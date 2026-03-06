use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Label, Orientation, ScrolledWindow};
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use rdm_common::config::RdmConfig;
use rdm_common::desktop_apps::{self, AppEntry};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// ─── Display mode (local copy, no dependency on rdm-panel) ───────

#[derive(Clone, Copy, PartialEq)]
enum DisplayMode {
    Icons,
    Text,
    Nerd,
}

impl DisplayMode {
    fn from_str(s: &str) -> Self {
        match s {
            "text" => Self::Text,
            "nerd" => Self::Nerd,
            _ => Self::Icons,
        }
    }
}

// ─── Main ────────────────────────────────────────────────────────

fn main() {
    env_logger::init();
    log::info!("Starting RDM Launcher");

    let config = RdmConfig::load();

    let app = Application::builder()
        .application_id("org.rdm.launcher")
        .build();

    let cfg = config.clone();
    app.connect_activate(move |app| build_launcher(app, &cfg));
    app.run();
}

fn build_launcher(app: &Application, config: &RdmConfig) {
    let mode = DisplayMode::from_str(&config.panel.taskbar_mode);
    let launcher_pos = config.menu.launcher_position.clone();
    let layout = rdm_common::theme::load_active_theme_layout();
    let is_full = launcher_pos == "full";
    let config = Rc::new(RefCell::new(config.clone()));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Launcher")
        .default_width(if is_full { 0 } else { 700 })
        .default_height(if is_full { 0 } else { 500 })
        .build();

    // Layer shell setup — overlay that grabs keyboard
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace("rdm-launcher");
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    // Position modes
    {
        let cfg = config.borrow();
        match launcher_pos.as_str() {
            "panel" => {
                let at_top = cfg.panel.position == "top";
                window.set_anchor(Edge::Left, true);
                if at_top {
                    window.set_anchor(Edge::Top, true);
                    window.set_margin(Edge::Top, cfg.panel.height);
                } else {
                    window.set_anchor(Edge::Bottom, true);
                    window.set_margin(Edge::Bottom, cfg.panel.height);
                }
            }
            "full" => {
                window.set_anchor(Edge::Top, true);
                window.set_anchor(Edge::Bottom, true);
                window.set_anchor(Edge::Left, true);
                window.set_anchor(Edge::Right, true);
            }
            _ => {
                // "center" — no anchors, layer-shell centers by default
            }
        }
    }

    // Escape closes
    let app_for_key = app.clone();
    let key_controller = gtk4::EventControllerKey::new();
    key_controller.connect_key_pressed(move |_, key, _, _| {
        if key == gtk4::gdk::Key::Escape {
            app_for_key.quit();
            return gtk4::glib::Propagation::Stop;
        }
        gtk4::glib::Propagation::Proceed
    });
    window.add_controller(key_controller);

    // Close on focus loss
    let app_for_focus = app.clone();
    window.connect_is_active_notify(move |win| {
        if !win.is_active() {
            app_for_focus.quit();
        }
    });

    // Build content
    let content = build_menu_content(app, &config, mode, is_full, &layout);
    window.set_child(Some(&content));

    load_css();
    window.present();
}

// ─── Icon color cache ────────────────────────────────────────────

type ColorCache = Rc<RefCell<HashMap<String, Option<(f64, f64, f64)>>>>;

fn build_color_cache(entries: &[AppEntry], mode: DisplayMode) -> ColorCache {
    let cache = Rc::new(RefCell::new(HashMap::new()));
    if mode == DisplayMode::Icons {
        return cache; // Icons mode doesn't use colors
    }
    for entry in entries {
        if let Some(ref icon_name) = entry.icon {
            if !cache.borrow().contains_key(icon_name) {
                let color = extract_icon_color(icon_name);
                cache.borrow_mut().insert(icon_name.clone(), color);
            }
        }
    }
    cache
}

// ─── Menu content ─────────────────────────────────────────────────

fn build_menu_content(
    app: &Application,
    config: &Rc<RefCell<RdmConfig>>,
    mode: DisplayMode,
    _is_full: bool,
    layout: &rdm_common::theme::ThemeLayout,
) -> gtk4::Box {
    let root = gtk4::Box::new(Orientation::Vertical, 0);
    root.add_css_class("menu-root");

    // Top bar: settings button + search entry
    let top_bar = gtk4::Box::new(Orientation::Horizontal, 6);
    top_bar.set_margin_top(12);
    top_bar.set_margin_bottom(8);
    top_bar.set_margin_start(12);
    top_bar.set_margin_end(12);

    let settings_btn = gtk4::Button::new();
    settings_btn.set_icon_name("preferences-system-symbolic");
    settings_btn.set_tooltip_text(Some("RDM Settings"));
    settings_btn.add_css_class("menu-settings-btn");
    settings_btn.set_valign(gtk4::Align::Center);
    settings_btn.connect_clicked(|_| {
        let _ = std::process::Command::new("rdm-settings").spawn();
    });
    let search_entry = gtk4::Entry::new();
    search_entry.set_placeholder_text(Some("Search applications..."));
    search_entry.add_css_class("menu-search");
    search_entry.set_hexpand(true);
    if layout.launcher.settings_side == "right" {
        top_bar.append(&search_entry);
        top_bar.append(&settings_btn);
    } else {
        top_bar.append(&settings_btn);
        top_bar.append(&search_entry);
    }
    root.append(&top_bar);

    // Main split: left (categories + app list) | right (favorites)
    let split = gtk4::Box::new(Orientation::Horizontal, 0);
    split.set_vexpand(true);

    // ── Left pane ──
    let left_pane = gtk4::Box::new(Orientation::Vertical, 0);
    left_pane.set_width_request(280);
    left_pane.add_css_class("menu-left");

    // Load apps
    let entries = Rc::new(desktop_apps::load_desktop_entries());
    let categorized = Rc::new(desktop_apps::categorize_entries(&entries));
    let color_cache = build_color_cache(&entries, mode);

    // Category list
    let cat_scroll = ScrolledWindow::new();
    cat_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    cat_scroll.set_vexpand(false);
    cat_scroll.set_max_content_height(160);
    cat_scroll.set_propagate_natural_height(true);

    let cat_list = gtk4::ListBox::new();
    cat_list.add_css_class("menu-categories");
    cat_list.set_selection_mode(gtk4::SelectionMode::Single);

    // "All" category
    let all_row = make_category_row("All", mode);
    cat_list.append(&all_row);

    let mut category_names: Vec<String> = categorized.keys().cloned().collect();
    category_names.sort();
    for name in &category_names {
        let row = make_category_row(name, mode);
        cat_list.append(&row);
    }

    cat_scroll.set_child(Some(&cat_list));
    left_pane.append(&cat_scroll);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    left_pane.append(&sep);

    // App list (scrollable)
    let app_scroll = ScrolledWindow::new();
    app_scroll.set_vexpand(true);
    app_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let app_list = gtk4::ListBox::new();
    app_list.add_css_class("menu-apps");

    app_scroll.set_child(Some(&app_list));
    left_pane.append(&app_scroll);

    // ── Right pane (Favorites) ──
    let right_pane = gtk4::Box::new(Orientation::Vertical, 8);
    right_pane.set_hexpand(true);
    right_pane.add_css_class("menu-right");
    right_pane.set_margin_top(8);
    right_pane.set_margin_start(12);
    right_pane.set_margin_end(12);
    right_pane.set_margin_bottom(8);

    let fav_header = Label::new(Some("Favorites"));
    fav_header.add_css_class("menu-fav-header");
    fav_header.set_halign(gtk4::Align::Start);
    right_pane.append(&fav_header);

    let fav_scroll = ScrolledWindow::new();
    fav_scroll.set_vexpand(true);
    fav_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let fav_flow_inner = gtk4::FlowBox::new();
    fav_flow_inner.set_selection_mode(gtk4::SelectionMode::None);
    fav_flow_inner.set_max_children_per_line(20);
    fav_flow_inner.set_min_children_per_line(1);
    fav_flow_inner.set_row_spacing(12);
    fav_flow_inner.set_column_spacing(12);
    fav_flow_inner.set_homogeneous(true);
    fav_flow_inner.set_hexpand(true);
    fav_flow_inner.set_valign(gtk4::Align::Start);
    fav_flow_inner.add_css_class("menu-favorites");

    // Wrap in Rc early so all closures share the same reference
    let fav_flow: Rc<gtk4::FlowBox> = Rc::new(fav_flow_inner);

    populate_favorites(&fav_flow, &entries, config, mode, &color_cache);

    fav_scroll.set_child(Some(fav_flow.as_ref()));
    right_pane.append(&fav_scroll);

    // Now populate app list (needs fav_flow reference for context menus)
    populate_app_list(
        app,
        &app_list,
        &entries,
        mode,
        config,
        &fav_flow,
        &color_cache,
    );
    let hint = Label::new(Some("Right-click app to add/remove from favorites."));
    hint.add_css_class("menu-hint");
    hint.set_halign(gtk4::Align::Start);
    right_pane.append(&hint);

    // ── Vertical separator between panes ──
    let vsep = gtk4::Separator::new(Orientation::Vertical);

    if layout.launcher.favorites_side == "left" {
        split.append(&right_pane);
        split.append(&vsep);
        split.append(&left_pane);
    } else {
        split.append(&left_pane);
        split.append(&vsep);
        split.append(&right_pane);
    }
    root.append(&split);

    // ── Wire interactions ──

    // Category selection filters the app list
    let entries_for_cat = entries.clone();
    let categorized_for_cat = categorized.clone();
    let app_list_for_cat = app_list.clone();
    let config_for_cat = config.clone();
    let fav_flow_for_cat = fav_flow.clone();
    let app_for_cat = app.clone();
    let cache_for_cat = color_cache.clone();
    cat_list.connect_row_selected(move |_, row| {
        if let Some(row) = row {
            let idx = row.index();
            if idx == 0 {
                // "All"
                populate_app_list(
                    &app_for_cat,
                    &app_list_for_cat,
                    &entries_for_cat,
                    mode,
                    &config_for_cat,
                    &fav_flow_for_cat,
                    &cache_for_cat,
                );
            } else {
                let cat_keys: Vec<String> = categorized_for_cat.keys().cloned().collect();
                if let Some(cat_name) = cat_keys.get((idx - 1) as usize) {
                    if let Some(cat_entries) = categorized_for_cat.get(cat_name) {
                        populate_app_list(
                            &app_for_cat,
                            &app_list_for_cat,
                            &Rc::new(cat_entries.clone()),
                            mode,
                            &config_for_cat,
                            &fav_flow_for_cat,
                            &cache_for_cat,
                        );
                    }
                }
            }
        }
    });

    // Select "All" by default
    if let Some(first_row) = cat_list.row_at_index(0) {
        cat_list.select_row(Some(&first_row));
    }

    // Search filtering
    let entries_for_search = entries.clone();
    let app_list_for_search = app_list.clone();
    let fav_flow_for_search = fav_flow.clone();
    let config_for_search = config.clone();
    let entries_for_fav_search = entries.clone();
    let fav_flow_for_search2 = fav_flow.clone();
    let entries_for_fav_rebuild = entries.clone();
    let app_for_search = app.clone();
    let cache_for_search = color_cache.clone();
    search_entry.connect_changed(move |entry| {
        let query = entry.text().to_string().to_lowercase();
        if query.is_empty() {
            populate_app_list(
                &app_for_search,
                &app_list_for_search,
                &entries_for_search,
                mode,
                &config_for_search,
                &fav_flow_for_search,
                &cache_for_search,
            );
            populate_favorites(
                &fav_flow_for_search2,
                &entries_for_fav_search,
                &config_for_search,
                mode,
                &cache_for_search,
            );
        } else {
            let filtered: Vec<AppEntry> = entries_for_search
                .iter()
                .filter(|e| {
                    e.name.to_lowercase().contains(&query)
                        || e.comment
                            .as_ref()
                            .map(|c| c.to_lowercase().contains(&query))
                            .unwrap_or(false)
                })
                .cloned()
                .collect();
            populate_app_list(
                &app_for_search,
                &app_list_for_search,
                &Rc::new(filtered),
                mode,
                &config_for_search,
                &fav_flow_for_search,
                &cache_for_search,
            );

            // Filter favorites too
            let fav_names: Vec<String> = config_for_search.borrow().menu.favorites.clone();
            let fav_entries: Vec<AppEntry> = entries_for_fav_search
                .iter()
                .filter(|e| {
                    fav_names.contains(&e.name)
                        && (e.name.to_lowercase().contains(&query)
                            || e.comment
                                .as_ref()
                                .map(|c| c.to_lowercase().contains(&query))
                                .unwrap_or(false))
                })
                .cloned()
                .collect();
            populate_favorites_from_entries(
                &fav_flow_for_search2,
                &fav_entries,
                &config_for_search,
                mode,
                &fav_flow_for_search2,
                &entries_for_fav_rebuild,
                &cache_for_search,
            );
        }
    });

    // Focus search on open
    search_entry.grab_focus();

    root
}

// ─── Category rows ────────────────────────────────────────────────

fn make_category_row(name: &str, mode: DisplayMode) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    let hbox = gtk4::Box::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(4);
    hbox.set_margin_bottom(4);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    if mode == DisplayMode::Nerd {
        let glyph = category_nerd_glyph(name);
        let icon_label = Label::new(Some(glyph));
        icon_label.add_css_class("nerd-icon");
        hbox.append(&icon_label);
    }

    let label = Label::new(Some(name));
    label.set_halign(gtk4::Align::Start);
    hbox.append(&label);

    row.set_child(Some(&hbox));
    row
}

fn category_nerd_glyph(category: &str) -> &'static str {
    match category {
        "All" => "\u{f03a}",
        "Development" => "\u{e70c}",
        "Games" => "\u{f11b}",
        "Graphics" => "\u{f1fc}",
        "Internet" => "\u{f0ac}",
        "Media" => "\u{f001}",
        "Office" => "\u{f15c}",
        "Science" => "\u{f19d}",
        "Settings" => "\u{f013}",
        "System" => "\u{f108}",
        "Utilities" => "\u{f0ad}",
        "Other" => "\u{f2d0}",
        _ => "\u{f2d0}",
    }
}

// ─── App list ─────────────────────────────────────────────────────

fn populate_app_list(
    app: &Application,
    list: &gtk4::ListBox,
    entries: &Rc<Vec<AppEntry>>,
    mode: DisplayMode,
    config: &Rc<RefCell<RdmConfig>>,
    fav_flow: &Rc<gtk4::FlowBox>,
    cache: &ColorCache,
) {
    // Clear existing
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }

    for entry in entries.iter() {
        let row = make_app_row(app, entry, mode, config, entries, fav_flow, cache);
        list.append(&row);
    }
}

fn make_app_row(
    app: &Application,
    entry: &AppEntry,
    mode: DisplayMode,
    config: &Rc<RefCell<RdmConfig>>,
    all_entries: &Rc<Vec<AppEntry>>,
    fav_flow: &Rc<gtk4::FlowBox>,
    cache: &ColorCache,
) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::new();
    let hbox = gtk4::Box::new(Orientation::Horizontal, 8);
    hbox.set_margin_top(3);
    hbox.set_margin_bottom(3);
    hbox.set_margin_start(12);
    hbox.set_margin_end(12);

    match mode {
        DisplayMode::Icons => {
            if let Some(ref icon_name) = entry.icon {
                let img = gtk4::Image::from_icon_name(icon_name);
                img.set_pixel_size(24);
                hbox.append(&img);
            }
            let name_label = Label::new(Some(&entry.name));
            name_label.add_css_class("menu-app-name");
            name_label.set_halign(gtk4::Align::Start);
            hbox.append(&name_label);
        }
        DisplayMode::Nerd => {
            let glyph = app_nerd_glyph(entry);
            let glyph_label = Label::new(Some(&glyph));
            glyph_label.add_css_class("nerd-icon");
            if let Some(ref icon_name) = entry.icon {
                apply_icon_color(&glyph_label, icon_name, cache);
            }
            hbox.append(&glyph_label);

            let name_label = Label::new(Some(&entry.name));
            name_label.add_css_class("menu-app-name");
            name_label.set_halign(gtk4::Align::Start);
            hbox.append(&name_label);
        }
        DisplayMode::Text => {
            let name_label = Label::new(Some(&entry.name));
            name_label.add_css_class("menu-app-name");
            name_label.set_halign(gtk4::Align::Start);
            if let Some(ref icon_name) = entry.icon {
                apply_icon_color(&name_label, icon_name, cache);
            }
            hbox.append(&name_label);
        }
    }

    // Click to launch
    let exec = entry.exec.clone();
    let app_for_launch = app.clone();
    let gesture_click = gtk4::GestureClick::new();
    gesture_click.set_button(1);
    gesture_click.connect_released(move |_, _, _, _| {
        launch_app(&exec);
        app_for_launch.quit();
    });
    row.add_controller(gesture_click);

    // Right-click: directly toggle favorite
    let app_name = entry.name.clone();
    let cfg = config.clone();
    let right_click = gtk4::GestureClick::new();
    right_click.set_button(3);
    let fav_flow_for_ctx = fav_flow.clone();
    let entries_for_ctx = all_entries.clone();
    let hbox_weak = hbox.downgrade();
    let cache_for_ctx = cache.clone();
    right_click.connect_released(move |_, _, _, _| {
        {
            let mut c = cfg.borrow_mut();
            if c.menu.favorites.contains(&app_name) {
                c.menu.favorites.retain(|n| n != &app_name);
            } else {
                c.menu.favorites.push(app_name.clone());
            }
        }
        if let Err(e) = cfg.borrow().save() {
            log::error!("Failed to save favorites: {}", e);
        }
        populate_favorites(
            &fav_flow_for_ctx,
            &entries_for_ctx,
            &cfg,
            mode,
            &cache_for_ctx,
        );
        // Brief visual feedback
        if let Some(hbox_ref) = hbox_weak.upgrade() {
            hbox_ref.add_css_class("fav-toggled");
            let hbox_timeout = hbox_ref.clone();
            gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(300), move || {
                hbox_timeout.remove_css_class("fav-toggled");
            });
        }
    });
    row.add_controller(right_click);

    row.set_child(Some(&hbox));
    row
}

fn app_nerd_glyph(entry: &AppEntry) -> String {
    let lower_name = entry.name.to_lowercase();
    let lower_exec = entry.exec.to_lowercase();
    let combined = format!("{} {}", lower_name, lower_exec);

    let glyph = match combined.as_str() {
        s if s.contains("firefox") => "\u{f269}",
        s if s.contains("chrome") || s.contains("chromium") => "\u{f268}",
        s if s.contains("brave") => "\u{f39f}",
        s if s.contains("foot")
            || s.contains("kitty")
            || s.contains("alacritty")
            || s.contains("terminal")
            || s.contains("wezterm")
            || s.contains("konsole") =>
        {
            "\u{f489}"
        }
        s if s.contains("code") || s.contains("vscode") => "\u{e70c}",
        s if s.contains("neovim") || s.contains("nvim") => "\u{e62b}",
        s if s.contains("vim") => "\u{e62b}",
        s if s.contains("emacs") => "\u{e632}",
        s if s.contains("sublime") => "\u{e7aa}",
        s if s.contains("thunar")
            || s.contains("nautilus")
            || s.contains("dolphin")
            || s.contains("files")
            || s.contains("pcmanfm") =>
        {
            "\u{f413}"
        }
        s if s.contains("spotify") => "\u{f1bc}",
        s if s.contains("vlc") => "\u{f40a}",
        s if s.contains("mpv") => "\u{f40a}",
        s if s.contains("discord") => "\u{f392}",
        s if s.contains("telegram") => "\u{f2c6}",
        s if s.contains("slack") => "\u{f198}",
        s if s.contains("signal") => "\u{f086}",
        s if s.contains("steam") => "\u{f1b6}",
        s if s.contains("gimp") => "\u{e69e}",
        s if s.contains("inkscape") => "\u{e69e}",
        s if s.contains("blender") => "\u{e69e}",
        s if s.contains("obs") => "\u{f03d}",
        s if s.contains("settings") || s.contains("control") => "\u{f013}",
        s if s.contains("htop") || s.contains("btop") || s.contains("monitor") => "\u{f080}",
        _ => "\u{f2d0}",
    };
    glyph.to_string()
}

// ─── Favorites ────────────────────────────────────────────────────

fn populate_favorites(
    flow: &Rc<gtk4::FlowBox>,
    entries: &Rc<Vec<AppEntry>>,
    config: &Rc<RefCell<RdmConfig>>,
    mode: DisplayMode,
    cache: &ColorCache,
) {
    let fav_names = config.borrow().menu.favorites.clone();
    let fav_entries: Vec<AppEntry> = fav_names
        .iter()
        .filter_map(|name| entries.iter().find(|e| &e.name == name).cloned())
        .collect();
    populate_favorites_from_entries(flow, &fav_entries, config, mode, flow, entries, cache);
}

fn populate_favorites_from_entries(
    flow: &Rc<gtk4::FlowBox>,
    fav_entries: &[AppEntry],
    config: &Rc<RefCell<RdmConfig>>,
    mode: DisplayMode,
    fav_flow_rc: &Rc<gtk4::FlowBox>,
    all_entries: &Rc<Vec<AppEntry>>,
    cache: &ColorCache,
) {
    // Clear
    while let Some(child) = flow.first_child() {
        flow.remove(&child);
    }

    if fav_entries.is_empty() {
        let empty = Label::new(Some("No favorites yet.\nRight-click an app to add it."));
        empty.add_css_class("menu-hint");
        empty.set_halign(gtk4::Align::Center);
        empty.set_valign(gtk4::Align::Center);
        flow.insert(&empty, -1);
        return;
    }

    for entry in fav_entries {
        let tile = make_favorite_tile(entry, mode, config, fav_flow_rc, all_entries, cache);
        flow.insert(&tile, -1);
    }
}

fn make_favorite_tile(
    entry: &AppEntry,
    mode: DisplayMode,
    config: &Rc<RefCell<RdmConfig>>,
    fav_flow: &Rc<gtk4::FlowBox>,
    all_entries: &Rc<Vec<AppEntry>>,
    cache: &ColorCache,
) -> gtk4::Box {
    let tile = gtk4::Box::new(Orientation::Vertical, 4);
    tile.add_css_class("menu-fav-tile");
    tile.set_halign(gtk4::Align::Center);
    tile.set_valign(gtk4::Align::Start);
    tile.set_size_request(80, -1);

    match mode {
        DisplayMode::Icons => {
            if let Some(ref icon_name) = entry.icon {
                let img = gtk4::Image::from_icon_name(icon_name);
                img.set_pixel_size(48);
                img.set_halign(gtk4::Align::Center);
                tile.append(&img);
            }
            let name = Label::new(Some(&entry.name));
            name.add_css_class("menu-fav-name");
            name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            name.set_max_width_chars(12);
            name.set_halign(gtk4::Align::Center);
            tile.append(&name);
        }
        DisplayMode::Nerd => {
            let glyph = app_nerd_glyph(entry);
            let glyph_label = Label::new(Some(&glyph));
            glyph_label.add_css_class("nerd-icon");
            glyph_label.add_css_class("menu-fav-glyph");
            if let Some(ref icon_name) = entry.icon {
                apply_icon_color(&glyph_label, icon_name, cache);
            }
            glyph_label.set_halign(gtk4::Align::Center);
            tile.append(&glyph_label);

            let name = Label::new(Some(&entry.name));
            name.add_css_class("menu-fav-name");
            name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            name.set_max_width_chars(12);
            name.set_halign(gtk4::Align::Center);
            tile.append(&name);
        }
        DisplayMode::Text => {
            let name = Label::new(Some(&entry.name));
            name.add_css_class("menu-fav-name");
            name.add_css_class("menu-fav-text-mode");
            name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            name.set_max_width_chars(12);
            name.set_halign(gtk4::Align::Center);
            name.set_valign(gtk4::Align::Center);
            if let Some(ref icon_name) = entry.icon {
                apply_icon_color(&name, icon_name, cache);
            }
            tile.append(&name);
        }
    }

    // Click to launch
    let exec = entry.exec.clone();
    let click = gtk4::GestureClick::new();
    click.set_button(1);
    let tile_for_quit = tile.clone();
    click.connect_released(move |_, _, _, _| {
        launch_app(&exec);
        if let Some(root) = tile_for_quit.root() {
            if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                window.close();
            }
        }
    });
    tile.add_controller(click);

    // Right-click to remove from favorites directly
    let app_name = entry.name.clone();
    let cfg = config.clone();
    let right_click = gtk4::GestureClick::new();
    right_click.set_button(3);
    let fav_flow_for_ctx = fav_flow.clone();
    let entries_for_ctx = all_entries.clone();
    let cache_for_ctx = cache.clone();
    right_click.connect_released(move |_, _, _, _| {
        cfg.borrow_mut().menu.favorites.retain(|n| n != &app_name);
        if let Err(e) = cfg.borrow().save() {
            log::error!("Failed to save favorites: {}", e);
        }
        populate_favorites(
            &fav_flow_for_ctx,
            &entries_for_ctx,
            &cfg,
            mode,
            &cache_for_ctx,
        );
    });
    tile.add_controller(right_click);

    tile
}

// ─── Icon color extraction ────────────────────────────────────────

fn apply_icon_color(label: &Label, icon_name: &str, cache: &ColorCache) {
    let color = cache.borrow().get(icon_name).copied().flatten();
    if let Some(color) = color {
        let css = format!(
            "color: rgb({},{},{});",
            (color.0 * 255.0) as u8,
            (color.1 * 255.0) as u8,
            (color.2 * 255.0) as u8,
        );
        let provider = CssProvider::new();
        provider.load_from_data(&format!("* {{ {} }}", css));
        label
            .style_context()
            .add_provider(&provider, gtk4::STYLE_PROVIDER_PRIORITY_USER + 2);
    }
}

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
    let mut best_color = (0.753, 0.792, 0.961); // fallback #c0caf5

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
            best_color = (r, g, b);
        }

        i += stride;
    }

    if best_sat > 0.1 {
        Some(best_color)
    } else {
        None
    }
}

// ─── App launching ────────────────────────────────────────────────

fn launch_app(exec: &str) {
    let cmd: String = exec
        .split_whitespace()
        .filter(|s| !s.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");

    log::info!("Launching: {}", cmd);

    match std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
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
        Err(e) => log::error!("Failed to launch '{}': {}", cmd, e),
    }
}

// ─── CSS ──────────────────────────────────────────────────────────

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
