use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::fs;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use rdm_panel_api::RdmPluginInfo;

// ── Config ────────────────────────────────────────────────────────────────────

/// Plugin config parsed from the `[panel.plugins.config]` TOML table.
struct Config {
    /// Polling interval when the popover is open (milliseconds).
    interval_ms: u32,
    /// Number of top-CPU processes to list.
    process_limit: usize,
    /// Show network totals row.
    show_network: bool,
    /// Label shown on the panel button.
    button_label: String,
}

impl Config {
    fn from_toml(src: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(src) = src else { return cfg };
        let Ok(val) = src.parse::<toml::Value>() else {
            return cfg;
        };
        if let Some(v) = val.get("interval_ms").and_then(|v| v.as_integer()) {
            if v > 0 { cfg.interval_ms = v.min(60_000) as u32; }
        }
        if let Some(v) = val.get("process_limit").and_then(|v| v.as_integer()) {
            if v > 0 { cfg.process_limit = v.min(20) as usize; }
        }
        if let Some(v) = val.get("show_network").and_then(|v| v.as_bool()) {
            cfg.show_network = v;
        }
        if let Some(v) = val.get("button_label").and_then(|v| v.as_str()) {
            cfg.button_label = v.to_owned();
        }
        cfg
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            interval_ms: 1000,
            process_limit: 8,
            show_network: true,
            button_label: " Sys ".to_owned(),
        }
    }
}

// ── Internal data types ───────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ProcEntry {
    pid: i32,
    name: String,
    cpu_percent: f32,
    mem_percent: f32,
}

#[derive(Debug, Clone)]
struct Snapshot {
    cpu_percent: f64,
    mem_used_mb: u64,
    mem_total_mb: u64,
    net_rx_mb: f64,
    net_tx_mb: f64,
    top_processes: Vec<ProcEntry>,
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct SysmonPlugin {
    // Kept alive here so GLib doesn't drop the widget while the plugin is active.
    #[allow(dead_code)]
    button: gtk4::MenuButton,
}

// ── Instance storage (GTK is single-threaded, thread_local is safe) ───────────

thread_local! {
    static INSTANCES: RefCell<Vec<SysmonPlugin>> = RefCell::new(Vec::new());
}

// ── Exported C ABI symbols ────────────────────────────────────────────────────

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"sysmon".as_ptr(),
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
    INSTANCES.with(|v| v.borrow_mut().push(SysmonPlugin { button }));
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
    btn.add_css_class("task-popup-btn");

    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(12);
    root.set_margin_end(12);
    root.set_size_request(380, -1);

    let title = gtk4::Label::new(Some("System Monitor"));
    title.set_halign(gtk4::Align::Start);
    title.add_css_class("task-popup-title");
    root.append(&title);

    let summary = gtk4::Label::new(Some("Open to refresh usage"));
    summary.set_halign(gtk4::Align::Start);
    summary.set_xalign(0.0);
    summary.set_width_chars(42);
    summary.set_max_width_chars(42);
    root.append(&summary);

    let network = gtk4::Label::new(Some("Network: --"));
    network.set_halign(gtk4::Align::Start);
    network.set_xalign(0.0);
    network.set_width_chars(42);
    network.set_max_width_chars(42);
    if !cfg.show_network {
        network.set_visible(false);
    }
    root.append(&network);

    let proc_header = gtk4::Label::new(Some("Top CPU Processes"));
    proc_header.set_halign(gtk4::Align::Start);
    proc_header.set_xalign(0.0);
    root.append(&proc_header);

    let proc_lines = gtk4::Label::new(Some("Loading..."));
    proc_lines.set_halign(gtk4::Align::Start);
    proc_lines.set_xalign(0.0);
    proc_lines.set_selectable(false);
    proc_lines.set_wrap(false);
    proc_lines.add_css_class("caption");
    proc_lines.set_width_chars(42);
    proc_lines.set_max_width_chars(42);
    proc_lines.set_lines(10);
    root.append(&proc_lines);

    let plugin_badge = gtk4::Label::new(Some("⚡ plugin active"));
    plugin_badge.set_halign(gtk4::Align::End);
    plugin_badge.set_xalign(1.0);
    plugin_badge.add_css_class("caption");
    root.append(&plugin_badge);

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    let interval_secs = ((cfg.interval_ms as f64) / 1000.0).ceil().max(1.0) as u32;
    let process_limit = cfg.process_limit;

    let refresh_timer: Rc<RefCell<Option<glib::SourceId>>> = Rc::new(RefCell::new(None));
    let refresh_in_flight = Rc::new(Cell::new(false));

    let summary_lbl = summary.clone();
    let network_lbl = network.clone();
    let proc_lbl = proc_lines.clone();
    let timer_ref = refresh_timer.clone();
    let inflight_ref = refresh_in_flight.clone();

    btn.connect_notify_local(Some("active"), move |b, _| {
        if !b.is_active() {
            if let Some(id) = timer_ref.borrow_mut().take() {
                id.remove();
            }
            return;
        }
        refresh_once(&summary_lbl, &network_lbl, &proc_lbl, &inflight_ref, process_limit);

        if timer_ref.borrow().is_none() {
            let btn_weak = b.downgrade();
            let s_tick = summary_lbl.clone();
            let n_tick = network_lbl.clone();
            let p_tick = proc_lbl.clone();
            let if_tick = inflight_ref.clone();
            let id = glib::timeout_add_seconds_local(interval_secs, move || {
                let Some(btn) = btn_weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if !btn.is_active() {
                    return glib::ControlFlow::Break;
                }
                refresh_once(&s_tick, &n_tick, &p_tick, &if_tick, process_limit);
                glib::ControlFlow::Continue
            });
            *timer_ref.borrow_mut() = Some(id);
        }
    });

    btn
}

// ── Refresh logic (identical to process_popup.rs) ────────────────────────────

fn refresh_once(
    summary_lbl: &gtk4::Label,
    network_lbl: &gtk4::Label,
    proc_lbl: &gtk4::Label,
    in_flight: &Rc<Cell<bool>>,
    process_limit: usize,
) {
    if in_flight.get() {
        return;
    }
    in_flight.set(true);

    let (tx, rx) = async_channel::bounded::<Result<Snapshot, String>>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(collect_snapshot(process_limit));
    });

    let summary_lbl = summary_lbl.clone();
    let network_lbl = network_lbl.clone();
    let proc_lbl = proc_lbl.clone();
    let in_flight = in_flight.clone();
    glib::spawn_future_local(async move {
        match rx.recv().await {
            Ok(Ok(s)) => {
                summary_lbl.set_text(&format!(
                    "CPU: {:>5.1}%   RAM: {} / {} MB",
                    s.cpu_percent, s.mem_used_mb, s.mem_total_mb
                ));
                network_lbl.set_text(&format!(
                    "Network totals: RX {:.1} MB   TX {:.1} MB",
                    s.net_rx_mb, s.net_tx_mb
                ));
                proc_lbl.set_text(&render_process_lines(&s.top_processes));
            }
            Ok(Err(e)) => {
                summary_lbl.set_text("CPU: --   RAM: --");
                network_lbl.set_text("Network: --");
                proc_lbl.set_text(&format!("Failed to collect stats: {e}"));
            }
            Err(_) => {
                summary_lbl.set_text("CPU: --   RAM: --");
                network_lbl.set_text("Network: --");
                proc_lbl.set_text("Failed to collect stats");
            }
        }
        in_flight.set(false);
    });
}

fn collect_snapshot(process_limit: usize) -> Result<Snapshot, String> {
    let cpu_percent = sample_cpu_percent().ok_or("cpu sample failed")?;
    let (mem_total_mb, mem_used_mb) = read_mem_mb().ok_or("memory sample failed")?;
    let (rx_bytes, tx_bytes) = read_net_totals().ok_or("network sample failed")?;
    let top_processes = read_top_processes(process_limit).unwrap_or_default();
    Ok(Snapshot {
        cpu_percent,
        mem_used_mb,
        mem_total_mb,
        net_rx_mb: rx_bytes as f64 / (1024.0 * 1024.0),
        net_tx_mb: tx_bytes as f64 / (1024.0 * 1024.0),
        top_processes,
    })
}

fn sample_cpu_percent() -> Option<f64> {
    let (idle_1, total_1) = read_cpu_times()?;
    std::thread::sleep(Duration::from_millis(120));
    let (idle_2, total_2) = read_cpu_times()?;
    let idle_delta = idle_2.saturating_sub(idle_1);
    let total_delta = total_2.saturating_sub(total_1);
    if total_delta == 0 {
        return Some(0.0);
    }
    let busy = total_delta.saturating_sub(idle_delta);
    Some((busy as f64 / total_delta as f64) * 100.0)
}

fn read_cpu_times() -> Option<(u64, u64)> {
    let stat = fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let mut parts = line.split_whitespace();
    if parts.next()? != "cpu" {
        return None;
    }
    let nums: Vec<u64> = parts.filter_map(|p| p.parse::<u64>().ok()).collect();
    if nums.len() < 5 {
        return None;
    }
    let idle = nums[3].saturating_add(*nums.get(4).unwrap_or(&0));
    let total = nums.iter().copied().sum::<u64>();
    Some((idle, total))
}

fn read_mem_mb() -> Option<(u64, u64)> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = None;
    let mut avail_kb = None;
    for line in meminfo.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = line.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok());
        } else if line.starts_with("MemAvailable:") {
            avail_kb = line.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok());
        }
    }
    let total = total_kb?;
    let avail = avail_kb?;
    Some((total / 1024, total.saturating_sub(avail) / 1024))
}

fn read_net_totals() -> Option<(u64, u64)> {
    let dev = fs::read_to_string("/proc/net/dev").ok()?;
    let mut rx_total = 0u64;
    let mut tx_total = 0u64;
    for line in dev.lines().skip(2) {
        let mut parts = line.split(':');
        let iface = parts.next()?.trim();
        if iface == "lo" {
            continue;
        }
        let data = parts.next()?.split_whitespace().collect::<Vec<_>>();
        if data.len() < 16 {
            continue;
        }
        rx_total = rx_total.saturating_add(data[0].parse::<u64>().unwrap_or(0));
        tx_total = tx_total.saturating_add(data[8].parse::<u64>().unwrap_or(0));
    }
    Some((rx_total, tx_total))
}

fn read_top_processes(limit: usize) -> Option<Vec<ProcEntry>> {
    let out = Command::new("ps")
        .args(["-eo", "pid,comm,%cpu,%mem", "--sort=-%cpu", "--no-headers"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let txt = String::from_utf8(out.stdout).ok()?;
    let mut entries = Vec::new();
    for line in txt.lines().take(limit) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 4 {
            continue;
        }
        let pid = cols[0].parse::<i32>().unwrap_or(0);
        let name = cols[1].to_string();
        let cpu_percent = cols[2].parse::<f32>().unwrap_or(0.0);
        let mem_percent = cols[3].parse::<f32>().unwrap_or(0.0);
        entries.push(ProcEntry { pid, name, cpu_percent, mem_percent });
    }
    Some(entries)
}

fn render_process_lines(procs: &[ProcEntry]) -> String {
    if procs.is_empty() {
        return "No process data".to_string();
    }
    let mut out = String::from("PID     CPU%   MEM%   NAME\n");
    for p in procs {
        out.push_str(&format!(
            "{:<7} {:>5.1}  {:>5.1}   {}\n",
            p.pid, p.cpu_percent, p.mem_percent, p.name
        ));
    }
    out
}
