use gtk4::prelude::*;

/// Set up the taskbar area — for now shows a placeholder.
/// Full implementation will use wlr-foreign-toplevel-management
/// protocol to list and control running windows.
pub fn setup_taskbar(container: &gtk4::Box) {
    // Placeholder — we'll wire up Wayland foreign-toplevel tracking later
    let placeholder = gtk4::Label::new(Some(""));
    placeholder.set_hexpand(true);
    container.append(&placeholder);

    log::info!("Taskbar initialized (placeholder mode — toplevel tracking coming soon)");
}
