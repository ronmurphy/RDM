use gtk4::prelude::*;
use gtk4::Application;

mod app;
mod config;
mod find;
mod menubar;
mod notebook;
mod output;
mod runner;
mod schemes;
mod sidebar;
mod statusbar;
mod tab;

#[cfg(feature = "preview")]
mod preview;

fn main() {
    env_logger::init();

    // Generate a GtkSourceView colour scheme that matches the active RDM theme.
    // This writes ~/.local/share/gtksourceview-4/styles/rdm-theme.xml before GTK
    // initialises so the scheme is available as soon as the StyleSchemeManager starts.
    schemes::generate_rdm_scheme();

    let app = Application::builder()
        .application_id("org.rdm.editor")
        .build();

    app.connect_activate(|application| {
        // Load RDM theme CSS (or fall back to system GTK theme silently).
        load_css();

        // Collect any file paths passed on the command line.
        let args: Vec<String> = std::env::args().skip(1).collect();
        let paths: Vec<std::path::PathBuf> = args
            .iter()
            .map(std::path::PathBuf::from)
            .filter(|p| p.exists())
            .collect();

        app::build_ui(application, paths);
    });

    app.run();
}

fn load_css() {
    let css = gtk4::CssProvider::new();
    css.load_from_data(&rdm_common::theme::load_theme_css());
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}
