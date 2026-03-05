use qmetaobject::prelude::*;
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

/// Color hex for battery level
fn battery_color_hex(capacity: u8, charging: bool) -> &'static str {
    if charging {
        "#7dcfff"
    } else if capacity <= 10 {
        "#f7768e"
    } else if capacity <= 25 {
        "#e0af68"
    } else {
        "#9ece6a"
    }
}

// ─── QML-exposed tray backend ────────────────────────────────────

#[derive(QObject, Default)]
pub struct TrayBackend {
    base: qt_base_class!(trait QObject),

    tray_label: qt_property!(QString; NOTIFY tray_changed),
    battery_menu_label: qt_property!(QString; NOTIFY tray_changed),
    battery_color: qt_property!(QString; NOTIFY tray_changed),
    battery_present: qt_property!(bool; NOTIFY tray_changed),

    tray_changed: qt_signal!(),

    update_battery: qt_method!(fn(&mut self)),
    lock: qt_method!(fn(&self)),
    logout: qt_method!(fn(&self)),
    reboot: qt_method!(fn(&self)),
    shutdown: qt_method!(fn(&self)),
}

impl TrayBackend {
    pub fn new() -> Self {
        let bat = read_battery();
        let mut backend = Self::default();
        backend.apply_battery(&bat);
        backend
    }

    fn apply_battery(&mut self, bat: &BatteryState) {
        if bat.present {
            let icon = battery_icon(bat.capacity, bat.charging);
            self.tray_label = QString::from(format!("{} {}%", icon, bat.capacity).as_str());
            let status = if bat.charging { "Charging" } else { "Battery" };
            self.battery_menu_label =
                QString::from(format!("{}  {} {}%", icon, status, bat.capacity).as_str());
            self.battery_color = QString::from(battery_color_hex(bat.capacity, bat.charging));
            self.battery_present = true;
        } else {
            self.tray_label = QString::from("\u{f0425}"); // 󰐥
            self.battery_menu_label = QString::from("AC Power");
            self.battery_color = QString::from("#9ece6a");
            self.battery_present = false;
        }
    }

    fn update_battery(&mut self) {
        let bat = read_battery();
        self.apply_battery(&bat);
        self.tray_changed();
    }

    fn lock(&self) {
        if let Err(e) = std::process::Command::new("swaylock").spawn() {
            log::error!("Failed to lock: {}", e);
        }
    }

    fn logout(&self) {
        let _ = std::process::Command::new("labwc").arg("--exit").spawn();
    }

    fn reboot(&self) {
        let _ = std::process::Command::new("systemctl").arg("reboot").spawn();
    }

    fn shutdown(&self) {
        let _ = std::process::Command::new("systemctl")
            .arg("poweroff")
            .spawn();
    }
}
