#![allow(deprecated)]

use gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

struct NavState {
    current_path: PathBuf,
    back_stack: Vec<PathBuf>,
    forward_stack: Vec<PathBuf>,
    show_hidden: bool,
    icon_mode: bool,
}

/// Shared widget handles (GTK objects are ref-counted, so Clone is cheap).
struct Widgets {
    path_entry: gtk4::Entry,
    list_store: gtk4::ListStore,
    tree_view: gtk4::TreeView,
    icon_view: gtk4::FlowBox,
    file_list_sw: gtk4::ScrolledWindow,
    status_bar: gtk4::Label,
}

pub fn connect_handlers(builder: &gtk4::Builder) {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let state = Rc::new(RefCell::new(NavState {
        current_path: PathBuf::from(&home),
        back_stack: Vec::new(),
        forward_stack: Vec::new(),
        show_hidden: false,
        icon_mode: false,
    }));

    // --- Widgets from builder ---
    let path_entry: gtk4::Entry = builder.object("path_entry").unwrap();
    let folder_tree_sw: gtk4::ScrolledWindow = builder.object("folder_tree").unwrap();
    let file_list_sw: gtk4::ScrolledWindow = builder.object("file_list").unwrap();
    let status_bar: gtk4::Label = builder.object("status_bar").unwrap();

    // --- Places sidebar ---
    let places_list = gtk4::ListBox::new();
    places_list.add_css_class("navigation-sidebar");
    folder_tree_sw.set_child(Some(&places_list));

    let desktop_p = format!("{home}/Desktop");
    let documents_p = format!("{home}/Documents");
    let downloads_p = format!("{home}/Downloads");
    let music_p = format!("{home}/Music");
    let pictures_p = format!("{home}/Pictures");
    let videos_p = format!("{home}/Videos");

    let places: &[(&str, &str, &str)] = &[
        ("user-home-symbolic", "Home", &home),
        ("folder-desktop-symbolic", "Desktop", &desktop_p),
        ("folder-documents-symbolic", "Documents", &documents_p),
        ("folder-download-symbolic", "Downloads", &downloads_p),
        ("folder-music-symbolic", "Music", &music_p),
        ("folder-pictures-symbolic", "Pictures", &pictures_p),
        ("folder-videos-symbolic", "Videos", &videos_p),
        ("drive-harddisk-symbolic", "File System", "/"),
    ];

    for &(icon, label, path) in places {
        if !std::path::Path::new(path).exists() && path != "/" {
            continue;
        }
        let row = gtk4::ListBoxRow::new();
        let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);
        hbox.append(&gtk4::Image::from_icon_name(icon));
        let lbl = gtk4::Label::new(Some(label));
        lbl.set_xalign(0.0);
        hbox.append(&lbl);
        row.set_child(Some(&hbox));
        row.set_widget_name(path);
        places_list.append(&row);
    }

    // --- List view (TreeView + ListStore) ---
    let list_store = gtk4::ListStore::new(&[
        String::static_type(),  // 0: icon-name
        String::static_type(),  // 1: file name
        String::static_type(),  // 2: size display
        String::static_type(),  // 3: type
        String::static_type(),  // 4: modified
        i64::static_type(),     // 5: raw size (for sorting)
        bool::static_type(),    // 6: is_dir
        Pixbuf::static_type(),  // 7: thumbnail pixbuf (nullable)
    ]);

    let tree_view = gtk4::TreeView::new();
    tree_view.set_model(Some(&list_store));
    tree_view.set_headers_visible(true);
    tree_view.set_enable_search(true);
    tree_view.set_search_column(1);

    // Name column (icon/thumbnail + text)
    {
        let col = gtk4::TreeViewColumn::new();
        col.set_title("Name");
        col.set_expand(true);
        col.set_sort_column_id(1);
        col.set_resizable(true);
        let icon_cell = gtk4::CellRendererPixbuf::new();
        col.pack_start(&icon_cell, false);
        col.add_attribute(&icon_cell, "icon-name", 0);
        col.add_attribute(&icon_cell, "pixbuf", 7); // overrides icon-name when non-null
        let text_cell = gtk4::CellRendererText::new();
        col.pack_start(&text_cell, true);
        col.add_attribute(&text_cell, "text", 1);
        tree_view.append_column(&col);
    }
    // Size column
    {
        let col = gtk4::TreeViewColumn::new();
        col.set_title("Size");
        col.set_sort_column_id(5);
        col.set_resizable(true);
        let cell = gtk4::CellRendererText::new();
        col.pack_start(&cell, false);
        col.add_attribute(&cell, "text", 2);
        tree_view.append_column(&col);
    }
    // Type column
    {
        let col = gtk4::TreeViewColumn::new();
        col.set_title("Type");
        col.set_sort_column_id(3);
        col.set_resizable(true);
        let cell = gtk4::CellRendererText::new();
        col.pack_start(&cell, false);
        col.add_attribute(&cell, "text", 3);
        tree_view.append_column(&col);
    }
    // Modified column
    {
        let col = gtk4::TreeViewColumn::new();
        col.set_title("Modified");
        col.set_sort_column_id(4);
        col.set_resizable(true);
        let cell = gtk4::CellRendererText::new();
        col.pack_start(&cell, false);
        col.add_attribute(&cell, "text", 4);
        tree_view.append_column(&col);
    }

    // Start with list view
    file_list_sw.set_child(Some(&tree_view));

    // --- Icon view (FlowBox) ---
    let icon_view = gtk4::FlowBox::new();
    icon_view.set_homogeneous(true);
    icon_view.set_min_children_per_line(3);
    icon_view.set_max_children_per_line(20);
    icon_view.set_column_spacing(4);
    icon_view.set_row_spacing(4);
    icon_view.set_valign(gtk4::Align::Start);
    icon_view.set_selection_mode(gtk4::SelectionMode::Single);
    icon_view.set_activate_on_single_click(false);

    // --- Bundle shared widgets ---
    let w = Rc::new(Widgets {
        path_entry: path_entry.clone(),
        list_store,
        tree_view,
        icon_view,
        file_list_sw: file_list_sw.clone(),
        status_bar: status_bar.clone(),
    });

    // --- Connect signals ---

    // Path entry
    {
        let s = state.clone();
        let w = w.clone();
        path_entry.connect_activate(move |entry| {
            navigate(&s, PathBuf::from(entry.text().as_str()), true, &w);
        });
    }

    // Places sidebar
    {
        let s = state.clone();
        let w = w.clone();
        places_list.connect_row_activated(move |_, row| {
            navigate(&s, PathBuf::from(row.widget_name().as_str()), true, &w);
        });
    }

    // List view double-click / Enter
    {
        let s = state.clone();
        let w = w.clone();
        let store_ref = w.list_store.clone();
        let tv = w.tree_view.clone();
        tv.connect_row_activated(move |_, path, _| {
            let model = store_ref.upcast_ref::<gtk4::TreeModel>();
            if let Some(iter) = model.iter(path) {
                let name: String = model.get_value(&iter, 1).get().unwrap();
                let is_dir: bool = model.get_value(&iter, 6).get().unwrap();
                let current = s.borrow().current_path.clone();
                let full = current.join(&name);
                if is_dir {
                    navigate(&s, full, true, &w);
                } else {
                    let _ = std::process::Command::new("xdg-open").arg(&full).spawn();
                }
            }
        });
    }

    // Icon view double-click / Enter
    {
        let s = state.clone();
        let w = w.clone();
        let iv = w.icon_view.clone();
        iv.connect_child_activated(move |_, child| {
            let name = child.widget_name();
            let current = s.borrow().current_path.clone();
            let full = current.join(name.as_str());
            if full.is_dir() {
                navigate(&s, full, true, &w);
            } else {
                let _ = std::process::Command::new("xdg-open").arg(&full).spawn();
            }
        });
    }

    // Back
    if let Some(btn) = builder.object::<gtk4::Button>("btn_back") {
        let s = state.clone();
        let w = w.clone();
        btn.connect_clicked(move |_| {
            let p = {
                let mut st = s.borrow_mut();
                st.back_stack.pop().map(|prev| {
                    let cur = st.current_path.clone();
                    st.forward_stack.push(cur);
                    prev
                })
            };
            if let Some(p) = p {
                navigate(&s, p, false, &w);
            }
        });
    }

    // Forward
    if let Some(btn) = builder.object::<gtk4::Button>("btn_forward") {
        let s = state.clone();
        let w = w.clone();
        btn.connect_clicked(move |_| {
            let p = {
                let mut st = s.borrow_mut();
                st.forward_stack.pop().map(|next| {
                    let cur = st.current_path.clone();
                    st.back_stack.push(cur);
                    next
                })
            };
            if let Some(p) = p {
                navigate(&s, p, false, &w);
            }
        });
    }

    // Up
    if let Some(btn) = builder.object::<gtk4::Button>("btn_up") {
        let s = state.clone();
        let w = w.clone();
        btn.connect_clicked(move |_| {
            let parent = s.borrow().current_path.parent().map(|p| p.to_path_buf());
            if let Some(p) = parent {
                navigate(&s, p, true, &w);
            }
        });
    }

    // Home
    if let Some(btn) = builder.object::<gtk4::Button>("btn_home") {
        let s = state.clone();
        let w = w.clone();
        let h = home.clone();
        btn.connect_clicked(move |_| {
            navigate(&s, PathBuf::from(&h), true, &w);
        });
    }

    // Refresh
    if let Some(btn) = builder.object::<gtk4::Button>("btn_refresh") {
        let s = state.clone();
        let w = w.clone();
        btn.connect_clicked(move |_| {
            let current = s.borrow().current_path.clone();
            navigate(&s, current, false, &w);
        });
    }

    // Hidden files toggle
    if let Some(btn) = builder.object::<gtk4::ToggleButton>("btn_hidden") {
        let s = state.clone();
        let w = w.clone();
        btn.connect_toggled(move |toggle| {
            s.borrow_mut().show_hidden = toggle.is_active();
            let current = s.borrow().current_path.clone();
            navigate(&s, current, false, &w);
        });
    }

    // View toggle (radio group)
    let btn_list: Option<gtk4::ToggleButton> = builder.object("btn_view_list");
    let btn_grid: Option<gtk4::ToggleButton> = builder.object("btn_view_grid");
    if let (Some(ref bl), Some(ref bg)) = (&btn_list, &btn_grid) {
        bg.set_group(Some(bl));
    }
    if let Some(btn) = btn_grid.clone() {
        let s = state.clone();
        let w = w.clone();
        btn.connect_toggled(move |toggle| {
            s.borrow_mut().icon_mode = toggle.is_active();
            let current = s.borrow().current_path.clone();
            navigate(&s, current, false, &w);
        });
    }

    // --- Initial navigation ---
    navigate(&state, PathBuf::from(&home), false, &w);
}

// ---------------------------------------------------------------------------
// Core navigation: read a directory and populate the active view
// ---------------------------------------------------------------------------

fn navigate(state: &Rc<RefCell<NavState>>, path: PathBuf, push_history: bool, w: &Widgets) {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            w.status_bar.set_text(&format!("Error: {e}"));
            return;
        }
    };

    if !canonical.is_dir() {
        return;
    }

    let (show_hidden, icon_mode) = {
        let mut s = state.borrow_mut();
        if push_history && s.current_path != canonical {
            let prev = s.current_path.clone();
            s.back_stack.push(prev);
            s.forward_stack.clear();
        }
        s.current_path = canonical.clone();
        (s.show_hidden, s.icon_mode)
    };

    w.path_entry.set_text(&canonical.to_string_lossy());

    let mut entries = match fs::read_dir(&canonical) {
        Ok(dir) => dir.flatten().collect::<Vec<_>>(),
        Err(e) => {
            w.status_bar.set_text(&format!("Cannot read directory: {e}"));
            return;
        }
    };

    // Sort: directories first, then alphabetical (case-insensitive)
    entries.sort_by(|a, b| {
        let ad = a.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let bd = b.file_type().map(|t| t.is_dir()).unwrap_or(false);
        bd.cmp(&ad).then_with(|| {
            a.file_name()
                .to_ascii_lowercase()
                .cmp(&b.file_name().to_ascii_lowercase())
        })
    });

    // Swap to the correct view
    if icon_mode {
        w.file_list_sw.set_child(Some(&w.icon_view));
        while let Some(child) = w.icon_view.first_child() {
            w.icon_view.remove(&child);
        }
    } else {
        w.file_list_sw.set_child(Some(&w.tree_view));
        w.list_store.clear();
    }

    let mut count = 0u32;
    for entry in &entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }

        let meta = entry.metadata().ok();
        let ft = entry.file_type().ok();
        let is_dir = ft.as_ref().map(|t| t.is_dir()).unwrap_or(false);
        let is_symlink = ft.as_ref().map(|t| t.is_symlink()).unwrap_or(false);
        let icon = if is_dir { "folder-symbolic" } else { file_icon(&name) };
        let full_path = canonical.join(&name);

        if icon_mode {
            // ----- Icon view with thumbnails -----
            const GRID_SIZE: i32 = 96;
            let icon_full = icon.trim_end_matches("-symbolic");
            let thumb = if !is_dir { load_thumbnail(&full_path, 256) } else { None };

            let item = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
            item.set_halign(gtk4::Align::Center);
            item.set_margin_top(4);
            item.set_margin_bottom(4);
            item.set_margin_start(4);
            item.set_margin_end(4);

            if let Some(ref pb) = thumb {
                // Use Picture for thumbnails — it scales content properly
                let texture = gtk4::gdk::Texture::for_pixbuf(pb);
                let pic = gtk4::Picture::for_paintable(&texture);
                pic.set_content_fit(gtk4::ContentFit::Contain);
                pic.set_can_shrink(true);
                pic.set_size_request(GRID_SIZE, GRID_SIZE);
                pic.set_halign(gtk4::Align::Center);
                pic.set_valign(gtk4::Align::Center);
                item.append(&pic);
            } else {
                let img = gtk4::Image::from_icon_name(icon_full);
                img.set_pixel_size(48);
                img.set_size_request(GRID_SIZE, GRID_SIZE);
                item.append(&img);
            };

            let label = gtk4::Label::new(Some(&name));
            label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
            label.set_max_width_chars(14);
            label.set_wrap(true);
            label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
            label.set_lines(2);
            label.set_justify(gtk4::Justification::Center);
            item.append(&label);

            let fb_child = gtk4::FlowBoxChild::new();
            fb_child.set_child(Some(&item));
            fb_child.set_widget_name(&name);
            w.icon_view.append(&fb_child);
        } else {
            // ----- List view with thumbnails -----
            let (size_display, raw_size) = if is_dir {
                ("\u{2014}".to_string(), -1i64)
            } else if let Some(ref m) = meta {
                let s = m.len();
                (format_size(s), s as i64)
            } else {
                ("?".to_string(), 0i64)
            };

            let type_str = if is_dir {
                "Folder".to_string()
            } else if is_symlink {
                "Symbolic link".to_string()
            } else {
                file_type_name(&name)
            };

            let modified = meta
                .and_then(|m| m.modified().ok())
                .map(format_time)
                .unwrap_or_else(|| "\u{2014}".to_string());

            let thumb = if !is_dir { load_thumbnail(&full_path, 24) } else { None };

            let iter = w.list_store.append();
            w.list_store.set_value(&iter, 0, &icon.to_value());
            w.list_store.set_value(&iter, 1, &name.to_value());
            w.list_store.set_value(&iter, 2, &size_display.to_value());
            w.list_store.set_value(&iter, 3, &type_str.to_value());
            w.list_store.set_value(&iter, 4, &modified.to_value());
            w.list_store.set_value(&iter, 5, &raw_size.to_value());
            w.list_store.set_value(&iter, 6, &is_dir.to_value());
            if let Some(ref pb) = thumb {
                w.list_store.set_value(&iter, 7, &pb.to_value());
            }
        }
        count += 1;
    }

    let dir_name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "/".to_string());
    w.status_bar.set_text(&format!("{count} items in {dir_name}"));
}

// ---------------------------------------------------------------------------
// Thumbnail loading
// ---------------------------------------------------------------------------

/// Try to load a thumbnail for image/video files. Returns None for other types
/// or on any loading error.
fn load_thumbnail(path: &Path, size: i32) -> Option<Pixbuf> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "webp" | "svg" | "ico" | "tiff" | "tif" => {
            Pixbuf::from_file_at_scale(path, size, size, true).ok()
        }
        "mp4" | "mkv" | "avi" | "webm" | "mov" | "wmv" | "flv" => {
            load_xdg_thumbnail(path, size)
        }
        _ => None,
    }
}

/// Look up a video thumbnail: first check the XDG thumbnail cache
/// (~/.cache/thumbnails/), then try generating one with ffmpegthumbnailer.
fn load_xdg_thumbnail(path: &Path, size: i32) -> Option<Pixbuf> {
    use gtk4::glib;
    // Use glib's URI encoder for correct percent-encoding (spaces, brackets, etc.)
    let uri = glib::filename_to_uri(path, None).ok()?;
    let md5 = glib::compute_checksum_for_data(glib::ChecksumType::Md5, uri.as_bytes())?;
    let home = std::env::var("HOME").ok()?;

    // Check existing cache — try large first (better quality), then normal
    for dir in &["large", "normal"] {
        let thumb_path = format!("{home}/.cache/thumbnails/{dir}/{md5}.png");
        if let Ok(pb) = Pixbuf::from_file_at_scale(&thumb_path, size, size, true) {
            return Some(pb);
        }
    }

    // Fall back to ffmpegthumbnailer if installed
    let tmp = format!("/tmp/rfm-thumb-{md5}.png");
    let ok = std::process::Command::new("ffmpegthumbnailer")
        .args(["-i", &path.to_string_lossy(), "-o", &tmp, "-s", "256"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let pb = Pixbuf::from_file_at_scale(&tmp, size, size, true).ok();
        let _ = std::fs::remove_file(&tmp);
        return pb;
    }

    None
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn format_time(time: std::time::SystemTime) -> String {
    let secs = time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, mo, d, h, mi) = epoch_to_ymd_hm(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}")
}

fn epoch_to_ymd_hm(epoch: u64) -> (u64, u64, u64, u64, u64) {
    let total_min = epoch / 60;
    let mi = total_min % 60;
    let total_hr = total_min / 60;
    let h = total_hr % 24;
    let mut days = total_hr / 24;

    let mut y = 1970u64;
    loop {
        let ylen = if is_leap(y) { 366 } else { 365 };
        if days < ylen {
            break;
        }
        days -= ylen;
        y += 1;
    }

    let mdays = if is_leap(y) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut mo = 1u64;
    for md in mdays {
        if days < md {
            break;
        }
        days -= md;
        mo += 1;
    }

    (y, mo, days + 1, h, mi)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn file_icon(name: &str) -> &'static str {
    let l = name.to_lowercase();
    match l.rsplit('.').next().unwrap_or("") {
        "rs" | "py" | "js" | "ts" | "c" | "h" | "cpp" | "go" | "java" | "rb" | "sh" => {
            "text-x-script-symbolic"
        }
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "bmp" | "webp" | "ico" => {
            "image-x-generic-symbolic"
        }
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" => "audio-x-generic-symbolic",
        "mp4" | "mkv" | "avi" | "webm" | "mov" | "wmv" => "video-x-generic-symbolic",
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst" => {
            "package-x-generic-symbolic"
        }
        "pdf" | "doc" | "docx" | "odt" => "x-office-document-symbolic",
        "xls" | "xlsx" | "ods" | "csv" => "x-office-spreadsheet-symbolic",
        "html" | "htm" => "text-html-symbolic",
        _ => "text-x-generic-symbolic",
    }
}

fn file_type_name(name: &str) -> String {
    let l = name.to_lowercase();
    let ext = l.rsplit('.').next().unwrap_or("");
    if ext == l || ext.is_empty() {
        return "File".to_string();
    }
    match ext {
        "txt" => "Text file",
        "md" => "Markdown",
        "rs" => "Rust source",
        "py" => "Python script",
        "js" => "JavaScript",
        "ts" => "TypeScript",
        "c" => "C source",
        "h" => "C header",
        "cpp" | "cc" | "cxx" => "C++ source",
        "go" => "Go source",
        "java" => "Java source",
        "rb" => "Ruby script",
        "sh" | "bash" | "zsh" => "Shell script",
        "html" | "htm" => "HTML document",
        "css" => "Stylesheet",
        "json" => "JSON",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "xml" => "XML",
        "ini" | "conf" | "cfg" => "Config file",
        "png" => "PNG image",
        "jpg" | "jpeg" => "JPEG image",
        "gif" => "GIF image",
        "svg" => "SVG image",
        "webp" => "WebP image",
        "mp3" => "MP3 audio",
        "flac" => "FLAC audio",
        "wav" => "WAV audio",
        "mp4" => "MP4 video",
        "mkv" => "MKV video",
        "avi" => "AVI video",
        "webm" => "WebM video",
        "pdf" => "PDF document",
        "doc" | "docx" => "Word document",
        "xls" | "xlsx" => "Spreadsheet",
        "zip" => "ZIP archive",
        "tar" => "TAR archive",
        "gz" => "Gzip archive",
        "xz" => "XZ archive",
        "7z" => "7-Zip archive",
        "rar" => "RAR archive",
        "zst" => "Zstd archive",
        "iso" => "Disk image",
        "log" => "Log file",
        "lock" => "Lock file",
        "csv" => "CSV data",
        "sql" => "SQL script",
        "ui" => "UI definition",
        other => return format!("{} file", other.to_uppercase()),
    }
    .to_string()
}
