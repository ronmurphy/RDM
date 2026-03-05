use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, CssProvider, Entry, Label, ListBox, ListBoxRow,
    Orientation, ScrolledWindow,
};
use gtk4_layer_shell::{KeyboardMode, Layer, LayerShell};
use rdm_common::config::RdmConfig;
use std::rc::Rc;

mod desktop_apps;

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
    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM Launcher")
        .default_width(config.launcher.width)
        .default_height(config.launcher.height)
        .build();

    // Layer shell setup — overlay that grabs keyboard
    window.init_layer_shell();
    window.set_layer(Layer::Overlay);
    window.set_namespace("rdm-launcher");
    window.set_keyboard_mode(KeyboardMode::Exclusive);

    // Main vertical layout
    let vbox = gtk4::Box::new(Orientation::Vertical, 8);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);
    vbox.add_css_class("launcher");

    // Title
    let title = Label::new(Some("Launch Application"));
    title.add_css_class("launcher-title");
    vbox.append(&title);

    // Search entry
    let search_entry = Entry::new();
    search_entry.set_placeholder_text(Some("Type to search..."));
    search_entry.add_css_class("launcher-search");
    vbox.append(&search_entry);

    // Results list
    let scrolled = ScrolledWindow::new();
    scrolled.set_vexpand(true);

    let list_box = ListBox::new();
    list_box.add_css_class("launcher-list");
    scrolled.set_child(Some(&list_box));
    vbox.append(&scrolled);

    // Load desktop entries
    let entries = Rc::new(desktop_apps::load_desktop_entries());
    log::info!("Loaded {} desktop entries", entries.len());

    // Populate initial list
    populate_list(&list_box, &entries, "");

    // Filter on search
    let list_box_clone = list_box.clone();
    let entries_clone = entries.clone();
    search_entry.connect_changed(move |entry| {
        let query = entry.text().to_string().to_lowercase();
        populate_list(&list_box_clone, &entries_clone, &query);
    });

    // Activate (Enter) launches selected app
    let app_handle = app.clone();
    let entries_for_activate = entries.clone();

    list_box.connect_row_activated(move |_, row| {
        let idx = row.index() as usize;
        if let Some(entry) = filtered_entries(&entries_for_activate, "").get(idx) {
            launch_app(&entry.exec);
            app_handle.quit();
        }
    });

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

    window.set_child(Some(&vbox));

    load_css();

    window.present();
    search_entry.grab_focus();
}

fn populate_list(list_box: &ListBox, entries: &[desktop_apps::AppEntry], query: &str) {
    // Remove existing rows
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let filtered = filtered_entries(entries, query);
    for entry in filtered.iter().take(50) {
        let row = ListBoxRow::new();
        let hbox = gtk4::Box::new(Orientation::Horizontal, 8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);

        let name_label = Label::new(Some(&entry.name));
        name_label.add_css_class("app-name");
        hbox.append(&name_label);

        if let Some(ref comment) = entry.comment {
            let comment_label = Label::new(Some(comment));
            comment_label.add_css_class("app-comment");
            comment_label.set_hexpand(true);
            comment_label.set_halign(gtk4::Align::End);
            hbox.append(&comment_label);
        }

        row.set_child(Some(&hbox));

        // Store exec command for activation
        let exec = entry.exec.clone();
        let list_box_clone = list_box.clone();
        row.connect_activate(move |_| {
            launch_app(&exec);
            if let Some(root) = list_box_clone.root() {
                if let Some(window) = root.downcast_ref::<gtk4::Window>() {
                    window.close();
                }
            }
        });

        list_box.append(&row);
    }
}

fn filtered_entries(entries: &[desktop_apps::AppEntry], query: &str) -> Vec<desktop_apps::AppEntry> {
    if query.is_empty() {
        return entries.to_vec();
    }
    entries
        .iter()
        .filter(|e| {
            e.name.to_lowercase().contains(query)
                || e.comment
                    .as_ref()
                    .map(|c| c.to_lowercase().contains(query))
                    .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn launch_app(exec: &str) {
    // Strip field codes like %f, %u, %F, %U from Exec line
    let cmd: String = exec
        .split_whitespace()
        .filter(|s| !s.starts_with('%'))
        .collect::<Vec<_>>()
        .join(" ");

    log::info!("Launching: {}", cmd);

    if let Err(e) = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        log::error!("Failed to launch '{}': {}", cmd, e);
    }
}

fn load_css() {
    let css = CssProvider::new();
    css.load_from_data(
        r#"
        .launcher {
            background-color: #1a1b26;
            border-radius: 12px;
            color: #c0caf5;
        }

        .launcher-title {
            font-size: 18px;
            font-weight: bold;
            color: #7aa2f7;
            margin-bottom: 4px;
        }

        .launcher-search {
            background-color: #24283b;
            color: #c0caf5;
            border: 1px solid #3b4261;
            border-radius: 8px;
            padding: 8px 12px;
            font-size: 14px;
        }

        .launcher-list {
            background-color: transparent;
        }

        .launcher-list row {
            background-color: transparent;
            border-radius: 6px;
            margin: 1px 0;
        }

        .launcher-list row:selected {
            background-color: #292e42;
        }

        .app-name {
            font-size: 13px;
            color: #c0caf5;
        }

        .app-comment {
            font-size: 11px;
            color: #565f89;
        }
    "#,
    );

    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
