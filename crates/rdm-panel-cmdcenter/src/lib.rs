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

// ── WiFi helpers (ported from rdm-panel wifi.rs) ──────────────────────────────

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
        let in_use    = parts[0].trim() == "*";
        let security  = parts[1].trim().to_string();
        let signal: u8 = parts[2].trim().parse().unwrap_or(0);
        let ssid      = parts[3].trim().to_string();
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

/// Show a password entry window and connect on submit.
fn show_password_dialog(ssid: String, result_lbl: gtk4::Label) {
    let win = gtk4::Window::builder()
        .title(format!("Connect to {}", ssid))
        .default_width(340)
        .resizable(false)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let lbl = gtk4::Label::new(Some(&format!("Password for \"{}\"", ssid)));
    vbox.append(&lbl);

    let entry = gtk4::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("WiFi password"));
    vbox.append(&entry);

    let btn_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label("Cancel");
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

    let win_c2    = win.clone();
    let entry_c   = entry.clone();
    let err_c     = err_lbl.clone();
    let ssid_c    = ssid.clone();
    let res_c     = result_lbl.clone();
    let do_connect = move || {
        let password = entry_c.text().to_string();
        if password.is_empty() { return; }
        let ssid2  = ssid_c.clone();
        let pw2    = password.clone();
        let win3   = win_c2.clone();
        let err3   = err_c.clone();
        let res3   = res_c.clone();
        let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
        let ssid_thread = ssid2.clone();
        std::thread::spawn(move || {
            let _ = tx.send_blocking(connect_new(&ssid_thread, &pw2));
        });
        glib::spawn_future_local(async move {
            match rx.recv().await {
                Ok(Ok(())) => {
                    res3.set_text(&format!("  Connected  ({})", ssid2));
                    win3.close();
                }
                Ok(Err(e)) => {
                    err3.set_text(&format!("Failed: {}", e));
                    err3.set_visible(true);
                }
                Err(_) => { win3.close(); }
            }
        });
    };

    let do_connect_c = do_connect.clone();
    connect.connect_clicked(move |_| do_connect_c());
    entry.connect_activate(move |_| do_connect());

    win.present();
}

// ── Volume / brightness helpers ───────────────────────────────────────────────

fn read_volume() -> Option<(f64, bool)> {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output().ok()?;
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
    let _ = Command::new("wpctl")
        .args(["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"])
        .status();
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

fn run_power_action(action: &str) {
    match action {
        "lock"     => { let _ = Command::new("swaylock").args(["-f", "-c", "1a1b26"]).spawn(); }
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

fn row_label(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.add_css_class("cmdcenter-row-label");
    l.set_halign(gtk4::Align::Start);
    l
}

fn slider_row(icon: &str, value: f64, extra: Option<gtk4::Widget>)
    -> (gtk4::Box, gtk4::Scale, gtk4::Label)
{
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let icon_lbl = gtk4::Label::new(Some(icon));
    icon_lbl.set_width_chars(2);
    row.append(&icon_lbl);
    let scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, None::<&gtk4::Adjustment>);
    scale.set_range(0.0, 100.0);
    scale.set_value(value);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    scale.set_increments(1.0, 5.0);
    row.append(&scale);
    let pct = gtk4::Label::new(Some(&format!("{:3.0}%", value)));
    pct.set_width_chars(5);
    pct.set_xalign(1.0);
    row.append(&pct);
    if let Some(w) = extra { row.append(&w); }
    (row, scale, pct)
}

// ── WiFi section widget ───────────────────────────────────────────────────────

/// Builds the WiFi card section. Returns the section box and a status label
/// that shows the current connection (updated on connect).
fn build_wifi_section() -> (gtk4::Box, gtk4::Label) {
    let section = section_box();

    let header_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let header_lbl = row_label("WI-FI");
    header_lbl.set_hexpand(true);
    let scan_btn = gtk4::Button::with_label("↺");
    scan_btn.add_css_class("tray-btn");
    scan_btn.set_tooltip_text(Some("Rescan networks"));
    header_row.append(&header_lbl);
    header_row.append(&scan_btn);
    section.append(&header_row);

    // Current connection status
    let status_lbl = gtk4::Label::new(Some("  Checking…"));
    status_lbl.set_halign(gtk4::Align::Start);
    status_lbl.add_css_class("settings-hint");
    section.append(&status_lbl);

    // Scrollable network list (max ~5 visible rows)
    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    list.add_css_class("cmdcenter-wifi-list");

    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_max_content_height(180);
    scroll.set_propagate_natural_height(true);
    scroll.set_child(Some(&list));
    section.append(&scroll);

    // Scanning state label (shown while scan is in progress)
    let scanning_lbl = gtk4::Label::new(Some("  Scanning…"));
    scanning_lbl.add_css_class("settings-hint");
    scanning_lbl.set_halign(gtk4::Align::Start);
    scanning_lbl.set_visible(false);
    section.append(&scanning_lbl);

    // ── Populate the list ──────────────────────────────────────────────────
    let do_scan = {
        let list_c      = list.clone();
        let scanning_c  = scanning_lbl.clone();
        let status_c    = status_lbl.clone();
        Rc::new(move || {
            // Clear existing rows
            while let Some(row) = list_c.first_child() {
                list_c.remove(&row);
            }
            scanning_c.set_visible(true);

            let list2     = list_c.clone();
            let scanning2 = scanning_c.clone();
            let status2   = status_c.clone();
            let (tx, rx) = async_channel::bounded::<Vec<WifiNetwork>>(1);
            std::thread::spawn(move || {
                let _ = tx.send_blocking(scan_networks());
            });
            glib::spawn_future_local(async move {
                let networks = rx.recv().await.unwrap_or_default();
                scanning2.set_visible(false);

                // Update current status line
                if let Some(active) = networks.iter().find(|n| n.in_use) {
                    status2.set_text(&format!("  {} (connected)", active.ssid));
                } else {
                    status2.set_text("  Not connected");
                }

                // Populate rows
                for net in networks.iter().take(12) {
                    let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
                    row_box.set_margin_top(4);
                    row_box.set_margin_bottom(4);
                    row_box.set_margin_start(4);
                    row_box.set_margin_end(4);

                    let icon = gtk4::Label::new(Some(signal_icon(net.signal, net.in_use)));
                    icon.set_width_chars(2);
                    row_box.append(&icon);

                    let name = gtk4::Label::new(Some(&net.ssid));
                    name.set_hexpand(true);
                    name.set_halign(gtk4::Align::Start);
                    name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                    name.set_max_width_chars(24);
                    row_box.append(&name);

                    // Lock icon for secured networks
                    if net.security.contains("WPA") || net.security.contains("WEP") {
                        let lock = gtk4::Label::new(Some("󰌾"));
                        lock.add_css_class("settings-hint");
                        row_box.append(&lock);
                    }

                    // Signal bar
                    let sig_lbl = gtk4::Label::new(Some(&format!("{}%", net.signal)));
                    sig_lbl.add_css_class("settings-hint");
                    sig_lbl.set_width_chars(4);
                    sig_lbl.set_xalign(1.0);
                    row_box.append(&sig_lbl);

                    let row = gtk4::ListBoxRow::new();
                    row.set_child(Some(&row_box));

                    // Clicking a row connects
                    let ssid_c    = net.ssid.clone();
                    let status3   = status2.clone();
                    let gesture   = gtk4::GestureClick::new();
                    gesture.connect_released(move |g, _, _, _| {
                        g.set_state(gtk4::EventSequenceState::Claimed);
                        let ssid2   = ssid_c.clone();
                        let status4 = status3.clone();
                        if is_known_network(&ssid2) {
                            let ssid3   = ssid2.clone();
                            let status5 = status4.clone();
                            let (tx2, rx2) = async_channel::bounded::<Result<(), String>>(1);
                            std::thread::spawn(move || {
                                let _ = tx2.send_blocking(connect_known(&ssid3));
                            });
                            glib::spawn_future_local(async move {
                                match rx2.recv().await {
                                    Ok(Ok(())) => {
                                        status5.set_text(&format!("  {} (connected)", ssid2));
                                    }
                                    Ok(Err(e)) => {
                                        status5.set_text(&format!("  Failed: {}", e));
                                    }
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

    // Scan button triggers a rescan
    {
        let scan_c = do_scan.clone();
        scan_btn.connect_clicked(move |_| scan_c());
    }

    // Initial scan
    do_scan();

    (section, status_lbl)
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

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    root.set_size_request(320, -1);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(8);
    root.set_margin_end(8);

    // ── Header: time + date ────────────────────────────────────────────────
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

    // ── Calendar ──────────────────────────────────────────────────────────
    let cal_box = section_box();
    cal_box.add_css_class("cmdcenter-calendar");
    cal_box.append(&gtk4::Calendar::new());
    root.append(&cal_box);

    // ── Volume section ────────────────────────────────────────────────────
    let vol_section = section_box();
    vol_section.append(&row_label("VOLUME"));
    let mute_btn = gtk4::Button::with_label("🔇");
    mute_btn.add_css_class("tray-btn");
    mute_btn.set_tooltip_text(Some("Toggle mute"));
    let (_, vol_scale, vol_pct) = slider_row("🔊", 50.0, Some(mute_btn.clone().upcast()));
    vol_section.append(&{
        // re-build the row since slider_row consumed the extra widget
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let icon = gtk4::Label::new(Some("🔊"));
        icon.set_width_chars(2);
        row.append(&icon);
        row.append(&vol_scale);
        let p = vol_pct.clone();
        row.append(&p);
        row.append(&mute_btn);
        row
    });
    root.append(&vol_section);

    // ── Brightness section ────────────────────────────────────────────────
    let bright_section = section_box();
    bright_section.append(&row_label("BRIGHTNESS"));
    bright_section.set_visible(cfg.show_brightness);
    let (bright_row, bright_scale, bright_pct) = slider_row("☀", 100.0, None);
    bright_section.append(&bright_row);
    root.append(&bright_section);

    // ── WiFi section ──────────────────────────────────────────────────────
    let (wifi_section, _wifi_status) = build_wifi_section();
    root.append(&wifi_section);

    // ── Bluetooth row ─────────────────────────────────────────────────────
    let bt_section = section_box();
    let bt_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let bt_lbl = gtk4::Label::new(Some("  Bluetooth"));
    bt_lbl.set_halign(gtk4::Align::Start);
    bt_lbl.set_hexpand(true);
    bt_row.append(&bt_lbl);
    let bt_state = Rc::new(Cell::new(false));
    if cfg.show_bluetooth {
        let bt_btn = gtk4::Button::with_label("Off");
        bt_btn.add_css_class("tray-btn");
        bt_row.append(&bt_btn);
        let bs2 = bt_state.clone();
        let bb2 = bt_btn.clone();
        bt_btn.connect_clicked(move |_| {
            let new = !bs2.get();
            bs2.set(new);
            std::thread::spawn(move || set_bluetooth(new));
            bb2.set_label(if new { "On" } else { "Off" });
        });
        // Store for popover-open refresh
        let bs3 = bt_state.clone();
        let bb3 = bt_btn.clone();
        btn.connect_notify_local(Some("active"), move |b, _| {
            if !b.is_active() { return; }
            let bs4 = bs3.clone();
            let bb4 = bb3.clone();
            let (tx, rx) = async_channel::bounded::<bool>(1);
            std::thread::spawn(move || { let _ = tx.send_blocking(read_bluetooth()); });
            glib::spawn_future_local(async move {
                if let Ok(on) = rx.recv().await {
                    bs4.set(on);
                    bb4.set_label(if on { "On" } else { "Off" });
                }
            });
        });
    }
    bt_section.append(&bt_row);
    root.append(&bt_section);

    // ── Power buttons ──────────────────────────────────────────────────────
    let power_section = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    power_section.set_homogeneous(true);
    power_section.set_margin_top(4);
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
        power_section.append(&b);
    }
    root.append(&power_section);

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    // ── Clock tick ─────────────────────────────────────────────────────────
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

    // ── Volume slider (safe debounce — no SourceId::remove) ───────────────
    let vol_updating = Rc::new(Cell::new(false));
    let vol_target   = Rc::new(Cell::new(50.0f64));
    let vol_armed    = Rc::new(Cell::new(false));
    {
        let vu  = vol_updating.clone();
        let vp  = vol_pct.clone();
        let vt  = vol_target.clone();
        let arm = vol_armed.clone();
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
    {
        let vs2 = vol_scale.clone();
        let vp2 = vol_pct.clone();
        let vu2 = vol_updating.clone();
        mute_btn.connect_clicked(move |_| {
            std::thread::spawn(toggle_mute);
            let vs3 = vs2.clone(); let vp3 = vp2.clone(); let vu3 = vu2.clone();
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                let (tx, rx) = async_channel::bounded::<(f64, bool)>(1);
                std::thread::spawn(move || { let _ = tx.send_blocking(read_volume().unwrap_or((0.0, false))); });
                glib::spawn_future_local(async move {
                    if let Ok((pct, _)) = rx.recv().await {
                        vu3.set(true); vs3.set_value(pct); vp3.set_text(&format!("{:3.0}%", pct)); vu3.set(false);
                    }
                });
            });
        });
    }

    // ── Brightness slider (created before popover-open so we share the Rc) ──
    let brg_updating = Rc::new(Cell::new(false));

    // ── Volume refresh on popover open ────────────────────────────────────
    {
        let vs  = vol_scale.clone();
        let vp  = vol_pct.clone();
        let vu  = vol_updating.clone();
        let bs  = bright_scale.clone();
        let bp  = bright_pct.clone();
        let bu  = brg_updating.clone();
        let bsec = bright_section.clone();
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
        });
    }

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

    btn
}
