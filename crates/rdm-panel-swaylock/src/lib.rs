//! # rdm-panel-swaylock
//!
//! A panel plugin that provides a popover menu with two actions:
//!   - **Disable swaylock** — kills any running `swaylock` process via `pkill -x swaylock`
//!   - **Enable swaylock**  — (re)starts `swaylock` in the background via `swaylock &`
//!
//! ## rdm.toml example
//!
//! ```toml
//! [[panel.plugins]]
//! name     = "swaylock"
//! position = "right"
//!
//! [panel.plugins.config]
//! button_label = " Lock "
//! ```

use gtk4::prelude::*;
use rdm_panel_api::RdmPluginInfo;
use std::cell::RefCell;
use std::process::Command;

// ── Config ────────────────────────────────────────────────────────────────────

struct Config {
    /// Label shown on the panel button.
    button_label: String,
    /// Command used to start swaylock (can be overridden in config).
    start_cmd: String,
}

impl Config {
    fn from_toml(src: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(src) = src else { return cfg };
        let Ok(val) = src.parse::<toml::Value>() else {
            return cfg;
        };
        if let Some(v) = val.get("button_label").and_then(|v| v.as_str()) {
            cfg.button_label = v.to_owned();
        }
        if let Some(v) = val.get("start_cmd").and_then(|v| v.as_str()) {
            cfg.start_cmd = v.to_owned();
        }
        cfg
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            button_label: " Lock ".to_owned(),
            start_cmd: "swaylock".to_owned(),
        }
    }
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct SwaylockPlugin {
    /// Kept alive so the raw GtkWidget pointer stays valid.
    #[allow(dead_code)]
    button: gtk4::MenuButton,
}

// ── Instance storage ──────────────────────────────────────────────────────────

thread_local! {
    static INSTANCES: RefCell<Vec<SwaylockPlugin>> = RefCell::new(Vec::new());
}

// ── Exported C ABI symbols ────────────────────────────────────────────────────

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"swaylock".as_ptr(),
        version: 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_new_instance(
    config_toml: *const std::ffi::c_char,
) -> *mut gtk4::ffi::GtkWidget {
    // REQUIRED: tell this .so's gtk4-rs copy that GTK is already initialised.
    unsafe { gtk4::set_initialized(); }

    let config_str = if config_toml.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(config_toml).to_str().ok() }
    };

    let cfg = Config::from_toml(config_str);
    let button = build_widget(cfg);
    let raw = button.upcast_ref::<gtk4::Widget>().as_ptr();

    INSTANCES.with(|v| v.borrow_mut().push(SwaylockPlugin { button }));
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

// ── Widget builder ────────────────────────────────────────────────────────────

fn build_widget(cfg: Config) -> gtk4::MenuButton {
    // ── Panel button ──────────────────────────────────────────────────────────
    let btn = gtk4::MenuButton::new();
    btn.set_label(&cfg.button_label);
    btn.add_css_class("tray-btn");
    btn.add_css_class("task-popup-btn");

    // ── Popover ───────────────────────────────────────────────────────────────
    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);

    // Outer container
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(14);
    root.set_margin_end(14);
    root.set_size_request(200, -1);

    // Title row
    let title = gtk4::Label::new(Some("Swaylock"));
    title.add_css_class("plugin-title");
    title.set_halign(gtk4::Align::Start);
    root.append(&title);

    // Separator
    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    sep.set_margin_top(2);
    sep.set_margin_bottom(4);
    root.append(&sep);

    // Status label — updated when the popover opens
    let status_lbl = gtk4::Label::new(Some("Checking…"));
    status_lbl.set_halign(gtk4::Align::Start);
    status_lbl.add_css_class("dim-label");
    root.append(&status_lbl);

    // ── "Disable swaylock" button ─────────────────────────────────────────────
    let kill_btn = gtk4::Button::with_label("⛔  Disable swaylock");
    kill_btn.add_css_class("tray-btn");
    kill_btn.set_tooltip_text(Some("Runs: pkill -x swaylock"));

    let status_lbl_kill = status_lbl.clone();
    let pop_kill = pop.clone();
    kill_btn.connect_clicked(move |_| {
        let output = Command::new("pkill")
            .args(["-x", "swaylock"])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                status_lbl_kill.set_text("swaylock killed.");
            }
            Ok(_) => {
                // pkill returns 1 when no process was matched
                status_lbl_kill.set_text("swaylock was not running.");
            }
            Err(e) => {
                status_lbl_kill.set_text(&format!("Error: {e}"));
            }
        }
        pop_kill.popdown();
    });

    root.append(&kill_btn);

    // ── "Enable swaylock" button ──────────────────────────────────────────────
    let start_cmd = cfg.start_cmd.clone();
    let enable_btn = gtk4::Button::with_label("🔒  Enable swaylock");
    enable_btn.add_css_class("tray-btn");
    enable_btn.set_tooltip_text(Some("Starts swaylock in the background"));

    let status_lbl_enable = status_lbl.clone();
    let pop_enable = pop.clone();
    enable_btn.connect_clicked(move |_| {
        // Split the command string into program + args to support overrides
        // like "swaylock -f -c 000000".
        let mut parts = start_cmd.split_whitespace();
        let prog = match parts.next() {
            Some(p) => p.to_owned(),
            None => {
                status_lbl_enable.set_text("Error: empty start_cmd");
                return;
            }
        };
        let args: Vec<String> = parts.map(|s| s.to_owned()).collect();

        match Command::new(&prog).args(&args).spawn() {
            Ok(_) => {
                status_lbl_enable.set_text("swaylock started.");
            }
            Err(e) => {
                status_lbl_enable.set_text(&format!("Error: {e}"));
            }
        }
        pop_enable.popdown();
    });

    root.append(&enable_btn);

    // ── Wire popover open → refresh status label ──────────────────────────────
    let status_lbl_open = status_lbl.clone();
    pop.connect_show(move |_| {
        // Check whether swaylock is currently running.
        let running = Command::new("pgrep")
            .args(["-x", "swaylock"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if running {
            status_lbl_open.set_text("Status: running");
        } else {
            status_lbl_open.set_text("Status: not running");
        }
    });

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    btn
}
