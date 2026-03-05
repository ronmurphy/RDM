mod clock;
mod taskbar;

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Label, Orientation};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use rdm_common::config::RdmConfig;

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

    // Left: launcher button
    let launcher_btn = gtk4::Button::with_label("  Apps  ");
    launcher_btn.add_css_class("launcher-btn");
    launcher_btn.connect_clicked(|_| {
        // Spawn rdm-launcher
        if let Err(e) = std::process::Command::new("rdm-launcher").spawn() {
            log::error!("Failed to launch rdm-launcher: {}", e);
        }
    });
    hbox.append(&launcher_btn);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Vertical);
    hbox.append(&sep);

    // Center: taskbar (running windows)
    let taskbar_box = gtk4::Box::new(Orientation::Horizontal, 4);
    taskbar_box.set_hexpand(true);
    taskbar_box.set_halign(gtk4::Align::Start);
    taskbar_box.add_css_class("taskbar");
    taskbar::setup_taskbar(&taskbar_box);
    hbox.append(&taskbar_box);

    // Right: clock
    if config.panel.show_clock {
        let clock_label = Label::new(None);
        clock_label.add_css_class("clock");
        clock::setup_clock(&clock_label, &config.panel.clock_format);
        hbox.append(&clock_label);
    }

    window.set_child(Some(&hbox));

    // Load CSS
    load_css();

    window.present();
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        .panel {
            background-color: #1a1b26;
            color: #c0caf5;
            padding: 0 8px;
            font-family: "Inter", "Noto Sans", sans-serif;
            font-size: 13px;
        }

        .launcher-btn {
            background: transparent;
            color: #7aa2f7;
            border: none;
            border-radius: 0;
            padding: 4px 12px;
            font-weight: bold;
            min-height: 0;
        }

        .launcher-btn:hover {
            background-color: #292e42;
        }

        .taskbar {
            padding: 2px 8px;
        }

        .taskbar-item {
            background: transparent;
            color: #c0caf5;
            border: none;
            border-radius: 4px;
            padding: 2px 10px;
            min-height: 0;
        }

        .taskbar-item:hover {
            background-color: #292e42;
        }

        .taskbar-item.active {
            background-color: #3d59a1;
            color: #ffffff;
        }

        .clock {
            padding: 4px 12px;
            color: #a9b1d6;
        }

        separator {
            background-color: #3b4261;
            margin: 6px 4px;
            min-width: 1px;
        }
    "#,
    );

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
