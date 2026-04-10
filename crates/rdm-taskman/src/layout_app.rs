//! rdm-taskman UI wiring: process list, system stats, refresh timer.
#![allow(deprecated)] // TreeView/ListStore still work and are far simpler than ColumnView boilerplate.

use gtk4::glib;
use gtk4::glib::ControlFlow;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::procfs::{self, CpuTotals, DiskIo, NetIo, ProcInfo};
use crate::services::{self, ServiceUnit};

const REFRESH_SECS: u64 = 2;

// Processes store columns
const COL_NAME: u32 = 0;
const COL_PID: u32 = 1;
const COL_STATE: u32 = 2;
const COL_USER: u32 = 3;
const COL_CPU_STR: u32 = 4;
const COL_CPU_RAW: u32 = 5;
const COL_MEM_STR: u32 = 6;
const COL_MEM_RAW: u32 = 7;
const COL_THREADS: u32 = 8;

// Users store columns
const UCOL_USER: u32 = 0;
const UCOL_UID: u32 = 1;
const UCOL_PROCS: u32 = 2;
const UCOL_CPU_STR: u32 = 3;
const UCOL_CPU_RAW: u32 = 4;
const UCOL_MEM_STR: u32 = 5;
const UCOL_MEM_RAW: u32 = 6;

// Services store columns
const SCOL_NAME: u32 = 0;
const SCOL_LOAD: u32 = 1;
const SCOL_ACTIVE: u32 = 2;
const SCOL_SUB: u32 = 3;
const SCOL_DESC: u32 = 4;

// Disk I/O store columns
const DCOL_DEVICE: u32 = 0;
const DCOL_READ_STR: u32 = 1;
const DCOL_READ_RAW: u32 = 2;
const DCOL_WRITE_STR: u32 = 3;
const DCOL_WRITE_RAW: u32 = 4;

// Disk usage store columns
const MCOL_MOUNT: u32 = 0;
const MCOL_DEVICE: u32 = 1;
const MCOL_FS: u32 = 2;
const MCOL_SIZE: u32 = 3;
const MCOL_USED: u32 = 4;
const MCOL_FREE: u32 = 5;
const MCOL_USE_STR: u32 = 6;
const MCOL_USE_RAW: u32 = 7;

// Network store columns
const NCOL_IFACE: u32 = 0;
const NCOL_RX_STR: u32 = 1;
const NCOL_RX_RAW: u32 = 2;
const NCOL_TX_STR: u32 = 3;
const NCOL_TX_RAW: u32 = 4;
const NCOL_RX_TOTAL: u32 = 5;
const NCOL_TX_TOTAL: u32 = 6;

struct SamplerState {
    prev_totals: CpuTotals,
    prev_per_core: Vec<CpuTotals>,
    prev_ticks: HashMap<i32, u64>,
    prev_disk: HashMap<String, DiskIo>,
    prev_net: HashMap<String, NetIo>,
    prev_instant: Instant,
    filter: String,
}

struct Widgets {
    list_store: gtk4::ListStore,
    users_store: gtk4::ListStore,
    users_summary_label: gtk4::Label,
    cpu_bar: gtk4::LevelBar,
    cpu_label: gtk4::Label,
    mem_bar: gtk4::LevelBar,
    mem_label: gtk4::Label,
    swap_bar: gtk4::LevelBar,
    swap_label: gtk4::Label,
    uptime_label: gtk4::Label,
    totals_label: gtk4::Label,
    perf_cpu_label: gtk4::Label,
    perf_mem_label: gtk4::Label,
    perf_load_label: gtk4::Label,
    per_core_bars: Vec<gtk4::LevelBar>,
    per_core_labels: Vec<gtk4::Label>,
    disk_io_store: gtk4::ListStore,
    disk_usage_store: gtk4::ListStore,
    network_store: gtk4::ListStore,
}

struct ServicesState {
    all: Vec<ServiceUnit>,
    filter: String,
}

pub fn setup(builder: &gtk4::Builder, window: &gtk4::ApplicationWindow) {
    let list_store = gtk4::ListStore::new(&[
        String::static_type(), // 0 name
        i32::static_type(),    // 1 pid
        String::static_type(), // 2 state
        String::static_type(), // 3 user
        String::static_type(), // 4 cpu str
        f32::static_type(),    // 5 cpu raw
        String::static_type(), // 6 mem str
        u64::static_type(),    // 7 mem raw kb
        u32::static_type(),    // 8 threads
    ]);

    let tree_view = gtk4::TreeView::new();
    tree_view.set_model(Some(&list_store));
    tree_view.set_headers_visible(true);
    tree_view.set_enable_search(true);
    tree_view.set_search_column(COL_NAME as i32);

    add_text_column(&tree_view, "Name", COL_NAME, COL_NAME, true);
    add_text_column(&tree_view, "PID", COL_PID, COL_PID, false);
    add_text_column(&tree_view, "Status", COL_STATE, COL_STATE, false);
    add_text_column(&tree_view, "User", COL_USER, COL_USER, false);
    add_text_column(&tree_view, "CPU %", COL_CPU_STR, COL_CPU_RAW, false);
    add_text_column(&tree_view, "Memory", COL_MEM_STR, COL_MEM_RAW, false);
    add_text_column(&tree_view, "Threads", COL_THREADS, COL_THREADS, false);

    let process_scroll: gtk4::ScrolledWindow = builder
        .object("process_scroll")
        .expect("process_scroll missing in layout.ui");
    process_scroll.set_child(Some(&tree_view));

    // ── Users TreeView ──
    let users_store = gtk4::ListStore::new(&[
        String::static_type(), // 0 user
        i32::static_type(),    // 1 uid
        u32::static_type(),    // 2 proc count
        String::static_type(), // 3 cpu str
        f32::static_type(),    // 4 cpu raw
        String::static_type(), // 5 mem str
        u64::static_type(),    // 6 mem raw
    ]);
    let users_view = gtk4::TreeView::new();
    users_view.set_model(Some(&users_store));
    users_view.set_headers_visible(true);
    add_text_column(&users_view, "User", UCOL_USER, UCOL_USER, true);
    add_text_column(&users_view, "UID", UCOL_UID, UCOL_UID, false);
    add_text_column(&users_view, "Processes", UCOL_PROCS, UCOL_PROCS, false);
    add_text_column(&users_view, "CPU %", UCOL_CPU_STR, UCOL_CPU_RAW, false);
    add_text_column(&users_view, "Memory", UCOL_MEM_STR, UCOL_MEM_RAW, false);
    let users_scroll: gtk4::ScrolledWindow = builder
        .object("users_scroll")
        .expect("users_scroll missing in layout.ui");
    users_scroll.set_child(Some(&users_view));

    // ── Services TreeView ──
    let services_store = gtk4::ListStore::new(&[
        String::static_type(), // 0 name
        String::static_type(), // 1 load
        String::static_type(), // 2 active
        String::static_type(), // 3 sub
        String::static_type(), // 4 description
    ]);
    let services_view = gtk4::TreeView::new();
    services_view.set_model(Some(&services_store));
    services_view.set_headers_visible(true);
    services_view.set_enable_search(true);
    services_view.set_search_column(SCOL_NAME as i32);
    add_text_column(&services_view, "Service", SCOL_NAME, SCOL_NAME, true);
    add_text_column(&services_view, "Load", SCOL_LOAD, SCOL_LOAD, false);
    add_text_column(&services_view, "Active", SCOL_ACTIVE, SCOL_ACTIVE, false);
    add_text_column(&services_view, "Sub", SCOL_SUB, SCOL_SUB, false);
    add_text_column(&services_view, "Description", SCOL_DESC, SCOL_DESC, true);
    let services_scroll: gtk4::ScrolledWindow = builder
        .object("services_scroll")
        .expect("services_scroll missing in layout.ui");
    services_scroll.set_child(Some(&services_view));

    // ── Per-core CPU bars (built once at startup) ──
    let per_core_grid: gtk4::Grid = builder.object("per_core_grid").unwrap();
    let initial_per_core = procfs::read_cpu_per_core();
    let core_count = initial_per_core.len();
    let mut per_core_bars: Vec<gtk4::LevelBar> = Vec::with_capacity(core_count);
    let mut per_core_labels: Vec<gtk4::Label> = Vec::with_capacity(core_count);
    let cols_per_row = if core_count > 8 { 2 } else { 1 };
    for i in 0..core_count {
        let col_group = (i % cols_per_row) * 3;
        let row = (i / cols_per_row) as i32;
        let name_label = gtk4::Label::new(Some(&format!("Core {i}")));
        name_label.set_xalign(0.0);
        name_label.set_width_chars(8);
        per_core_grid.attach(&name_label, col_group as i32, row, 1, 1);
        let bar = gtk4::LevelBar::builder()
            .min_value(0.0)
            .max_value(100.0)
            .hexpand(true)
            .build();
        per_core_grid.attach(&bar, col_group as i32 + 1, row, 1, 1);
        let pct = gtk4::Label::new(Some("0%"));
        pct.set_xalign(1.0);
        pct.set_width_chars(6);
        per_core_grid.attach(&pct, col_group as i32 + 2, row, 1, 1);
        per_core_bars.push(bar);
        per_core_labels.push(pct);
    }

    // ── Disk I/O TreeView ──
    let disk_io_store = gtk4::ListStore::new(&[
        String::static_type(), // device
        String::static_type(), // read str
        u64::static_type(),    // read raw
        String::static_type(), // write str
        u64::static_type(),    // write raw
    ]);
    let disk_io_view = gtk4::TreeView::new();
    disk_io_view.set_model(Some(&disk_io_store));
    disk_io_view.set_headers_visible(true);
    add_text_column(&disk_io_view, "Device", DCOL_DEVICE, DCOL_DEVICE, true);
    add_text_column(&disk_io_view, "Read", DCOL_READ_STR, DCOL_READ_RAW, false);
    add_text_column(&disk_io_view, "Write", DCOL_WRITE_STR, DCOL_WRITE_RAW, false);
    let disk_io_scroll: gtk4::ScrolledWindow = builder.object("disk_io_scroll").unwrap();
    disk_io_scroll.set_child(Some(&disk_io_view));

    // ── Disk Usage TreeView ──
    let disk_usage_store = gtk4::ListStore::new(&[
        String::static_type(), // mount
        String::static_type(), // device
        String::static_type(), // fs
        String::static_type(), // size
        String::static_type(), // used
        String::static_type(), // free
        String::static_type(), // use str
        f32::static_type(),    // use raw
    ]);
    let disk_usage_view = gtk4::TreeView::new();
    disk_usage_view.set_model(Some(&disk_usage_store));
    disk_usage_view.set_headers_visible(true);
    add_text_column(&disk_usage_view, "Mount", MCOL_MOUNT, MCOL_MOUNT, true);
    add_text_column(&disk_usage_view, "Device", MCOL_DEVICE, MCOL_DEVICE, false);
    add_text_column(&disk_usage_view, "Type", MCOL_FS, MCOL_FS, false);
    add_text_column(&disk_usage_view, "Size", MCOL_SIZE, MCOL_SIZE, false);
    add_text_column(&disk_usage_view, "Used", MCOL_USED, MCOL_USED, false);
    add_text_column(&disk_usage_view, "Free", MCOL_FREE, MCOL_FREE, false);
    add_text_column(&disk_usage_view, "Use", MCOL_USE_STR, MCOL_USE_RAW, false);
    let disk_usage_scroll: gtk4::ScrolledWindow = builder.object("disk_usage_scroll").unwrap();
    disk_usage_scroll.set_child(Some(&disk_usage_view));

    // ── Network TreeView ──
    let network_store = gtk4::ListStore::new(&[
        String::static_type(), // iface
        String::static_type(), // rx str
        u64::static_type(),    // rx raw
        String::static_type(), // tx str
        u64::static_type(),    // tx raw
        String::static_type(), // rx total
        String::static_type(), // tx total
    ]);
    let network_view = gtk4::TreeView::new();
    network_view.set_model(Some(&network_store));
    network_view.set_headers_visible(true);
    add_text_column(&network_view, "Interface", NCOL_IFACE, NCOL_IFACE, true);
    add_text_column(&network_view, "↓ Rate", NCOL_RX_STR, NCOL_RX_RAW, false);
    add_text_column(&network_view, "↑ Rate", NCOL_TX_STR, NCOL_TX_RAW, false);
    add_text_column(&network_view, "↓ Total", NCOL_RX_TOTAL, NCOL_RX_TOTAL, false);
    add_text_column(&network_view, "↑ Total", NCOL_TX_TOTAL, NCOL_TX_TOTAL, false);
    let network_scroll: gtk4::ScrolledWindow = builder.object("network_scroll").unwrap();
    network_scroll.set_child(Some(&network_view));

    let widgets = Rc::new(Widgets {
        list_store: list_store.clone(),
        users_store: users_store.clone(),
        users_summary_label: builder.object("users_summary_label").unwrap(),
        cpu_bar: builder.object("cpu_bar").unwrap(),
        cpu_label: builder.object("cpu_label").unwrap(),
        mem_bar: builder.object("mem_bar").unwrap(),
        mem_label: builder.object("mem_label").unwrap(),
        swap_bar: builder.object("swap_bar").unwrap(),
        swap_label: builder.object("swap_label").unwrap(),
        uptime_label: builder.object("uptime_label").unwrap(),
        totals_label: builder.object("totals_label").unwrap(),
        perf_cpu_label: builder.object("perf_cpu_label").unwrap(),
        perf_mem_label: builder.object("perf_mem_label").unwrap(),
        perf_load_label: builder.object("perf_load_label").unwrap(),
        per_core_bars,
        per_core_labels,
        disk_io_store: disk_io_store.clone(),
        disk_usage_store: disk_usage_store.clone(),
        network_store: network_store.clone(),
    });

    let perf_kernel_label: gtk4::Label = builder.object("perf_kernel_label").unwrap();
    let perf_host_label: gtk4::Label = builder.object("perf_host_label").unwrap();
    perf_kernel_label.set_text(&format!("Kernel: {}", procfs::read_kernel()));
    perf_host_label.set_text(&format!("Host: {}", procfs::read_hostname()));

    let search_entry: gtk4::SearchEntry = builder.object("search_entry").unwrap();
    let end_task_btn: gtk4::Button = builder.object("end_task_btn").unwrap();
    let details_btn: gtk4::Button = builder.object("details_btn").unwrap();
    let refresh_btn: gtk4::Button = builder.object("refresh_btn").unwrap();

    let state = Rc::new(RefCell::new(SamplerState {
        prev_totals: procfs::read_cpu_totals(),
        prev_per_core: initial_per_core,
        prev_ticks: HashMap::new(),
        prev_disk: HashMap::new(),
        prev_net: HashMap::new(),
        prev_instant: Instant::now(),
        filter: String::new(),
    }));
    let user_cache = Rc::new(RefCell::new(HashMap::<u32, String>::new()));

    // End Task
    {
        let tree_view = tree_view.clone();
        let window = window.clone();
        end_task_btn.connect_clicked(move |_| {
            if let Some(pid) = selected_pid(&tree_view) {
                confirm_and_kill(&window, pid);
            }
        });
    }

    // Details
    {
        let tree_view = tree_view.clone();
        let window = window.clone();
        details_btn.connect_clicked(move |_| {
            if let Some(pid) = selected_pid(&tree_view) {
                show_details_dialog(&window, pid);
            }
        });
    }

    // Selection changes toggle button sensitivity
    {
        let end_task_btn = end_task_btn.clone();
        let details_btn = details_btn.clone();
        tree_view.selection().connect_changed(move |sel| {
            let has = sel.selected().is_some();
            end_task_btn.set_sensitive(has);
            details_btn.set_sensitive(has);
        });
    }

    // Search filter
    {
        let state = state.clone();
        let widgets = widgets.clone();
        let sampler = state.clone();
        let cache = user_cache.clone();
        search_entry.connect_search_changed(move |entry| {
            state.borrow_mut().filter = entry.text().to_string().to_lowercase();
            // Re-paint immediately with new filter
            tick(&widgets, &sampler, &cache);
        });
    }

    // Refresh button
    {
        let widgets = widgets.clone();
        let state = state.clone();
        let cache = user_cache.clone();
        refresh_btn.connect_clicked(move |_| {
            tick(&widgets, &state, &cache);
        });
    }

    // ── Services wiring ──
    let services_state = Rc::new(RefCell::new(ServicesState {
        all: Vec::new(),
        filter: String::new(),
    }));
    let services_search: gtk4::SearchEntry = builder.object("services_search").unwrap();
    let services_status_label: gtk4::Label = builder.object("services_status_label").unwrap();
    let svc_refresh_btn: gtk4::Button = builder.object("svc_refresh_btn").unwrap();
    let svc_start_btn: gtk4::Button = builder.object("svc_start_btn").unwrap();
    let svc_stop_btn: gtk4::Button = builder.object("svc_stop_btn").unwrap();
    let svc_restart_btn: gtk4::Button = builder.object("svc_restart_btn").unwrap();

    // Search filter rebuilds the visible store from the cached full list.
    {
        let services_state = services_state.clone();
        let services_store = services_store.clone();
        let services_status_label = services_status_label.clone();
        services_search.connect_search_changed(move |entry| {
            services_state.borrow_mut().filter =
                entry.text().to_string().to_lowercase();
            repaint_services(&services_state, &services_store, &services_status_label);
        });
    }

    // Refresh button: re-run systemctl and repaint.
    {
        let services_state = services_state.clone();
        let services_store = services_store.clone();
        let services_status_label = services_status_label.clone();
        svc_refresh_btn.connect_clicked(move |_| {
            reload_services(&services_state, &services_store, &services_status_label);
        });
    }

    // Selection enables action buttons based on the current Active state.
    {
        let svc_start_btn = svc_start_btn.clone();
        let svc_stop_btn = svc_stop_btn.clone();
        let svc_restart_btn = svc_restart_btn.clone();
        services_view.selection().connect_changed(move |sel| {
            if let Some((model, iter)) = sel.selected() {
                let active = model.get::<String>(&iter, SCOL_ACTIVE as i32);
                let is_active = active == "active";
                svc_start_btn.set_sensitive(!is_active);
                svc_stop_btn.set_sensitive(is_active);
                svc_restart_btn.set_sensitive(true);
            } else {
                svc_start_btn.set_sensitive(false);
                svc_stop_btn.set_sensitive(false);
                svc_restart_btn.set_sensitive(false);
            }
        });
    }

    // Action buttons
    for (btn, action) in [
        (&svc_start_btn, "start"),
        (&svc_stop_btn, "stop"),
        (&svc_restart_btn, "restart"),
    ] {
        let services_view = services_view.clone();
        let services_state = services_state.clone();
        let services_store_for_cb = services_store.clone();
        let services_status_label_for_cb = services_status_label.clone();
        let window = window.clone();
        let action = action.to_string();
        btn.connect_clicked(move |_| {
            let Some(name) = selected_service_name(&services_view) else {
                return;
            };
            match services::run_action(&action, &name) {
                Ok(()) => {
                    reload_services(
                        &services_state,
                        &services_store_for_cb,
                        &services_status_label_for_cb,
                    );
                }
                Err(e) => {
                    show_error_dialog(&window, &format!("{action} {name}"), &e);
                }
            }
        });
    }

    // First load of services (so the tab is populated when the user clicks it).
    reload_services(&services_state, &services_store, &services_status_label);

    // ── Right-click context menu on the process TreeView ──
    install_process_context_menu(&tree_view, window);

    // ── Keyboard shortcuts ──
    {
        let notebook: gtk4::Notebook = builder.object("main_notebook").unwrap();
        let end_task_btn = end_task_btn.clone();
        let refresh_btn = refresh_btn.clone();
        let search_entry = search_entry.clone();
        let key = gtk4::EventControllerKey::new();
        key.connect_key_pressed(move |_, keyval, _keycode, modifier| {
            use gtk4::gdk::Key;
            let on_processes_tab = notebook.current_page() == Some(0);
            if modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK) && keyval == Key::f {
                if on_processes_tab {
                    search_entry.grab_focus();
                }
                return glib::Propagation::Stop;
            }
            if keyval == Key::F5 {
                refresh_btn.emit_clicked();
                return glib::Propagation::Stop;
            }
            if keyval == Key::Delete && on_processes_tab && end_task_btn.is_sensitive() {
                end_task_btn.emit_clicked();
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        window.add_controller(key);
    }

    // Initial paint
    tick(&widgets, &state, &user_cache);

    // Periodic refresh
    {
        let widgets = widgets.clone();
        let state = state.clone();
        let cache = user_cache.clone();
        glib::timeout_add_local(Duration::from_secs(REFRESH_SECS), move || {
            tick(&widgets, &state, &cache);
            ControlFlow::Continue
        });
    }
}

fn add_text_column(
    tree_view: &gtk4::TreeView,
    title: &str,
    text_col: u32,
    sort_col: u32,
    expand: bool,
) {
    let col = gtk4::TreeViewColumn::new();
    col.set_title(title);
    col.set_resizable(true);
    col.set_sort_column_id(sort_col as i32);
    col.set_expand(expand);
    let cell = gtk4::CellRendererText::new();
    col.pack_start(&cell, true);
    col.add_attribute(&cell, "text", text_col as i32);
    tree_view.append_column(&col);
}

fn selected_pid(tree_view: &gtk4::TreeView) -> Option<i32> {
    let selection = tree_view.selection();
    let (model, iter) = selection.selected()?;
    model.get::<i32>(&iter, COL_PID as i32).into()
}

fn tick(
    w: &Rc<Widgets>,
    state: &Rc<RefCell<SamplerState>>,
    user_cache: &Rc<RefCell<HashMap<u32, String>>>,
) {
    let now = Instant::now();
    let curr_totals = procfs::read_cpu_totals();
    let mem = procfs::read_meminfo();

    let (cpu_percent, interval_secs) = {
        let st = state.borrow();
        let total_d = curr_totals.total.saturating_sub(st.prev_totals.total);
        let idle_d = curr_totals.idle.saturating_sub(st.prev_totals.idle);
        let pct = if total_d == 0 {
            0.0f32
        } else {
            (1.0 - idle_d as f32 / total_d as f32) * 100.0
        };
        let dt = now.duration_since(st.prev_instant).as_secs_f32().max(0.001);
        (pct.clamp(0.0, 100.0), dt)
    };

    w.cpu_bar.set_value(cpu_percent as f64);
    w.cpu_label.set_text(&format!("{cpu_percent:.1}%"));
    w.perf_cpu_label.set_text(&format!("CPU: {cpu_percent:.1}% ({} cores)", w.per_core_bars.len()));

    // ── Per-core CPU ──
    let curr_per_core = procfs::read_cpu_per_core();
    {
        let st = state.borrow();
        for (i, bar) in w.per_core_bars.iter().enumerate() {
            let curr = curr_per_core.get(i).copied().unwrap_or_default();
            let prev = st.prev_per_core.get(i).copied().unwrap_or_default();
            let pct = procfs::cpu_percent(prev, curr);
            bar.set_value(pct as f64);
            if let Some(lbl) = w.per_core_labels.get(i) {
                lbl.set_text(&format!("{pct:.0}%"));
            }
        }
    }

    let mem_used_kb = mem.mem_total_kb.saturating_sub(mem.mem_avail_kb);
    let mem_pct = if mem.mem_total_kb > 0 {
        (mem_used_kb as f64 / mem.mem_total_kb as f64) * 100.0
    } else {
        0.0
    };
    w.mem_bar.set_value(mem_pct);
    w.mem_label.set_text(&format!(
        "{} / {}",
        procfs::format_kb_as_gb(mem_used_kb),
        procfs::format_kb_as_gb(mem.mem_total_kb)
    ));
    w.perf_mem_label.set_text(&format!(
        "Memory: {} / {} ({:.1}%)",
        procfs::format_kb_as_gb(mem_used_kb),
        procfs::format_kb_as_gb(mem.mem_total_kb),
        mem_pct
    ));

    let swap_used_kb = mem.swap_total_kb.saturating_sub(mem.swap_free_kb);
    let swap_pct = if mem.swap_total_kb > 0 {
        (swap_used_kb as f64 / mem.swap_total_kb as f64) * 100.0
    } else {
        0.0
    };
    w.swap_bar.set_value(swap_pct);
    w.swap_label.set_text(&format!(
        "{} / {}",
        procfs::format_kb_as_gb(swap_used_kb),
        procfs::format_kb_as_gb(mem.swap_total_kb)
    ));

    w.uptime_label.set_text(&procfs::format_uptime(procfs::read_uptime_secs()));
    w.perf_load_label.set_text(&format!("Load average: {}", procfs::read_loadavg()));

    // ── Disk I/O rates ──
    let curr_disk = procfs::read_disk_io();
    {
        let st = state.borrow();
        w.disk_io_store.clear();
        for d in &curr_disk {
            let prev = st.prev_disk.get(&d.device);
            let (read_rate, write_rate) = match prev {
                Some(p) => (
                    delta_rate(d.read_bytes, p.read_bytes, interval_secs),
                    delta_rate(d.write_bytes, p.write_bytes, interval_secs),
                ),
                None => (0, 0),
            };
            w.disk_io_store.insert_with_values(
                None,
                &[
                    (DCOL_DEVICE, &d.device),
                    (DCOL_READ_STR, &procfs::format_rate(read_rate)),
                    (DCOL_READ_RAW, &read_rate),
                    (DCOL_WRITE_STR, &procfs::format_rate(write_rate)),
                    (DCOL_WRITE_RAW, &write_rate),
                ],
            );
        }
    }

    // ── Disk usage (statvfs is expensive, so only every 5 ticks ≈ 10s) ──
    {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if n % 5 == 0 {
            w.disk_usage_store.clear();
            for m in procfs::read_mount_usage() {
                let pct = (m.used_bytes as f64 / m.total_bytes as f64 * 100.0) as f32;
                w.disk_usage_store.insert_with_values(
                    None,
                    &[
                        (MCOL_MOUNT, &m.mount_point),
                        (MCOL_DEVICE, &m.device),
                        (MCOL_FS, &m.fs_type),
                        (MCOL_SIZE, &procfs::format_bytes(m.total_bytes)),
                        (MCOL_USED, &procfs::format_bytes(m.used_bytes)),
                        (MCOL_FREE, &procfs::format_bytes(m.free_bytes)),
                        (MCOL_USE_STR, &format!("{pct:.0}%")),
                        (MCOL_USE_RAW, &pct),
                    ],
                );
            }
        }
    }

    // ── Network I/O ──
    let curr_net = procfs::read_net_io();
    {
        let st = state.borrow();
        w.network_store.clear();
        for n in &curr_net {
            let prev = st.prev_net.get(&n.iface);
            let (rx_rate, tx_rate) = match prev {
                Some(p) => (
                    delta_rate(n.rx_bytes, p.rx_bytes, interval_secs),
                    delta_rate(n.tx_bytes, p.tx_bytes, interval_secs),
                ),
                None => (0, 0),
            };
            w.network_store.insert_with_values(
                None,
                &[
                    (NCOL_IFACE, &n.iface),
                    (NCOL_RX_STR, &procfs::format_rate(rx_rate)),
                    (NCOL_RX_RAW, &rx_rate),
                    (NCOL_TX_STR, &procfs::format_rate(tx_rate)),
                    (NCOL_TX_RAW, &tx_rate),
                    (NCOL_RX_TOTAL, &procfs::format_bytes(n.rx_bytes)),
                    (NCOL_TX_TOTAL, &procfs::format_bytes(n.tx_bytes)),
                ],
            );
        }
    }

    // ── Processes (diff-update preserves selection and sort) ──
    let mut cache = user_cache.borrow_mut();
    let mut procs: Vec<ProcInfo> = procfs::read_processes(&mut cache);
    drop(cache);

    let clk = procfs::clock_ticks_per_sec() as f32;
    let prev_ticks: HashMap<i32, u64> = state.borrow().prev_ticks.clone();
    for p in procs.iter_mut() {
        let prev = prev_ticks.get(&p.pid).copied().unwrap_or(p.cpu_ticks);
        let delta = p.cpu_ticks.saturating_sub(prev) as f32;
        p.cpu_percent = (delta / clk / interval_secs) * 100.0;
    }

    let filter = state.borrow().filter.clone();
    let thread_total: u64 = procs.iter().map(|p| p.threads as u64).sum();
    let proc_count = procs.len();

    diff_update_processes(&w.list_store, &procs, &filter);

    w.totals_label.set_text(&format!("{proc_count} processes · {thread_total} threads"));

    // ── Users aggregation from the same process snapshot ──
    struct UserAgg {
        name: String,
        uid: u32,
        proc_count: u32,
        cpu_sum: f32,
        mem_sum_kb: u64,
    }
    let mut agg: HashMap<u32, UserAgg> = HashMap::new();
    for p in &procs {
        let e = agg.entry(p.uid).or_insert_with(|| UserAgg {
            name: p.user.clone(),
            uid: p.uid,
            proc_count: 0,
            cpu_sum: 0.0,
            mem_sum_kb: 0,
        });
        e.proc_count += 1;
        e.cpu_sum += p.cpu_percent;
        e.mem_sum_kb += p.rss_kb;
    }
    let mut users: Vec<UserAgg> = agg.into_values().collect();
    users.sort_by(|a, b| b.cpu_sum.partial_cmp(&a.cpu_sum).unwrap_or(std::cmp::Ordering::Equal));

    w.users_store.clear();
    for u in &users {
        w.users_store.insert_with_values(
            None,
            &[
                (UCOL_USER, &u.name),
                (UCOL_UID, &(u.uid as i32)),
                (UCOL_PROCS, &u.proc_count),
                (UCOL_CPU_STR, &format!("{:.1}%", u.cpu_sum)),
                (UCOL_CPU_RAW, &u.cpu_sum),
                (UCOL_MEM_STR, &procfs::format_kb_as_mb(u.mem_sum_kb)),
                (UCOL_MEM_RAW, &u.mem_sum_kb),
            ],
        );
    }
    w.users_summary_label.set_text(&format!(
        "{} user(s) · aggregated from {proc_count} processes",
        users.len()
    ));

    let mut st = state.borrow_mut();
    st.prev_totals = curr_totals;
    st.prev_per_core = curr_per_core;
    st.prev_instant = now;
    st.prev_ticks = procs.iter().map(|p| (p.pid, p.cpu_ticks)).collect();
    st.prev_disk = curr_disk.into_iter().map(|d| (d.device.clone(), d)).collect();
    st.prev_net = curr_net.into_iter().map(|n| (n.iface.clone(), n)).collect();
}

fn delta_rate(curr: u64, prev: u64, interval_secs: f32) -> u64 {
    let delta = curr.saturating_sub(prev) as f32;
    (delta / interval_secs.max(0.001)) as u64
}

fn diff_update_processes(store: &gtk4::ListStore, procs: &[ProcInfo], filter: &str) {
    let filter_lower = filter.to_lowercase();
    let target: HashMap<i32, &ProcInfo> = procs
        .iter()
        .filter(|p| filter_lower.is_empty() || p.name.to_lowercase().contains(&filter_lower))
        .map(|p| (p.pid, p))
        .collect();

    let mut seen: HashSet<i32> = HashSet::new();

    // Walk existing rows: update in place, or remove if no longer present.
    if let Some(iter) = store.iter_first() {
        let mut valid = true;
        while valid {
            let pid = store.get::<i32>(&iter, COL_PID as i32);
            if let Some(p) = target.get(&pid) {
                store.set(
                    &iter,
                    &[
                        (COL_NAME, &p.name),
                        (COL_STATE, &procfs::state_label(p.state).to_string()),
                        (COL_USER, &p.user),
                        (COL_CPU_STR, &format!("{:.1}%", p.cpu_percent)),
                        (COL_CPU_RAW, &p.cpu_percent),
                        (COL_MEM_STR, &procfs::format_kb_as_mb(p.rss_kb)),
                        (COL_MEM_RAW, &p.rss_kb),
                        (COL_THREADS, &p.threads),
                    ],
                );
                seen.insert(pid);
                valid = store.iter_next(&iter);
            } else {
                valid = store.remove(&iter);
            }
        }
    }

    // Append rows for PIDs not previously in the store.
    for (pid, p) in &target {
        if !seen.contains(pid) {
            store.insert_with_values(
                None,
                &[
                    (COL_NAME, &p.name),
                    (COL_PID, &p.pid),
                    (COL_STATE, &procfs::state_label(p.state).to_string()),
                    (COL_USER, &p.user),
                    (COL_CPU_STR, &format!("{:.1}%", p.cpu_percent)),
                    (COL_CPU_RAW, &p.cpu_percent),
                    (COL_MEM_STR, &procfs::format_kb_as_mb(p.rss_kb)),
                    (COL_MEM_RAW, &p.rss_kb),
                    (COL_THREADS, &p.threads),
                ],
            );
        }
    }
}

// ── Services helpers ───────────────────────────────────────────────────────

fn reload_services(
    state: &Rc<RefCell<ServicesState>>,
    store: &gtk4::ListStore,
    status_label: &gtk4::Label,
) {
    let units = services::list_services();
    state.borrow_mut().all = units;
    repaint_services(state, store, status_label);
}

fn repaint_services(
    state: &Rc<RefCell<ServicesState>>,
    store: &gtk4::ListStore,
    status_label: &gtk4::Label,
) {
    let st = state.borrow();
    store.clear();
    let mut shown = 0usize;
    let mut active = 0usize;
    let mut failed = 0usize;
    for u in &st.all {
        if u.active == "active" {
            active += 1;
        }
        if u.active == "failed" {
            failed += 1;
        }
        if !st.filter.is_empty() && !u.name.to_lowercase().contains(&st.filter) {
            continue;
        }
        store.insert_with_values(
            None,
            &[
                (SCOL_NAME, &u.name),
                (SCOL_LOAD, &u.load),
                (SCOL_ACTIVE, &u.active),
                (SCOL_SUB, &u.sub),
                (SCOL_DESC, &u.description),
            ],
        );
        shown += 1;
    }
    status_label.set_text(&format!(
        "{} shown · {} total · {} active · {} failed",
        shown,
        st.all.len(),
        active,
        failed
    ));
}

fn selected_service_name(tree_view: &gtk4::TreeView) -> Option<String> {
    let selection = tree_view.selection();
    let (model, iter) = selection.selected()?;
    Some(model.get::<String>(&iter, SCOL_NAME as i32))
}

// ── Process right-click context menu ───────────────────────────────────────

fn install_process_context_menu(
    tree_view: &gtk4::TreeView,
    window: &gtk4::ApplicationWindow,
) {
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    let tv = tree_view.clone();
    let win = window.clone();
    gesture.connect_pressed(move |g, _n, x, y| {
        // If the click hit a row, select it first so selection matches the menu action.
        if let Some((Some(path), _, _, _)) = tv.path_at_pos(x as i32, y as i32) {
            tv.selection().select_path(&path);
        } else {
            return;
        }
        let Some(pid) = selected_pid(&tv) else {
            return;
        };
        let Some((_, iter)) = tv.selection().selected() else {
            return;
        };
        let name: String = tv.model().unwrap().get(&iter, COL_NAME as i32);
        show_process_context_menu(&tv, &win, pid, &name, x, y);
        g.set_state(gtk4::EventSequenceState::Claimed);
    });
    tree_view.add_controller(gesture);
}

fn show_process_context_menu(
    parent: &gtk4::TreeView,
    window: &gtk4::ApplicationWindow,
    pid: i32,
    name: &str,
    x: f64,
    y: f64,
) {
    let popover = gtk4::PopoverMenu::from_model(None::<&gtk4::gio::MenuModel>);
    popover.set_parent(parent);
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    vbox.set_margin_top(6);
    vbox.set_margin_bottom(6);
    vbox.set_margin_start(6);
    vbox.set_margin_end(6);

    let header = gtk4::Label::new(Some(&format!("{name}  (PID {pid})")));
    header.set_xalign(0.0);
    header.add_css_class("dim-label");
    header.set_margin_start(8);
    header.set_margin_end(8);
    vbox.append(&header);
    vbox.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    // Terminate (SIGTERM)
    {
        let pop = popover.clone();
        let win = window.clone();
        let btn = menu_button("Terminate (SIGTERM)", move |_| {
            pop.popdown();
            confirm_and_signal(&win, pid, libc::SIGTERM, "Terminate");
        });
        vbox.append(&btn);
    }

    // Kill (SIGKILL)
    {
        let pop = popover.clone();
        let win = window.clone();
        let btn = menu_button("Kill (SIGKILL)", move |_| {
            pop.popdown();
            confirm_and_signal(&win, pid, libc::SIGKILL, "Kill");
        });
        vbox.append(&btn);
    }

    vbox.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));

    // Details
    {
        let pop = popover.clone();
        let win = window.clone();
        let btn = menu_button("Details…", move |_| {
            pop.popdown();
            show_details_dialog(&win, pid);
        });
        vbox.append(&btn);
    }

    // Copy PID
    {
        let pop = popover.clone();
        let parent = parent.clone();
        let btn = menu_button("Copy PID", move |_| {
            pop.popdown();
            parent.display().clipboard().set_text(&pid.to_string());
        });
        vbox.append(&btn);
    }

    // Copy Name
    {
        let pop = popover.clone();
        let parent = parent.clone();
        let name = name.to_string();
        let btn = menu_button("Copy Name", move |_| {
            pop.popdown();
            parent.display().clipboard().set_text(&name);
        });
        vbox.append(&btn);
    }

    // Open working dir in rFM
    {
        let pop = popover.clone();
        let btn = menu_button("Open Working Directory", move |_| {
            pop.popdown();
            let cwd = std::fs::read_link(format!("/proc/{pid}/cwd"));
            if let Ok(path) = cwd {
                let _ = std::process::Command::new("rfm").arg(&path).spawn();
            }
        });
        vbox.append(&btn);
    }

    popover.set_child(Some(&vbox));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn menu_button(label: &str, clicked: impl Fn(&gtk4::Button) + 'static) -> gtk4::Button {
    let btn = gtk4::Button::with_label(label);
    btn.add_css_class("flat");
    btn.set_halign(gtk4::Align::Fill);
    if let Some(l) = btn.child().and_then(|c| c.downcast::<gtk4::Label>().ok()) {
        l.set_xalign(0.0);
    }
    btn.connect_clicked(clicked);
    btn
}

fn confirm_and_signal(
    window: &gtk4::ApplicationWindow,
    pid: i32,
    signal: libc::c_int,
    verb: &str,
) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk4::MessageType::Warning)
        .buttons(gtk4::ButtonsType::OkCancel)
        .text(&format!("{verb} process {pid}?"))
        .secondary_text("Unsaved data may be lost.")
        .build();
    dialog.connect_response(move |dlg, resp| {
        if resp == gtk4::ResponseType::Ok {
            let rc = unsafe { libc::kill(pid as libc::pid_t, signal) };
            if rc != 0 {
                log::warn!("kill({pid}, {signal}) failed");
            }
        }
        dlg.close();
    });
    dialog.present();
}

fn show_error_dialog(window: &gtk4::ApplicationWindow, title: &str, body: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk4::MessageType::Error)
        .buttons(gtk4::ButtonsType::Close)
        .text(title)
        .secondary_text(body)
        .build();
    dialog.connect_response(|dlg, _| dlg.close());
    dialog.present();
}

fn confirm_and_kill(window: &gtk4::ApplicationWindow, pid: i32) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk4::MessageType::Warning)
        .buttons(gtk4::ButtonsType::OkCancel)
        .text(&format!("End process {pid}?"))
        .secondary_text("The process will receive SIGTERM. Unsaved data may be lost.")
        .build();
    dialog.connect_response(move |dlg, resp| {
        if resp == gtk4::ResponseType::Ok {
            let rc = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
            if rc != 0 {
                log::warn!("kill({pid}, SIGTERM) failed");
            }
        }
        dlg.close();
    });
    dialog.present();
}

fn show_details_dialog(window: &gtk4::ApplicationWindow, pid: i32) {
    let exe = std::fs::read_link(format!("/proc/{pid}/exe"))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "—".into());
    let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .map(|s| s.replace('\0', " ").trim().to_string())
        .unwrap_or_else(|_| "—".into());
    let cwd = std::fs::read_link(format!("/proc/{pid}/cwd"))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "—".into());

    let dialog = gtk4::MessageDialog::builder()
        .transient_for(window)
        .modal(true)
        .message_type(gtk4::MessageType::Info)
        .buttons(gtk4::ButtonsType::Close)
        .text(&format!("PID {pid}"))
        .secondary_text(&format!(
            "Executable:\n  {exe}\n\nCommand line:\n  {cmdline}\n\nWorking directory:\n  {cwd}"
        ))
        .build();
    dialog.connect_response(|dlg, _| dlg.close());
    dialog.present();
}
