use gtk4::prelude::*;
use std::path::Path;

/// Battery state read from sysfs
struct BatteryState {
    capacity: u8,
    charging: bool,
    present: bool,
}

fn read_battery() -> BatteryState {
    let base = Path::new("/sys/class/power_supply/BAT0");
    if !base.exists() {
        return BatteryState { capacity: 0, charging: false, present: false };
    }

    let capacity = std::fs::read_to_string(base.join("capacity"))
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    let status = std::fs::read_to_string(base.join("status"))
        .unwrap_or_default();
    let charging = status.trim() == "Charging" || status.trim() == "Full";

    BatteryState { capacity, charging, present: true }
}

/// Pick a nerd font battery icon based on level + charging state
fn battery_icon(capacity: u8, charging: bool) -> &'static str {
    if charging {
        match capacity {
            0..=10  => "\u{f089c}",  // 󰢜 battery-charging-10
            11..=20 => "\u{f089c}",  // 󰢜
            21..=30 => "\u{f0086}",  // 󰂆
            31..=40 => "\u{f0087}",  // 󰂇
            41..=50 => "\u{f0088}",  // 󰂈
            51..=60 => "\u{f089e}",  // 󰢞
            61..=70 => "\u{f0089}",  // 󰂉
            71..=80 => "\u{f089f}",  // 󰢟
            81..=90 => "\u{f008a}",  // 󰂊
            _       => "\u{f0085}",  // 󰂅 battery-charging-100
        }
    } else {
        match capacity {
            0..=5   => "\u{f008e}",  // 󰂎 battery-outline
            6..=10  => "\u{f007a}",  // 󰁺 battery-10
            11..=20 => "\u{f007b}",  // 󰁻 battery-20
            21..=30 => "\u{f007c}",  // 󰁼 battery-30
            31..=40 => "\u{f007d}",  // 󰁽 battery-40
            41..=50 => "\u{f007e}",  // 󰁾 battery-50
            51..=60 => "\u{f007f}",  // 󰁿 battery-60
            61..=70 => "\u{f0080}",  // 󰂀 battery-70
            71..=80 => "\u{f0081}",  // 󰂁 battery-80
            81..=90 => "\u{f0082}",  // 󰂂 battery-90
            _       => "\u{f0079}",  // 󰁹 battery-full
        }
    }
}

/// Color for battery level
fn battery_css_class(capacity: u8, charging: bool) -> &'static str {
    if charging {
        "battery-charging"
    } else if capacity <= 10 {
        "battery-critical"
    } else if capacity <= 25 {
        "battery-low"
    } else {
        "battery-normal"
    }
}

/// Build the system tray area: battery label + power/session menu.
/// Returns a Box widget to append to the panel.
pub fn setup_tray(app: &gtk4::Application) -> gtk4::Box {
    let tray_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    tray_box.add_css_class("tray");

    // --- Battery indicator ---
    let bat = read_battery();
    let battery_label = gtk4::Label::new(None);
    battery_label.add_css_class("tray-battery");
    battery_label.add_css_class("nerd-icon");

    if bat.present {
        update_battery_label(&battery_label, &bat);
        tray_box.append(&battery_label);

        // Update every 30 seconds
        let label = battery_label.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_secs(30), move || {
            let bat = read_battery();
            update_battery_label(&label, &bat);
            gtk4::glib::ControlFlow::Continue
        });
    }

    // --- Power / session menu button ---
    let power_btn = gtk4::MenuButton::new();
    power_btn.add_css_class("power-btn");
    power_btn.add_css_class("nerd-icon");
    power_btn.set_label("\u{f0425}"); // 󰐥 power icon (nerd)

    let menu = gtk4::gio::Menu::new();

    // Power section
    let power_section = gtk4::gio::Menu::new();
    power_section.append(Some("\u{f033e}  Lock"), Some("app.lock"));        // 󰌾
    power_section.append(Some("\u{f0343}  Logout"), Some("app.logout"));    // 󰍃
    power_section.append(Some("\u{f0709}  Reboot"), Some("app.reboot"));    // 󰜉
    power_section.append(Some("\u{f0425}  Shutdown"), Some("app.shutdown")); // 󰐥
    menu.append_section(None, &power_section);

    power_btn.set_menu_model(Some(&menu));

    // Wire up actions
    let action_lock = gtk4::gio::SimpleAction::new("lock", None);
    action_lock.connect_activate(|_, _| {
        if let Err(e) = std::process::Command::new("swaylock").spawn() {
            log::error!("Failed to lock: {}", e);
        }
    });

    let action_logout = gtk4::gio::SimpleAction::new("logout", None);
    action_logout.connect_activate(|_, _| {
        let _ = std::process::Command::new("labwc").arg("--exit").spawn();
    });

    let action_reboot = gtk4::gio::SimpleAction::new("reboot", None);
    action_reboot.connect_activate(|_, _| {
        let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
    });

    let action_shutdown = gtk4::gio::SimpleAction::new("shutdown", None);
    action_shutdown.connect_activate(|_, _| {
        let _ = std::process::Command::new("systemctl").arg("poweroff").spawn();
    });

    app.add_action(&action_lock);
    app.add_action(&action_logout);
    app.add_action(&action_reboot);
    app.add_action(&action_shutdown);

    tray_box.append(&power_btn);

    tray_box
}

fn update_battery_label(label: &gtk4::Label, bat: &BatteryState) {
    let icon = battery_icon(bat.capacity, bat.charging);
    label.set_label(&format!("{} {}%", icon, bat.capacity));

    // Update tooltip
    let status = if bat.charging { "Charging" } else { "On battery" };
    label.set_tooltip_text(Some(&format!("{}  —  {}%", status, bat.capacity)));

    // Update CSS class for color
    for cls in &["battery-normal", "battery-low", "battery-critical", "battery-charging"] {
        label.remove_css_class(cls);
    }
    label.add_css_class(battery_css_class(bat.capacity, bat.charging));
}
