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
    /// strftime format shown on the panel button and in the popover header.
    time_format: String,
    /// Second line in the popover header (date).
    date_format: String,
    /// Whether to show the brightness slider (hide on desktops without backlight).
    show_brightness: bool,
    /// Whether to show the Bluetooth row.
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

// ── System helpers ────────────────────────────────────────────────────────────

/// Read current volume 0–100. Returns None on failure.
fn read_volume() -> Option<(f64, bool)> {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()?;
    let txt = String::from_utf8(out.stdout).ok()?;
    // "Volume: 0.75" or "Volume: 0.75 [MUTED]"
    let mut parts = txt.split_whitespace();
    parts.next(); // "Volume:"
    let raw: f64 = parts.next()?.parse().ok()?;
    let muted = txt.contains("[MUTED]");
    Some((raw * 100.0, muted))
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

/// Read brightness 0–100. Returns None if brightnessctl isn't available.
fn read_brightness() -> Option<f64> {
    let cur = Command::new("brightnessctl")
        .arg("get")
        .output()
        .ok()?;
    let max = Command::new("brightnessctl")
        .arg("max")
        .output()
        .ok()?;
    let cur: f64 = String::from_utf8(cur.stdout).ok()?.trim().parse().ok()?;
    let max: f64 = String::from_utf8(max.stdout).ok()?.trim().parse().ok()?;
    if max == 0.0 { return None; }
    Some((cur / max * 100.0).clamp(0.0, 100.0))
}

fn set_brightness(pct: f64) {
    let _ = Command::new("brightnessctl")
        .args(["set", &format!("{:.0}%", pct)])
        .status();
}

/// Returns (interface_name, connected) for the first non-loopback interface that is up.
fn read_network() -> String {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return "Network: unknown".to_owned();
    };
    for entry in entries.flatten() {
        let iface = entry.file_name().to_string_lossy().to_string();
        if iface == "lo" { continue; }
        let state_path = entry.path().join("operstate");
        let state = std::fs::read_to_string(state_path).unwrap_or_default();
        if state.trim() == "up" {
            // Try to get SSID for wireless interfaces
            if let Ok(out) = Command::new("iwgetid").args([&iface, "-r"]).output() {
                let ssid = String::from_utf8_lossy(&out.stdout).trim().to_owned();
                if !ssid.is_empty() {
                    return format!("  {}  ({})", ssid, iface);
                }
            }
            return format!("  Connected  ({})", iface);
        }
    }
    "  Not connected".to_owned()
}

/// Returns true if bluetooth adapter is powered on.
fn read_bluetooth() -> bool {
    let Ok(out) = Command::new("bluetoothctl").arg("show").output() else {
        return false;
    };
    let txt = String::from_utf8_lossy(&out.stdout);
    txt.lines()
        .find(|l| l.trim().starts_with("Powered:"))
        .map(|l| l.contains("yes"))
        .unwrap_or(false)
}

fn set_bluetooth(on: bool) {
    let _ = Command::new("bluetoothctl")
        .args(["power", if on { "on" } else { "off" }])
        .status();
}

// ── Widget builder ────────────────────────────────────────────────────────────

fn build_widget(cfg: Config) -> gtk4::MenuButton {
    let btn = gtk4::MenuButton::new();
    btn.set_label(&Local::now().format(&cfg.time_format).to_string());
    btn.add_css_class("tray-btn");
    btn.add_css_class("task-popup-btn");

    // ── Popover shell ──────────────────────────────────────────────────────
    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    root.set_size_request(320, -1);
    root.set_margin_top(12);
    root.set_margin_bottom(12);
    root.set_margin_start(16);
    root.set_margin_end(16);

    // ── Clock header ───────────────────────────────────────────────────────
    let time_lbl = gtk4::Label::new(Some(&Local::now().format(&cfg.time_format).to_string()));
    time_lbl.add_css_class("task-popup-title");
    time_lbl.set_halign(gtk4::Align::Center);
    // Make the time large
    time_lbl.set_markup(&format!(
        "<span size='xx-large' weight='bold'>{}</span>",
        Local::now().format(&cfg.time_format)
    ));

    let date_lbl = gtk4::Label::new(Some(&Local::now().format(&cfg.date_format).to_string()));
    date_lbl.add_css_class("settings-hint");
    date_lbl.set_halign(gtk4::Align::Center);
    date_lbl.set_margin_bottom(10);

    root.append(&time_lbl);
    root.append(&date_lbl);
    root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    // ── Volume row ─────────────────────────────────────────────────────────
    let vol_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    vol_box.set_margin_top(10);
    vol_box.set_margin_bottom(4);

    let vol_icon = gtk4::Label::new(Some("🔊"));
    vol_icon.set_width_chars(2);
    vol_box.append(&vol_icon);

    let vol_scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, None::<&gtk4::Adjustment>);
    vol_scale.set_range(0.0, 100.0);
    vol_scale.set_value(50.0);
    vol_scale.set_hexpand(true);
    vol_scale.set_draw_value(false);
    vol_scale.set_increments(1.0, 5.0);
    vol_box.append(&vol_scale);

    let vol_pct = gtk4::Label::new(Some(" 50%"));
    vol_pct.set_width_chars(5);
    vol_pct.set_xalign(1.0);
    vol_box.append(&vol_pct);

    let mute_btn = gtk4::Button::with_label("🔇");
    mute_btn.add_css_class("tray-btn");
    mute_btn.set_tooltip_text(Some("Toggle mute"));
    vol_box.append(&mute_btn);

    root.append(&vol_box);

    // ── Brightness row (hidden if brightnessctl not available) ─────────────
    let bright_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    bright_box.set_margin_top(4);
    bright_box.set_margin_bottom(4);
    bright_box.set_visible(cfg.show_brightness);

    let bright_icon = gtk4::Label::new(Some("☀"));
    bright_icon.set_width_chars(2);
    bright_box.append(&bright_icon);

    let bright_scale = gtk4::Scale::new(gtk4::Orientation::Horizontal, None::<&gtk4::Adjustment>);
    bright_scale.set_range(0.0, 100.0);
    bright_scale.set_value(100.0);
    bright_scale.set_hexpand(true);
    bright_scale.set_draw_value(false);
    bright_scale.set_increments(1.0, 5.0);
    bright_box.append(&bright_scale);

    let bright_pct = gtk4::Label::new(Some("100%"));
    bright_pct.set_width_chars(5);
    bright_pct.set_xalign(1.0);
    bright_box.append(&bright_pct);

    root.append(&bright_box);

    root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    // ── Network + Bluetooth row ────────────────────────────────────────────
    let status_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    status_box.set_margin_top(10);
    status_box.set_margin_bottom(10);

    let net_lbl = gtk4::Label::new(Some("  Checking…"));
    net_lbl.set_halign(gtk4::Align::Start);
    net_lbl.add_css_class("settings-hint");
    status_box.append(&net_lbl);

    let bt_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let bt_lbl = gtk4::Label::new(Some("  Bluetooth"));
    bt_lbl.set_halign(gtk4::Align::Start);
    bt_lbl.set_hexpand(true);
    let bt_btn = gtk4::Button::with_label("Off");
    bt_btn.add_css_class("tray-btn");
    bt_btn.set_tooltip_text(Some("Toggle Bluetooth"));

    bt_box.append(&bt_lbl);
    if cfg.show_bluetooth {
        bt_box.append(&bt_btn);
    }
    status_box.append(&bt_box);

    root.append(&status_box);

    root.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    // ── Power buttons ──────────────────────────────────────────────────────
    let power_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    power_box.set_margin_top(10);
    power_box.set_homogeneous(true);

    let lock_btn     = gtk4::Button::with_label("🔒 Lock");
    let logout_btn   = gtk4::Button::with_label("⏏ Logout");
    let shutdown_btn = gtk4::Button::with_label("⏻ Shutdown");
    let reboot_btn   = gtk4::Button::with_label("↺ Reboot");

    lock_btn.add_css_class("tray-btn");
    logout_btn.add_css_class("tray-btn");
    shutdown_btn.add_css_class("destructive-action");
    reboot_btn.add_css_class("tray-btn");

    power_box.append(&lock_btn);
    power_box.append(&logout_btn);
    power_box.append(&shutdown_btn);
    power_box.append(&reboot_btn);

    root.append(&power_box);

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    // ── Always-running clock tick (updates button label + popover header) ──
    let btn_weak   = btn.downgrade();
    let time_lbl_w = time_lbl.downgrade();
    let date_lbl_w = date_lbl.downgrade();
    let tfmt = cfg.time_format.clone();
    let dfmt = cfg.date_format.clone();

    glib::timeout_add_seconds_local(1, move || {
        let (Some(btn), Some(tlbl), Some(dlbl)) = (
            btn_weak.upgrade(), time_lbl_w.upgrade(), date_lbl_w.upgrade()
        ) else {
            return glib::ControlFlow::Break;
        };
        let now = Local::now();
        btn.set_label(&now.format(&tfmt).to_string());
        tlbl.set_markup(&format!(
            "<span size='xx-large' weight='bold'>{}</span>",
            now.format(&tfmt)
        ));
        dlbl.set_text(&now.format(&dfmt).to_string());
        glib::ControlFlow::Continue
    });

    // ── State shared across callbacks ──────────────────────────────────────
    let vol_updating  = Rc::new(Cell::new(false));
    let brg_updating  = Rc::new(Cell::new(false));
    let vol_pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let brg_pending: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let bt_state      = Rc::new(Cell::new(false));
    let refresh_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    // ── Volume slider change ───────────────────────────────────────────────
    {
        let vu  = vol_updating.clone();
        let vp  = vol_pct.clone();
        let vi  = vol_icon.clone();
        let pnd = vol_pending.clone();
        vol_scale.connect_value_changed(move |s| {
            if vu.get() { return; }
            let pct = s.value();
            vp.set_text(&format!("{:3.0}%", pct));
            vi.set_text(if pct == 0.0 { "🔈" } else if pct <= 50.0 { "🔉" } else { "🔊" });
            // Debounce: cancel any pending command and schedule a new one
            if let Some(id) = pnd.borrow_mut().take() { id.remove(); }
            let id = glib::timeout_add_local_once(Duration::from_millis(80), move || {
                set_volume(pct);
            });
            *pnd.borrow_mut() = Some(id);
        });
    }

    // ── Mute button ────────────────────────────────────────────────────────
    {
        let vs  = vol_scale.clone();
        let vi  = vol_icon.clone();
        let vp  = vol_pct.clone();
        let vu  = vol_updating.clone();
        mute_btn.connect_clicked(move |_| {
            toggle_mute();
            // Re-read after short delay so wpctl state has settled
            let vs2 = vs.clone();
            let vi2 = vi.clone();
            let vp2 = vp.clone();
            let vu2 = vu.clone();
            glib::timeout_add_local_once(Duration::from_millis(120), move || {
                if let Some((pct, muted)) = read_volume() {
                    vu2.set(true);
                    vs2.set_value(pct);
                    vp2.set_text(&format!("{:3.0}%", pct));
                    vi2.set_text(if muted { "🔇" } else if pct <= 50.0 { "🔉" } else { "🔊" });
                    vu2.set(false);
                }
            });
        });
    }

    // ── Brightness slider change ───────────────────────────────────────────
    {
        let bu  = brg_updating.clone();
        let bp  = bright_pct.clone();
        let pnd = brg_pending.clone();
        bright_scale.connect_value_changed(move |s| {
            if bu.get() { return; }
            let pct = s.value();
            bp.set_text(&format!("{:3.0}%", pct));
            if let Some(id) = pnd.borrow_mut().take() { id.remove(); }
            let id = glib::timeout_add_local_once(Duration::from_millis(80), move || {
                set_brightness(pct);
            });
            *pnd.borrow_mut() = Some(id);
        });
    }

    // ── Bluetooth button ───────────────────────────────────────────────────
    {
        let bt  = bt_state.clone();
        let bb  = bt_btn.clone();
        bt_btn.connect_clicked(move |_| {
            let new_state = !bt.get();
            bt.set(new_state);
            set_bluetooth(new_state);
            bb.set_label(if new_state { "On" } else { "Off" });
        });
    }

    // ── Power actions ──────────────────────────────────────────────────────
    {
        let pop_w = pop.downgrade();
        lock_btn.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() { p.popdown(); }
            let _ = Command::new("swaylock").args(["-f", "-c", "1a1b26"]).spawn();
        });
    }
    {
        let pop_w = pop.downgrade();
        logout_btn.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() { p.popdown(); }
            let _ = Command::new("pkill").arg("labwc").spawn();
        });
    }
    {
        let pop_w = pop.downgrade();
        shutdown_btn.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() { p.popdown(); }
            let _ = Command::new("systemctl").arg("poweroff").spawn();
        });
    }
    {
        let pop_w = pop.downgrade();
        reboot_btn.connect_clicked(move |_| {
            if let Some(p) = pop_w.upgrade() { p.popdown(); }
            let _ = Command::new("systemctl").arg("reboot").spawn();
        });
    }

    // ── Refresh on popover open ────────────────────────────────────────────
    {
        let vs   = vol_scale.clone();
        let vp   = vol_pct.clone();
        let vi   = vol_icon.clone();
        let vu   = vol_updating.clone();
        let bs   = bright_scale.clone();
        let bp   = bright_pct.clone();
        let bu   = brg_updating.clone();
        let bb   = bright_box.clone();
        let nl   = net_lbl.clone();
        let btb  = bt_btn.clone();
        let bts  = bt_state.clone();
        let rtmr = refresh_timer.clone();

        btn.connect_notify_local(Some("active"), move |b, _| {
            // Popover closed — cancel refresh timer
            if !b.is_active() {
                if let Some(id) = rtmr.borrow_mut().take() { id.remove(); }
                return;
            }

            // Read volume
            if let Some((pct, muted)) = read_volume() {
                vu.set(true);
                vs.set_value(pct);
                vp.set_text(&format!("{:3.0}%", pct));
                vi.set_text(if muted { "🔇" } else if pct <= 50.0 { "🔉" } else { "🔊" });
                vu.set(false);
            }

            // Read brightness — hide row if unavailable
            if let Some(pct) = read_brightness() {
                bu.set(true);
                bs.set_value(pct);
                bp.set_text(&format!("{:3.0}%", pct));
                bu.set(false);
                bb.set_visible(true);
            } else {
                bb.set_visible(false);
            }

            // Read network + bluetooth in a background thread to avoid blocking
            let nl2   = nl.clone();
            let btb2  = btb.clone();
            let bts2  = bts.clone();
            let (tx, rx) = async_channel::bounded::<(String, bool)>(1);
            std::thread::spawn(move || {
                let net = read_network();
                let bt  = read_bluetooth();
                let _ = tx.send_blocking((net, bt));
            });
            glib::spawn_future_local(async move {
                if let Ok((net, bt)) = rx.recv().await {
                    nl2.set_text(&net);
                    bts2.set(bt);
                    btb2.set_label(if bt { "On" } else { "Off" });
                }
            });

            // Refresh network/bt every 10 seconds while open
            let b_weak = b.downgrade();
            let nl3    = nl.clone();
            let btb3   = btb.clone();
            let bts3   = bts.clone();
            let id = glib::timeout_add_seconds_local(10, move || {
                let Some(btn) = b_weak.upgrade() else { return glib::ControlFlow::Break; };
                if !btn.is_active() { return glib::ControlFlow::Break; }
                let nl4   = nl3.clone();
                let btb4  = btb3.clone();
                let bts4  = bts3.clone();
                let (tx2, rx2) = async_channel::bounded::<(String, bool)>(1);
                std::thread::spawn(move || {
                    let _ = tx2.send_blocking((read_network(), read_bluetooth()));
                });
                glib::spawn_future_local(async move {
                    if let Ok((net, bt)) = rx2.recv().await {
                        nl4.set_text(&net);
                        bts4.set(bt);
                        btb4.set_label(if bt { "On" } else { "Off" });
                    }
                });
                glib::ControlFlow::Continue
            });
            *rtmr.borrow_mut() = Some(id);
        });
    }

    btn
}
