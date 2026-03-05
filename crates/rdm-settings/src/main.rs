use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider,
    DropDown, Entry, Label, Orientation, Switch, StringList,
};
use rdm_common::config::RdmConfig;
use std::cell::RefCell;
use std::rc::Rc;

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

/// Apply changes: save config and hot-reload the session.
/// rdm-session reads wallpaper config from rdm.toml when starting swaybg,
/// so rdm-reload handles both panel and wallpaper changes.
fn apply_changes(_config: &RdmConfig) {
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
