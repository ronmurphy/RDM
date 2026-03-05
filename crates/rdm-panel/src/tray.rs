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

/// Build the system tray: single menu button with battery info + session submenu.
pub fn setup_tray(app: &gtk4::Application) -> gtk4::MenuButton {
    let tray_btn = gtk4::MenuButton::new();
    tray_btn.add_css_class("tray-btn");
    tray_btn.add_css_class("nerd-icon");

    let menu = gtk4::gio::Menu::new();

    // --- Battery section (if present) ---
    let bat = read_battery();
    if bat.present {
        let battery_section = gtk4::gio::Menu::new();
        let bat_label = battery_menu_label(&bat);
        // Battery item is just informational — uses a no-op action
        battery_section.append(Some(&bat_label), Some("app.battery-info"));
        menu.append_section(None, &battery_section);

        // No-op action for battery display
        let battery_action = gtk4::gio::SimpleAction::new("battery-info", None);
        battery_action.set_enabled(false);
        app.add_action(&battery_action);

        // Update the tray button label with battery info and refresh menu periodically
        update_tray_button(&tray_btn, &bat);

        let btn_clone = tray_btn.clone();
        let menu_ref = menu.clone();
        gtk4::glib::timeout_add_local(std::time::Duration::from_secs(30), move || {
            let bat = read_battery();
            update_tray_button(&btn_clone, &bat);
            // Update battery item label — replace section 0 entirely
            if menu_ref.n_items() > 0 {
                menu_ref.remove(0);
                let bat_section = gtk4::gio::Menu::new();
                bat_section.append(Some(&battery_menu_label(&bat)), Some("app.battery-info"));
                menu_ref.insert_section(0, None, &bat_section);
            }
            gtk4::glib::ControlFlow::Continue
        });
    } else {
        // No battery — just show power icon
        tray_btn.set_label("\u{f0425}"); // 󰐥
    }

    // --- WiFi submenu ---
    let wifi_submenu = crate::wifi::build_wifi_submenu(app);
    menu.append_submenu(Some("\u{f05a9}  WiFi"), &wifi_submenu);

    // --- Session submenu ---
    let session_submenu = gtk4::gio::Menu::new();
    session_submenu.append(Some("\u{f033e}  Lock"), Some("app.lock"));
    session_submenu.append(Some("\u{f0343}  Logout"), Some("app.logout"));
    session_submenu.append(Some("\u{f0709}  Reboot"), Some("app.reboot"));
    session_submenu.append(Some("\u{f0425}  Shutdown"), Some("app.shutdown"));

    menu.append_submenu(Some("\u{f0425}  Session"), &session_submenu);

    tray_btn.set_menu_model(Some(&menu));

    // --- Wire up session actions ---
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

    tray_btn
}

fn battery_menu_label(bat: &BatteryState) -> String {
    let icon = battery_icon(bat.capacity, bat.charging);
    let status = if bat.charging { "Charging" } else { "Battery" };
    format!("{}  {} {}%", icon, status, bat.capacity)
}

fn update_tray_button(btn: &gtk4::MenuButton, bat: &BatteryState) {
    let icon = battery_icon(bat.capacity, bat.charging);
    btn.set_label(&format!("{} {}%", icon, bat.capacity));

    // Color the button based on battery state
    for cls in &["battery-normal", "battery-low", "battery-critical", "battery-charging"] {
        btn.remove_css_class(cls);
    }
    btn.add_css_class(battery_css_class(bat.capacity, bat.charging));

    let status = if bat.charging { "Charging" } else { "On battery" };
    btn.set_tooltip_text(Some(&format!("{}  —  {}%", status, bat.capacity)));
}
