#![allow(deprecated)]

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::prelude::*;
use gtk4::{gio, glib};
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
    /// Clipboard for cut/copy operations: (paths, is_cut)
    clipboard: Option<(Vec<PathBuf>, bool)>,
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
        clipboard: None,
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

    // --- Right-click context menu (list view) ---
    {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3); // right-click
        let s = state.clone();
        let wc = w.clone();
        let tv = w.tree_view.clone();
        let store = w.list_store.clone();
        gesture.connect_pressed(move |g, _n, x, y| {
            // Find which row was clicked
            if let Some((Some(path), _, _, _)) = tv.path_at_pos(x as i32, y as i32) {
                let model = store.upcast_ref::<gtk4::TreeModel>();
                if let Some(iter) = model.iter(&path) {
                    let name: String = model.get_value(&iter, 1).get().unwrap();
                    let is_dir: bool = model.get_value(&iter, 6).get().unwrap();
                    let full = s.borrow().current_path.join(&name);
                    show_context_menu(&s, &wc, &tv, full, is_dir, x, y);
                }
            } else {
                // Right-click on empty area — show background menu
                let cwd = s.borrow().current_path.clone();
                show_background_menu(&s, &wc, &tv, cwd, x, y);
            }
            g.set_state(gtk4::EventSequenceState::Claimed);
        });
        w.tree_view.add_controller(gesture);
    }

    // --- Right-click context menu (icon view) ---
    {
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3);
        let s = state.clone();
        let wc = w.clone();
        let iv = w.icon_view.clone();
        gesture.connect_pressed(move |g, _n, x, y| {
            // Find which child was clicked via pick()
            if let Some(picked) = iv.pick(x, y, gtk4::PickFlags::DEFAULT) {
                // Walk up to find the FlowBoxChild
                let mut widget = Some(picked);
                let mut found_child: Option<gtk4::FlowBoxChild> = None;
                while let Some(w) = widget {
                    if let Ok(fbc) = w.clone().downcast::<gtk4::FlowBoxChild>() {
                        found_child = Some(fbc);
                        break;
                    }
                    widget = w.parent();
                }
                if let Some(fbc) = found_child {
                    let name = fbc.widget_name();
                    if !name.is_empty() {
                        let full = s.borrow().current_path.join(name.as_str());
                        let is_dir = full.is_dir();
                        show_context_menu(&s, &wc, &iv, full, is_dir, x, y);
                        g.set_state(gtk4::EventSequenceState::Claimed);
                        return;
                    }
                }
            }
            // Empty area
            let cwd = s.borrow().current_path.clone();
            show_background_menu(&s, &wc, &iv, cwd, x, y);
            g.set_state(gtk4::EventSequenceState::Claimed);
        });
        w.icon_view.add_controller(gesture);
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

// ---------------------------------------------------------------------------
// Context menus
// ---------------------------------------------------------------------------

fn show_context_menu(
    state: &Rc<RefCell<NavState>>,
    w: &Rc<Widgets>,
    parent: &impl IsA<gtk4::Widget>,
    path: PathBuf,
    is_dir: bool,
    x: f64,
    y: f64,
) {
    let popover = gtk4::PopoverMenu::from_model(None::<&gio::MenuModel>);
    popover.set_parent(parent);
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    vbox.set_margin_top(6);
    vbox.set_margin_bottom(6);
    vbox.set_margin_start(6);
    vbox.set_margin_end(6);

    // Open / Open With
    if !is_dir {
        let open_btn = menu_button("Open", {
            let p = path.clone();
            let pop = popover.clone();
            move |_| {
                pop.popdown();
                let _ = std::process::Command::new("xdg-open").arg(&p).spawn();
            }
        });
        vbox.append(&open_btn);

        // "Open With" submenu
        let apps = find_apps_for_file(&path);
        if !apps.is_empty() {
            let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
            vbox.append(&sep);
            let label = gtk4::Label::new(Some("Open With"));
            label.set_xalign(0.0);
            label.add_css_class("dim-label");
            label.set_margin_start(8);
            vbox.append(&label);

            for (app_name, exec, icon) in apps {
                let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
                if let Some(ref ic) = icon {
                    let img = gtk4::Image::from_icon_name(ic);
                    img.set_pixel_size(16);
                    hbox.append(&img);
                }
                let lbl = gtk4::Label::new(Some(&app_name));
                lbl.set_xalign(0.0);
                hbox.append(&lbl);

                let btn = gtk4::Button::new();
                btn.set_child(Some(&hbox));
                btn.add_css_class("flat");
                let p = path.clone();
                let pop = popover.clone();
                btn.connect_clicked(move |_| {
                    pop.popdown();
                    launch_desktop_exec(&exec, &p);
                });
                vbox.append(&btn);
            }
        }
    } else {
        let open_btn = menu_button("Open Folder", {
            let s = state.clone();
            let wc = w.clone();
            let p = path.clone();
            let pop = popover.clone();
            move |_| {
                pop.popdown();
                navigate(&s, p.clone(), true, &wc);
            }
        });
        vbox.append(&open_btn);

        let term_btn = menu_button("Open Terminal Here", {
            let p = path.clone();
            let pop = popover.clone();
            move |_| {
                pop.popdown();
                let _ = std::process::Command::new("foot")
                    .current_dir(&p)
                    .spawn();
            }
        });
        vbox.append(&term_btn);
    }

    let sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    vbox.append(&sep1);

    // Cut / Copy / Paste
    let cut_btn = menu_button("Cut", {
        let s = state.clone();
        let p = path.clone();
        let pop = popover.clone();
        move |_| {
            pop.popdown();
            s.borrow_mut().clipboard = Some((vec![p.clone()], true));
        }
    });
    vbox.append(&cut_btn);

    let copy_btn = menu_button("Copy", {
        let s = state.clone();
        let p = path.clone();
        let pop = popover.clone();
        move |_| {
            pop.popdown();
            s.borrow_mut().clipboard = Some((vec![p.clone()], false));
        }
    });
    vbox.append(&copy_btn);

    // Only show Paste if clipboard has content
    if state.borrow().clipboard.is_some() {
        let paste_btn = menu_button("Paste", {
            let s = state.clone();
            let wc = w.clone();
            let pop = popover.clone();
            move |_| {
                pop.popdown();
                do_paste(&s, &wc);
            }
        });
        vbox.append(&paste_btn);
    }

    let sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    vbox.append(&sep2);

    // Rename
    let rename_btn = menu_button("Rename", {
        let s = state.clone();
        let wc = w.clone();
        let p = path.clone();
        let pop = popover.clone();
        let parent_win = parent.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        move |_| {
            pop.popdown();
            show_rename_dialog(&s, &wc, &p, parent_win.as_ref());
        }
    });
    vbox.append(&rename_btn);

    // New Folder
    let newfolder_btn = menu_button("New Folder", {
        let s = state.clone();
        let wc = w.clone();
        let pop = popover.clone();
        let parent_win = parent.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        move |_| {
            pop.popdown();
            show_new_folder_dialog(&s, &wc, parent_win.as_ref());
        }
    });
    vbox.append(&newfolder_btn);

    // Copy Path
    let copy_path_btn = menu_button("Copy Path", {
        let p = path.clone();
        let pop = popover.clone();
        move |_| {
            pop.popdown();
            if let Some(display) = gtk4::gdk::Display::default() {
                display.clipboard().set_text(&p.to_string_lossy());
            }
        }
    });
    vbox.append(&copy_path_btn);

    let sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
    vbox.append(&sep3);

    // Move to Trash
    let trash_btn = menu_button("Move to Trash", {
        let s = state.clone();
        let wc = w.clone();
        let p = path.clone();
        let pop = popover.clone();
        move |_| {
            pop.popdown();
            let _ = std::process::Command::new("gio")
                .args(["trash", &p.to_string_lossy()])
                .spawn();
            // Refresh after a short delay
            let sc = s.clone();
            let wcc = wc.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                let cwd = sc.borrow().current_path.clone();
                navigate(&sc, cwd, false, &wcc);
            });
        }
    });
    vbox.append(&trash_btn);

    popover.set_child(Some(&vbox));
    // Unparent on close so refreshing the view doesn't trigger "remove non-child"
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

/// Background context menu (right-click on empty space)
fn show_background_menu(
    state: &Rc<RefCell<NavState>>,
    w: &Rc<Widgets>,
    parent: &impl IsA<gtk4::Widget>,
    cwd: PathBuf,
    x: f64,
    y: f64,
) {
    let popover = gtk4::PopoverMenu::from_model(None::<&gio::MenuModel>);
    popover.set_parent(parent);
    popover.set_has_arrow(false);
    popover.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    vbox.set_margin_top(6);
    vbox.set_margin_bottom(6);
    vbox.set_margin_start(6);
    vbox.set_margin_end(6);

    // Paste (if clipboard has content)
    if state.borrow().clipboard.is_some() {
        let paste_btn = menu_button("Paste", {
            let s = state.clone();
            let wc = w.clone();
            let pop = popover.clone();
            move |_| {
                pop.popdown();
                do_paste(&s, &wc);
            }
        });
        vbox.append(&paste_btn);

        let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        vbox.append(&sep);
    }

    // New Folder
    let newfolder_btn = menu_button("New Folder", {
        let s = state.clone();
        let wc = w.clone();
        let pop = popover.clone();
        let parent_win = parent.root().and_then(|r| r.downcast::<gtk4::Window>().ok());
        move |_| {
            pop.popdown();
            show_new_folder_dialog(&s, &wc, parent_win.as_ref());
        }
    });
    vbox.append(&newfolder_btn);

    // Open Terminal Here
    let term_btn = menu_button("Open Terminal Here", {
        let pop = popover.clone();
        move |_| {
            pop.popdown();
            let _ = std::process::Command::new("foot")
                .current_dir(&cwd)
                .spawn();
        }
    });
    vbox.append(&term_btn);

    popover.set_child(Some(&vbox));
    popover.connect_closed(|p| p.unparent());
    popover.popup();
}

fn menu_button(label: &str, clicked: impl Fn(&gtk4::Button) + 'static) -> gtk4::Button {
    let btn = gtk4::Button::with_label(label);
    btn.add_css_class("flat");
    btn.set_halign(gtk4::Align::Fill);
    let lbl = btn
        .child()
        .and_then(|c| c.downcast::<gtk4::Label>().ok());
    if let Some(l) = lbl {
        l.set_xalign(0.0);
    }
    btn.connect_clicked(clicked);
    btn
}

// ---------------------------------------------------------------------------
// Rename / New Folder dialogs
// ---------------------------------------------------------------------------

fn show_rename_dialog(
    state: &Rc<RefCell<NavState>>,
    w: &Rc<Widgets>,
    path: &Path,
    parent: Option<&gtk4::Window>,
) {
    let old_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let dialog = gtk4::Window::builder()
        .title("Rename")
        .default_width(360)
        .modal(true)
        .build();
    if let Some(p) = parent {
        dialog.set_transient_for(Some(p));
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let entry = gtk4::Entry::new();
    entry.set_text(&old_name);
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label("Cancel");
    let rename = gtk4::Button::with_label("Rename");
    rename.add_css_class("suggested-action");
    btn_box.append(&cancel);
    btn_box.append(&rename);
    vbox.append(&btn_box);
    dialog.set_child(Some(&vbox));

    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());

    let d = dialog.clone();
    let p = path.to_path_buf();
    let s = state.clone();
    let wc = w.clone();
    let entry_c = entry.clone();
    let do_rename = move || {
        let new_name = entry_c.text().to_string();
        if !new_name.is_empty() && new_name != old_name {
            let new_path = p.parent().unwrap_or(Path::new("/")).join(&new_name);
            if let Err(e) = fs::rename(&p, &new_path) {
                log::error!("Rename failed: {}", e);
            }
            let cwd = s.borrow().current_path.clone();
            navigate(&s, cwd, false, &wc);
        }
        d.close();
    };
    let do_rename_c = do_rename.clone();
    rename.connect_clicked(move |_| do_rename_c());
    entry.connect_activate(move |_| do_rename());

    dialog.present();
}

fn show_new_folder_dialog(
    state: &Rc<RefCell<NavState>>,
    w: &Rc<Widgets>,
    parent: Option<&gtk4::Window>,
) {
    let dialog = gtk4::Window::builder()
        .title("New Folder")
        .default_width(360)
        .modal(true)
        .build();
    if let Some(p) = parent {
        dialog.set_transient_for(Some(p));
    }

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let entry = gtk4::Entry::new();
    entry.set_placeholder_text(Some("Folder name"));
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);
    let cancel = gtk4::Button::with_label("Cancel");
    let create = gtk4::Button::with_label("Create");
    create.add_css_class("suggested-action");
    btn_box.append(&cancel);
    btn_box.append(&create);
    vbox.append(&btn_box);
    dialog.set_child(Some(&vbox));

    let d = dialog.clone();
    cancel.connect_clicked(move |_| d.close());

    let d = dialog.clone();
    let s = state.clone();
    let wc = w.clone();
    let entry_c = entry.clone();
    let do_create = move || {
        let name = entry_c.text().to_string();
        if !name.is_empty() {
            let dir = s.borrow().current_path.join(&name);
            if let Err(e) = fs::create_dir_all(&dir) {
                log::error!("Create folder failed: {}", e);
            }
            let cwd = s.borrow().current_path.clone();
            navigate(&s, cwd, false, &wc);
        }
        d.close();
    };
    let do_create_c = do_create.clone();
    create.connect_clicked(move |_| do_create_c());
    entry.connect_activate(move |_| do_create());

    dialog.present();
}

// ---------------------------------------------------------------------------
// Cut / Copy / Paste
// ---------------------------------------------------------------------------

fn do_paste(state: &Rc<RefCell<NavState>>, w: &Rc<Widgets>) {
    let (paths, is_cut) = match state.borrow().clipboard.clone() {
        Some(c) => c,
        None => return,
    };
    let dest = state.borrow().current_path.clone();

    for src in &paths {
        let name = src.file_name().unwrap_or_default();
        let target = dest.join(name);
        if target == *src {
            continue; // same location
        }
        if is_cut {
            if let Err(e) = fs::rename(src, &target) {
                // rename fails across devices — fall back to copy+delete
                if copy_recursive(src, &target).is_ok() {
                    let _ = remove_recursive(src);
                } else {
                    log::error!("Move failed: {}", e);
                }
            }
        } else {
            if let Err(e) = copy_recursive(src, &target) {
                log::error!("Copy failed: {}", e);
            }
        }
    }

    // Clear clipboard on cut (it's been moved)
    if is_cut {
        state.borrow_mut().clipboard = None;
    }

    let cwd = state.borrow().current_path.clone();
    navigate(state, cwd, false, w);
}

fn copy_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    if src.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &dest.join(entry.file_name()))?;
        }
    } else {
        fs::copy(src, dest)?;
    }
    Ok(())
}

fn remove_recursive(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

// ---------------------------------------------------------------------------
// "Open With" — scan .desktop files for matching MimeType
// ---------------------------------------------------------------------------

/// Returns Vec of (app_name, exec_command, icon_name) for apps that handle
/// the given file's MIME type.
fn find_apps_for_file(path: &Path) -> Vec<(String, String, Option<String>)> {
    let mime = guess_mime_type(path);
    if mime.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<(String, String, Option<String>)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let dirs = desktop_entry_dirs();
    for dir in &dirs {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            let Ok(content) = fs::read_to_string(&p) else { continue };
            if !content.contains(&mime) {
                continue; // quick pre-filter
            }
            if let Some(app) = parse_desktop_entry_for_mime(&content, &mime) {
                if seen.insert(app.0.clone()) {
                    results.push(app);
                }
            }
        }
    }

    results
}

fn desktop_entry_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    // User-local first
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }
    // XDG_DATA_DIRS
    if let Ok(xdg) = std::env::var("XDG_DATA_DIRS") {
        for d in xdg.split(':') {
            dirs.push(PathBuf::from(d).join("applications"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/local/share/applications"));
        dirs.push(PathBuf::from("/usr/share/applications"));
    }
    // Flatpak exports
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".local/share/flatpak/exports/share/applications"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share/applications"));
    dirs
}

/// Parse a .desktop file's content and return (Name, Exec, Icon) if it
/// supports the given MIME type.
fn parse_desktop_entry_for_mime(
    content: &str,
    mime: &str,
) -> Option<(String, String, Option<String>)> {
    let mut in_desktop = false;
    let mut name = String::new();
    let mut exec = String::new();
    let mut icon = None;
    let mut mime_types = String::new();
    let mut no_display = false;

    for line in content.lines() {
        let line = line.trim();
        if line == "[Desktop Entry]" {
            in_desktop = true;
            continue;
        }
        if line.starts_with('[') {
            if in_desktop {
                break; // entered a different section
            }
            continue;
        }
        if !in_desktop {
            continue;
        }
        if let Some(v) = line.strip_prefix("Name=") {
            if name.is_empty() {
                name = v.to_string();
            }
        } else if let Some(v) = line.strip_prefix("Exec=") {
            exec = v.to_string();
        } else if let Some(v) = line.strip_prefix("Icon=") {
            icon = Some(v.to_string());
        } else if let Some(v) = line.strip_prefix("MimeType=") {
            mime_types = v.to_string();
        } else if line.starts_with("NoDisplay=true") {
            no_display = true;
        }
    }

    if no_display || name.is_empty() || exec.is_empty() {
        return None;
    }

    // Check if MIME type matches
    let matches = mime_types
        .split(';')
        .any(|m| m.trim().eq_ignore_ascii_case(mime));
    if !matches {
        return None;
    }

    Some((name, exec, icon))
}

/// Launch a .desktop Exec= command with the file path substituted.
fn launch_desktop_exec(exec: &str, file_path: &Path) {
    let file_str = file_path.to_string_lossy();
    // Replace %f, %F, %u, %U with the file path; strip other % codes
    let cmd = exec
        .replace("%f", &file_str)
        .replace("%F", &file_str)
        .replace("%u", &file_str)
        .replace("%U", &file_str)
        .replace("%i", "")
        .replace("%c", "")
        .replace("%k", "");
    // If no substitution happened, append the file path
    let cmd = if cmd == exec {
        format!("{} {}", cmd.trim(), shell_quote(&file_str))
    } else {
        cmd
    };

    let _ = std::process::Command::new("sh")
        .args(["-c", &cmd])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Guess the MIME type of a file based on its extension.
fn guess_mime_type(path: &Path) -> String {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return String::new(),
    };
    // Common mappings — covers most user files
    let mime = match ext.as_str() {
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "wmv" => "video/x-ms-wmv",
        "flv" => "video/x-flv",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "bz2" => "application/x-bzip2",
        "xz" => "application/x-xz",
        "7z" => "application/x-7z-compressed",
        "rar" => "application/vnd.rar",
        "zst" => "application/zstd",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "iso" => "application/x-iso9660-image",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "c" | "h" => "text/x-csrc",
        "cpp" | "cc" | "cxx" => "text/x-c++src",
        "java" => "text/x-java",
        "go" => "text/x-go",
        "sh" | "bash" | "zsh" => "application/x-shellscript",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/x-yaml",
        "sql" => "application/sql",
        "appimage" => "application/x-executable",
        _ => "",
    };
    mime.to_string()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
