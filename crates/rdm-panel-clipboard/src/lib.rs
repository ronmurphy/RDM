use gtk4::glib;
use gtk4::prelude::*;
use rdm_panel_api::RdmPluginInfo;
use std::cell::RefCell;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Config ────────────────────────────────────────────────────────────────────

struct Config {
    /// Maximum number of clipboard entries to keep.
    max_entries: usize,
    /// Maximum display length (chars) for text previews in the list.
    preview_chars: usize,
    /// Maximum size in bytes for image entries (skip larger images).
    max_image_bytes: usize,
}

impl Config {
    fn from_toml(src: Option<&str>) -> Self {
        let mut cfg = Self::default();
        let Some(src) = src else { return cfg };
        let Ok(val) = src.parse::<toml::Value>() else {
            return cfg;
        };
        if let Some(v) = val.get("max_entries").and_then(|v| v.as_integer()) {
            if v > 0 {
                cfg.max_entries = v.min(200) as usize;
            }
        }
        if let Some(v) = val.get("preview_chars").and_then(|v| v.as_integer()) {
            if v > 0 {
                cfg.preview_chars = v.min(500) as usize;
            }
        }
        if let Some(v) = val.get("max_image_bytes").and_then(|v| v.as_integer()) {
            if v > 0 {
                cfg.max_image_bytes = v as usize;
            }
        }
        cfg
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            max_entries: 50,
            preview_chars: 80,
            max_image_bytes: 10 * 1024 * 1024, // 10 MB
        }
    }
}

// ── Clipboard entry ───────────────────────────────────────────────────────────

#[derive(Clone)]
enum ClipEntry {
    Text(String),
    Image(Vec<u8>), // PNG data
}

impl ClipEntry {
    fn preview(&self, max_chars: usize) -> String {
        match self {
            ClipEntry::Text(s) => {
                let line = s.lines().next().unwrap_or("");
                if line.len() > max_chars {
                    format!("{}…", &line[..max_chars])
                } else if s.lines().count() > 1 {
                    format!("{} …", line)
                } else {
                    line.to_string()
                }
            }
            ClipEntry::Image(data) => {
                let kb = data.len() / 1024;
                format!("🖼 Image ({} KB)", kb)
            }
        }
    }

    fn is_duplicate(&self, other: &ClipEntry) -> bool {
        match (self, other) {
            (ClipEntry::Text(a), ClipEntry::Text(b)) => a == b,
            (ClipEntry::Image(a), ClipEntry::Image(b)) => a == b,
            _ => false,
        }
    }
}

// ── Shared state ──────────────────────────────────────────────────────────────

struct ClipState {
    entries: Vec<ClipEntry>,
    max_entries: usize,
    max_image_bytes: usize,
}

impl ClipState {
    fn new(max_entries: usize, max_image_bytes: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
            max_image_bytes,
        }
    }

    fn push(&mut self, entry: ClipEntry) {
        // Skip oversized images
        if let ClipEntry::Image(ref data) = entry {
            if data.len() > self.max_image_bytes {
                return;
            }
        }
        // Skip empty text
        if let ClipEntry::Text(ref s) = entry {
            if s.trim().is_empty() {
                return;
            }
        }
        // Remove duplicate if already in history
        self.entries.retain(|e| !e.is_duplicate(&entry));
        // Insert at front (most recent first)
        self.entries.insert(0, entry);
        // Trim to max
        self.entries.truncate(self.max_entries);
    }
}

// ── Plugin struct ─────────────────────────────────────────────────────────────

struct ClipboardPlugin {
    #[allow(dead_code)]
    button: gtk4::MenuButton,
}

thread_local! {
    static INSTANCES: RefCell<Vec<ClipboardPlugin>> = RefCell::new(Vec::new());
}

// ── Exported C ABI symbols ────────────────────────────────────────────────────

#[no_mangle]
pub extern "C-unwind" fn rdm_plugin_info() -> RdmPluginInfo {
    RdmPluginInfo {
        name: c"clipboard".as_ptr(),
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
    INSTANCES.with(|v| v.borrow_mut().push(ClipboardPlugin { button }));
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
    btn.set_label(" 📋 ");
    btn.add_css_class("tray-btn");
    btn.add_css_class("task-popup-btn");

    let pop = gtk4::Popover::new();
    pop.set_has_arrow(false);
    pop.add_css_class("clipboard-popover");

    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    root.set_margin_top(8);
    root.set_margin_bottom(8);
    root.set_margin_start(10);
    root.set_margin_end(10);
    root.set_size_request(360, -1);

    // Title bar with clear button
    let header = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let title = gtk4::Label::new(Some("Clipboard History"));
    title.set_halign(gtk4::Align::Start);
    title.set_hexpand(true);
    title.add_css_class("task-popup-title");
    header.append(&title);

    let clear_btn = gtk4::Button::with_label("Clear");
    clear_btn.add_css_class("destructive-action");
    header.append(&clear_btn);
    root.append(&header);

    // Scrollable list area
    let scroll = gtk4::ScrolledWindow::new();
    scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    scroll.set_min_content_height(100);
    scroll.set_max_content_height(400);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);
    list_box.add_css_class("clipboard-list");
    scroll.set_child(Some(&list_box));
    root.append(&scroll);

    // Empty state label
    let empty_label = gtk4::Label::new(Some("No clipboard entries yet"));
    empty_label.add_css_class("dim-label");
    empty_label.set_margin_top(20);
    empty_label.set_margin_bottom(20);
    list_box.append(&empty_label);

    pop.set_child(Some(&root));
    btn.set_popover(Some(&pop));

    // Shared state
    let state = Arc::new(Mutex::new(ClipState::new(cfg.max_entries, cfg.max_image_bytes)));
    let preview_chars = cfg.preview_chars;

    // Channel for background watcher → GTK thread
    let (tx, rx) = async_channel::unbounded::<ClipEntry>();

    // Start background clipboard watcher thread
    start_clipboard_watcher(tx);

    // Receive new entries on the GTK thread
    let state_rx = Arc::clone(&state);
    let list_box_rx = list_box.clone();
    let pop_ref = pop.clone();
    glib::spawn_future_local({
        let empty_label = empty_label.clone();
        async move {
            while let Ok(entry) = rx.recv().await {
                {
                    let mut st = state_rx.lock().unwrap();
                    st.push(entry);
                }
                rebuild_list(&list_box_rx, &state_rx, preview_chars, &pop_ref, &empty_label);
            }
        }
    });

    // Clear button
    let state_clear = Arc::clone(&state);
    let list_box_clear = list_box.clone();
    let pop_clear = pop.clone();
    let empty_label_clear = empty_label.clone();
    clear_btn.connect_clicked(move |_| {
        {
            let mut st = state_clear.lock().unwrap();
            st.entries.clear();
        }
        rebuild_list(&list_box_clear, &state_clear, preview_chars, &pop_clear, &empty_label_clear);
    });

    btn
}

// ── List UI builder ───────────────────────────────────────────────────────────

fn rebuild_list(
    list_box: &gtk4::ListBox,
    state: &Arc<Mutex<ClipState>>,
    preview_chars: usize,
    popover: &gtk4::Popover,
    empty_label: &gtk4::Label,
) {
    // Remove all existing rows
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let st = state.lock().unwrap();

    if st.entries.is_empty() {
        empty_label.set_visible(true);
        list_box.append(empty_label);
        return;
    }
    empty_label.set_visible(false);

    for (i, entry) in st.entries.iter().enumerate() {
        let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row.set_margin_top(2);
        row.set_margin_bottom(2);
        row.set_margin_start(4);
        row.set_margin_end(4);

        match entry {
            ClipEntry::Image(data) => {
                // Show thumbnail
                let bytes = glib::Bytes::from(data);
                let stream = gtk4::gio::MemoryInputStream::from_bytes(&bytes);
                if let Ok(pixbuf) =
                    gtk4::gdk_pixbuf::Pixbuf::from_stream_at_scale(
                        &stream,
                        48,
                        48,
                        true,
                        None::<&gtk4::gio::Cancellable>,
                    )
                {
                    let texture = gtk4::gdk::Texture::for_pixbuf(&pixbuf);
                    let img = gtk4::Image::from_paintable(Some(&texture));
                    img.set_pixel_size(48);
                    row.append(&img);
                }
                let lbl = gtk4::Label::new(Some(&entry.preview(preview_chars)));
                lbl.set_hexpand(true);
                lbl.set_halign(gtk4::Align::Start);
                lbl.set_xalign(0.0);
                lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                row.append(&lbl);
            }
            ClipEntry::Text(_) => {
                let lbl = gtk4::Label::new(Some(&entry.preview(preview_chars)));
                lbl.set_hexpand(true);
                lbl.set_halign(gtk4::Align::Start);
                lbl.set_xalign(0.0);
                lbl.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                row.append(&lbl);
            }
        }

        // Copy button
        let copy_btn = gtk4::Button::with_label("Copy");
        copy_btn.add_css_class("flat");
        copy_btn.add_css_class("clipboard-copy-btn");
        let entry_clone = entry.clone();
        let pop_clone = popover.clone();
        copy_btn.connect_clicked(move |_| {
            copy_entry_to_clipboard(&entry_clone);
            pop_clone.popdown();
        });
        row.append(&copy_btn);

        list_box.append(&row);

        // Limit visible rows to avoid a massive popover
        if i >= 29 {
            break;
        }
    }
}

// ── Copy entry back to clipboard ──────────────────────────────────────────────

fn copy_entry_to_clipboard(entry: &ClipEntry) {
    match entry {
        ClipEntry::Text(text) => {
            let mut child = match Command::new("wl-copy")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Some(ref mut stdin) = child.stdin {
                use std::io::Write;
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
        ClipEntry::Image(data) => {
            let mut child = match Command::new("wl-copy")
                .arg("--type")
                .arg("image/png")
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(_) => return,
            };
            if let Some(ref mut stdin) = child.stdin {
                use std::io::Write;
                let _ = stdin.write_all(data);
            }
            let _ = child.wait();
        }
    }
}

// ── Background clipboard watcher ──────────────────────────────────────────────

fn start_clipboard_watcher(tx: async_channel::Sender<ClipEntry>) {
    // Two watchers: one for text, one for images.
    // They run in separate threads.

    // Text watcher
    let tx_text = tx.clone();
    std::thread::spawn(move || {
        loop {
            // wl-paste --watch writes the clipboard contents to stdout every
            // time the clipboard changes. We use --no-newline to preserve exact content.
            let child = Command::new("wl-paste")
                .args(["--watch", "cat"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();

            let mut child = match child {
                Ok(c) => c,
                Err(_) => {
                    // wl-paste not available, retry after a delay
                    std::thread::sleep(Duration::from_secs(10));
                    continue;
                }
            };

            if let Some(stdout) = child.stdout.take() {
                // wl-paste --watch spawns `cat` each time clipboard changes,
                // and outputs the contents. We read each "chunk" separated by
                // process boundaries. However, --watch keeps the stream open,
                // so we need a different approach: run wl-paste in a poll loop.
                drop(stdout);
            }
            let _ = child.kill();
            let _ = child.wait();
            break;
        }

        // Fallback: poll-based approach — simpler and more reliable
        let mut last_text = String::new();
        loop {
            std::thread::sleep(Duration::from_millis(500));

            // Check for text content
            if let Ok(output) = Command::new("wl-paste")
                .args(["--no-newline"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
            {
                if output.status.success() {
                    if let Ok(text) = String::from_utf8(output.stdout) {
                        if !text.is_empty() && text != last_text {
                            last_text = text.clone();
                            let _ = tx_text.send_blocking(ClipEntry::Text(text));
                        }
                    }
                }
            }
        }
    });

    // Image watcher
    std::thread::spawn(move || {
        let mut last_hash: u64 = 0;
        loop {
            std::thread::sleep(Duration::from_millis(800));

            // Try to get image/png from clipboard
            if let Ok(output) = Command::new("wl-paste")
                .args(["--no-newline", "--type", "image/png"])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .output()
            {
                if output.status.success() && !output.stdout.is_empty() {
                    // Simple hash to detect changes without storing full image for comparison
                    let hash = simple_hash(&output.stdout);
                    if hash != last_hash {
                        last_hash = hash;
                        let _ = tx.send_blocking(ClipEntry::Image(output.stdout));
                    }
                }
            }
        }
    });
}

/// Quick non-cryptographic hash for change detection only.
fn simple_hash(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}
