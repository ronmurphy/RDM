use chrono::Local;
use gtk4::glib;
use gtk4::prelude::*;
use rdm_panel_api::RdmPluginInfo;
use std::cell::{Cell, RefCell};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

// ── Config ────────────────────────────────────────────────────────────────────

struct Config {
    time_format: String,
    date_format: String,
    show_brightness: bool,
    show_bluetooth: bool,
}

impl Config {
    fn from_toml(src: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(src) = src else { return cfg };
        let Ok(val) = src.parse::<toml::Value>() else { return cfg };
        if let Some(v) = val.get("time_format").and_then(|v| v.as_str()) {
            cfg.time_format = v.to_owned();
        }
        if let Some(v) = val.get("date_format").and_then(|v| v.as_str()) {
            cfg.date_format = v.to_owned();
        }
        if let Some(v) = val.get("show_brightness").and_then(|v| v.as_bool()) {
            cfg.show_brightness = v;
        }
        if let Some(v) = val.get("show_bluetooth").and_then(|v| v.as_bool()) {
            cfg.show_bluetooth = v;
        }
        cfg
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            time_format: "%I:%M %p".to_owned(),
            date_format: "%A, %B %-d".to_owned(),
            show_brightness: true,
            show_bluetooth: true,
        }
    }
}

// ── Plugin instance storage ───────────────────────────────────────────────────

struct CmdCenterPlugin {
    #[allow(dead_code)]
    button: gtk4::MenuButton,
}

thread_local! {
    static INSTANCES: RefCell<Vec<CmdCenterPlugin>> = RefCell::new(Vec::new());
}

// ── C-ABI exports ─────────────────────────────────────────────────────────────

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"cmdcenter".as_ptr(),
        version: 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_new_instance(
    config_toml: *const std::ffi::c_char,
) -> *mut gtk4::ffi::GtkWidget {
    unsafe { gtk4::set_initialized(); }
    let cfg = if config_toml.is_null() {
        Config::default()
    } else {
        let s = unsafe { std::ffi::CStr::from_ptr(config_toml).to_str().ok() };
        Config::from_toml(s)
    };
    let button = build_widget(cfg);
    let raw = button.upcast_ref::<gtk4::Widget>().as_ptr();
    INSTANCES.with(|v| v.borrow_mut().push(CmdCenterPlugin { button }));
    raw
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_remove_instances() {
    INSTANCES.with(|v| v.borrow_mut().clear());
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_exit() {
    INSTANCES.with(|v| v.borrow_mut().clear());
}

// ── WiFi helpers ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct WifiNetwork {
    ssid: String,
    signal: u8,
    security: String,
    in_use: bool,
}

fn scan_networks() -> Vec<WifiNetwork> {
    let Ok(out) = Command::new("nmcli")
        .args(["-t", "-f", "SSID,SIGNAL,SECURITY,IN-USE", "dev", "wifi", "list"])
        .output()
    else { return Vec::new(); };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut seen = std::collections::HashSet::new();
    let mut networks = Vec::new();
    for line in stdout.lines() {
        let parts: Vec<&str> = line.rsplitn(4, ':').collect();
        if parts.len() < 4 { continue; }
        let in_use   = parts[0].trim() == "*";
        let security = parts[1].trim().to_string();
        let signal: u8 = parts[2].trim().parse().unwrap_or(0);
        let ssid     = parts[3].trim().to_string();
        if ssid.is_empty() || !seen.insert(ssid.clone()) { continue; }
        networks.push(WifiNetwork { ssid, signal, security, in_use });
    }
    networks.sort_by(|a, b| b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal)));
    networks
}

fn is_known_network(ssid: &str) -> bool {
    Command::new("nmcli").args(["-t", "-f", "NAME", "con", "show"]).output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == ssid))
        .unwrap_or(false)
}

fn connect_known(ssid: &str) -> Result<(), String> {
    let out = Command::new("nmcli").args(["con", "up", ssid]).output()
        .map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

fn connect_new(ssid: &str, password: &str) -> Result<(), String> {
    let out = Command::new("nmcli")
        .args(["dev", "wifi", "connect", ssid, "password", password])
        .output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

fn signal_icon(signal: u8, in_use: bool) -> &'static str {
    if in_use { "󰖩" } else {
        match signal {
            0..=25  => "󰤟",
            26..=50 => "󰤢",
            51..=75 => "󰤥",
            _       => "󰤨",
        }
    }
}

fn show_password_dialog(ssid: String, result_lbl: gtk4::Label) {
    let win = gtk4::Window::builder()
        .title(format!("Connect to {}", ssid))
        .default_width(340)
        .resizable(false)
        .build();
    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16); vbox.set_margin_bottom(16);
    vbox.set_margin_start(16); vbox.set_margin_end(16);
    let lbl = gtk4::Label::new(Some(&format!("Password for \"{}\"", ssid)));
    vbox.append(&lbl);
    let entry = gtk4::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("WiFi password"));
    vbox.append(&entry);
    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    let cancel  = gtk4::Button::with_label("Cancel");
    let connect = gtk4::Button::with_label("Connect");
    connect.add_css_class("suggested-action");
    btn_row.append(&cancel);
    btn_row.append(&connect);
    vbox.append(&btn_row);
    let err_lbl = gtk4::Label::new(None);
    err_lbl.add_css_class("wifi-error");
    err_lbl.set_visible(false);
    vbox.append(&err_lbl);
    win.set_child(Some(&vbox));
    let win_c = win.clone();
    cancel.connect_clicked(move |_| win_c.close());
    let win_c2  = win.clone();
    let entry_c = entry.clone();
    let err_c   = err_lbl.clone();
    let ssid_c  = ssid.clone();
    let res_c   = result_lbl.clone();
    let do_connect = move || {
        let password = entry_c.text().to_string();
        if password.is_empty() { return; }
        let ssid_thread = ssid_c.clone();
        let ssid_msg    = ssid_c.clone();
        let pw2  = password.clone();
        let win3 = win_c2.clone();
        let err3 = err_c.clone();
        let res3 = res_c.clone();
        let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
        std::thread::spawn(move || { let _ = tx.send_blocking(connect_new(&ssid_thread, &pw2)); });
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(())) => { res3.set_text(&format!("  {} (connected)", ssid_msg)); win3.close(); }
                Ok(Err(e)) => { err3.set_text(&format!("Failed: {}", e)); err3.set_visible(true); }
                Err(_) => { win3.close(); }
            }
        });
    };
    let do_connect_c = do_connect.clone();
    connect.connect_clicked(move |_| do_connect_c());
    entry.connect_activate(move |_| do_connect());
    win.present();
}

// ── Volume / brightness / bluetooth helpers ───────────────────────────────────

fn read_volume() -> Option<(f64, bool)> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let txt = String::from_utf8(out.stdout).ok()?;
    let mut parts = txt.split_whitespace();
    parts.next();
    let raw: f64 = parts.next()?.parse().ok()?;
    Some(((raw * 100.0).clamp(0.0, 100.0), txt.contains("[MUTED]")))
}

fn set_volume(pct: f64) {
    let _ = Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{:.0}%", pct)])
        .status();
}

fn toggle_mute() {
    let _ = Command::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]).status();
}

fn read_brightness() -> Option<f64> {
    let cur: f64 = String::from_utf8(Command::new("brightnessctl").arg("get").output().ok()?.stdout).ok()?.trim().parse().ok()?;
    let max: f64 = String::from_utf8(Command::new("brightnessctl").arg("max").output().ok()?.stdout).ok()?.trim().parse().ok()?;
    if max == 0.0 { return None; }
    Some((cur / max * 100.0).clamp(0.0, 100.0))
}

fn set_brightness(pct: f64) {
    let _ = Command::new("brightnessctl").args(["set", &format!("{:.0}%", pct)]).status();
}

fn read_bluetooth() -> bool {
    Command::new("bluetoothctl").arg("show").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).lines()
            .find(|l| l.trim().starts_with("Powered:"))
            .map(|l| l.contains("yes")).unwrap_or(false))
        .unwrap_or(false)
}

fn set_bluetooth(on: bool) {
    let _ = Command::new("bluetoothctl").args(["power", if on { "on" } else { "off" }]).status();
}

fn read_tiling_enabled() -> bool {
    Command::new("rdm-snap").arg("keybinds-status").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
        .unwrap_or(false)
}

fn update_tiling_config(enabled: bool) {
    let config_path = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
                .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        })
        .join("rdm/rdm.toml");
    let Ok(content) = std::fs::read_to_string(&config_path) else { return };
    // Simple line-based update/insert for tiling_enabled
    let key = "tiling_enabled";
    let new_line = format!("{} = {}", key, enabled);
    if content.contains(key) {
        let updated: String = content.lines().map(|l| {
            if l.trim().starts_with(key) { new_line.as_str() } else { l }
        }).collect::<Vec<_>>().join("\n");
        let _ = std::fs::write(&config_path, updated);
    } else if content.contains("[snap]") {
        let updated = content.replace("[snap]", &format!("[snap]\n{}", new_line));
        let _ = std::fs::write(&config_path, updated);
    } else {
        let updated = format!("{}\n\n[snap]\n{}\n", content.trim_end(), new_line);
        let _ = std::fs::write(&config_path, updated);
    }
}

// ── Battery helpers ───────────────────────────────────────────────────────────

struct BatteryState {
    capacity: u8,
    charging: bool,
}

fn find_battery_path() -> Option<std::path::PathBuf> {
    let ps_dir = std::path::Path::new("/sys/class/power_supply");
    let mut entries: Vec<_> = std::fs::read_dir(ps_dir).ok()?.flatten().collect();
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

fn read_battery() -> Option<BatteryState> {
    let base = find_battery_path()?;
    let capacity: u8 = std::fs::read_to_string(base.join("capacity"))
        .ok()?.trim().parse().unwrap_or(0);
    let status = std::fs::read_to_string(base.join("status")).unwrap_or_default();
    let charging = matches!(status.trim(), "Charging" | "Full");
    Some(BatteryState { capacity, charging })
}

fn battery_icon(capacity: u8, charging: bool) -> &'static str {
    if charging {
        match capacity {
            0..=20  => "\u{f089c}",
            21..=30 => "\u{f0086}",
            31..=40 => "\u{f0087}",
            41..=60 => "\u{f0088}",
            61..=70 => "\u{f0089}",
            71..=80 => "\u{f089f}",
            81..=90 => "\u{f008a}",
            _       => "\u{f0085}",
        }
    } else {
        match capacity {
            0..=5   => "\u{f008e}",
            6..=10  => "\u{f007a}",
            11..=20 => "\u{f007b}",
            21..=30 => "\u{f007c}",
            31..=40 => "\u{f007d}",
            41..=50 => "\u{f007e}",
            51..=60 => "\u{f007f}",
            61..=70 => "\u{f0080}",
            71..=80 => "\u{f0081}",
            81..=90 => "\u{f0082}",
            _       => "\u{f0079}",
        }
    }
}

fn run_power_action(action: &str) {
    match action {
        "lock"     => { let _ = Command::new("rdm-lock").spawn(); }
        "logout"   => { let _ = Command::new("pkill").arg("labwc").spawn(); }
        "shutdown" => { let _ = Command::new("systemctl").arg("poweroff").spawn(); }
        "reboot"   => { let _ = Command::new("systemctl").arg("reboot").spawn(); }
        _ => {}
    }
}

// ── Layout helpers ────────────────────────────────────────────────────────────

fn section_box() -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    b.add_css_class("cmdcenter-section");
    b
}

fn compact_tile() -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    b.add_css_class("cmdcenter-section");
    b.add_css_class("cmdcenter-tile");
    b.set_hexpand(true);
    b
}

fn row_label(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.add_css_class("cmdcenter-row-label");
    l.set_halign(gtk4::Align::Start);
    l
}

// ── WiFi expandable section ───────────────────────────────────────────────────
// Returns (section_box, status_label, rescan_fn)
// The caller decides when to first scan (deferred until user opens it).

fn build_wifi_list() -> (gtk4::Box, gtk4::Label, Rc<dyn Fn()>) {
    let section = section_box();

    let header_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let header_lbl = row_label("WI-FI NETWORKS");
    header_lbl.set_hexpand(true);
    let scan_btn = gtk4::Button::with_label("↺");
    scan_btn.add_css_class("tray-btn");
    scan_btn.set_tooltip_text(Some("Rescan networks"));
    header_row.append(&header_lbl);
    header_row.append(&scan_btn);
    section.append(&header_row);

    let status_lbl = gtk4::Label::new(Some("  Tap ↺ to scan"));
    status_lbl.set_halign(gtk4::Align::Start);
    status_lbl.add_css_class("settings-hint");
    section.append(&status_lbl);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("cmdcenter-wifi-list");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_max_content_height(180);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));
    section.append(&scroll);

    let scanning_lbl = gtk4::Label::new(Some("  Scanning…"));
    scanning_lbl.add_css_class("settings-hint");
    scanning_lbl.set_halign(gtk4::Align::Start);
    scanning_lbl.set_visible(false);
    section.append(&scanning_lbl);

    let do_scan: Rc<dyn Fn()> = {
        let list_c     = list.clone();
        let scanning_c = scanning_lbl.clone();
        let status_c   = status_lbl.clone();
        Rc::new(move || {
            while let Some(row) = list_c.first_child() { list_c.remove(&row); }
            scanning_c.set_visible(true);
            let list2     = list_c.clone();
            let scanning2 = scanning_c.clone();
            let status2   = status_c.clone();
            let (tx, rx) = async_channel::bounded::<Vec<WifiNetwork>>(1);
            std::thread::spawn(move || { let _ = tx.send_blocking(scan_networks()); });
            glib::spawn_future_local(async move {
                let networks = rx.recv().await.unwrap_or_default();
                scanning2.set_visible(false);
                if let Some(active) = networks.iter().find(|n| n.in_use) {
                    status2.set_text(&format!("  {} (connected)", active.ssid));
                } else {
                    status2.set_text("  Not connected");
                }
                for net in networks.iter().take(12) {
                    let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                    row_box.set_margin_top(4); row_box.set_margin_bottom(4);
                    row_box.set_margin_start(4); row_box.set_margin_end(4);
                    let icon = gtk4::Label::new(Some(signal_icon(net.signal, net.in_use)));
                    icon.set_width_chars(2);
                    row_box.append(&icon);
                    let name = gtk4::Label::new(Some(&net.ssid));
                    name.set_hexpand(true);
                    name.set_halign(gtk4::Align::Start);
                    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    name.set_max_width_chars(22);
                    row_box.append(&name);
                    if net.security.contains("WPA") || net.security.contains("WEP") {
                        let lock = gtk4::Label::new(Some("󰌾"));
                        lock.add_css_class("settings-hint");
                        row_box.append(&lock);
                    }
                    let sig_lbl = gtk4::Label::new(Some(&format!("{}%", net.signal)));
                    sig_lbl.add_css_class("settings-hint");
                    sig_lbl.set_width_chars(4);
                    sig_lbl.set_xalign(1.0);
                    row_box.append(&sig_lbl);
                    let row = gtk4::ListBoxRow::new();
                    row.set_child(Some(&row_box));
                    let ssid_c  = net.ssid.clone();
                    let status3 = status2.clone();
                    let gesture = gtk4::GestureClick::new();
                    gesture.connect_released(move |g, _, _, _| {
                        g.set_state(gtk4::EventSequenceState::Claimed);
                        let ssid2   = ssid_c.clone();
                        let status4 = status3.clone();
                        if is_known_network(&ssid2) {
                            let ssid3   = ssid2.clone();
                            let status5 = status4.clone();
                            let (tx2, rx2) = async_channel::bounded::<Result<(), String>>(1);
                            std::thread::spawn(move || { let _ = tx2.send_blocking(connect_known(&ssid3)); });
                            glib::spawn_future_local(async move {
                                match rx2.recv().await {
                                    Ok(Ok(())) => { status5.set_text(&format!("  {} (connected)", ssid2)); }
                                    Ok(Err(e)) => { status5.set_text(&format!("  Failed: {}", e)); }
                                    Err(_) => {}
                                }
                            });
                        } else {
                            show_password_dialog(ssid2, status4);
                        }
                    });
                    row.add_controller(gesture);
                    list2.append(&row);
                }
            });
        })
    };

    {
        let scan_c = do_scan.clone();
        scan_btn.connect_clicked(move |_| scan_c());
    }

    (section, status_lbl, do_scan)
}

// ── Main widget builder ───────────────────────────────────────────────────────

fn build_widget(cfg: Config) -> gtk4::MenuButton {
    let btn = gtk4::MenuButton::new();
    btn.set_label(&Local::now().format(&cfg.time_format).to_string());
    btn.add_css_class("tray-btn");
    btn.add_css_class("task-popup-btn");

    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);
    pop.add_css_class("cmdcenter-popover");

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    root.set_size_request(340, -1);
    root.set_margin_top(8); root.set_margin_bottom(8);
    root.set_margin_start(8); root.set_margin_end(8);

    // ── Time / Date header ────────────────────────────────────────────────────
    let time_lbl = gtk4::Label::new(None);
    time_lbl.add_css_class("cmdcenter-time");
    time_lbl.set_halign(gtk4::Align::Center);
    time_lbl.set_markup(&format!(
        "<span size='xx-large' weight='bold'>{}</span>",
        Local::now().format(&cfg.time_format)
    ));
    let date_lbl = gtk4::Label::new(Some(&Local::now().format(&cfg.date_format).to_string()));
    date_lbl.add_css_class("cmdcenter-date");
    date_lbl.set_halign(gtk4::Align::Center);
    root.append(&time_lbl);
    root.append(&date_lbl);

    // ── Row 1: Volume tile | Brightness tile ──────────────────────────────────
    let row1 = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    // — Volume tile —
    let vol_tile = compact_tile();
    let vol_hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let vol_icon_lbl = gtk4::Label::new(Some("🔊"));
    let vol_title = row_label("VOLUME");
    vol_title.set_hexpand(true);
    let mute_btn = gtk4::Button::with_label("🔇");
    mute_btn.add_css_class("tray-btn");
    mute_btn.set_tooltip_text(Some("Toggle mute"));
    vol_hdr.append(&vol_icon_lbl);
    vol_hdr.append(&vol_title);
    vol_hdr.append(&mute_btn);
    vol_tile.append(&vol_hdr);
    let vol_scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, None::<&gtk4::Adjustment>);
    vol_scale.set_range(0.0, 100.0);
    vol_scale.set_value(50.0);
    vol_scale.set_hexpand(true);
    vol_scale.set_draw_value(false);
    vol_scale.set_increments(1.0, 5.0);
    let vol_pct = gtk4::Label::new(Some(" 50%"));
    vol_pct.set_width_chars(4);
    vol_pct.set_xalign(1.0);
    let vol_slider_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    vol_slider_row.append(&vol_scale);
    vol_slider_row.append(&vol_pct);
    vol_tile.append(&vol_slider_row);
    row1.append(&vol_tile);

    // — Brightness tile —
    let brg_tile = compact_tile();
    brg_tile.set_visible(cfg.show_brightness);
    let brg_hdr = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let brg_icon_lbl = gtk4::Label::new(Some("☀"));
    let brg_title = row_label("BRIGHTNESS");
    brg_title.set_hexpand(true);
    brg_hdr.append(&brg_icon_lbl);
    brg_hdr.append(&brg_title);
    brg_tile.append(&brg_hdr);
    let bright_scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, None::<&gtk4::Adjustment>);
    bright_scale.set_range(0.0, 100.0);
    bright_scale.set_value(100.0);
    bright_scale.set_hexpand(true);
    bright_scale.set_draw_value(false);
    bright_scale.set_increments(1.0, 5.0);
    let bright_pct = gtk4::Label::new(Some("100%"));
    bright_pct.set_width_chars(4);
    bright_pct.set_xalign(1.0);
    let brg_slider_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    brg_slider_row.append(&bright_scale);
    brg_slider_row.append(&bright_pct);
    brg_tile.append(&brg_slider_row);
    row1.append(&brg_tile);
    root.append(&row1);

    // ── Row 2: Bluetooth tile | WiFi tile ──────────────────────────────────────
    let row2 = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    // — Bluetooth tile —
    let bt_tile = compact_tile();
    if !cfg.show_bluetooth { bt_tile.set_visible(false); }
    let bt_icon = gtk4::Label::new(Some("󰂯"));
    bt_icon.set_halign(gtk4::Align::Center);
    bt_icon.add_css_class("cmdcenter-tile-icon");
    let bt_label = row_label("BLUETOOTH");
    bt_label.set_halign(gtk4::Align::Center);
    let bt_state  = Rc::new(Cell::new(false));
    let bt_switch = gtk4::Switch::new();
    bt_switch.set_halign(gtk4::Align::Center);
    bt_switch.set_active(false);
    bt_tile.append(&bt_icon);
    bt_tile.append(&bt_label);
    bt_tile.append(&bt_switch);
    row2.append(&bt_tile);

    // — WiFi tile (tap to reveal network list) —
    let wifi_tile = compact_tile();
    wifi_tile.add_css_class("cmdcenter-tile-clickable");
    let wifi_icon = gtk4::Label::new(Some("󰖩"));
    wifi_icon.set_halign(gtk4::Align::Center);
    wifi_icon.add_css_class("cmdcenter-tile-icon");
    let wifi_label = row_label("WI-FI");
    wifi_label.set_halign(gtk4::Align::Center);
    let wifi_chevron = gtk4::Label::new(Some("▸"));
    wifi_chevron.add_css_class("cmdcenter-row-label");
    wifi_chevron.set_halign(gtk4::Align::Center);
    wifi_tile.append(&wifi_icon);
    wifi_tile.append(&wifi_label);
    wifi_tile.append(&wifi_chevron);
    row2.append(&wifi_tile);
    root.append(&row2);

    // ── WiFi revealer (lazy scan on first open) ────────────────────────────────
    let wifi_revealer = gtk4::Revealer::new();
    wifi_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    wifi_revealer.set_transition_duration(200);
    wifi_revealer.set_reveal_child(false);
    let (wifi_section, _wifi_status, wifi_scan) = build_wifi_list();
    wifi_revealer.set_child(Some(&wifi_section));
    root.append(&wifi_revealer);

    {
        let rev          = wifi_revealer.clone();
        let chev         = wifi_chevron.clone();
        let scan         = wifi_scan.clone();
        let scanned_once = Rc::new(Cell::new(false));
        let gesture      = gtk4::GestureClick::new();
        gesture.connect_released(move |g, _, _, _| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            let showing = !rev.reveals_child();
            rev.set_reveal_child(showing);
            chev.set_text(if showing { "▾" } else { "▸" });
            if showing && !scanned_once.get() {
                scanned_once.set(true);
                scan();
            }
        });
        wifi_tile.add_controller(gesture);
    }

    // ── Row 3: Tiling tile | Battery tile (if laptop) ────────────────────────
    let row3 = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);

    // — Tiling tile —
    let tile_tile = compact_tile();
    let tile_icon = gtk4::Label::new(Some("󰕰"));
    tile_icon.set_halign(gtk4::Align::Center);
    tile_icon.add_css_class("cmdcenter-tile-icon");
    let tile_lbl = gtk4::Label::new(Some("TILING"));
    tile_lbl.add_css_class("cmdcenter-row-label");
    tile_lbl.set_halign(gtk4::Align::Center);
    let tile_switch = gtk4::Switch::new();
    tile_switch.set_halign(gtk4::Align::Center);
    tile_switch.set_valign(gtk4::Align::Center);
    tile_tile.append(&tile_icon);
    tile_tile.append(&tile_lbl);
    tile_tile.append(&tile_switch);
    row3.append(&tile_tile);

    // — Battery tile (only on laptops) —
    let has_battery = find_battery_path().is_some();
    let bat_icon_lbl  = gtk4::Label::new(Some("󰁹"));
    let bat_pct_lbl   = gtk4::Label::new(Some("–%"));
    let bat_level_bar = gtk4::LevelBar::new();
    if has_battery {
        let bat_tile = compact_tile();
        bat_icon_lbl.set_halign(gtk4::Align::Center);
        bat_icon_lbl.add_css_class("cmdcenter-tile-icon");
        let bat_lbl = gtk4::Label::new(Some("BATTERY"));
        bat_lbl.add_css_class("cmdcenter-row-label");
        bat_lbl.set_halign(gtk4::Align::Center);
        bat_pct_lbl.add_css_class("cmdcenter-row-label");
        bat_pct_lbl.set_halign(gtk4::Align::Center);
        bat_level_bar.set_min_value(0.0);
        bat_level_bar.set_max_value(100.0);
        bat_level_bar.set_value(50.0);
        bat_level_bar.set_hexpand(true);
        bat_level_bar.add_css_class("cmdcenter-bat-bar");
        bat_tile.append(&bat_icon_lbl);
        bat_tile.append(&bat_lbl);
        bat_tile.append(&bat_pct_lbl);
        bat_tile.append(&bat_level_bar);
        row3.append(&bat_tile);
    }

    root.append(&row3);

    let tile_updating = Rc::new(Cell::new(false));
    {
        let tu = tile_updating.clone();
        tile_switch.connect_active_notify(move |sw| {
            if tu.get() { return; }
            let enabled = sw.is_active();
            if enabled {
                let _ = Command::new("rdm-snap").arg("enable-keybinds").status();
                // Start the daemon if it isn't already running
                let _ = Command::new("rdm-snap").arg("daemon").spawn();
            } else {
                let _ = Command::new("rdm-snap").arg("disable-keybinds").status();
                // Stop the daemon immediately
                let _ = Command::new("rdm-snap").arg("quit").status();
            }
            // Update rdm.toml
            std::thread::spawn(move || update_tiling_config(enabled));
        });
    }

    // ── Calendar expander ──────────────────────────────────────────────────────
    let cal_bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    cal_bar.add_css_class("cmdcenter-section");
    cal_bar.add_css_class("cmdcenter-tile-clickable");
    let cal_bar_lbl = gtk4::Label::new(Some("📅  Calendar"));
    cal_bar_lbl.add_css_class("cmdcenter-row-label");
    cal_bar_lbl.set_hexpand(true);
    cal_bar_lbl.set_halign(gtk4::Align::Start);
    let cal_chevron = gtk4::Label::new(Some("▸"));
    cal_chevron.add_css_class("cmdcenter-row-label");
    cal_bar.append(&cal_bar_lbl);
    cal_bar.append(&cal_chevron);
    root.append(&cal_bar);

    let cal_revealer = gtk4::Revealer::new();
    cal_revealer.set_transition_type(gtk4::RevealerTransitionType::SlideDown);
    cal_revealer.set_transition_duration(200);
    cal_revealer.set_reveal_child(false);
    let cal_inner = section_box();
    cal_inner.add_css_class("cmdcenter-calendar");
    cal_inner.append(&gtk4::Calendar::new());
    cal_revealer.set_child(Some(&cal_inner));
    root.append(&cal_revealer);

    {
        let rev  = cal_revealer.clone();
        let chev = cal_chevron.clone();
        let gesture = gtk4::GestureClick::new();
        gesture.connect_released(move |g, _, _, _| {
            g.set_state(gtk4::EventSequenceState::Claimed);
            let showing = !rev.reveals_child();
            rev.set_reveal_child(showing);
            chev.set_text(if showing { "▾" } else { "▸" });
        });
        cal_bar.add_controller(gesture);
    }

    // ── Power buttons ──────────────────────────────────────────────────────────
    let power_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    power_row.set_homogeneous(true);
    power_row.set_margin_top(2);
    for (label, destructive, action) in [
        ("🔒 Lock",     false, "lock"),
        ("⏏ Logout",   false, "logout"),
        ("⏻ Shutdown", true,  "shutdown"),
        ("↺ Reboot",   false, "reboot"),
    ] {
        let b = gtk4::Button::with_label(label);
        b.add_css_class("cmdcenter-power-btn");
        if destructive { b.add_css_class("destructive-action"); }
        let pop_w  = pop.downgrade();
        let action = action.to_owned();
        b.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() { p.popdown(); }
            run_power_action(&action);
        });
        power_row.append(&b);
    }
    root.append(&power_row);

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    // ── Clock tick (WeakRef — stops when button is dropped) ───────────────────
    let btn_weak   = btn.downgrade();
    let time_lbl_w = time_lbl.downgrade();
    let date_lbl_w = date_lbl.downgrade();
    let tfmt = cfg.time_format.clone();
    let dfmt = cfg.date_format.clone();
    glib::timeout_add_seconds_local(1, move || {
        let (Some(b), Some(tl), Some(dl)) = (
            btn_weak.upgrade(), time_lbl_w.upgrade(), date_lbl_w.upgrade()
        ) else { return glib::ControlFlow::Break; };
        let now = Local::now();
        b.set_label(&now.format(&tfmt).to_string());
        tl.set_markup(&format!("<span size='xx-large' weight='bold'>{}</span>", now.format(&tfmt)));
        dl.set_text(&now.format(&dfmt).to_string());
        glib::ControlFlow::Continue
    });

    // ── Volume safe debounce (no SourceId::remove) ────────────────────────────
    let vol_updating = Rc::new(Cell::new(false));
    let vol_target   = Rc::new(Cell::new(50.0f64));
    let vol_armed    = Rc::new(Cell::new(false));
    {
        let vu = vol_updating.clone(); let vp = vol_pct.clone();
        let vt = vol_target.clone();   let arm = vol_armed.clone();
        vol_scale.connect_value_changed(move |s| {
            if vu.get() { return; }
            let pct = s.value();
            vp.set_text(&format!("{:3.0}%", pct));
            vt.set(pct);
            if !arm.get() {
                arm.set(true);
                let vt2 = vt.clone(); let arm2 = arm.clone();
                glib::timeout_add_local_once(Duration::from_millis(80), move || {
                    let p = vt2.get(); arm2.set(false);
                    std::thread::spawn(move || set_volume(p));
                });
            }
        });
    }
    // Mute button
    {
        let vs = vol_scale.clone(); let vp = vol_pct.clone(); let vu = vol_updating.clone();
        mute_btn.connect_clicked(move |_| {
            std::thread::spawn(toggle_mute);
            let vs2 = vs.clone(); let vp2 = vp.clone(); let vu2 = vu.clone();
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                let (tx, rx) = async_channel::bounded::<(f64, bool)>(1);
                std::thread::spawn(move || { let _ = tx.send_blocking(read_volume().unwrap_or((0.0, false))); });
                glib::spawn_future_local(async move {
                    if let Ok((pct, _)) = rx.recv().await {
                        vu2.set(true); vs2.set_value(pct); vp2.set_text(&format!("{:3.0}%", pct)); vu2.set(false);
                    }
                });
            });
        });
    }

    // ── Bluetooth switch ──────────────────────────────────────────────────────
    let bt_updating = Rc::new(Cell::new(false));
    if cfg.show_bluetooth {
        let bs = bt_state.clone();
        let bu = bt_updating.clone();
        bt_switch.connect_active_notify(move |sw| {
            if bu.get() { return; }
            let new = sw.is_active();
            if bs.get() == new { return; }
            bs.set(new);
            std::thread::spawn(move || set_bluetooth(new));
        });
    }

    // ── Brightness safe debounce (Rc shared with popover-open handler) ────────
    let brg_updating = Rc::new(Cell::new(false));
    let brg_target   = Rc::new(Cell::new(100.0f64));
    let brg_armed    = Rc::new(Cell::new(false));
    {
        let bu = brg_updating.clone(); let bp = bright_pct.clone();
        let bt = brg_target.clone();   let arm = brg_armed.clone();
        bright_scale.connect_value_changed(move |s| {
            if bu.get() { return; }
            let pct = s.value(); bp.set_text(&format!("{:3.0}%", pct)); bt.set(pct);
            if !arm.get() {
                arm.set(true);
                let bt2 = bt.clone(); let arm2 = arm.clone();
                glib::timeout_add_local_once(Duration::from_millis(80), move || {
                    let p = bt2.get(); arm2.set(false);
                    std::thread::spawn(move || set_brightness(p));
                });
            }
        });
    }

    // ── Popover-open: refresh volume, brightness, bluetooth ───────────────────
    {
        let vs = vol_scale.clone();    let vp = vol_pct.clone();    let vu = vol_updating.clone();
        let bs = bright_scale.clone(); let bp = bright_pct.clone(); let bu = brg_updating.clone();
        let bsec   = brg_tile.clone();
        let bt_sw  = bt_switch.clone();
        let bt_upd = bt_updating.clone();
        let bt_st  = bt_state.clone();
        let tile_sw  = tile_switch.clone();
        let tile_upd = tile_updating.clone();
        btn.connect_notify_local(Some("active"), move |b, _| {
            if !b.is_active() { return; }
            // Volume
            if let Some((pct, _)) = read_volume() {
                vu.set(true); vs.set_value(pct); vp.set_text(&format!("{:3.0}%", pct)); vu.set(false);
            }
            // Brightness
            if let Some(pct) = read_brightness() {
                bu.set(true); bs.set_value(pct); bp.set_text(&format!("{:3.0}%", pct)); bu.set(false);
                bsec.set_visible(true);
            } else {
                bsec.set_visible(false);
            }
            // Bluetooth (async to not block main thread)
            let bt_sw2  = bt_sw.clone();
            let bt_upd2 = bt_upd.clone();
            let bt_st2  = bt_st.clone();
            let (tx, rx) = async_channel::bounded::<bool>(1);
            std::thread::spawn(move || { let _ = tx.send_blocking(read_bluetooth()); });
            glib::spawn_future_local(async move {
                if let Ok(on) = rx.recv().await {
                    bt_upd2.set(true);
                    bt_sw2.set_active(on);
                    bt_st2.set(on);
                    bt_upd2.set(false);
                }
            });
            // Tiling (async)
            let tile_sw2  = tile_sw.clone();
            let tile_upd2 = tile_upd.clone();
            let (tx_t, rx_t) = async_channel::bounded::<bool>(1);
            std::thread::spawn(move || { let _ = tx_t.send_blocking(read_tiling_enabled()); });
            glib::spawn_future_local(async move {
                if let Ok(on) = rx_t.recv().await {
                    tile_upd2.set(true);
                    tile_sw2.set_active(on);
                    tile_upd2.set(false);
                }
            });
            // Battery (sync read — sysfs is fast)
            if has_battery {
                if let Some(bat) = read_battery() {
                    bat_icon_lbl.set_text(battery_icon(bat.capacity, bat.charging));
                    bat_pct_lbl.set_text(&format!("{}%{}", bat.capacity,
                        if bat.charging { " ⚡" } else { "" }));
                    bat_level_bar.set_value(bat.capacity as f64);
                }
            }
        });
    }

    btn
}
