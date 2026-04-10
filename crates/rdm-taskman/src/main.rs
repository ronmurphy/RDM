use gtk4::prelude::*;
use gtk4::Application;

mod layout_app;
mod procfs;
mod services;

fn main() {
    env_logger::init();

    let app = Application::builder()
        .application_id("org.rdm.taskman")
        .build();

    app.connect_activate(|app| {
        let builder = gtk4::Builder::from_string(include_str!("../layout.ui"));

        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("Task Manager")
            .default_width(880)
            .default_height(640)
            .build();

        if let Some(root) = builder.object::<gtk4::Widget>("main_box") {
            window.set_child(Some(&root));
        }
        if let Some(hb) = builder.object::<gtk4::HeaderBar>("header_bar") {
            hb.unparent();
            window.set_titlebar(Some(&hb));
        }

        layout_app::setup(&builder, &window);

        window.present();
    });

    app.run();
}
