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

// ── System helpers ────────────────────────────────────────────────────────────

fn read_volume() -> Option<(f64, bool)> {
    let out = Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output().ok()?;
    let txt = String::from_utf8(out.stdout).ok()?;
    let mut parts = txt.split_whitespace();
    parts.next();
    let raw: f64 = parts.next()?.parse().ok()?;
    let muted = txt.contains("[MUTED]");
    Some(((raw * 100.0).clamp(0.0, 100.0), muted))
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
    let cur_out = Command::new("brightnessctl").arg("get").output().ok()?;
    let max_out = Command::new("brightnessctl").arg("max").output().ok()?;
    let cur: f64 = String::from_utf8(cur_out.stdout).ok()?.trim().parse().ok()?;
    let max: f64 = String::from_utf8(max_out.stdout).ok()?.trim().parse().ok()?;
    if max == 0.0 { return None; }
    Some((cur / max * 100.0).clamp(0.0, 100.0))
}

fn set_brightness(pct: f64) {
    let _ = Command::new("brightnessctl")
        .args(["set", &format!("{:.0}%", pct)])
        .status();
}

fn read_network() -> String {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return "  Unknown".to_owned();
    };
    for entry in entries.flatten() {
        let iface = entry.file_name().to_string_lossy().to_string();
        if iface == "lo" { continue; }
        let state = std::fs::read_to_string(entry.path().join("operstate"))
            .unwrap_or_default();
        if state.trim() == "up" {
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

fn read_bluetooth() -> bool {
    let Ok(out) = Command::new("bluetoothctl").arg("show").output() else {
        return false;
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .find(|l| l.trim().starts_with("Powered:"))
        .map(|l| l.contains("yes"))
        .unwrap_or(false)
}

fn set_bluetooth(on: bool) {
    let _ = Command::new("bluetoothctl")
        .args(["power", if on { "on" } else { "off" }])
        .status();
}

// ── Small layout helpers ──────────────────────────────────────────────────────

/// A card-style section box matching .cmdcenter-section CSS.
fn section_box() -> gtk4::Box {
    let b = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    b.add_css_class("cmdcenter-section");
    b
}

/// A row label (small muted caps header above a control).
fn row_label(text: &str) -> gtk4::Label {
    let l = gtk4::Label::new(Some(text));
    l.add_css_class("cmdcenter-row-label");
    l.set_halign(gtk4::Align::Start);
    l
}

/// A horizontal slider row: [icon] [Scale] [pct label] [optional button]
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

    if let Some(w) = extra {
        row.append(&w);
    }

    (row, scale, pct)
}

// ── Widget builder ────────────────────────────────────────────────────────────

fn build_widget(cfg: Config) -> gtk4::MenuButton {
    let btn = gtk4::MenuButton::new();
    btn.set_label(&Local::now().format(&cfg.time_format).to_string());
    btn.add_css_class("tray-btn");
    btn.add_css_class("task-popup-btn");

    // ── Popover ────────────────────────────────────────────────────────────
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
    let cal_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    cal_box.add_css_class("cmdcenter-section");
    cal_box.add_css_class("cmdcenter-calendar");
    let calendar = gtk4::Calendar::new();
    cal_box.append(&calendar);
    root.append(&cal_box);

    // ── Volume section ────────────────────────────────────────────────────
    let vol_section = section_box();
    vol_section.append(&row_label("VOLUME"));

    let mute_btn = gtk4::Button::with_label("🔇");
    mute_btn.add_css_class("tray-btn");
    mute_btn.set_tooltip_text(Some("Toggle mute"));

    let (vol_row, vol_scale, vol_pct) =
        slider_row("🔊", 50.0, Some(mute_btn.clone().upcast()));
    vol_section.append(&vol_row);
    root.append(&vol_section);

    // ── Brightness section (hidden if brightnessctl unavailable) ──────────
    let bright_section = section_box();
    bright_section.append(&row_label("BRIGHTNESS"));
    bright_section.set_visible(cfg.show_brightness);

    let (bright_row, bright_scale, bright_pct) =
        slider_row("☀", 100.0, None);
    bright_section.append(&bright_row);
    root.append(&bright_section);

    // ── Network + Bluetooth section ───────────────────────────────────────
    let status_section = section_box();

    let net_lbl = gtk4::Label::new(Some("  Checking…"));
    net_lbl.set_halign(gtk4::Align::Start);
    net_lbl.add_css_class("settings-hint");
    status_section.append(&net_lbl);

    if cfg.show_bluetooth {
        let bt_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        let bt_lbl = gtk4::Label::new(Some("  Bluetooth"));
        bt_lbl.set_halign(gtk4::Align::Start);
        bt_lbl.set_hexpand(true);
        let bt_btn = gtk4::Button::with_label("Off");
        bt_btn.add_css_class("tray-btn");
        bt_btn.set_tooltip_text(Some("Toggle Bluetooth"));
        bt_row.append(&bt_lbl);
        bt_row.append(&bt_btn);
        status_section.append(&bt_row);

        // Bluetooth toggle callback
        let bt_state = Rc::new(Cell::new(false));
        let bt_state2 = bt_state.clone();
        let bb2 = bt_btn.clone();
        bt_btn.connect_clicked(move |_| {
            let new_state = !bt_state2.get();
            bt_state2.set(new_state);
            set_bluetooth(new_state);
            bb2.set_label(if new_state { "On" } else { "Off" });
        });

        // Store bt_state and bt_btn for refresh — via weak refs in the open callback below
        let bt_state_outer = bt_state.clone();
        let bt_btn_outer = bt_btn.clone();

        // We need these in the connect_notify_local below; store them for capture
        // by saving them as locals that the closure can move over
        build_open_callback(
            &btn, &pop,
            &vol_scale, &vol_pct, &vol_section,
            &bright_scale, &bright_pct, &bright_section,
            &net_lbl,
            Some((bt_state_outer, bt_btn_outer)),
        );
    } else {
        build_open_callback(
            &btn, &pop,
            &vol_scale, &vol_pct, &vol_section,
            &bright_scale, &bright_pct, &bright_section,
            &net_lbl,
            None,
        );
    }

    root.append(&status_section);

    // ── Power buttons section ──────────────────────────────────────────────
    let power_section = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    power_section.set_homogeneous(true);
    power_section.set_margin_top(4);

    for (label, is_destructive, action) in [
        ("🔒 Lock",      false, "lock"),
        ("⏏ Logout",    false, "logout"),
        ("⏻ Shutdown",  true,  "shutdown"),
        ("↺ Reboot",    false, "reboot"),
    ] {
        let b = gtk4::Button::with_label(label);
        b.add_css_class("cmdcenter-power-btn");
        if is_destructive { b.add_css_class("destructive-action"); }
        let pop_w = pop.downgrade();
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

    // ── Always-running clock tick ──────────────────────────────────────────
    let btn_weak   = btn.downgrade();
    let time_lbl_w = time_lbl.downgrade();
    let date_lbl_w = date_lbl.downgrade();
    let tfmt = cfg.time_format.clone();
    let dfmt = cfg.date_format.clone();
    glib::timeout_add_seconds_local(1, move || {
        let (Some(b), Some(tl), Some(dl)) = (
            btn_weak.upgrade(), time_lbl_w.upgrade(), date_lbl_w.upgrade()
        ) else {
            return glib::ControlFlow::Break;
        };
        let now = Local::now();
        b.set_label(&now.format(&tfmt).to_string());
        tl.set_markup(&format!(
            "<span size='xx-large' weight='bold'>{}</span>",
            now.format(&tfmt)
        ));
        dl.set_text(&now.format(&dfmt).to_string());
        glib::ControlFlow::Continue
    });

    // ── Volume callbacks ───────────────────────────────────────────────────
    // Use a shared target cell + single-timer flag instead of SourceId::remove()
    // (glib-rs 0.20 panics when removing an already-fired source).
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
                let vt2  = vt.clone();
                let arm2 = arm.clone();
                glib::timeout_add_local_once(Duration::from_millis(80), move || {
                    let final_pct = vt2.get();
                    arm2.set(false);
                    std::thread::spawn(move || { set_volume(final_pct); });
                });
            }
        });
    }
    {
        let vs2 = vol_scale.clone();
        let vp2 = vol_pct.clone();
        let vu2 = vol_updating.clone();
        mute_btn.connect_clicked(move |_| {
            std::thread::spawn(|| { toggle_mute(); });
            let vs3 = vs2.clone();
            let vp3 = vp2.clone();
            let vu3 = vu2.clone();
            // Re-read volume after mute settles
            glib::timeout_add_local_once(Duration::from_millis(150), move || {
                let (tx, rx) = async_channel::bounded::<(f64, bool)>(1);
                std::thread::spawn(move || {
                    let _ = tx.send_blocking(read_volume().unwrap_or((0.0, false)));
                });
                glib::spawn_future_local(async move {
                    if let Ok((pct, _)) = rx.recv().await {
                        vu3.set(true);
                        vs3.set_value(pct);
                        vp3.set_text(&format!("{:3.0}%", pct));
                        vu3.set(false);
                    }
                });
            });
        });
    }

    // ── Brightness callbacks ───────────────────────────────────────────────
    let brg_updating = Rc::new(Cell::new(false));
    let brg_target   = Rc::new(Cell::new(100.0f64));
    let brg_armed    = Rc::new(Cell::new(false));
    {
        let bu  = brg_updating.clone();
        let bp  = bright_pct.clone();
        let bt  = brg_target.clone();
        let arm = brg_armed.clone();
        bright_scale.connect_value_changed(move |s| {
            if bu.get() { return; }
            let pct = s.value();
            bp.set_text(&format!("{:3.0}%", pct));
            bt.set(pct);
            if !arm.get() {
                arm.set(true);
                let bt2  = bt.clone();
                let arm2 = arm.clone();
                glib::timeout_add_local_once(Duration::from_millis(80), move || {
                    let final_pct = bt2.get();
                    arm2.set(false);
                    std::thread::spawn(move || { set_brightness(final_pct); });
                });
            }
        });
    }

    btn
}

// ── Popover open/close refresh callback ──────────────────────────────────────

fn build_open_callback(
    btn: &gtk4::MenuButton,
    _pop: &gtk4::Popover,
    vol_scale: &gtk4::Scale,
    vol_pct: &gtk4::Label,
    vol_section: &gtk4::Box,
    bright_scale: &gtk4::Scale,
    bright_pct: &gtk4::Label,
    bright_section: &gtk4::Box,
    net_lbl: &gtk4::Label,
    bt: Option<(Rc<Cell<bool>>, gtk4::Button)>,
) {
    let vol_updating = Rc::new(Cell::new(false));
    let brg_updating = Rc::new(Cell::new(false));
    let refresh_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));

    let vs  = vol_scale.clone();
    let vp  = vol_pct.clone();
    let vs_box = vol_section.clone();
    let vu  = vol_updating;
    let bs  = bright_scale.clone();
    let bp  = bright_pct.clone();
    let bs_box = bright_section.clone();
    let bu  = brg_updating;
    let nl  = net_lbl.clone();
    let rtmr = refresh_timer;

    btn.connect_notify_local(Some("active"), move |b, _| {
        if !b.is_active() {
            if let Some(id) = rtmr.borrow_mut().take() { id.remove(); }
            return;
        }

        // Volume
        if let Some((pct, _muted)) = read_volume() {
            vu.set(true);
            vs.set_value(pct);
            vp.set_text(&format!("{:3.0}%", pct));
            vu.set(false);
            vs_box.set_visible(true);
        }

        // Brightness
        if let Some(pct) = read_brightness() {
            bu.set(true);
            bs.set_value(pct);
            bp.set_text(&format!("{:3.0}%", pct));
            bu.set(false);
            bs_box.set_visible(true);
        } else {
            bs_box.set_visible(false);
        }

        // Network + Bluetooth in background thread
        let nl2 = nl.clone();
        let bt2 = bt.clone().map(|(s, b)| (s, b));
        let (tx, rx) = async_channel::bounded::<(String, bool)>(1);
        std::thread::spawn(move || {
            let _ = tx.send_blocking((read_network(), read_bluetooth()));
        });
        glib::spawn_future_local(async move {
            if let Ok((net, bt_on)) = rx.recv().await {
                nl2.set_text(&net);
                if let Some((state, btn)) = bt2 {
                    state.set(bt_on);
                    btn.set_label(if bt_on { "On" } else { "Off" });
                }
            }
        });

        // Refresh every 10s while open
        let b_weak = b.downgrade();
        let nl3 = nl.clone();
        let bt3 = bt.clone();
        let id = glib::timeout_add_seconds_local(10, move || {
            let Some(btn) = b_weak.upgrade() else { return glib::ControlFlow::Break; };
            if !btn.is_active() { return glib::ControlFlow::Break; }
            let nl4 = nl3.clone();
            let bt4 = bt3.clone();
            let (tx2, rx2) = async_channel::bounded::<(String, bool)>(1);
            std::thread::spawn(move || {
                let _ = tx2.send_blocking((read_network(), read_bluetooth()));
            });
            glib::spawn_future_local(async move {
                if let Ok((net, bt_on)) = rx2.recv().await {
                    nl4.set_text(&net);
                    if let Some((state, btn)) = bt4 {
                        state.set(bt_on);
                        btn.set_label(if bt_on { "On" } else { "Off" });
                    }
                }
            });
            glib::ControlFlow::Continue
        });
        *rtmr.borrow_mut() = Some(id);
    });
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
