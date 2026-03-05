use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider,
    DropDown, Entry, Label, Orientation, Switch, StringList,
};
use rdm_common::config::RdmConfig;
use std::cell::RefCell;
use std::rc::Rc;

// ─── Display Arrangement Types ──────────────────────────────────

struct MonitorRect {
    index: usize,
    name: String,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    enabled: bool,
}

struct ArrangementState {
    rects: Vec<MonitorRect>,
    drag_index: Option<usize>,
    drag_origin_x: i32,
    drag_origin_y: i32,
    // Cached rendering transform (updated each draw)
    render_scale: f64,
    render_offset_x: f64,
    render_offset_y: f64,
    render_min_x: f64,
    render_min_y: f64,
    // Prevent recursive spinbutton <-> canvas updates
    syncing: bool,
}

impl ArrangementState {
    fn new() -> Self {
        Self {
            rects: Vec::new(),
            drag_index: None,
            drag_origin_x: 0,
            drag_origin_y: 0,
            render_scale: 1.0,
            render_offset_x: 0.0,
            render_offset_y: 0.0,
            render_min_x: 0.0,
            render_min_y: 0.0,
            syncing: false,
        }
    }

    fn hit_test(&self, cx: f64, cy: f64) -> Option<usize> {
        // Test in reverse order so topmost (last drawn) wins
        for rect in self.rects.iter().rev() {
            if !rect.enabled {
                continue;
            }
            let rx = (rect.x as f64 - self.render_min_x) * self.render_scale + self.render_offset_x;
            let ry = (rect.y as f64 - self.render_min_y) * self.render_scale + self.render_offset_y;
            let rw = rect.width as f64 * self.render_scale;
            let rh = rect.height as f64 * self.render_scale;
            if cx >= rx && cx <= rx + rw && cy >= ry && cy <= ry + rh {
                return Some(rect.index);
            }
        }
        None
    }
}

// ─── Main ───────────────────────────────────────────────────────

fn main() {
    env_logger::init();

    let app = Application::builder()
        .application_id("org.rdm.settings")
        .build();

    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let config = Rc::new(RefCell::new(RdmConfig::load()));

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Settings")
        .default_width(520)
        .default_height(480)
        .resizable(true)
        .build();

    let stack = gtk4::Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);

    let sidebar = gtk4::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_size_request(140, -1);

    // --- Panel page ---
    let panel_page = build_panel_page(&config);
    stack.add_titled(&panel_page, Some("panel"), "Panel");

    // --- Wallpaper page ---
    let wallpaper_page = build_wallpaper_page(&config, &window);
    stack.add_titled(&wallpaper_page, Some("wallpaper"), "Wallpaper");

    // --- Displays page ---
    let displays_page = build_displays_page(&config);
    stack.add_titled(&displays_page, Some("displays"), "Displays");

    // --- Main layout ---
    let main_box = GtkBox::new(Orientation::Vertical, 0);

    let content = GtkBox::new(Orientation::Horizontal, 0);
    content.append(&sidebar);

    let sep = gtk4::Separator::new(Orientation::Vertical);
    content.append(&sep);

    stack.set_hexpand(true);
    stack.set_vexpand(true);
    content.append(&stack);

    main_box.append(&content);

    // --- Bottom bar: Apply / Cancel ---
    let bottom_bar = GtkBox::new(Orientation::Horizontal, 8);
    bottom_bar.set_margin_top(8);
    bottom_bar.set_margin_bottom(12);
    bottom_bar.set_margin_start(12);
    bottom_bar.set_margin_end(12);
    bottom_bar.set_halign(gtk4::Align::End);

    let cancel_btn = Button::with_label("Cancel");
    let apply_btn = Button::with_label("Apply");
    apply_btn.add_css_class("suggested-action");

    let window_cancel = window.clone();
    cancel_btn.connect_clicked(move |_| {
        window_cancel.close();
    });

    let config_apply = config.clone();
    let window_apply = window.clone();
    apply_btn.connect_clicked(move |_| {
        let cfg = config_apply.borrow();
        match cfg.save() {
            Ok(()) => {
                log::info!("Config saved, applying changes...");
                apply_changes(&cfg);
                window_apply.close();
            }
            Err(e) => {
                log::error!("Failed to save config: {}", e);
            }
        }
    });

    bottom_bar.append(&cancel_btn);
    bottom_bar.append(&apply_btn);

    let bottom_sep = gtk4::Separator::new(Orientation::Horizontal);
    main_box.append(&bottom_sep);
    main_box.append(&bottom_bar);

    window.set_child(Some(&main_box));
    load_css();
    window.present();
}

// ─── Panel Settings ──────────────────────────────────────────────

fn build_panel_page(config: &Rc<RefCell<RdmConfig>>) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 16);
    page.set_margin_top(20);
    page.set_margin_bottom(20);
    page.set_margin_start(20);
    page.set_margin_end(20);

    // Section header
    let header = Label::new(Some("Panel"));
    header.add_css_class("settings-header");
    header.set_halign(gtk4::Align::Start);
    page.append(&header);

    // Taskbar mode
    let taskbar_row = setting_row("Taskbar Mode");
    let taskbar_modes = StringList::new(&["icons", "text", "nerd"]);
    let taskbar_dropdown = DropDown::new(Some(taskbar_modes), gtk4::Expression::NONE);
    let current = &config.borrow().panel.taskbar_mode;
    taskbar_dropdown.set_selected(match current.as_str() {
        "icons" => 0,
        "text" => 1,
        "nerd" => 2,
        _ => 0,
    });
    let cfg = config.clone();
    taskbar_dropdown.connect_selected_notify(move |dd| {
        let mode = match dd.selected() {
            1 => "text",
            2 => "nerd",
            _ => "icons",
        };
        cfg.borrow_mut().panel.taskbar_mode = mode.to_string();
    });
    taskbar_row.append(&taskbar_dropdown);
    page.append(&taskbar_row);

    // Panel position
    let pos_row = setting_row("Panel Position");
    let pos_modes = StringList::new(&["top", "bottom"]);
    let pos_dropdown = DropDown::new(Some(pos_modes), gtk4::Expression::NONE);
    pos_dropdown.set_selected(if config.borrow().panel.position == "bottom" { 1 } else { 0 });
    let cfg = config.clone();
    pos_dropdown.connect_selected_notify(move |dd| {
        let pos = if dd.selected() == 1 { "bottom" } else { "top" };
        cfg.borrow_mut().panel.position = pos.to_string();
    });
    pos_row.append(&pos_dropdown);
    page.append(&pos_row);

    // Panel height
    let height_row = setting_row("Panel Height");
    let height_adj = gtk4::Adjustment::new(
        config.borrow().panel.height as f64,
        24.0, 64.0, 1.0, 4.0, 0.0,
    );
    let height_spin = gtk4::SpinButton::new(Some(&height_adj), 1.0, 0);
    let cfg = config.clone();
    height_spin.connect_value_changed(move |spin| {
        cfg.borrow_mut().panel.height = spin.value() as i32;
    });
    height_row.append(&height_spin);
    page.append(&height_row);

    // Show clock
    let clock_row = setting_row("Show Clock");
    let clock_switch = Switch::new();
    clock_switch.set_active(config.borrow().panel.show_clock);
    clock_switch.set_valign(gtk4::Align::Center);
    let cfg = config.clone();
    clock_switch.connect_active_notify(move |sw| {
        cfg.borrow_mut().panel.show_clock = sw.is_active();
    });
    clock_row.append(&clock_switch);
    page.append(&clock_row);

    // Clock format
    let fmt_row = setting_row("Clock Format");
    let fmt_entry = Entry::new();
    fmt_entry.set_text(&config.borrow().panel.clock_format);
    fmt_entry.set_hexpand(true);
    let cfg = config.clone();
    fmt_entry.connect_changed(move |e| {
        cfg.borrow_mut().panel.clock_format = e.text().to_string();
    });
    fmt_row.append(&fmt_entry);
    page.append(&fmt_row);

    page
}

// ─── Wallpaper Settings ──────────────────────────────────────────

fn build_wallpaper_page(config: &Rc<RefCell<RdmConfig>>, window: &ApplicationWindow) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 16);
    page.set_margin_top(20);
    page.set_margin_bottom(20);
    page.set_margin_start(20);
    page.set_margin_end(20);

    let header = Label::new(Some("Wallpaper"));
    header.add_css_class("settings-header");
    header.set_halign(gtk4::Align::Start);
    page.append(&header);

    // Current wallpaper path display + browse
    let path_row = setting_row("Image");
    let wp_path = config.borrow().wallpaper.path.clone();
    let display_text = if wp_path.is_empty() {
        "(none — solid color)".to_string()
    } else {
        wp_path
    };
    let path_label = Label::new(Some(&display_text));
    path_label.set_hexpand(true);
    path_label.set_halign(gtk4::Align::Start);
    path_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    path_label.add_css_class("wallpaper-path");

    let browse_btn = Button::with_label("Browse…");
    let cfg = config.clone();
    let lbl = path_label.clone();
    let win = window.clone();
    browse_btn.connect_clicked(move |_| {
        let dialog = gtk4::FileChooserNative::new(
            Some("Choose Wallpaper"),
            Some(&win),
            gtk4::FileChooserAction::Open,
            Some("Select"),
            Some("Cancel"),
        );

        let filter = gtk4::FileFilter::new();
        filter.add_mime_type("image/png");
        filter.add_mime_type("image/jpeg");
        filter.add_mime_type("image/webp");
        filter.add_mime_type("image/bmp");
        filter.set_name(Some("Images"));
        dialog.add_filter(&filter);

        let cfg = cfg.clone();
        let lbl = lbl.clone();
        dialog.connect_response(move |dlg, response| {
            if response == gtk4::ResponseType::Accept {
                if let Some(file) = dlg.file() {
                    if let Some(path) = file.path() {
                        let path_str = path.to_string_lossy().to_string();
                        lbl.set_label(&path_str);
                        cfg.borrow_mut().wallpaper.path = path_str;
                    }
                }
            }
        });

        dialog.show();
    });

    let clear_btn = Button::with_label("Clear");
    let cfg = config.clone();
    let lbl = path_label.clone();
    clear_btn.connect_clicked(move |_| {
        cfg.borrow_mut().wallpaper.path = String::new();
        lbl.set_label("(none — solid color)");
    });

    path_row.append(&path_label);
    path_row.append(&browse_btn);
    path_row.append(&clear_btn);
    page.append(&path_row);

    // Wallpaper mode
    let mode_row = setting_row("Mode");
    let modes = StringList::new(&["fill", "center", "stretch", "fit", "tile"]);
    let mode_dropdown = DropDown::new(Some(modes), gtk4::Expression::NONE);
    let idx = match config.borrow().wallpaper.mode.as_str() {
        "fill" => 0,
        "center" => 1,
        "stretch" => 2,
        "fit" => 3,
        "tile" => 4,
        _ => 0,
    };
    mode_dropdown.set_selected(idx);
    let cfg = config.clone();
    mode_dropdown.connect_selected_notify(move |dd| {
        let mode = match dd.selected() {
            0 => "fill",
            1 => "center",
            2 => "stretch",
            3 => "fit",
            4 => "tile",
            _ => "fill",
        };
        cfg.borrow_mut().wallpaper.mode = mode.to_string();
    });
    mode_row.append(&mode_dropdown);
    page.append(&mode_row);

    // Background color
    let color_row = setting_row("Background Color");
    let color_entry = Entry::new();
    color_entry.set_text(&config.borrow().wallpaper.color);
    color_entry.set_max_width_chars(10);
    let cfg = config.clone();
    color_entry.connect_changed(move |e| {
        cfg.borrow_mut().wallpaper.color = e.text().to_string();
    });
    color_row.append(&color_entry);
    page.append(&color_row);

    // Preview hint
    let hint = Label::new(Some("Changes apply after clicking Apply. Panel will hot-reload."));
    hint.add_css_class("settings-hint");
    hint.set_halign(gtk4::Align::Start);
    hint.set_margin_top(12);
    page.append(&hint);

    page
}

// ─── Helpers ─────────────────────────────────────────────────────

fn setting_row(label_text: &str) -> GtkBox {
    let row = GtkBox::new(Orientation::Horizontal, 12);
    row.set_margin_bottom(4);
    let label = Label::new(Some(label_text));
    label.set_halign(gtk4::Align::Start);
    label.set_width_chars(16);
    row.append(&label);
    row
}

// ─── Display Arrangement Drawing ─────────────────────────────────

fn parse_mode_dimensions(
    mode_str: &str,
    info: &rdm_common::display::DisplayInfo,
) -> (u32, u32) {
    // Try parsing from config mode string "WIDTHxHEIGHT@RATE"
    if !mode_str.is_empty() {
        let res_part = mode_str.split('@').next().unwrap_or("");
        if let Some((w, h)) = res_part.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>()) {
                return (w, h);
            }
        }
    }
    // Fallback: use current mode from detected info
    if let Some(m) = info.modes.iter().find(|m| m.current) {
        return (m.width, m.height);
    }
    // Fallback: first available mode
    if let Some(m) = info.modes.first() {
        return (m.width, m.height);
    }
    (1920, 1080)
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -std::f64::consts::FRAC_PI_2, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, std::f64::consts::FRAC_PI_2);
    cr.arc(
        x + r,
        y + h - r,
        r,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + r,
        y + r,
        r,
        std::f64::consts::PI,
        3.0 * std::f64::consts::FRAC_PI_2,
    );
    cr.close_path();
}

fn draw_arrangement(
    cr: &cairo::Context,
    canvas_w: i32,
    canvas_h: i32,
    state: &mut ArrangementState,
) {
    let cw = canvas_w as f64;
    let ch = canvas_h as f64;

    // Background
    cr.set_source_rgb(0.086, 0.086, 0.118); // #16161e
    cr.rectangle(0.0, 0.0, cw, ch);
    let _ = cr.fill();

    if state.rects.is_empty() {
        return;
    }

    // Compute bounding box of all monitors in display coordinates
    let mut min_x = i32::MAX as f64;
    let mut min_y = i32::MAX as f64;
    let mut max_x = i32::MIN as f64;
    let mut max_y = i32::MIN as f64;

    for rect in &state.rects {
        let rx = rect.x as f64;
        let ry = rect.y as f64;
        let rw = rect.width as f64;
        let rh = rect.height as f64;
        if rx < min_x {
            min_x = rx;
        }
        if ry < min_y {
            min_y = ry;
        }
        if rx + rw > max_x {
            max_x = rx + rw;
        }
        if ry + rh > max_y {
            max_y = ry + rh;
        }
    }

    let bbox_w = (max_x - min_x).max(1.0);
    let bbox_h = (max_y - min_y).max(1.0);

    let pad = 20.0;
    let scale_x = (cw - 2.0 * pad) / bbox_w;
    let scale_y = (ch - 2.0 * pad) / bbox_h;
    let scale = scale_x.min(scale_y);

    let offset_x = pad + (cw - 2.0 * pad - bbox_w * scale) / 2.0;
    let offset_y = pad + (ch - 2.0 * pad - bbox_h * scale) / 2.0;

    // Cache transform for hit-testing and drag
    state.render_scale = scale;
    state.render_offset_x = offset_x;
    state.render_offset_y = offset_y;
    state.render_min_x = min_x;
    state.render_min_y = min_y;

    // Draw each monitor
    for rect in &state.rects {
        let rx = (rect.x as f64 - min_x) * scale + offset_x;
        let ry = (rect.y as f64 - min_y) * scale + offset_y;
        let rw = rect.width as f64 * scale;
        let rh = rect.height as f64 * scale;

        if !rect.enabled {
            // Disabled: dimmed fill, dashed border
            rounded_rect(cr, rx, ry, rw, rh, 4.0);
            cr.set_source_rgb(0.231, 0.259, 0.380); // #3b4261
            let _ = cr.fill_preserve();
            cr.set_source_rgb(0.337, 0.373, 0.478); // #565f89
            cr.set_dash(&[4.0, 4.0], 0.0);
            cr.set_line_width(1.5);
            let _ = cr.stroke();
            cr.set_dash(&[], 0.0);
        } else {
            // Fill
            rounded_rect(cr, rx, ry, rw, rh, 4.0);
            cr.set_source_rgb(0.161, 0.180, 0.259); // #292e42
            let _ = cr.fill_preserve();

            // Border
            let is_dragging = state.drag_index == Some(rect.index);
            if is_dragging {
                cr.set_source_rgb(0.733, 0.604, 0.969); // #bb9af7 purple
                cr.set_line_width(2.5);
            } else {
                cr.set_source_rgb(0.478, 0.635, 0.969); // #7aa2f7 blue
                cr.set_line_width(2.0);
            }
            let _ = cr.stroke();
        }

        // Monitor name (centered in upper portion)
        cr.set_font_size(12.0 * (scale * 8.0).min(1.0).max(0.5));
        cr.set_source_rgb(0.753, 0.792, 0.961); // #c0caf5
        if let Ok(extents) = cr.text_extents(&rect.name) {
            let tx = rx + (rw - extents.width()) / 2.0 - extents.x_bearing();
            let ty = ry + rh / 2.0 - 4.0;
            cr.move_to(tx, ty);
            let _ = cr.show_text(&rect.name);
        }

        // Resolution text (below name)
        let res_text = format!("{}x{}", rect.width, rect.height);
        cr.set_font_size(9.0 * (scale * 8.0).min(1.0).max(0.5));
        cr.set_source_rgb(0.337, 0.373, 0.478); // #565f89
        if let Ok(extents) = cr.text_extents(&res_text) {
            let tx = rx + (rw - extents.width()) / 2.0 - extents.x_bearing();
            let ty = ry + rh / 2.0 + 10.0;
            cr.move_to(tx, ty);
            let _ = cr.show_text(&res_text);
        }
    }
}

fn snap_to_edges(rects: &mut [MonitorRect], drag_idx: usize, threshold: i32) {
    let dx = rects[drag_idx].x;
    let dy = rects[drag_idx].y;
    let dw = rects[drag_idx].width as i32;
    let dh = rects[drag_idx].height as i32;

    let mut best_snap_x: Option<i32> = None;
    let mut best_snap_y: Option<i32> = None;
    let mut best_dist_x = threshold + 1;
    let mut best_dist_y = threshold + 1;

    for (i, other) in rects.iter().enumerate() {
        if i == drag_idx || !other.enabled {
            continue;
        }
        let ox = other.x;
        let oy = other.y;
        let ow = other.width as i32;
        let oh = other.height as i32;

        // Horizontal snapping
        // Dragged left -> Other right
        let dist = (dx - (ox + ow)).abs();
        if dist < best_dist_x {
            best_dist_x = dist;
            best_snap_x = Some(ox + ow);
        }
        // Dragged right -> Other left
        let dist = ((dx + dw) - ox).abs();
        if dist < best_dist_x {
            best_dist_x = dist;
            best_snap_x = Some(ox - dw);
        }
        // Left-to-left alignment
        let dist = (dx - ox).abs();
        if dist < best_dist_x {
            best_dist_x = dist;
            best_snap_x = Some(ox);
        }
        // Right-to-right alignment
        let dist = ((dx + dw) - (ox + ow)).abs();
        if dist < best_dist_x {
            best_dist_x = dist;
            best_snap_x = Some(ox + ow - dw);
        }

        // Vertical snapping
        // Dragged top -> Other bottom
        let dist = (dy - (oy + oh)).abs();
        if dist < best_dist_y {
            best_dist_y = dist;
            best_snap_y = Some(oy + oh);
        }
        // Dragged bottom -> Other top
        let dist = ((dy + dh) - oy).abs();
        if dist < best_dist_y {
            best_dist_y = dist;
            best_snap_y = Some(oy - dh);
        }
        // Top-to-top alignment
        let dist = (dy - oy).abs();
        if dist < best_dist_y {
            best_dist_y = dist;
            best_snap_y = Some(oy);
        }
        // Bottom-to-bottom alignment
        let dist = ((dy + dh) - (oy + oh)).abs();
        if dist < best_dist_y {
            best_dist_y = dist;
            best_snap_y = Some(oy + oh - dh);
        }
    }

    if let Some(snap_x) = best_snap_x {
        if best_dist_x <= threshold {
            rects[drag_idx].x = snap_x;
        }
    }
    if let Some(snap_y) = best_snap_y {
        if best_dist_y <= threshold {
            rects[drag_idx].y = snap_y;
        }
    }
}

// ─── Displays Settings ──────────────────────────────────────────

fn build_displays_page(config: &Rc<RefCell<RdmConfig>>) -> GtkBox {
    let page = GtkBox::new(Orientation::Vertical, 8);
    page.set_margin_top(20);
    page.set_margin_bottom(20);
    page.set_margin_start(20);
    page.set_margin_end(20);

    let header = Label::new(Some("Displays"));
    header.add_css_class("settings-header");
    header.set_halign(gtk4::Align::Start);
    page.append(&header);

    // Wrap in a scrolled window for many monitors
    let scrolled = gtk4::ScrolledWindow::new();
    scrolled.set_vexpand(true);
    scrolled.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

    let inner = GtkBox::new(Orientation::Vertical, 8);

    // Query current display state
    let displays = match rdm_common::display::query_displays() {
        Ok(d) => d,
        Err(e) => {
            let err = Label::new(Some(&format!("Failed to detect displays: {}", e)));
            err.add_css_class("settings-hint");
            err.set_halign(gtk4::Align::Start);
            inner.append(&err);

            let hint = Label::new(Some(
                "Display detection requires wlr-randr and a compatible compositor (labwc).",
            ));
            hint.add_css_class("settings-hint");
            hint.set_halign(gtk4::Align::Start);
            inner.append(&hint);

            scrolled.set_child(Some(&inner));
            page.append(&scrolled);
            return page;
        }
    };

    if displays.is_empty() {
        let msg = Label::new(Some("No displays detected."));
        msg.add_css_class("settings-hint");
        msg.set_halign(gtk4::Align::Start);
        inner.append(&msg);
        scrolled.set_child(Some(&inner));
        page.append(&scrolled);
        return page;
    }

    // Merge detected displays with any saved config
    let saved = config.borrow().displays.clone();
    let merged = rdm_common::display::merge_with_saved(&displays, &saved);
    config.borrow_mut().displays = merged;

    // ── Build arrangement state ──
    let arrangement = Rc::new(RefCell::new(ArrangementState::new()));
    for (i, info) in displays.iter().enumerate() {
        let mode_str = config.borrow().displays[i].mode.clone();
        let (w, h) = parse_mode_dimensions(&mode_str, info);
        let pos_str = config.borrow().displays[i].position.clone();
        let pos_parts: Vec<i32> = pos_str
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        let x = pos_parts.first().copied().unwrap_or(0);
        let y = pos_parts.get(1).copied().unwrap_or(0);
        let enabled = config.borrow().displays[i].enabled;

        arrangement.borrow_mut().rects.push(MonitorRect {
            index: i,
            name: info.name.clone(),
            width: w,
            height: h,
            x,
            y,
            enabled,
        });
    }

    // ── Drawing area ──
    let drawing_area = gtk4::DrawingArea::new();
    drawing_area.set_content_width(460);
    drawing_area.set_content_height(220);
    drawing_area.add_css_class("display-arrangement");

    let arr_draw = arrangement.clone();
    drawing_area.set_draw_func(move |_widget, cr, width, height| {
        draw_arrangement(cr, width, height, &mut arr_draw.borrow_mut());
    });

    page.append(&drawing_area);

    // ── Build display sections (collecting spinbutton pairs) ──
    let spin_pairs: Rc<RefCell<Vec<(gtk4::SpinButton, gtk4::SpinButton)>>> =
        Rc::new(RefCell::new(Vec::new()));

    for (i, info) in displays.iter().enumerate() {
        let (x_spin, y_spin) = build_display_section(
            &inner,
            config,
            info,
            i,
            &arrangement,
            &drawing_area,
        );
        spin_pairs.borrow_mut().push((x_spin, y_spin));
    }

    // ── GestureDrag controller for arrangement ──
    let drag = gtk4::GestureDrag::new();

    // drag_begin: hit-test and start tracking
    let arr_begin = arrangement.clone();
    drag.connect_drag_begin(move |_gesture, start_x, start_y| {
        let mut arr = arr_begin.borrow_mut();
        if let Some(idx) = arr.hit_test(start_x, start_y) {
            arr.drag_index = Some(idx);
            arr.drag_origin_x = arr.rects[idx].x;
            arr.drag_origin_y = arr.rects[idx].y;
        }
    });

    // drag_update: move the monitor
    let arr_update = arrangement.clone();
    let cfg_update = config.clone();
    let spins_update = spin_pairs.clone();
    let da_update = drawing_area.clone();
    drag.connect_drag_update(move |_gesture, offset_x, offset_y| {
        let drag_idx;
        let origin_x;
        let origin_y;
        let scale;
        {
            let arr = arr_update.borrow();
            drag_idx = match arr.drag_index {
                Some(idx) => idx,
                None => return,
            };
            origin_x = arr.drag_origin_x;
            origin_y = arr.drag_origin_y;
            scale = arr.render_scale;
        }

        if scale <= 0.0 {
            return;
        }

        let new_x = origin_x + (offset_x / scale) as i32;
        let new_y = origin_y + (offset_y / scale) as i32;

        // Update arrangement state
        {
            let mut arr = arr_update.borrow_mut();
            arr.rects[drag_idx].x = new_x;
            arr.rects[drag_idx].y = new_y;
            arr.syncing = true;
        }

        // Update config
        cfg_update.borrow_mut().displays[drag_idx].position =
            format!("{},{}", new_x, new_y);

        // Update spinbuttons
        {
            let spins = spins_update.borrow();
            if let Some((ref x_spin, ref y_spin)) = spins.get(drag_idx) {
                x_spin.set_value(new_x as f64);
                y_spin.set_value(new_y as f64);
            }
        }

        arr_update.borrow_mut().syncing = false;
        da_update.queue_draw();
    });

    // drag_end: snap to edges
    let arr_end = arrangement.clone();
    let cfg_end = config.clone();
    let spins_end = spin_pairs.clone();
    let da_end = drawing_area.clone();
    drag.connect_drag_end(move |_gesture, _offset_x, _offset_y| {
        let drag_idx;
        {
            let arr = arr_end.borrow();
            drag_idx = match arr.drag_index {
                Some(idx) => idx,
                None => return,
            };
        }

        // Snap
        {
            let mut arr = arr_end.borrow_mut();
            snap_to_edges(&mut arr.rects, drag_idx, 50);
            arr.syncing = true;
        }

        // Read snapped position
        let snapped_x = arr_end.borrow().rects[drag_idx].x;
        let snapped_y = arr_end.borrow().rects[drag_idx].y;

        // Update config with snapped position
        cfg_end.borrow_mut().displays[drag_idx].position =
            format!("{},{}", snapped_x, snapped_y);

        // Update spinbuttons
        {
            let spins = spins_end.borrow();
            if let Some((ref x_spin, ref y_spin)) = spins.get(drag_idx) {
                x_spin.set_value(snapped_x as f64);
                y_spin.set_value(snapped_y as f64);
            }
        }

        {
            let mut arr = arr_end.borrow_mut();
            arr.syncing = false;
            arr.drag_index = None;
        }

        da_end.queue_draw();
    });

    drawing_area.add_controller(drag);

    scrolled.set_child(Some(&inner));
    page.append(&scrolled);

    // Hint
    let hint = Label::new(Some(
        "Display changes are applied via wlr-randr when you click Apply.",
    ));
    hint.add_css_class("settings-hint");
    hint.set_halign(gtk4::Align::Start);
    hint.set_margin_top(8);
    page.append(&hint);

    page
}

fn build_display_section(
    container: &GtkBox,
    config: &Rc<RefCell<RdmConfig>>,
    info: &rdm_common::display::DisplayInfo,
    index: usize,
    arrangement: &Rc<RefCell<ArrangementState>>,
    drawing_area: &gtk4::DrawingArea,
) -> (gtk4::SpinButton, gtk4::SpinButton) {
    // Separator between monitors
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.set_margin_top(8);
    sep.set_margin_bottom(4);
    container.append(&sep);

    // Monitor name + description
    let name_text = if info.description.is_empty() {
        info.name.clone()
    } else {
        format!("{} — {}", info.name, info.description)
    };
    let name_label = Label::new(Some(&name_text));
    name_label.add_css_class("display-name");
    name_label.set_halign(gtk4::Align::Start);
    container.append(&name_label);

    // --- Enable/Disable switch ---
    let enable_row = setting_row("Enabled");
    let enable_switch = Switch::new();
    enable_switch.set_active(config.borrow().displays[index].enabled);
    enable_switch.set_valign(gtk4::Align::Center);
    let cfg = config.clone();
    let arr = arrangement.clone();
    let da = drawing_area.clone();
    enable_switch.connect_active_notify(move |sw| {
        cfg.borrow_mut().displays[index].enabled = sw.is_active();
        arr.borrow_mut().rects[index].enabled = sw.is_active();
        da.queue_draw();
    });
    enable_row.append(&enable_switch);
    container.append(&enable_row);

    // --- Resolution dropdown ---
    // Collect unique resolutions
    let mut resolutions: Vec<(u32, u32)> = Vec::new();
    for m in &info.modes {
        let res = (m.width, m.height);
        if !resolutions.contains(&res) {
            resolutions.push(res);
        }
    }

    let res_strings: Vec<String> = resolutions
        .iter()
        .map(|(w, h)| format!("{}x{}", w, h))
        .collect();
    let res_str_refs: Vec<&str> = res_strings.iter().map(|s| s.as_str()).collect();

    let res_row = setting_row("Resolution");
    let res_list = StringList::new(&res_str_refs);
    let res_dropdown = DropDown::new(Some(res_list), gtk4::Expression::NONE);

    // Find current resolution from saved mode
    let current_mode = config.borrow().displays[index].mode.clone();
    let current_res = current_mode.split('@').next().unwrap_or("").to_string();
    let res_idx = res_strings
        .iter()
        .position(|s| *s == current_res)
        .unwrap_or(0) as u32;
    res_dropdown.set_selected(res_idx);

    // --- Refresh rate dropdown ---
    let rate_row = setting_row("Refresh Rate");

    // Get rates for the currently selected resolution
    let selected_res = resolutions.get(res_idx as usize).copied().unwrap_or((0, 0));
    let rates: Vec<f64> = info
        .modes
        .iter()
        .filter(|m| m.width == selected_res.0 && m.height == selected_res.1)
        .map(|m| m.refresh)
        .collect();
    let rate_strings: Vec<String> = rates.iter().map(|r| format!("{:.0} Hz", r)).collect();
    let rate_str_refs: Vec<&str> = rate_strings.iter().map(|s| s.as_str()).collect();

    let rate_list = StringList::new(&rate_str_refs);
    let rate_dropdown = DropDown::new(Some(rate_list), gtk4::Expression::NONE);

    // Find current rate
    let current_rate_str = current_mode
        .split('@')
        .nth(1)
        .unwrap_or("")
        .to_string();
    let rate_idx = rates
        .iter()
        .position(|r| format!("{:.0}", r) == current_rate_str)
        .unwrap_or(0) as u32;
    rate_dropdown.set_selected(rate_idx);

    // Store info we need in closures for resolution/rate linking
    let modes_for_res = info.modes.clone();
    let resolutions_for_res = resolutions.clone();

    // When resolution changes, rebuild the rate dropdown options and update config
    let cfg = config.clone();
    let rate_dd = rate_dropdown.clone();
    let modes_clone = modes_for_res.clone();
    let res_clone = resolutions_for_res.clone();
    let arr = arrangement.clone();
    let da = drawing_area.clone();
    res_dropdown.connect_selected_notify(move |dd| {
        let sel = dd.selected() as usize;
        if let Some(&(w, h)) = res_clone.get(sel) {
            // Rebuild rate list for this resolution
            let new_rates: Vec<f64> = modes_clone
                .iter()
                .filter(|m| m.width == w && m.height == h)
                .map(|m| m.refresh)
                .collect();
            let new_rate_strings: Vec<String> =
                new_rates.iter().map(|r| format!("{:.0} Hz", r)).collect();
            let new_refs: Vec<&str> = new_rate_strings.iter().map(|s| s.as_str()).collect();
            let new_list = StringList::new(&new_refs);
            rate_dd.set_model(Some(&new_list));
            rate_dd.set_selected(0);

            // Update config with the first available rate
            let rate = new_rates.first().copied().unwrap_or(60.0);
            cfg.borrow_mut().displays[index].mode =
                format!("{}x{}@{:.0}", w, h, rate);

            // Update arrangement rect dimensions
            arr.borrow_mut().rects[index].width = w;
            arr.borrow_mut().rects[index].height = h;
            da.queue_draw();
        }
    });

    // When rate changes, update config
    let cfg = config.clone();
    let modes_for_rate = modes_for_res.clone();
    let res_dd = res_dropdown.clone();
    let res_for_rate = resolutions_for_res.clone();
    rate_dropdown.connect_selected_notify(move |dd| {
        let res_sel = res_dd.selected() as usize;
        if let Some(&(w, h)) = res_for_rate.get(res_sel) {
            let rates: Vec<f64> = modes_for_rate
                .iter()
                .filter(|m| m.width == w && m.height == h)
                .map(|m| m.refresh)
                .collect();
            let rate_sel = dd.selected() as usize;
            if let Some(&rate) = rates.get(rate_sel) {
                cfg.borrow_mut().displays[index].mode =
                    format!("{}x{}@{:.0}", w, h, rate);
            }
        }
    });

    res_row.append(&res_dropdown);
    container.append(&res_row);

    rate_row.append(&rate_dropdown);
    container.append(&rate_row);

    // --- Position X, Y ---
    let pos_row = setting_row("Position");
    let current_pos = config.borrow().displays[index].position.clone();
    let pos_parts: Vec<i32> = current_pos
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let pos_x = pos_parts.first().copied().unwrap_or(0);
    let pos_y = pos_parts.get(1).copied().unwrap_or(0);

    let x_label = Label::new(Some("X:"));
    pos_row.append(&x_label);
    let x_adj = gtk4::Adjustment::new(pos_x as f64, -8192.0, 8192.0, 1.0, 10.0, 0.0);
    let x_spin = gtk4::SpinButton::new(Some(&x_adj), 1.0, 0);
    x_spin.set_width_chars(6);

    let y_label = Label::new(Some("Y:"));
    let y_adj = gtk4::Adjustment::new(pos_y as f64, -8192.0, 8192.0, 1.0, 10.0, 0.0);
    let y_spin = gtk4::SpinButton::new(Some(&y_adj), 1.0, 0);
    y_spin.set_width_chars(6);

    // X spinbutton handler: update config + arrangement
    let cfg = config.clone();
    let arr = arrangement.clone();
    let da = drawing_area.clone();
    let y_spin_ref = y_spin.clone();
    x_spin.connect_value_changed(move |spin| {
        if arr.borrow().syncing {
            return;
        }
        let x = spin.value() as i32;
        let y = y_spin_ref.value() as i32;
        cfg.borrow_mut().displays[index].position = format!("{},{}", x, y);
        arr.borrow_mut().rects[index].x = x;
        da.queue_draw();
    });

    // Y spinbutton handler: update config + arrangement
    let cfg = config.clone();
    let arr = arrangement.clone();
    let da = drawing_area.clone();
    let x_spin_ref = x_spin.clone();
    y_spin.connect_value_changed(move |spin| {
        if arr.borrow().syncing {
            return;
        }
        let x = x_spin_ref.value() as i32;
        let y = spin.value() as i32;
        cfg.borrow_mut().displays[index].position = format!("{},{}", x, y);
        arr.borrow_mut().rects[index].y = y;
        da.queue_draw();
    });

    pos_row.append(&x_spin);
    pos_row.append(&y_label);
    pos_row.append(&y_spin);
    container.append(&pos_row);

    // --- Scale ---
    let scale_row = setting_row("Scale");
    let scale_adj = gtk4::Adjustment::new(
        config.borrow().displays[index].scale,
        0.5,
        3.0,
        0.25,
        0.5,
        0.0,
    );
    let scale_spin = gtk4::SpinButton::new(Some(&scale_adj), 0.25, 2);
    let cfg = config.clone();
    scale_spin.connect_value_changed(move |spin| {
        cfg.borrow_mut().displays[index].scale = spin.value();
    });
    scale_row.append(&scale_spin);
    container.append(&scale_row);

    // --- Transform ---
    let transform_row = setting_row("Rotation");
    let transforms = StringList::new(&[
        "normal",
        "90",
        "180",
        "270",
        "flipped",
        "flipped-90",
        "flipped-180",
        "flipped-270",
    ]);
    let transform_dropdown = DropDown::new(Some(transforms), gtk4::Expression::NONE);
    let current_transform = config.borrow().displays[index].transform.clone();
    let transform_idx = match current_transform.as_str() {
        "normal" => 0,
        "90" => 1,
        "180" => 2,
        "270" => 3,
        "flipped" => 4,
        "flipped-90" => 5,
        "flipped-180" => 6,
        "flipped-270" => 7,
        _ => 0,
    };
    transform_dropdown.set_selected(transform_idx);
    let cfg = config.clone();
    transform_dropdown.connect_selected_notify(move |dd| {
        let transform = match dd.selected() {
            0 => "normal",
            1 => "90",
            2 => "180",
            3 => "270",
            4 => "flipped",
            5 => "flipped-90",
            6 => "flipped-180",
            7 => "flipped-270",
            _ => "normal",
        };
        cfg.borrow_mut().displays[index].transform = transform.to_string();
    });
    transform_row.append(&transform_dropdown);
    container.append(&transform_row);

    (x_spin, y_spin)
}

/// Apply changes: save config and hot-reload the session.
/// Display changes are applied first via wlr-randr so the panel sees the correct layout.
fn apply_changes(config: &RdmConfig) {
    // Apply display configuration first (before hot-reload restarts the panel)
    if !config.displays.is_empty() {
        if let Err(e) = rdm_common::display::apply_display_config(&config.displays) {
            log::error!("Failed to apply display config: {}", e);
        }
    }

    // Hot-reload: rdm-session kills all children and restarts them.
    // swaybg args are built from rdm.toml, so wallpaper is applied automatically.
    let _ = std::process::Command::new("rdm-reload").status();
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        window {
            background-color: #1a1b26;
            color: #c0caf5;
        }

        .settings-header {
            font-size: 18px;
            font-weight: bold;
            color: #7aa2f7;
            margin-bottom: 4px;
        }

        .settings-hint {
            color: #565f89;
            font-size: 11px;
            font-style: italic;
        }

        .wallpaper-path {
            color: #a9b1d6;
            font-size: 12px;
        }

        .display-name {
            font-size: 14px;
            font-weight: bold;
            color: #bb9af7;
            margin-top: 8px;
        }

        .display-arrangement {
            border: 1px solid #3b4261;
            border-radius: 8px;
            margin-bottom: 12px;
        }

        stacksidebar {
            background-color: #16161e;
        }

        stacksidebar row {
            color: #c0caf5;
            padding: 8px 12px;
        }

        stacksidebar row:selected {
            background-color: #3d59a1;
            color: #ffffff;
        }

        button {
            background-color: #292e42;
            color: #c0caf5;
            border: 1px solid #3b4261;
            border-radius: 6px;
            padding: 4px 12px;
            min-height: 0;
        }

        button:hover {
            background-color: #3b4261;
        }

        button.suggested-action {
            background-color: #7aa2f7;
            color: #1a1b26;
            border: none;
        }

        button.suggested-action:hover {
            background-color: #89b4fa;
        }

        entry, spinbutton {
            background-color: #292e42;
            color: #c0caf5;
            border: 1px solid #3b4261;
            border-radius: 6px;
            padding: 4px 8px;
            min-height: 24px;
        }

        dropdown, dropdown button {
            background-color: #292e42;
            color: #c0caf5;
            border: 1px solid #3b4261;
            border-radius: 6px;
        }

        switch {
            background-color: #3b4261;
        }

        switch:checked {
            background-color: #7aa2f7;
        }

        separator {
            background-color: #3b4261;
            min-height: 1px;
            min-width: 1px;
        }

        label {
            color: #c0caf5;
        }
    "#,
    );

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
