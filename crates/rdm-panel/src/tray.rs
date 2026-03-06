use crate::taskbar::TaskbarMode;
use gtk4::prelude::*;
use std::path::Path;
use std::process::Command;

/// Battery state read from sysfs
struct BatteryState {
    capacity: u8,
    charging: bool,
    present: bool,
}

fn read_battery() -> BatteryState {
    let base = match find_battery_path() {
        Some(p) => p,
        None => {
            return BatteryState {
                capacity: 0,
                charging: false,
                present: false,
            }
        }
    };

    let capacity = std::fs::read_to_string(base.join("capacity"))
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .unwrap_or(0);

    let status = std::fs::read_to_string(base.join("status")).unwrap_or_default();
    let charging = status.trim() == "Charging" || status.trim() == "Full";

    BatteryState {
        capacity,
        charging,
        present: true,
    }
}

/// Find the first real battery in /sys/class/power_supply/ (BAT0, BAT1, etc.)
fn find_battery_path() -> Option<std::path::PathBuf> {
    let ps_dir = Path::new("/sys/class/power_supply");
    let mut entries: Vec<_> = std::fs::read_dir(ps_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let dev_type = std::fs::read_to_string(path.join("type")).unwrap_or_default();
        if dev_type.trim() == "Battery" && path.join("capacity").exists() {
            return Some(path);
        }
    }
    None
}

/// Pick a nerd font battery icon based on level + charging state
fn battery_icon(capacity: u8, charging: bool) -> &'static str {
    if charging {
        match capacity {
            0..=10 => "\u{f089c}",  // 󰢜 battery-charging-10
            11..=20 => "\u{f089c}", // 󰢜
            21..=30 => "\u{f0086}", // 󰂆
            31..=40 => "\u{f0087}", // 󰂇
            41..=50 => "\u{f0088}", // 󰂈
            51..=60 => "\u{f089e}", // 󰢞
            61..=70 => "\u{f0089}", // 󰂉
            71..=80 => "\u{f089f}", // 󰢟
            81..=90 => "\u{f008a}", // 󰂊
            _ => "\u{f0085}",       // 󰂅 battery-charging-100
        }
    } else {
        match capacity {
            0..=5 => "\u{f008e}",   // 󰂎 battery-outline
            6..=10 => "\u{f007a}",  // 󰁺 battery-10
            11..=20 => "\u{f007b}", // 󰁻 battery-20
            21..=30 => "\u{f007c}", // 󰁼 battery-30
            31..=40 => "\u{f007d}", // 󰁽 battery-40
            41..=50 => "\u{f007e}", // 󰁾 battery-50
            51..=60 => "\u{f007f}", // 󰁿 battery-60
            61..=70 => "\u{f0080}", // 󰂀 battery-70
            71..=80 => "\u{f0081}", // 󰂁 battery-80
            81..=90 => "\u{f0082}", // 󰂂 battery-90
            _ => "\u{f0079}",       // 󰁹 battery-full
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

/// Read battery state from a UPower D-Bus proxy's cached properties
fn read_upower_state(proxy: &gtk4::gio::DBusProxy) -> BatteryState {
    let capacity = proxy
        .cached_property("Percentage")
        .and_then(|v| v.get::<f64>())
        .unwrap_or(0.0) as u8;
    let state = proxy
        .cached_property("State")
        .and_then(|v| v.get::<u32>())
        .unwrap_or(0);
    // UPower states: 1=Charging, 4=FullyCharged
    let charging = state == 1 || state == 4;
    BatteryState {
        capacity,
        charging,
        present: true,
    }
}

/// Subscribe to UPower D-Bus signals for real-time battery updates.
/// Falls back silently if UPower is not available.
fn subscribe_upower_battery(btn: gtk4::MenuButton, menu: gtk4::gio::Menu, mode: TaskbarMode) {
    let bat_name = match find_battery_path() {
        Some(p) => match p.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => return,
        },
        None => return,
    };
    let device_path = format!("/org/freedesktop/UPower/devices/battery_{}", bat_name);

    let proxy = match gtk4::gio::DBusProxy::for_bus_sync(
        gtk4::gio::BusType::System,
        gtk4::gio::DBusProxyFlags::NONE,
        None,
        "org.freedesktop.UPower",
        &device_path,
        "org.freedesktop.UPower.Device",
        gtk4::gio::Cancellable::NONE,
    ) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("UPower D-Bus unavailable, battery won't auto-update: {}", e);
            return;
        }
    };

    log::info!("Subscribed to UPower battery signals on {}", device_path);

    proxy.connect_local("g-properties-changed", false, move |values| {
        let proxy: gtk4::gio::DBusProxy = values[0].get().unwrap();
        let bat = read_upower_state(&proxy);
        update_tray_button(&btn, &bat, mode);
        if menu.n_items() > 0 {
            menu.remove(0);
            let section = gtk4::gio::Menu::new();
            section.append(
                Some(&battery_menu_label(&bat, mode)),
                Some("app.battery-info"),
            );
            menu.insert_section(0, None, &section);
        }
        None
    });
}

/// Build the system tray: single menu button with battery info + session submenu.
pub fn setup_tray(app: &gtk4::Application, mode: TaskbarMode) -> gtk4::MenuButton {
    let tray_btn = gtk4::MenuButton::new();
    tray_btn.add_css_class("tray-btn");
    if mode == TaskbarMode::Nerd {
        tray_btn.add_css_class("nerd-icon");
    }

    let menu = gtk4::gio::Menu::new();

    // --- Battery section (if present) ---
    let has_battery = find_battery_path().is_some();
    if has_battery {
        let bat = read_battery();
        let battery_section = gtk4::gio::Menu::new();
        battery_section.append(
            Some(&battery_menu_label(&bat, mode)),
            Some("app.battery-info"),
        );
        menu.append_section(None, &battery_section);

        if app.lookup_action("battery-info").is_none() {
            let battery_action = gtk4::gio::SimpleAction::new("battery-info", None);
            battery_action.set_enabled(false);
            app.add_action(&battery_action);
        }

        update_tray_button(&tray_btn, &bat, mode);

        // Subscribe to UPower D-Bus for real-time battery updates
        subscribe_upower_battery(tray_btn.clone(), menu.clone(), mode);
    } else {
        tray_btn.set_label(&mode_label(mode, "\u{f0425}", "\u{23fb}", "Power"));
    }

    // --- WiFi submenu (populated on demand, not at startup) ---
    let wifi_submenu = crate::wifi::build_wifi_submenu(app, mode);
    menu.append_submenu(
        Some(&mode_label(mode, "\u{f05a9}", "\u{1f4f6}", "WiFi")),
        &wifi_submenu,
    );

    // --- Refresh battery + WiFi on menu open (no background polling) ---
    let btn_for_open = tray_btn.clone();
    let menu_for_open = menu.clone();
    let wifi_for_open = wifi_submenu.clone();
    tray_btn.connect_notify_local(Some("active"), move |btn, _| {
        let active: bool = btn.property("active");
        if !active {
            return;
        }
        // Refresh battery
        if has_battery {
            let bat = read_battery();
            update_tray_button(&btn_for_open, &bat, mode);
            if menu_for_open.n_items() > 0 {
                menu_for_open.remove(0);
                let bat_section = gtk4::gio::Menu::new();
                bat_section.append(
                    Some(&battery_menu_label(&bat, mode)),
                    Some("app.battery-info"),
                );
                menu_for_open.insert_section(0, None, &bat_section);
            }
        }
        // Refresh WiFi
        crate::wifi::populate_wifi_menu(&wifi_for_open, mode);
    });

    // --- Session submenu ---
    let session_submenu = gtk4::gio::Menu::new();
    session_submenu.append(
        Some(&mode_label(mode, "\u{f033e}", "\u{1f512}", "Lock")),
        Some("app.lock"),
    );
    session_submenu.append(
        Some(&mode_label(mode, "\u{f0343}", "\u{21aa}", "Logout")),
        Some("app.logout"),
    );
    session_submenu.append(
        Some(&mode_label(mode, "\u{f0709}", "\u{27f3}", "Reboot")),
        Some("app.reboot"),
    );
    session_submenu.append(
        Some(&mode_label(mode, "\u{f0425}", "\u{23fb}", "Shutdown")),
        Some("app.shutdown"),
    );

    menu.append_submenu(
        Some(&mode_label(mode, "\u{f0425}", "\u{2699}", "Session")),
        &session_submenu,
    );

    tray_btn.set_menu_model(Some(&menu));

    // --- Wire up session actions (guard against duplicate registration for multi-monitor) ---
    if app.lookup_action("lock").is_none() {
        let action_lock = gtk4::gio::SimpleAction::new("lock", None);
        action_lock.connect_activate(|_, _| {
            if let Err(e) = std::process::Command::new("swaylock").spawn() {
                log::error!("Failed to lock: {}", e);
                notify_error("Lock failed", &e.to_string());
            }
        });

        let action_logout = gtk4::gio::SimpleAction::new("logout", None);
        action_logout.connect_activate(|_, _| match Command::new("labwc").arg("--exit").status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                let msg = format!("labwc --exit returned {}", status);
                log::error!("{}", msg);
                notify_error("Logout failed", &msg);
            }
            Err(e) => {
                log::error!("Failed to logout: {}", e);
                notify_error("Logout failed", &e.to_string());
            }
        });

        let action_reboot = gtk4::gio::SimpleAction::new("reboot", None);
        action_reboot.connect_activate(|_, _| {
            match Command::new("systemctl").arg("reboot").status() {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    let msg = format!("systemctl reboot returned {}", status);
                    log::error!("{}", msg);
                    notify_error("Reboot failed", &msg);
                }
                Err(e) => {
                    log::error!("Failed to reboot: {}", e);
                    notify_error("Reboot failed", &e.to_string());
                }
            }
        });

        let action_shutdown = gtk4::gio::SimpleAction::new("shutdown", None);
        action_shutdown.connect_activate(|_, _| {
            match Command::new("systemctl").arg("poweroff").status() {
                Ok(status) if status.success() => {}
                Ok(status) => {
                    let msg = format!("systemctl poweroff returned {}", status);
                    log::error!("{}", msg);
                    notify_error("Shutdown failed", &msg);
                }
                Err(e) => {
                    log::error!("Failed to shutdown: {}", e);
                    notify_error("Shutdown failed", &e.to_string());
                }
            }
        });

        app.add_action(&action_lock);
        app.add_action(&action_logout);
        app.add_action(&action_reboot);
        app.add_action(&action_shutdown);
    }

    tray_btn
}

fn notify_error(summary: &str, body: &str) {
    let _ = Command::new("dbus-send")
        .args([
            "--session",
            "--dest=org.freedesktop.Notifications",
            "--type=method_call",
            "/org/freedesktop/Notifications",
            "org.freedesktop.Notifications.Notify",
            "string:rdm-panel",
            "uint32:0",
            "string:",
            &format!("string:{}", summary),
            &format!("string:{}", body),
            "array:string:",
            "dict:string:variant:",
            "int32:4000",
        ])
        .status();
}

fn mode_label(mode: TaskbarMode, nerd_icon: &str, icon_symbol: &str, text: &str) -> String {
    match mode {
        TaskbarMode::Nerd => format!("{}  {}", nerd_icon, text),
        TaskbarMode::Icons => format!("{}  {}", icon_symbol, text),
        TaskbarMode::Text => text.to_string(),
    }
}

fn battery_menu_label(bat: &BatteryState, mode: TaskbarMode) -> String {
    let status = if bat.charging { "Charging" } else { "Battery" };
    if mode == TaskbarMode::Text {
        return format!("{} {}%", status, bat.capacity);
    }

    let icon = battery_icon(bat.capacity, bat.charging);
    let icon = if mode == TaskbarMode::Icons {
        if bat.charging {
            "\u{26a1}\u{1f50b}"
        } else {
            "\u{1f50b}"
        }
    } else {
        icon
    };
    format!("{}  {} {}%", icon, status, bat.capacity)
}

fn update_tray_button(btn: &gtk4::MenuButton, bat: &BatteryState, mode: TaskbarMode) {
    let icon = battery_icon(bat.capacity, bat.charging);
    let label = match mode {
        TaskbarMode::Nerd => format!("{} {}%", icon, bat.capacity),
        TaskbarMode::Icons => {
            let icon = if bat.charging {
                "\u{26a1}\u{1f50b}"
            } else {
                "\u{1f50b}"
            };
            format!("{} {}%", icon, bat.capacity)
        }
        TaskbarMode::Text => format!("Battery {}%", bat.capacity),
    };
    btn.set_label(&label);

    // Color the button based on battery state
    for cls in &[
        "battery-normal",
        "battery-low",
        "battery-critical",
        "battery-charging",
    ] {
        btn.remove_css_class(cls);
    }
    btn.add_css_class(battery_css_class(bat.capacity, bat.charging));

    let status = if bat.charging {
        "Charging"
    } else {
        "On battery"
    };
    btn.set_tooltip_text(Some(&format!("{}  —  {}%", status, bat.capacity)));
}
