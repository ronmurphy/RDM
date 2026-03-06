use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, CssProvider, Label};
use gtk4_layer_shell::{Edge, Layer, LayerShell};

fn main() {
    env_logger::init();
    log::info!("Starting RDM Watermark");

    let version = rdm_common::build_version_string();
    log::info!("Version: {}", version);

    let app = Application::builder()
        .application_id("org.rdm.watermark")
        .build();

    app.connect_activate(move |app| build_watermark(app, &version));
    app.run();
}

fn build_watermark(app: &Application, version: &str) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Watermark")
        .default_width(1)
        .default_height(1)
        .build();

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
        "window.background {{ background-color: transparent; }}\n{}",
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
