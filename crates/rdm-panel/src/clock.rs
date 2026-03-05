use chrono::Local;
use gtk4::Label;

pub fn setup_clock(label: &Label, format: &str) {
    let fmt = format.to_string();
    let update = {
        let label = label.clone();
        let fmt = fmt.clone();
        move || {
            let now = Local::now();
            label.set_label(&now.format(&fmt).to_string());
            gtk4::glib::ControlFlow::Continue
        }
    };

    // Initial update
    let now = Local::now();
    label.set_label(&now.format(&fmt).to_string());

    // Update every second
    gtk4::glib::timeout_add_seconds_local(1, update);
}
