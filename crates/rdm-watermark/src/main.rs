use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Label};
use gtk4_layer_shell::{Edge, Layer, LayerShell};
use std::env;

fn main() {
    env_logger::init();
    log::info!("Starting RDM Watermark");

    if !is_rdm_session() {
        log::warn!(
            "Not starting rdm-watermark: non-RDM desktop detected \
             (RDM_SESSION={:?}, XDG_SESSION_TYPE={:?}, XDG_CURRENT_DESKTOP={:?})",
            env::var("RDM_SESSION").ok(),
            env::var("XDG_SESSION_TYPE").ok(),
            env::var("XDG_CURRENT_DESKTOP").ok(),
        );
        return;
    }

    let version = rdm_common::build_version_string();
    log::info!("Version: {}", version);

    let app = Application::builder()
        .application_id("org.rdm.watermark")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| build_watermark(app, &version));
    app.run();
}

fn is_rdm_session() -> bool {
    let has_session_marker = env::var("RDM_SESSION")
        .ok()
        .map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);

    let is_wayland = env::var("XDG_SESSION_TYPE")
        .ok()
        .map(|v| v.trim().eq_ignore_ascii_case("wayland"))
        .unwrap_or(false);

    let has_rdm_desktop_marker = ["XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
        .iter()
        .any(|name| {
            env::var(name)
                .ok()
                .map(|v| v.split(':').any(|p| p.trim().eq_ignore_ascii_case("rdm")))
                .unwrap_or(false)
        });

    has_session_marker && is_wayland && has_rdm_desktop_marker
}

fn build_watermark(app: &Application, version: &str) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Watermark")
        .default_width(1)
        .default_height(1)
        .build();
    window.add_css_class("watermark-window");

    // Layer shell: Bottom layer — sits above wallpaper but below all windows
    window.init_layer_shell();
    window.set_layer(Layer::Bottom);
    window.set_namespace("rdm-watermark");

    // Anchor to bottom-right corner
    window.set_anchor(Edge::Bottom, true);
    window.set_anchor(Edge::Right, true);
    window.set_anchor(Edge::Top, false);
    window.set_anchor(Edge::Left, false);

    // Don't reserve any space
    window.set_exclusive_zone(0);

    // Margin from edges
    window.set_margin(Edge::Bottom, 8);
    window.set_margin(Edge::Right, 12);

    let label = Label::new(Some(version));
    label.add_css_class("watermark");

    window.set_child(Some(&label));

    load_css();
    window.present();
}

fn load_css() {
    let theme_css = rdm_common::theme::load_theme_css();
    // Transparent background is mandatory for watermark, regardless of theme
    let full_css = format!(
        ".watermark-window {{ background-color: transparent; }}\n{}",
        theme_css,
    );

    let css = CssProvider::new();
    css.load_from_data(&full_css);

    // Priority 801 beats the user's ~/.config/gtk-4.0/gtk.css (loaded at 800)
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}
