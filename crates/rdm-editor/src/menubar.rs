use gtk4::prelude::*;
use gtk4::gio::{Menu, MenuItem, SimpleAction};
use gtk4::Application;

/// Build the application menu bar and register all actions on `app`.
///
/// Returns the `PopoverMenuBar` widget to be placed at the top of the window.
pub fn build(app: &Application) -> gtk4::PopoverMenuBar {
    let menubar_model = Menu::new();

    // ── File ─────────────────────────────────────────────────────
    let file_menu = Menu::new();
    file_menu.append(Some("New"),     Some("app.new-tab"));
    file_menu.append(Some("Open…"),  Some("app.open"));
    file_menu.append(Some("Save"),    Some("app.save"));
    file_menu.append(Some("Save As…"), Some("app.save-as"));
    file_menu.append(Some("Close Tab"), Some("app.close-tab"));

    let file_item = MenuItem::new(Some("File"), None);
    file_item.set_submenu(Some(&file_menu));
    menubar_model.append_item(&file_item);

    // ── Edit ─────────────────────────────────────────────────────
    let edit_menu = Menu::new();
    edit_menu.append(Some("Cut"),        Some("app.cut"));
    edit_menu.append(Some("Copy"),       Some("app.copy"));
    edit_menu.append(Some("Paste"),      Some("app.paste"));
    edit_menu.append(Some("Select All"), Some("app.select-all"));
    edit_menu.append(Some("Find…"),      Some("app.find"));

    let edit_item = MenuItem::new(Some("Edit"), None);
    edit_item.set_submenu(Some(&edit_menu));
    menubar_model.append_item(&edit_item);

    // ── View ─────────────────────────────────────────────────────
    let view_menu = Menu::new();
    view_menu.append(Some("Toggle Sidebar"),  Some("app.toggle-sidebar"));
    view_menu.append(Some("Toggle Output"),   Some("app.toggle-output"));
    view_menu.append(Some("Toggle Preview"),  Some("app.toggle-preview"));

    let view_item = MenuItem::new(Some("View"), None);
    view_item.set_submenu(Some(&view_menu));
    menubar_model.append_item(&view_item);

    // ── Run ──────────────────────────────────────────────────────
    let run_menu = Menu::new();
    run_menu.append(Some("Run"),          Some("app.run"));
    run_menu.append(Some("Build"),        Some("app.build"));
    run_menu.append(Some("Stop"),         Some("app.stop"));
    run_menu.append(Some("Open in Browser"), Some("app.open-browser"));

    let run_item = MenuItem::new(Some("Run"), None);
    run_item.set_submenu(Some(&run_menu));
    menubar_model.append_item(&run_item);

    // ── Help ─────────────────────────────────────────────────────
    let help_menu = Menu::new();
    help_menu.append(Some("About rdm-editor"), Some("app.about"));

    let help_item = MenuItem::new(Some("Help"), None);
    help_item.set_submenu(Some(&help_menu));
    menubar_model.append_item(&help_item);

    // Register stub actions (wired later by app.rs via add_action callbacks).
    for name in &[
        "new-tab", "open", "save", "save-as", "close-tab",
        "cut", "copy", "paste", "select-all", "find",
        "toggle-sidebar", "toggle-output", "toggle-preview",
        "run", "build", "stop", "open-browser",
        "about",
    ] {
        let action = SimpleAction::new(name, None);
        app.add_action(&action);
    }

    gtk4::PopoverMenuBar::from_model(Some(&menubar_model))
}

/// Convenience: register a typed callback for a named app action.
pub fn connect_action<F>(app: &Application, name: &str, cb: F)
where
    F: Fn() + 'static,
{
    if let Some(action) = app.lookup_action(name) {
        if let Some(sa) = action.downcast_ref::<SimpleAction>() {
            sa.connect_activate(move |_, _| cb());
        }
    }
}
