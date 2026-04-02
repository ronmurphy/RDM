//! # rdm-panel-terminal
//!
//! A panel plugin that opens a drop-down terminal via a GTK4 popover.
//! Uses VTE4 for the embedded terminal — no external terminal app needed.
//!
//! ## rdm.toml example
//!
//! ```toml
//! [[panel.plugins]]
//! name     = "terminal"
//! position = "right"
//!
//! [panel.plugins.config]
//! button_label = " Term"
//! shell        = "/bin/bash"
//! width        = 650
//! height       = 380
//! ```

use gtk4::prelude::*;
use rdm_panel_api::RdmPluginInfo;
use vte4::{TerminalExt, TerminalExtManual};
use std::cell::RefCell;

// ── Config ────────────────────────────────────────────────────────────────────

struct Config {
    button_label: String,
    shell: String,
    width: i32,
    height: i32,
}

impl Config {
    fn from_toml(src: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(src) = src else { return cfg };
        let Ok(val) = src.parse::<toml::Value>() else { return cfg };
        if let Some(v) = val.get("button_label").and_then(|v| v.as_str()) {
            cfg.button_label = v.to_owned();
        }
        if let Some(v) = val.get("shell").and_then(|v| v.as_str()) {
            cfg.shell = v.to_owned();
        }
        if let Some(v) = val.get("width").and_then(|v| v.as_integer()) {
            cfg.width = v as i32;
        }
        if let Some(v) = val.get("height").and_then(|v| v.as_integer()) {
            cfg.height = v as i32;
        }
        cfg
    }
}

impl Default for Config {
    fn default() -> Self {
        // Prefer $SHELL from the environment, fall back to bash.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_owned());
        Self {
            button_label: " Term".to_owned(),
            shell,
            width: 650,
            height: 380,
        }
    }
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct TerminalPlugin {
    #[allow(dead_code)]
    button: gtk4::MenuButton,
}

// ── Instance storage ──────────────────────────────────────────────────────────

thread_local! {
    static INSTANCES: RefCell<Vec<TerminalPlugin>> = RefCell::new(Vec::new());
}

// ── Exported C ABI symbols ────────────────────────────────────────────────────

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"terminal".as_ptr(),
        version: 1,
    }
}

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_new_instance(
    config_toml: *const std::ffi::c_char,
) -> *mut gtk4::ffi::GtkWidget {
    unsafe { gtk4::set_initialized(); }

    let config_str = if config_toml.is_null() {
        None
    } else {
        unsafe { std::ffi::CStr::from_ptr(config_toml).to_str().ok() }
    };

    let cfg = Config::from_toml(config_str);
    let button = build_widget(cfg);
    let raw = button.upcast_ref::<gtk4::Widget>().as_ptr();

    INSTANCES.with(|v| v.borrow_mut().push(TerminalPlugin { button }));
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
    let btn = gtk4::MenuButton::new();
    btn.set_label(&cfg.button_label);
    btn.add_css_class("tray-btn");

    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);

    // Outer container — fixed size so the terminal has room to render.
    let outer = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    outer.set_size_request(cfg.width, cfg.height);

    // Build the VTE terminal widget.
    let term = vte4::Terminal::new();
    term.set_hexpand(true);
    term.set_vexpand(true);
    term.set_scrollback_lines(5000);

    // Scrolled window so the terminal scrollbar works.
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_hexpand(true);
    scroll.set_vexpand(true);
    scroll.set_hscrollbar_policy(gtk4::PolicyType::Never);
    scroll.set_vscrollbar_policy(gtk4::PolicyType::Automatic);
    scroll.set_child(Some(&term));

    outer.append(&scroll);
    pop.set_child(Some(&outer));

    // Spawn the shell when the popover becomes visible.
    // We track whether a shell is already running so re-opens reuse it.
    let shell = cfg.shell.clone();
    let term_for_show = term.clone();
    let spawned = std::rc::Rc::new(std::cell::Cell::new(false));

    pop.connect_show(move |_| {
        if spawned.get() {
            // Shell already running — just focus the terminal.
            term_for_show.grab_focus();
            return;
        }
        spawned.set(true);

        let argv = [shell.as_str()];
        let envv: Vec<String> = std::env::vars()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let envv_refs: Vec<&str> = envv.iter().map(|s| s.as_str()).collect();

        term_for_show.spawn_async(
            vte4::PtyFlags::DEFAULT,
            None,               // working directory (None = home)
            &argv,
            &envv_refs,
            gtk4::glib::SpawnFlags::DEFAULT,
            || {},              // child setup callback
            -1,                 // timeout (-1 = default)
            gtk4::gio::Cancellable::NONE,
            |_result| {},       // callback after spawn
        );

        term_for_show.grab_focus();
    });

    // Close the popover when the shell exits.
    let pop_for_exit = pop.clone();
    term.connect_child_exited(move |_term, _status| {
        pop_for_exit.popdown();
    });

    btn.set_popover(Some(&pop));
    btn
}
