use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, DropDown, Entry, Label,
    Orientation, Paned, Picture, Popover, ScrolledWindow, StringList, Switch, TextView,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

const THUMB_QUEUE_CAPACITY: usize = 512;
const THUMB_CACHE_MAX_ITEMS: usize = 256;
const THUMB_CACHE_MAX_BYTES: usize = 192 * 1024 * 1024;
const VIDEO_THUMB_DISK_CACHE_MAX_BYTES: u64 = 750 * 1024 * 1024;
const VIDEO_THUMB_DISK_PRUNE_INTERVAL: Duration = Duration::from_secs(90);

#[derive(Clone, Copy, PartialEq, Eq)]
enum RenderMode {
    Raw,
    Text,
    Icons,
    Nerd,
}

impl RenderMode {
    fn from_selected(idx: u32) -> Self {
        match idx {
            1 => Self::Text,
            2 => Self::Icons,
            3 => Self::Nerd,
            _ => Self::Raw,
        }
    }

    fn selected_index(self) -> u32 {
        match self {
            Self::Raw => 0,
            Self::Text => 1,
            Self::Icons => 2,
            Self::Nerd => 3,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Text => "text",
            Self::Icons => "icons",
            Self::Nerd => "nerd",
        }
    }

    fn from_str(s: &str) -> Self {
        match s.trim() {
            "text" => Self::Text,
            "icons" => Self::Icons,
            "nerd" => Self::Nerd,
            _ => Self::Raw,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone)]
struct LsEntry {
    name: String,
    path: PathBuf,
    kind: EntryKind,
    perms: String,
    size: String,
    size_bytes: u64,
    modified: String,
    modified_epoch: u64,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct ThumbCacheKey {
    path: PathBuf,
    size: u32,
    file_size: u64,
    modified_epoch: u64,
}

impl ThumbCacheKey {
    fn from_entry(entry: &LsEntry, size: u32) -> Self {
        Self {
            path: entry.path.clone(),
            size,
            file_size: entry.size_bytes,
            modified_epoch: entry.modified_epoch,
        }
    }
}

#[derive(Clone)]
struct ThumbWidgetRefs {
    picture: gtk4::glib::WeakRef<Picture>,
    stack: gtk4::glib::WeakRef<gtk4::Stack>,
}

struct CachedThumb {
    pixbuf: gtk4::gdk_pixbuf::Pixbuf,
    bytes: usize,
}

#[derive(Clone)]
struct ThumbJob {
    key: ThumbCacheKey,
    generation: u64,
}

#[derive(Clone)]
struct ThumbResult {
    key: ThumbCacheKey,
    generation: u64,
    thumb_path: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortField {
    Name,
    Type,
    Size,
    Modified,
}

impl SortField {
    fn from_selected(idx: u32) -> Self {
        match idx {
            1 => Self::Type,
            2 => Self::Size,
            3 => Self::Modified,
            _ => Self::Name,
        }
    }

    fn selected_index(self) -> u32 {
        match self {
            Self::Name => 0,
            Self::Type => 1,
            Self::Size => 2,
            Self::Modified => 3,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Type => "type",
            Self::Size => "size",
            Self::Modified => "modified",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    fn from_selected(idx: u32) -> Self {
        if idx == 1 {
            Self::Desc
        } else {
            Self::Asc
        }
    }

    fn selected_index(self) -> u32 {
        match self {
            Self::Asc => 0,
            Self::Desc => 1,
        }
    }

    fn label(self, field: SortField) -> &'static str {
        match (field, self) {
            (SortField::Modified, SortOrder::Asc) => "oldest first",
            (SortField::Modified, SortOrder::Desc) => "newest first",
            (SortField::Size, SortOrder::Asc) => "smallest first",
            (SortField::Size, SortOrder::Desc) => "largest first",
            (_, SortOrder::Asc) => "ascending",
            (_, SortOrder::Desc) => "descending",
        }
    }
}

struct UiState {
    cwd: PathBuf,
    mode: RenderMode,
    show_hidden: bool,
    search_query: String,
    output_box: GtkBox,
    cwd_label: Label,
    breadcrumb_box: GtkBox,
    preview_label: Label,
    preview_text: TextView,
    preview_image: Picture,
    preview_stack: gtk4::Stack,
    paned: Paned,
    open_system_btn: Button,
    selected_path: Option<PathBuf>,
    refresh_ls_widget: Option<gtk4::Widget>,
    output_scroll: ScrolledWindow,
    cmd_blocks: VecDeque<GtkBox>,
    nav_history: Vec<PathBuf>,
    nav_pos: usize,
    back_btn: Button,
    forward_btn: Button,
    status_label: Label,
    icon_size: u32,
    places_box: GtkBox,
    custom_places: Vec<PathBuf>,
    cmd_history: Vec<String>,
    history_pos: Option<usize>,
    history_draft: String,
    sort_field: SortField,
    sort_order: SortOrder,
    folders_first: bool,
    batch_select_mode: bool,
    selected_paths: HashSet<PathBuf>,
    thumb_job_tx: async_channel::Sender<ThumbJob>,
    thumb_generation: u64,
    thumb_widgets: HashMap<ThumbCacheKey, Vec<ThumbWidgetRefs>>,
    thumb_pending: HashSet<ThumbCacheKey>,
    thumb_cache: HashMap<ThumbCacheKey, CachedThumb>,
    thumb_lru: VecDeque<ThumbCacheKey>,
    thumb_cache_bytes: usize,
    last_thumb_disk_prune: Instant,
}

fn main() {
    env_logger::init();
    let app = Application::builder()
        .application_id("org.rdm.noterm")
        .build();
    app.connect_activate(build_ui);
    app.run();
}

fn build_ui(app: &Application) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let initial_mode = load_saved_mode();
    let initial_icon_size = load_saved_icon_size();
    let (thumb_result_tx, thumb_result_rx) = async_channel::unbounded::<ThumbResult>();
    let (thumb_job_tx, thumb_job_rx) = async_channel::bounded::<ThumbJob>(THUMB_QUEUE_CAPACITY);
    start_thumbnail_workers(
        thumb_job_rx,
        thumb_result_tx,
        thumbnail_worker_count(),
        VIDEO_THUMB_DISK_CACHE_MAX_BYTES,
    );

    let window = ApplicationWindow::builder()
        .application(app)
        .title("RDM NoTerm")
        .default_width(1100)
        .default_height(700)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);

    let top = GtkBox::new(Orientation::Horizontal, 8);
    let cwd_title = Label::new(Some("CWD:"));
    cwd_title.set_xalign(0.0);
    let cwd_label = Label::new(Some(&cwd.display().to_string()));
    cwd_label.set_hexpand(true);
    cwd_label.set_xalign(0.0);
    let mode_label = Label::new(Some("Mode"));
    let mode_dd = DropDown::new(
        Some(StringList::new(&["raw", "text", "icons", "nerd"])),
        gtk4::Expression::NONE,
    );
    mode_dd.set_selected(initial_mode.selected_index());
    let size_label = Label::new(Some("Size"));
    let size_dd = DropDown::new(
        Some(StringList::new(&["32", "64", "96", "128"])),
        gtk4::Expression::NONE,
    );
    size_dd.set_selected(match initial_icon_size {
        32 => 0,
        96 => 2,
        128 => 3,
        _ => 1,
    });
    top.append(&cwd_title);
    top.append(&cwd_label);
    top.append(&mode_label);
    top.append(&mode_dd);
    top.append(&size_label);
    top.append(&size_dd);
    root.append(&top);

    let nav_row = GtkBox::new(Orientation::Horizontal, 8);
    let back_btn = Button::with_label("◀");
    back_btn.set_tooltip_text(Some("Back"));
    back_btn.set_sensitive(false);
    let forward_btn = Button::with_label("▶");
    forward_btn.set_tooltip_text(Some("Forward"));
    forward_btn.set_sensitive(false);
    let breadcrumb_box = GtkBox::new(Orientation::Horizontal, 2);
    breadcrumb_box.set_hexpand(true);
    let search_entry = Entry::new();
    search_entry.set_placeholder_text(Some("Search in enhanced ls"));
    search_entry.set_width_chars(24);
    let folders_first_label = Label::new(Some("Folders First"));
    let folders_first_switch = Switch::new();
    folders_first_switch.set_active(true);
    let hidden_label = Label::new(Some("Hidden"));
    let hidden_switch = Switch::new();
    let sort_btn = Button::with_label("Sort");
    let batch_mode_btn = Button::with_label("Batch: Off");
    let batch_actions_btn = Button::with_label("Selection");
    nav_row.append(&back_btn);
    nav_row.append(&forward_btn);
    nav_row.append(&breadcrumb_box);
    nav_row.append(&search_entry);
    nav_row.append(&folders_first_label);
    nav_row.append(&folders_first_switch);
    nav_row.append(&hidden_label);
    nav_row.append(&hidden_switch);
    nav_row.append(&sort_btn);
    nav_row.append(&batch_mode_btn);
    nav_row.append(&batch_actions_btn);
    root.append(&nav_row);

    let cmd_row = GtkBox::new(Orientation::Horizontal, 8);
    let cmd_entry = Entry::new();
    cmd_entry.set_hexpand(true);
    cmd_entry.set_placeholder_text(Some("Type command (ls, cd, pwd, cat, ... )"));
    let run_btn = Button::with_label("Run");
    cmd_row.append(&cmd_entry);
    cmd_row.append(&run_btn);

    let paned = Paned::new(Orientation::Horizontal);
    paned.set_wide_handle(true);
    paned.set_resize_start_child(true);
    paned.set_resize_end_child(true);
    paned.set_shrink_start_child(true);
    paned.set_shrink_end_child(true);

    let output_scroll = ScrolledWindow::new();
    output_scroll.set_vexpand(true);
    output_scroll.set_hexpand(true);
    output_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
    let output_box = GtkBox::new(Orientation::Vertical, 8);
    output_scroll.set_child(Some(&output_box));
    paned.set_start_child(Some(&output_scroll));

    let preview_panel = GtkBox::new(Orientation::Vertical, 8);
    preview_panel.set_hexpand(true);
    preview_panel.set_vexpand(true);
    preview_panel.set_margin_start(8);
    let preview_header = GtkBox::new(Orientation::Horizontal, 8);
    let close_preview_btn = Button::with_label("X");
    close_preview_btn.set_tooltip_text(Some("Close preview"));
    let preview_label = Label::new(Some("No selection"));
    preview_label.set_hexpand(true);
    preview_label.set_xalign(0.0);
    preview_label.set_single_line_mode(true);
    preview_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
    let open_system_btn = Button::with_label("Open With System");
    open_system_btn.set_sensitive(false);
    preview_header.append(&close_preview_btn);
    preview_header.append(&preview_label);
    preview_header.append(&open_system_btn);
    preview_panel.append(&preview_header);

    let preview_stack = gtk4::Stack::new();
    preview_stack.set_vexpand(true);
    preview_stack.set_hexpand(true);
    let placeholder = Label::new(Some("Select a file to preview."));
    let preview_text = TextView::new();
    preview_text.set_editable(false);
    preview_text.set_monospace(true);
    let preview_text_scroll = ScrolledWindow::new();
    preview_text_scroll.set_vexpand(true);
    preview_text_scroll.set_hexpand(true);
    preview_text_scroll.set_child(Some(&preview_text));
    let preview_image = Picture::new();
    preview_image.set_hexpand(true);
    preview_image.set_vexpand(true);
    preview_image.set_halign(gtk4::Align::Fill);
    preview_image.set_valign(gtk4::Align::Fill);
    preview_image.set_content_fit(gtk4::ContentFit::Contain);
    preview_stack.add_titled(&placeholder, Some("empty"), "Empty");
    preview_stack.add_titled(&preview_text_scroll, Some("text"), "Text");
    preview_stack.add_titled(&preview_image, Some("image"), "Image");
    preview_stack.set_visible_child_name("empty");
    preview_panel.append(&preview_stack);
    paned.set_end_child(Some(&preview_panel));
    paned.set_position(1040);

    let places_box = GtkBox::new(Orientation::Vertical, 6);
    places_box.set_width_request(140);
    let main_row = GtkBox::new(Orientation::Horizontal, 8);
    main_row.append(&places_box);
    main_row.append(&paned);

    let status_label = Label::new(Some(""));
    status_label.set_xalign(0.0);
    status_label.add_css_class("noterm-status");

    root.append(&main_row);
    root.append(&cmd_row);
    root.append(&status_label);
    window.set_child(Some(&root));

    let initial_cwd = cwd.clone();
    let state = Rc::new(RefCell::new(UiState {
        cwd,
        mode: initial_mode,
        show_hidden: false,
        search_query: String::new(),
        output_box: output_box.clone(),
        cwd_label: cwd_label.clone(),
        breadcrumb_box: breadcrumb_box.clone(),
        preview_label: preview_label.clone(),
        preview_text: preview_text.clone(),
        preview_image: preview_image.clone(),
        preview_stack: preview_stack.clone(),
        paned: paned.clone(),
        open_system_btn: open_system_btn.clone(),
        selected_path: None,
        refresh_ls_widget: None,
        output_scroll: output_scroll.clone(),
        cmd_blocks: VecDeque::new(),
        nav_history: vec![initial_cwd],
        nav_pos: 0,
        back_btn: back_btn.clone(),
        forward_btn: forward_btn.clone(),
        status_label: status_label.clone(),
        icon_size: initial_icon_size,
        places_box: places_box.clone(),
        custom_places: load_custom_places(),
        cmd_history: Vec::new(),
        history_pos: None,
        history_draft: String::new(),
        sort_field: SortField::Name,
        sort_order: SortOrder::Asc,
        folders_first: true,
        batch_select_mode: false,
        selected_paths: HashSet::new(),
        thumb_job_tx: thumb_job_tx.clone(),
        thumb_generation: 1,
        thumb_widgets: HashMap::new(),
        thumb_pending: HashSet::new(),
        thumb_cache: HashMap::new(),
        thumb_lru: VecDeque::new(),
        thumb_cache_bytes: 0,
        last_thumb_disk_prune: Instant::now(),
    }));

    {
        let state_thumb = state.clone();
        gtk4::glib::spawn_future_local(async move {
            while let Ok(result) = thumb_result_rx.recv().await {
                handle_thumbnail_result(&state_thumb, result);
            }
        });
    }

    {
        let state_mode = state.clone();
        mode_dd.connect_selected_notify(move |dd| {
            let mode = RenderMode::from_selected(dd.selected());
            state_mode.borrow_mut().mode = mode;
            save_mode(mode);
            refresh_ls_view(&state_mode);
        });
    }

    {
        let state_size = state.clone();
        size_dd.connect_selected_notify(move |dd| {
            let size = match dd.selected() {
                0 => 32u32,
                2 => 96,
                3 => 128,
                _ => 64,
            };
            state_size.borrow_mut().icon_size = size;
            save_icon_size(size);
            refresh_ls_view(&state_size);
        });
    }

    {
        let state_search = state.clone();
        search_entry.connect_changed(move |e| {
            state_search.borrow_mut().search_query = e.text().to_string();
            refresh_ls_view(&state_search);
        });
    }

    {
        let state_folders_first = state.clone();
        folders_first_switch.connect_active_notify(move |sw| {
            state_folders_first.borrow_mut().folders_first = sw.is_active();
            refresh_ls_view(&state_folders_first);
            update_status_bar(&state_folders_first);
        });
    }

    {
        let state_hidden = state.clone();
        hidden_switch.connect_active_notify(move |sw| {
            state_hidden.borrow_mut().show_hidden = sw.is_active();
            refresh_ls_view(&state_hidden);
            update_status_bar(&state_hidden);
        });
    }

    {
        let sort_pop = Popover::new();
        sort_pop.set_parent(&sort_btn);
        sort_pop.set_has_arrow(true);

        let vbox = GtkBox::new(Orientation::Vertical, 8);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        vbox.set_margin_start(8);
        vbox.set_margin_end(8);

        let field_lbl = Label::new(Some("Sort by"));
        field_lbl.set_xalign(0.0);
        let field_dd = DropDown::new(
            Some(StringList::new(&["Name", "Type", "Size", "Modified"])),
            gtk4::Expression::NONE,
        );
        field_dd.set_selected(state.borrow().sort_field.selected_index());

        let order_lbl = Label::new(Some("Order"));
        order_lbl.set_xalign(0.0);
        let order_dd = DropDown::new(
            Some(StringList::new(&[
                "Ascending (A-Z / oldest / smallest)",
                "Descending (Z-A / newest / largest)",
            ])),
            gtk4::Expression::NONE,
        );
        order_dd.set_selected(state.borrow().sort_order.selected_index());

        vbox.append(&field_lbl);
        vbox.append(&field_dd);
        vbox.append(&order_lbl);
        vbox.append(&order_dd);
        sort_pop.set_child(Some(&vbox));

        {
            let pop = sort_pop.clone();
            sort_btn.connect_clicked(move |_| {
                pop.popup();
            });
        }
        {
            let state_sort = state.clone();
            field_dd.connect_selected_notify(move |dd| {
                state_sort.borrow_mut().sort_field = SortField::from_selected(dd.selected());
                refresh_ls_view(&state_sort);
                update_status_bar(&state_sort);
            });
        }
        {
            let state_sort = state.clone();
            order_dd.connect_selected_notify(move |dd| {
                state_sort.borrow_mut().sort_order = SortOrder::from_selected(dd.selected());
                refresh_ls_view(&state_sort);
                update_status_bar(&state_sort);
            });
        }
    }

    {
        let state_batch = state.clone();
        let btn_ref = batch_mode_btn.clone();
        batch_mode_btn.connect_clicked(move |_| {
            let now_on = {
                let mut s = state_batch.borrow_mut();
                s.batch_select_mode = !s.batch_select_mode;
                if !s.batch_select_mode {
                    s.selected_paths.clear();
                }
                s.batch_select_mode
            };
            btn_ref.set_label(if now_on { "Batch: On" } else { "Batch: Off" });
            refresh_ls_view(&state_batch);
            update_status_bar(&state_batch);
        });
    }

    {
        let batch_pop = Popover::new();
        batch_pop.set_parent(&batch_actions_btn);
        batch_pop.set_has_arrow(true);

        let vbox = GtkBox::new(Orientation::Vertical, 4);
        vbox.set_margin_top(6);
        vbox.set_margin_bottom(6);
        vbox.set_margin_start(6);
        vbox.set_margin_end(6);

        let select_all_btn = Button::with_label("Select All Visible");
        select_all_btn.add_css_class("flat");
        let clear_btn = Button::with_label("Clear Selection");
        clear_btn.add_css_class("flat");
        let copy_btn = Button::with_label("Copy Selected Paths");
        copy_btn.add_css_class("flat");
        let open_btn = Button::with_label("Open Selected");
        open_btn.add_css_class("flat");
        let trash_btn = Button::with_label("Move Selected to Trash");
        trash_btn.add_css_class("flat");

        vbox.append(&select_all_btn);
        vbox.append(&clear_btn);
        vbox.append(&copy_btn);
        vbox.append(&open_btn);
        vbox.append(&trash_btn);
        batch_pop.set_child(Some(&vbox));

        {
            let pop = batch_pop.clone();
            batch_actions_btn.connect_clicked(move |_| {
                pop.popup();
            });
        }
        {
            let state_sel = state.clone();
            let pop = batch_pop.clone();
            select_all_btn.connect_clicked(move |_| {
                pop.popdown();
                select_all_visible(&state_sel);
            });
        }
        {
            let state_sel = state.clone();
            let pop = batch_pop.clone();
            clear_btn.connect_clicked(move |_| {
                pop.popdown();
                state_sel.borrow_mut().selected_paths.clear();
                refresh_ls_view(&state_sel);
                update_status_bar(&state_sel);
            });
        }
        {
            let state_copy = state.clone();
            let pop = batch_pop.clone();
            copy_btn.connect_clicked(move |_| {
                pop.popdown();
                let selected = selected_paths_vec(&state_copy);
                if selected.is_empty() {
                    state_copy
                        .borrow()
                        .status_label
                        .set_text("No selected items to copy");
                    return;
                }
                let text = selected
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join("\n");
                if let Some(display) = gtk4::gdk::Display::default() {
                    display.clipboard().set_text(&text);
                }
                state_copy
                    .borrow()
                    .status_label
                    .set_text(&format!("Copied {} path(s) to clipboard", selected.len()));
            });
        }
        {
            let state_open_sel = state.clone();
            let pop = batch_pop.clone();
            open_btn.connect_clicked(move |_| {
                pop.popdown();
                let selected = selected_paths_vec(&state_open_sel);
                if selected.is_empty() {
                    state_open_sel
                        .borrow()
                        .status_label
                        .set_text("No selected items to open");
                    return;
                }
                for p in &selected {
                    open_with_system(&state_open_sel, p);
                }
                state_open_sel.borrow().status_label.set_text(&format!(
                    "Opening {} selected item(s) with system handler",
                    selected.len()
                ));
            });
        }
        {
            let state_trash_sel = state.clone();
            let pop = batch_pop.clone();
            trash_btn.connect_clicked(move |_| {
                pop.popdown();
                let selected = selected_paths_vec(&state_trash_sel);
                if selected.is_empty() {
                    state_trash_sel
                        .borrow()
                        .status_label
                        .set_text("No selected items to trash");
                    return;
                }
                let mut moved = 0usize;
                for p in &selected {
                    let ok = Command::new("gio")
                        .args(["trash", &p.display().to_string()])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);
                    if ok {
                        moved += 1;
                    }
                }
                {
                    let mut s = state_trash_sel.borrow_mut();
                    s.selected_paths.clear();
                }
                refresh_ls_view(&state_trash_sel);
                update_status_bar(&state_trash_sel);
                state_trash_sel.borrow().status_label.set_text(&format!(
                    "Moved {} of {} selected item(s) to trash",
                    moved,
                    selected.len()
                ));
            });
        }
    }

    {
        let state_run = state.clone();
        let cmd_entry_run = cmd_entry.clone();
        run_btn.connect_clicked(move |_| {
            let cmd = cmd_entry_run.text().to_string();
            if !cmd.trim().is_empty() {
                run_command(&state_run, cmd.trim());
                cmd_entry_run.set_text("");
            }
        });
    }

    {
        let state_enter = state.clone();
        cmd_entry.connect_activate(move |entry| {
            let cmd = entry.text().to_string();
            if !cmd.trim().is_empty() {
                run_command(&state_enter, cmd.trim());
                entry.set_text("");
            }
        });
    }

    {
        let state_key = state.clone();
        let cmd_entry_key = cmd_entry.clone();
        let key_ctrl = gtk4::EventControllerKey::new();
        key_ctrl.connect_key_pressed(move |_, key, _, _| match key {
            gtk4::gdk::Key::Up => {
                let mut s = state_key.borrow_mut();
                let len = s.cmd_history.len();
                if len == 0 {
                    return gtk4::glib::Propagation::Proceed;
                }
                if s.history_pos.is_none() {
                    s.history_draft = cmd_entry_key.text().to_string();
                    s.history_pos = Some(len - 1);
                } else if let Some(pos) = s.history_pos {
                    if pos > 0 {
                        s.history_pos = Some(pos - 1);
                    }
                }
                if let Some(pos) = s.history_pos {
                    let text = s.cmd_history[pos].clone();
                    drop(s);
                    cmd_entry_key.set_text(&text);
                    cmd_entry_key.set_position(-1);
                }
                gtk4::glib::Propagation::Stop
            }
            gtk4::gdk::Key::Down => {
                let mut s = state_key.borrow_mut();
                if let Some(pos) = s.history_pos {
                    if pos + 1 < s.cmd_history.len() {
                        s.history_pos = Some(pos + 1);
                        let text = s.cmd_history[pos + 1].clone();
                        drop(s);
                        cmd_entry_key.set_text(&text);
                        cmd_entry_key.set_position(-1);
                    } else {
                        s.history_pos = None;
                        let draft = s.history_draft.clone();
                        drop(s);
                        cmd_entry_key.set_text(&draft);
                        cmd_entry_key.set_position(-1);
                    }
                }
                gtk4::glib::Propagation::Stop
            }
            _ => gtk4::glib::Propagation::Proceed,
        });
        cmd_entry.add_controller(key_ctrl);
    }

    {
        let state_open = state.clone();
        open_system_btn.connect_clicked(move |_| {
            if let Some(path) = state_open.borrow().selected_path.clone() {
                open_with_system(&state_open, &path);
            }
        });
    }

    {
        let state_close = state.clone();
        close_preview_btn.connect_clicked(move |_| {
            hide_preview(&state_close);
        });
    }

    {
        let state_back = state.clone();
        back_btn.connect_clicked(move |_| {
            navigate_back(&state_back);
        });
    }

    {
        let state_fwd = state.clone();
        forward_btn.connect_clicked(move |_| {
            navigate_forward(&state_fwd);
        });
    }

    refresh_places_sidebar(&state);
    rebuild_breadcrumb(&state);
    refresh_ls_view(&state);
    update_status_bar(&state);
    load_css();
    window.present();
}

fn run_command(state: &Rc<RefCell<UiState>>, cmd: &str) {
    {
        let mut s = state.borrow_mut();
        // Don't push duplicate of the last entry
        if s.cmd_history.last().map(|s| s.as_str()) != Some(cmd) {
            s.cmd_history.push(cmd.to_string());
        }
        s.history_pos = None;
        s.history_draft = String::new();
    }

    let block = begin_cmd_block(state, &format!("$ {}", cmd));
    // Jump to the newest command immediately, then again after output arrives.
    scroll_output_to_bottom(state);

    if cmd == "pwd" {
        let cwd = state.borrow().cwd.display().to_string();
        add_text_to_block(&block, &format!("{}\n", cwd));
        scroll_output_to_bottom(state);
        return;
    }

    if let Some(rest) = cmd.strip_prefix("cd ") {
        let mut s = state.borrow_mut();
        let target = resolve_cd_target(&s.cwd, rest.trim());
        if target.is_dir() {
            s.cwd = target;
            s.cwd_label.set_text(&s.cwd.display().to_string());
            drop(s);
            rebuild_breadcrumb(state);
            add_text_to_block(&block, "");
        } else {
            drop(s);
            add_text_to_block(&block, "cd: target is not a directory\n");
        }
        scroll_output_to_bottom(state);
        return;
    }

    let cwd = state.borrow().cwd.clone();
    let mode = state.borrow().mode;
    let (show_hidden, query, sort_field, sort_order, folders_first) = {
        let s = state.borrow();
        (
            s.show_hidden,
            s.search_query.clone(),
            s.sort_field,
            s.sort_order,
            s.folders_first,
        )
    };
    let cmd_owned = cmd.to_string();
    let state_result = state.clone();

    gtk4::glib::spawn_future_local(async move {
        let (tx, rx) = async_channel::bounded::<Result<std::process::Output, String>>(1);
        let cmd_thread = cmd_owned.clone();
        let cwd_thread = cwd.clone();
        std::thread::spawn(move || {
            let res = Command::new("sh")
                .arg("-lc")
                .arg(&cmd_thread)
                .current_dir(&cwd_thread)
                .output()
                .map_err(|e| e.to_string());
            let _ = tx.send_blocking(res);
        });

        let output = match rx.recv().await {
            Ok(v) => v,
            Err(_) => {
                add_text_to_block(&block, "Failed to receive command output\n");
                scroll_output_to_bottom(&state_result);
                return;
            }
        };

        match output {
            Ok(out) => {
                let mut combined = String::new();
                combined.push_str(&String::from_utf8_lossy(&out.stdout));
                combined.push_str(&String::from_utf8_lossy(&out.stderr));

                if mode != RenderMode::Raw && cmd_owned.starts_with("ls") && out.status.success() {
                    let entries = build_ls_entries_from_fs(
                        &cwd,
                        &cmd_owned,
                        show_hidden,
                        &query,
                        sort_field,
                        sort_order,
                        folders_first,
                    );
                    if entries.is_empty() {
                        let parsed = parse_ls_entries(&combined, &cwd);
                        if parsed.is_empty() {
                            add_text_to_block(&block, &combined);
                        } else {
                            let flow = build_ls_flow(&state_result, parsed);
                            block.append(&flow);
                        }
                    } else {
                        let flow = build_ls_flow(&state_result, entries);
                        block.append(&flow);
                    }
                } else {
                    add_text_to_block(&block, &combined);
                }
            }
            Err(e) => {
                add_text_to_block(&block, &format!("Failed to execute command: {}\n", e));
            }
        }
        scroll_output_to_bottom(&state_result);
    });
}

// Creates a command block container, prunes oldest if over 10, appends to output_box.
fn begin_cmd_block(state: &Rc<RefCell<UiState>>, header: &str) -> GtkBox {
    let block = GtkBox::new(Orientation::Vertical, 4);
    let label = Label::new(Some(header));
    label.set_xalign(0.0);
    label.add_css_class("noterm-command");
    block.append(&label);

    let mut s = state.borrow_mut();
    if s.cmd_blocks.len() >= 10 {
        if let Some(old) = s.cmd_blocks.pop_front() {
            s.output_box.remove(&old);
        }
    }
    s.output_box.append(&block);
    s.cmd_blocks.push_back(block.clone());
    block
}

fn add_text_to_block(block: &GtkBox, text: &str) {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("noterm-output");
    view.buffer().set_text(text);
    block.append(&view);
}

// Defers scroll-to-bottom until after GTK has finished the current layout pass.
fn scroll_output_to_bottom(state: &Rc<RefCell<UiState>>) {
    let scroll = state.borrow().output_scroll.clone();
    gtk4::glib::idle_add_local_once(move || {
        let adj = scroll.vadjustment();
        adj.set_value(adj.upper() - adj.page_size());
    });
}

fn thumbnail_worker_count() -> usize {
    let cpus = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    cpus.clamp(2, 4)
}

fn start_thumbnail_workers(
    job_rx: async_channel::Receiver<ThumbJob>,
    result_tx: async_channel::Sender<ThumbResult>,
    workers: usize,
    disk_cache_max_bytes: u64,
) {
    for _ in 0..workers {
        let rx = job_rx.clone();
        let tx = result_tx.clone();
        std::thread::spawn(move || {
            while let Ok(job) = rx.recv_blocking() {
                if let Ok(path) = video_thumbnail_path(&job.key.path, job.key.size) {
                    let _ = tx.send_blocking(ThumbResult {
                        key: job.key,
                        generation: job.generation,
                        thumb_path: path,
                    });
                }
            }
            prune_video_thumbnail_disk_cache(disk_cache_max_bytes);
        });
    }
}

fn register_thumb_widget(
    state: &Rc<RefCell<UiState>>,
    key: ThumbCacheKey,
    picture: &Picture,
    stack: &gtk4::Stack,
) {
    let pic_ref = gtk4::glib::WeakRef::<Picture>::new();
    pic_ref.set(Some(picture));
    let stack_ref = gtk4::glib::WeakRef::<gtk4::Stack>::new();
    stack_ref.set(Some(stack));
    let mut s = state.borrow_mut();
    s.thumb_widgets
        .entry(key)
        .or_default()
        .push(ThumbWidgetRefs {
            picture: pic_ref,
            stack: stack_ref,
        });
}

fn queue_video_thumbnail_job(state: &Rc<RefCell<UiState>>, key: ThumbCacheKey) {
    let (tx, generation, should_send) = {
        let mut s = state.borrow_mut();
        if s.thumb_pending.contains(&key) {
            (s.thumb_job_tx.clone(), s.thumb_generation, false)
        } else {
            s.thumb_pending.insert(key.clone());
            (s.thumb_job_tx.clone(), s.thumb_generation, true)
        }
    };
    if !should_send {
        return;
    }
    if tx
        .try_send(ThumbJob {
            key: key.clone(),
            generation,
        })
        .is_err()
    {
        state.borrow_mut().thumb_pending.remove(&key);
    }
}

fn thumbnail_cache_get(
    state: &Rc<RefCell<UiState>>,
    key: &ThumbCacheKey,
) -> Option<gtk4::gdk_pixbuf::Pixbuf> {
    let mut s = state.borrow_mut();
    if let Some(cached) = s.thumb_cache.get(key).map(|c| c.pixbuf.clone()) {
        touch_thumbnail_lru(&mut s.thumb_lru, key);
        return Some(cached);
    }
    None
}

fn thumbnail_cache_insert(s: &mut UiState, key: ThumbCacheKey, pixbuf: gtk4::gdk_pixbuf::Pixbuf) {
    let bytes = estimate_pixbuf_bytes(&pixbuf);
    if let Some(old) = s.thumb_cache.remove(&key) {
        s.thumb_cache_bytes = s.thumb_cache_bytes.saturating_sub(old.bytes);
    }
    s.thumb_cache
        .insert(key.clone(), CachedThumb { pixbuf, bytes });
    s.thumb_cache_bytes = s.thumb_cache_bytes.saturating_add(bytes);
    touch_thumbnail_lru(&mut s.thumb_lru, &key);
    while s.thumb_cache.len() > THUMB_CACHE_MAX_ITEMS || s.thumb_cache_bytes > THUMB_CACHE_MAX_BYTES
    {
        if let Some(old_key) = s.thumb_lru.pop_front() {
            if let Some(old) = s.thumb_cache.remove(&old_key) {
                s.thumb_cache_bytes = s.thumb_cache_bytes.saturating_sub(old.bytes);
            }
        } else {
            break;
        }
    }
}

fn touch_thumbnail_lru(lru: &mut VecDeque<ThumbCacheKey>, key: &ThumbCacheKey) {
    if let Some(pos) = lru.iter().position(|k| k == key) {
        lru.remove(pos);
    }
    lru.push_back(key.clone());
}

fn estimate_pixbuf_bytes(pb: &gtk4::gdk_pixbuf::Pixbuf) -> usize {
    (pb.rowstride().max(0) as usize).saturating_mul(pb.height().max(0) as usize)
}

fn handle_thumbnail_result(state: &Rc<RefCell<UiState>>, result: ThumbResult) {
    let (key, thumb_path, refs_opt, current_generation) = {
        let mut s = state.borrow_mut();
        s.thumb_pending.remove(&result.key);
        let refs = s.thumb_widgets.get(&result.key).cloned();
        (result.key, result.thumb_path, refs, s.thumb_generation)
    };
    if result.generation != current_generation {
        return;
    }
    let Some(refs) = refs_opt else {
        return;
    };
    let size = key.size as i32;
    let Ok(pb) = gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(&thumb_path, size, size, true) else {
        return;
    };
    {
        let mut s = state.borrow_mut();
        thumbnail_cache_insert(&mut s, key.clone(), pb.clone());
    }
    for refs in refs {
        if let (Some(picture), Some(stack)) = (refs.picture.upgrade(), refs.stack.upgrade()) {
            picture.set_pixbuf(Some(&pb));
            stack.set_visible_child_name("thumb");
        }
    }
}

fn purge_thumbnail_cache_outside_cwd(s: &mut UiState, cwd: &Path) {
    let evict_keys: Vec<ThumbCacheKey> = s
        .thumb_cache
        .keys()
        .filter(|k| !k.path.starts_with(cwd))
        .cloned()
        .collect();
    for key in evict_keys {
        if let Some(old) = s.thumb_cache.remove(&key) {
            s.thumb_cache_bytes = s.thumb_cache_bytes.saturating_sub(old.bytes);
        }
        if let Some(pos) = s.thumb_lru.iter().position(|k| *k == key) {
            s.thumb_lru.remove(pos);
        }
    }
}

fn maybe_prune_video_thumbnail_disk_cache(s: &mut UiState) {
    if s.last_thumb_disk_prune.elapsed() < VIDEO_THUMB_DISK_PRUNE_INTERVAL {
        return;
    }
    s.last_thumb_disk_prune = Instant::now();
    std::thread::spawn(|| {
        prune_video_thumbnail_disk_cache(VIDEO_THUMB_DISK_CACHE_MAX_BYTES);
    });
}

fn prune_video_thumbnail_disk_cache(max_bytes: u64) {
    let dir = thumbnail_cache_dir();
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };

    let mut files = Vec::new();
    let mut total = 0u64;
    for item in rd.flatten() {
        let path = item.path();
        let Ok(meta) = item.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let len = meta.len();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        files.push((path, len, modified));
        total = total.saturating_add(len);
    }

    if total <= max_bytes {
        return;
    }

    files.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in files {
        if total <= max_bytes {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
}

fn build_ls_flow(state: &Rc<RefCell<UiState>>, entries: Vec<LsEntry>) -> gtk4::FlowBox {
    let flow = gtk4::FlowBox::new();
    flow.add_css_class("noterm-list");
    flow.set_selection_mode(gtk4::SelectionMode::None);
    flow.set_homogeneous(false);
    flow.set_min_children_per_line(4);
    flow.set_max_children_per_line(14);
    flow.set_row_spacing(4);
    flow.set_column_spacing(4);
    flow.set_activate_on_single_click(false);
    let entries = Rc::new(entries);

    for entry in entries.iter() {
        let tile_btn = Button::new();
        tile_btn.add_css_class("noterm-tile");
        if state.borrow().selected_paths.contains(&entry.path) {
            tile_btn.add_css_class("noterm-tile-selected");
        }
        // Vertical layout: icon/thumbnail on top, name centered below.
        let h = GtkBox::new(Orientation::Vertical, 4);
        h.set_halign(gtk4::Align::Center);
        h.set_margin_top(4);
        h.set_margin_bottom(4);
        h.set_margin_start(6);
        h.set_margin_end(6);

        let (mode, icon_size) = {
            let s = state.borrow();
            (s.mode, s.icon_size)
        };
        // Icons mode + media file: image decode is immediate, video thumbnailing is async.
        if mode == RenderMode::Icons && (is_image(&entry.path) || is_video(&entry.path)) {
            let sz = icon_size as i32;
            let icon_stack = gtk4::Stack::new();
            icon_stack.set_width_request(sz);
            icon_stack.set_height_request(sz);
            icon_stack.set_halign(gtk4::Align::Center);
            icon_stack.set_vexpand(false);

            let icon_label = Label::new(None);
            let pt = icon_size * 3 / 4;
            icon_label.set_markup(&format!(
                "<span font=\"{}\">{}</span>",
                pt,
                icon_for_file_emoji(&entry.path)
            ));
            icon_label.set_halign(gtk4::Align::Center);
            icon_label.set_valign(gtk4::Align::Center);
            icon_label.add_css_class("noterm-icon");

            let pic = Picture::new();
            pic.set_width_request(sz);
            pic.set_height_request(sz);
            pic.set_halign(gtk4::Align::Center);
            pic.set_content_fit(gtk4::ContentFit::Contain);

            icon_stack.add_named(&icon_label, Some("fallback"));
            icon_stack.add_named(&pic, Some("thumb"));
            icon_stack.set_visible_child_name("fallback");

            if is_image(&entry.path) {
                if let Ok(pb) =
                    gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(&entry.path, sz, sz, true)
                {
                    pic.set_pixbuf(Some(&pb));
                    icon_stack.set_visible_child_name("thumb");
                }
            } else {
                let key = ThumbCacheKey::from_entry(entry, icon_size);
                register_thumb_widget(state, key.clone(), &pic, &icon_stack);
                if let Some(pb) = thumbnail_cache_get(state, &key) {
                    pic.set_pixbuf(Some(&pb));
                    icon_stack.set_visible_child_name("thumb");
                } else {
                    queue_video_thumbnail_job(state, key);
                }
            }

            h.append(&icon_stack);
        } else {
            let icon = icon_for_entry(mode, entry);
            if !icon.is_empty() {
                let icon_label = Label::new(None);
                let pt = icon_size * 3 / 4;
                icon_label.set_markup(&format!("<span font=\"{}\">{}</span>", pt, icon));
                icon_label.set_halign(gtk4::Align::Center);
                icon_label.add_css_class("noterm-icon");
                h.append(&icon_label);
            }
        }

        let name_label = Label::new(Some(&entry.name));
        name_label.set_halign(gtk4::Align::Center);
        name_label.set_xalign(0.5);
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name_label.set_max_width_chars(14);
        h.append(&name_label);

        tile_btn.set_child(Some(&h));
        tile_btn.set_tooltip_text(Some(&format!(
            "{}  {}  {}\n{}",
            entry.perms,
            entry.size,
            entry.modified,
            entry.path.display()
        )));

        // Primary single-click: navigate (dir) or preview (file).
        {
            let state_click = state.clone();
            let entry_click = entry.clone();
            tile_btn.connect_clicked(move |_| {
                {
                    let mut s = state_click.borrow_mut();
                    if s.batch_select_mode {
                        if s.selected_paths.contains(&entry_click.path) {
                            s.selected_paths.remove(&entry_click.path);
                        } else {
                            s.selected_paths.insert(entry_click.path.clone());
                        }
                        drop(s);
                        refresh_ls_view(&state_click);
                        update_status_bar(&state_click);
                        return;
                    }
                }
                if entry_click.kind == EntryKind::Directory {
                    navigate_to(&state_click, entry_click.path.clone());
                } else {
                    update_preview(&state_click, &entry_click.path);
                }
            });
        }
        // Double-click: open non-previewable files with the system handler.
        if !is_previewable(&entry.path) {
            let path_dbl = entry.path.clone();
            let state_dbl = state.clone();
            let click = gtk4::GestureClick::new();
            click.connect_pressed(move |_, n_press, _, _| {
                if state_dbl.borrow().batch_select_mode {
                    return;
                }
                if n_press == 2 {
                    open_with_system(&state_dbl, &path_dbl);
                }
            });
            tile_btn.add_controller(click);
        }
        // Right-click: context menu.
        {
            let state_rc = state.clone();
            let entry_rc = entry.clone();
            let tile_ref = tile_btn.clone();
            let right_click = gtk4::GestureClick::new();
            right_click.set_button(3);
            right_click.connect_pressed(move |gesture, _, _, _| {
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                show_context_menu(&state_rc, &tile_ref, &entry_rc);
            });
            tile_btn.add_controller(right_click);
        }

        flow.insert(&tile_btn, -1);
    }
    flow
}

fn refresh_ls_view(state: &Rc<RefCell<UiState>>) {
    let (cwd, mode, show_hidden, query, sort_field, sort_order, folders_first) = {
        let mut s = state.borrow_mut();
        s.thumb_generation = s.thumb_generation.wrapping_add(1);
        s.thumb_widgets.clear();
        s.thumb_pending.clear();
        let cwd_keep = s.cwd.clone();
        purge_thumbnail_cache_outside_cwd(&mut s, &cwd_keep);
        maybe_prune_video_thumbnail_disk_cache(&mut s);
        (
            s.cwd.clone(),
            s.mode,
            s.show_hidden,
            s.search_query.clone(),
            s.sort_field,
            s.sort_order,
            s.folders_first,
        )
    };

    let replacement: gtk4::Widget = if mode == RenderMode::Raw {
        let text = build_raw_ls_text(&cwd, show_hidden, &query);
        let view = TextView::new();
        view.set_editable(false);
        view.set_cursor_visible(false);
        view.set_monospace(true);
        view.add_css_class("noterm-output");
        view.buffer().set_text(&text);
        view.upcast()
    } else {
        let entries = build_ls_entries_from_fs(
            &cwd,
            "ls",
            show_hidden,
            &query,
            sort_field,
            sort_order,
            folders_first,
        );
        build_ls_flow(state, entries).upcast()
    };

    replace_refresh_ls_widget(state, replacement);
}

fn replace_refresh_ls_widget(state: &Rc<RefCell<UiState>>, widget: gtk4::Widget) {
    let mut s = state.borrow_mut();
    if let Some(prev) = s.refresh_ls_widget.take() {
        s.output_box.remove(&prev);
    }
    s.output_box.append(&widget);
    s.refresh_ls_widget = Some(widget);
}

fn build_raw_ls_text(cwd: &Path, show_hidden: bool, query: &str) -> String {
    let mut names = Vec::new();
    let q = query.trim().to_lowercase();
    if let Ok(rd) = std::fs::read_dir(cwd) {
        for item in rd.flatten() {
            let name = item.file_name().to_string_lossy().to_string();
            if !show_hidden && name.starts_with('.') {
                continue;
            }
            if !q.is_empty() && !name.to_lowercase().contains(&q) {
                continue;
            }
            names.push(name);
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    if names.is_empty() {
        String::new()
    } else {
        format!("{}\n", names.join("\n"))
    }
}

fn default_places() -> Vec<(&'static str, PathBuf)> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let docs = home.join("Documents");
    let dl = home.join("Downloads");
    let desk = home.join("Desktop");
    vec![
        ("Home", home.clone()),
        ("Desktop", desk),
        ("Documents", docs),
        ("Downloads", dl),
        ("Root", PathBuf::from("/")),
    ]
}

fn noterm_places_path() -> PathBuf {
    rdm_common::config::config_dir().join("noterm-places")
}

fn load_custom_places() -> Vec<PathBuf> {
    let path = noterm_places_path();
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Vec::new();
    };

    let mut places = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let place = PathBuf::from(trimmed);
        if place.is_dir() && !places.contains(&place) {
            places.push(place);
        }
    }
    places
}

fn save_custom_places(places: &[PathBuf]) -> Result<(), String> {
    let path = noterm_places_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = places
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&path, format!("{}\n", body)).map_err(|e| e.to_string())
}

fn refresh_places_sidebar(state: &Rc<RefCell<UiState>>) {
    let (box_widget, custom_places) = {
        let s = state.borrow();
        (s.places_box.clone(), s.custom_places.clone())
    };

    while let Some(child) = box_widget.first_child() {
        box_widget.remove(&child);
    }

    let title = Label::new(Some("Places"));
    title.set_xalign(0.0);
    box_widget.append(&title);

    let mut built_in_paths = Vec::new();
    for (name, path) in default_places() {
        if name != "Root" && !path.exists() {
            continue;
        }
        built_in_paths.push(path.clone());
        let btn = Button::with_label(name);
        btn.set_halign(gtk4::Align::Fill);
        let state_btn = state.clone();
        btn.connect_clicked(move |_| {
            if path.is_dir() {
                navigate_to(&state_btn, path.clone());
            }
        });
        box_widget.append(&btn);
    }

    for path in custom_places {
        if !path.is_dir() || built_in_paths.contains(&path) {
            continue;
        }
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| path.display().to_string());
        let btn = Button::with_label(&label);
        btn.set_halign(gtk4::Align::Fill);
        btn.set_tooltip_text(Some(&path.display().to_string()));
        let state_btn = state.clone();
        let path_click = path.clone();
        btn.connect_clicked(move |_| {
            if path_click.is_dir() {
                navigate_to(&state_btn, path_click.clone());
            }
        });
        {
            let state_remove = state.clone();
            let path_remove = path.clone();
            let btn_ref = btn.clone();
            let right_click = gtk4::GestureClick::new();
            right_click.set_button(3);
            right_click.connect_pressed(move |gesture, _, _, _| {
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                show_place_context_menu(&state_remove, &btn_ref, &path_remove);
            });
            btn.add_controller(right_click);
        }
        box_widget.append(&btn);
    }
}

fn add_to_places(state: &Rc<RefCell<UiState>>, path: &Path) {
    if !path.is_dir() {
        state
            .borrow()
            .status_label
            .set_text("Add to Places only works for directories");
        return;
    }

    let normalized = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let built_in_paths: Vec<PathBuf> = default_places().into_iter().map(|(_, p)| p).collect();

    {
        let mut s = state.borrow_mut();
        if built_in_paths.contains(&normalized) || s.custom_places.contains(&normalized) {
            s.status_label.set_text("Directory is already in Places");
            return;
        }
        s.custom_places.push(normalized.clone());
        s.custom_places
            .sort_by_key(|p| p.to_string_lossy().to_string());
        if let Err(err) = save_custom_places(&s.custom_places) {
            s.status_label
                .set_text(&format!("Failed to save Places: {}", err));
            log::warn!("Failed to save custom places: {}", err);
            return;
        }
        s.status_label
            .set_text(&format!("Added to Places: {}", normalized.display()));
    }

    refresh_places_sidebar(state);
}

fn remove_from_places(state: &Rc<RefCell<UiState>>, path: &Path) {
    let removed = {
        let mut s = state.borrow_mut();
        let before = s.custom_places.len();
        s.custom_places.retain(|p| p != path);
        let changed = s.custom_places.len() != before;
        if !changed {
            s.status_label.set_text("Directory is not in custom Places");
            return;
        }
        if let Err(err) = save_custom_places(&s.custom_places) {
            s.status_label
                .set_text(&format!("Failed to save Places: {}", err));
            log::warn!("Failed to save custom places: {}", err);
            false
        } else {
            s.status_label
                .set_text(&format!("Removed from Places: {}", path.display()));
            true
        }
    };

    if removed {
        refresh_places_sidebar(state);
    }
}

fn show_place_context_menu(state: &Rc<RefCell<UiState>>, widget: &Button, path: &Path) {
    let popover = Popover::new();
    popover.set_parent(widget);
    popover.set_has_arrow(true);

    let vbox = GtkBox::new(Orientation::Vertical, 2);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);

    let remove_btn = Button::with_label("Remove from Places");
    remove_btn.add_css_class("flat");
    {
        let state_remove = state.clone();
        let path_remove = path.to_path_buf();
        let pop = popover.clone();
        remove_btn.connect_clicked(move |_| {
            pop.popdown();
            remove_from_places(&state_remove, &path_remove);
        });
    }
    vbox.append(&remove_btn);

    popover.set_child(Some(&vbox));
    popover.popup();
}

fn rebuild_breadcrumb(state: &Rc<RefCell<UiState>>) {
    let (cwd, box_widget) = {
        let s = state.borrow();
        (s.cwd.clone(), s.breadcrumb_box.clone())
    };

    while let Some(child) = box_widget.first_child() {
        box_widget.remove(&child);
    }

    let parts: Vec<std::path::Component<'_>> = cwd.components().collect();
    if parts.is_empty() {
        return;
    }

    let mut current = PathBuf::new();
    for (idx, comp) in parts.iter().enumerate() {
        let part = comp.as_os_str().to_string_lossy().to_string();
        if idx == 0 && part == "/" {
            current.push("/");
        } else {
            current.push(&part);
        }

        let btn = Button::with_label(if part.is_empty() { "/" } else { &part });
        btn.add_css_class("noterm-breadcrumb");
        let target = current.clone();
        let state_btn = state.clone();
        btn.connect_clicked(move |_| {
            if target.is_dir() {
                navigate_to(&state_btn, target.clone());
            }
        });
        box_widget.append(&btn);
        if idx + 1 < parts.len() {
            box_widget.append(&Label::new(Some(">")));
        }
    }
}

fn resolve_cd_target(cwd: &Path, arg: &str) -> PathBuf {
    if arg == "~" {
        return dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf());
    }
    let p = Path::new(arg);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    }
}

fn parse_ls_entries(output: &str, cwd: &Path) -> Vec<LsEntry> {
    let mut entries = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() || line.starts_with("total ") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        let perms = parts[0].to_string();
        let size = parts[4].to_string();
        let size_bytes = parts[4].parse::<u64>().unwrap_or(0);
        let modified = format!("{} {} {}", parts[5], parts[6], parts[7]);
        let name = parts[8..].join(" ");
        if name == "." || name == ".." {
            continue;
        }
        let kind = match perms.chars().next() {
            Some('d') => EntryKind::Directory,
            Some('l') => EntryKind::Symlink,
            Some('-') => EntryKind::File,
            _ => EntryKind::Other,
        };
        entries.push(LsEntry {
            name: name.clone(),
            path: cwd.join(name),
            kind,
            perms,
            size,
            size_bytes,
            modified,
            modified_epoch: 0,
        });
    }
    entries
}

fn build_ls_entries_from_fs(
    cwd: &Path,
    cmd: &str,
    show_hidden: bool,
    query: &str,
    sort_field: SortField,
    sort_order: SortOrder,
    folders_first: bool,
) -> Vec<LsEntry> {
    let include_hidden = show_hidden || ls_show_hidden(cmd);
    let q = query.trim().to_lowercase();
    let target = ls_target_dir(cwd, cmd).unwrap_or_else(|| cwd.to_path_buf());
    let mut entries = Vec::new();
    let rd = match std::fs::read_dir(&target) {
        Ok(v) => v,
        Err(_) => return entries,
    };

    // Always include parent navigation first.
    let parent = cwd.parent().unwrap_or(cwd).to_path_buf();
    entries.push(LsEntry {
        name: "..".to_string(),
        path: parent,
        kind: EntryKind::Directory,
        perms: "drwxr-xr-x".to_string(),
        size: "-".to_string(),
        size_bytes: 0,
        modified: "-".to_string(),
        modified_epoch: 0,
    });

    for item in rd.flatten() {
        let name = item.file_name().to_string_lossy().to_string();
        if !include_hidden && name.starts_with('.') {
            continue;
        }
        if !q.is_empty() && !name.to_lowercase().contains(&q) {
            continue;
        }
        let path = item.path();
        let md = match item.metadata() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let file_type = match item.file_type() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let kind = if file_type.is_dir() {
            EntryKind::Directory
        } else if file_type.is_symlink() {
            EntryKind::Symlink
        } else if file_type.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        };
        entries.push(LsEntry {
            name,
            path,
            kind,
            perms: perms_from_meta(&md, kind),
            size: human_size(md.len()),
            size_bytes: md.len(),
            modified: modified_short(&md),
            modified_epoch: modified_epoch(&md),
        });
    }

    if entries.len() > 1 {
        sort_entries(&mut entries[1..], sort_field, sort_order, folders_first);
    }
    entries
}

fn sort_entries(
    entries: &mut [LsEntry],
    sort_field: SortField,
    sort_order: SortOrder,
    folders_first: bool,
) {
    entries.sort_by(|a, b| {
        if folders_first {
            let a_is_dir = a.kind == EntryKind::Directory;
            let b_is_dir = b.kind == EntryKind::Directory;
            if a_is_dir != b_is_dir {
                return b_is_dir.cmp(&a_is_dir);
            }
        }
        let by_field = match sort_field {
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Type => type_sort_key(a).cmp(&type_sort_key(b)),
            SortField::Size => a.size_bytes.cmp(&b.size_bytes),
            SortField::Modified => a.modified_epoch.cmp(&b.modified_epoch),
        };
        let mut ord = if by_field == std::cmp::Ordering::Equal {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        } else {
            by_field
        };
        if sort_order == SortOrder::Desc {
            ord = ord.reverse();
        }
        ord
    });
}

fn type_sort_key(entry: &LsEntry) -> String {
    match entry.kind {
        EntryKind::Directory => "0-dir".to_string(),
        EntryKind::Symlink => "1-link".to_string(),
        EntryKind::File => format!(
            "2-{}",
            entry
                .path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase()
        ),
        EntryKind::Other => "9-other".to_string(),
    }
}

fn ls_show_hidden(cmd: &str) -> bool {
    cmd.split_whitespace().any(|t| {
        t == "-a"
            || t == "--all"
            || t == "-A"
            || (t.starts_with('-') && t.contains('a'))
            || (t.starts_with('-') && t.contains('A'))
    })
}

fn ls_target_dir(cwd: &Path, cmd: &str) -> Option<PathBuf> {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.is_empty() || parts[0] != "ls" {
        return None;
    }
    let mut candidate: Option<&str> = None;
    for p in parts.iter().skip(1) {
        if p.starts_with('-') {
            continue;
        }
        candidate = Some(p);
    }
    let c = candidate?;
    let p = Path::new(c);
    let resolved = if p.is_absolute() {
        p.to_path_buf()
    } else {
        cwd.join(p)
    };
    if resolved.is_dir() {
        Some(resolved)
    } else {
        None
    }
}

fn perms_from_meta(md: &std::fs::Metadata, kind: EntryKind) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = md.permissions().mode() & 0o777;
        let mut s = String::new();
        s.push(match kind {
            EntryKind::Directory => 'd',
            EntryKind::Symlink => 'l',
            EntryKind::File => '-',
            EntryKind::Other => '?',
        });
        for shift in [6, 3, 0] {
            let bits = (mode >> shift) & 0o7;
            s.push(if bits & 0o4 != 0 { 'r' } else { '-' });
            s.push(if bits & 0o2 != 0 { 'w' } else { '-' });
            s.push(if bits & 0o1 != 0 { 'x' } else { '-' });
        }
        s
    }
    #[cfg(not(unix))]
    {
        String::from("----------")
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{}", bytes, UNITS[unit])
    } else {
        format!("{:.1}{}", value, UNITS[unit])
    }
}

fn modified_short(md: &std::fs::Metadata) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
    secs.to_string()
}

fn modified_epoch(md: &std::fs::Metadata) -> u64 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn icon_for_entry(mode: RenderMode, entry: &LsEntry) -> &'static str {
    match mode {
        RenderMode::Raw | RenderMode::Text => "",
        RenderMode::Icons => match entry.kind {
            EntryKind::Directory => "📁",
            EntryKind::Symlink => "🔗",
            EntryKind::File => icon_for_file_emoji(&entry.path),
            EntryKind::Other => "❓",
        },
        RenderMode::Nerd => match entry.kind {
            EntryKind::Directory => "\u{f115}",
            EntryKind::Symlink => "\u{f481}",
            EntryKind::File => nerd_for_file(&entry.path),
            EntryKind::Other => "\u{f059}",
        },
    }
}

fn icon_for_file_emoji(path: &Path) -> &'static str {
    if is_image(path) {
        "🖼️"
    } else if is_video(path) {
        "🎞️"
    } else if is_text(path) {
        "📄"
    } else {
        "📦"
    }
}

fn nerd_for_file(path: &Path) -> &'static str {
    if is_image(path) {
        "\u{f71e}"
    } else if is_video(path) {
        "\u{f03d}"
    } else if is_text(path) {
        "\u{f15c}"
    } else {
        "\u{f016}"
    }
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg"
            )
        })
        .unwrap_or(false)
}

fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "mp4"
                    | "mkv"
                    | "webm"
                    | "mov"
                    | "avi"
                    | "m4v"
                    | "mpg"
                    | "mpeg"
                    | "wmv"
                    | "flv"
                    | "3gp"
            )
        })
        .unwrap_or(false)
}

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "txt"
                    | "md"
                    | "markdown"
                    | "rs"
                    | "toml"
                    | "json"
                    | "yaml"
                    | "yml"
                    | "log"
                    | "css"
                    | "js"
                    | "ts"
                    | "sh"
            )
        })
        .unwrap_or(false)
}

fn is_previewable(path: &Path) -> bool {
    is_image(path) || is_video(path) || is_text(path)
}

fn thumbnail_cache_dir() -> PathBuf {
    rdm_common::config::config_dir().join("noterm-video-thumbnails")
}

fn thumbnail_cache_key(path: &Path, size: u32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    size.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(path) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(age) = modified.duration_since(std::time::UNIX_EPOCH) {
                age.as_secs().hash(&mut hasher);
                age.subsec_nanos().hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn video_thumbnail_path(path: &Path, size: u32) -> Result<PathBuf, String> {
    if !is_video(path) {
        return Err("not a video file".to_string());
    }
    let cache_dir = thumbnail_cache_dir();
    std::fs::create_dir_all(&cache_dir).map_err(|e| format!("cache dir: {}", e))?;
    let out = cache_dir.join(format!(
        "{:016x}-{}.png",
        thumbnail_cache_key(path, size),
        size
    ));
    if out.exists() {
        return Ok(out);
    }

    let thumb_ok = Command::new("ffmpegthumbnailer")
        .arg("-i")
        .arg(path)
        .arg("-o")
        .arg(&out)
        .arg("-s")
        .arg(size.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if thumb_ok && out.exists() {
        return Ok(out);
    }

    let ffmpeg_ok = Command::new("ffmpeg")
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-ss")
        .arg("00:00:01")
        .arg("-i")
        .arg(path)
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(format!(
            "scale={}:{}:force_original_aspect_ratio=decrease",
            size, size
        ))
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if ffmpeg_ok && out.exists() {
        return Ok(out);
    }

    let _ = std::fs::remove_file(&out);
    Err(format!(
        "failed to generate thumbnail for {}",
        path.display()
    ))
}

fn noterm_mode_path() -> PathBuf {
    rdm_common::config::config_dir().join("noterm-mode")
}

fn load_saved_mode() -> RenderMode {
    let path = noterm_mode_path();
    match std::fs::read_to_string(path) {
        Ok(v) => RenderMode::from_str(&v),
        Err(_) => RenderMode::Raw,
    }
}

fn save_mode(mode: RenderMode) {
    let path = noterm_mode_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("Failed to create config directory for NoTerm mode: {}", e);
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, format!("{}\n", mode.as_str())) {
        log::warn!("Failed to save NoTerm mode to {}: {}", path.display(), e);
    }
}

fn noterm_icon_size_path() -> PathBuf {
    rdm_common::config::config_dir().join("noterm-icon-size")
}

fn load_saved_icon_size() -> u32 {
    let path = noterm_icon_size_path();
    match std::fs::read_to_string(path) {
        Ok(v) => match v.trim() {
            "32" => 32,
            "96" => 96,
            "128" => 128,
            _ => 64,
        },
        Err(_) => 64,
    }
}

fn save_icon_size(size: u32) {
    let path = noterm_icon_size_path();
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!(
                "Failed to create config directory for NoTerm icon size: {}",
                e
            );
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, format!("{}\n", size)) {
        log::warn!(
            "Failed to save NoTerm icon size to {}: {}",
            path.display(),
            e
        );
    }
}

fn show_preview_panel(state: &Rc<RefCell<UiState>>) {
    let s = state.borrow();
    let width = s.paned.width();
    let usable = if width > 0 { width } else { 1100 };
    let start_width = ((usable as f64) * 0.25).round() as i32;
    s.paned.set_position(start_width.max(220));
}

fn hide_preview(state: &Rc<RefCell<UiState>>) {
    let mut s = state.borrow_mut();
    s.selected_path = None;
    s.preview_label.set_text("No selection");
    s.open_system_btn.set_sensitive(false);
    s.preview_image
        .set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
    s.preview_stack.set_visible_child_name("empty");
    let width = s.paned.width();
    let usable = if width > 0 { width } else { 1100 };
    s.paned.set_position(usable);
}

fn update_preview(state: &Rc<RefCell<UiState>>, path: &Path) {
    let mut s = state.borrow_mut();
    s.selected_path = Some(path.to_path_buf());
    s.preview_label.set_text(&path.display().to_string());
    s.open_system_btn.set_sensitive(true);

    if is_image(path) {
        match gtk4::gdk_pixbuf::Pixbuf::from_file(path) {
            Ok(pb) => {
                s.preview_image.set_pixbuf(Some(&pb));
                s.preview_stack.set_visible_child_name("image");
                drop(s);
                show_preview_panel(state);
                return;
            }
            Err(e) => {
                s.preview_image
                    .set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
                s.preview_text
                    .buffer()
                    .set_text(&format!("Failed to decode image: {}", e));
                s.preview_stack.set_visible_child_name("text");
                drop(s);
                show_preview_panel(state);
                return;
            }
        }
    }

    if is_video(path) {
        match video_thumbnail_path(path, 640) {
            Ok(thumb) => match gtk4::gdk_pixbuf::Pixbuf::from_file(&thumb) {
                Ok(pb) => {
                    s.preview_image.set_pixbuf(Some(&pb));
                    s.preview_stack.set_visible_child_name("image");
                }
                Err(e) => {
                    s.preview_image
                        .set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
                    s.preview_text
                        .buffer()
                        .set_text(&format!("Failed to load video thumbnail: {}", e));
                    s.preview_stack.set_visible_child_name("text");
                }
            },
            Err(e) => {
                s.preview_image
                    .set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
                s.preview_text.buffer().set_text(&format!(
                    "Video thumbnail unavailable: {}\n\nUse Open Externally to play this file.",
                    e
                ));
                s.preview_stack.set_visible_child_name("text");
            }
        }
        drop(s);
        show_preview_panel(state);
        return;
    }

    if is_text(path) {
        s.preview_image
            .set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let preview = truncate_chars(&content, 30_000);
                s.preview_text.buffer().set_text(&preview);
            }
            Err(e) => {
                s.preview_text
                    .buffer()
                    .set_text(&format!("Failed to read file: {}", e));
            }
        }
        s.preview_stack.set_visible_child_name("text");
        drop(s);
        show_preview_panel(state);
        return;
    }

    drop(s);
    hide_preview(state);
}

fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    for (count, ch) in s.chars().enumerate() {
        if count >= max {
            out.push_str("\n\n[truncated]");
            break;
        }
        out.push(ch);
    }
    out
}

fn load_css() {
    let css = CssProvider::new();
    let extra = r#"
        .noterm-command { font-weight: bold; }
        .noterm-output { padding: 2px 4px; }
        .noterm-list row { padding: 4px 6px; }
        .noterm-tile { border: none; background: transparent; padding: 0; }
        .noterm-tile:hover { background: alpha(@theme_surface, 0.9); }
        .noterm-tile-selected { background: alpha(@theme_selected_bg_color, 0.35); border-radius: 6px; }
        .noterm-breadcrumb { border: none; background: transparent; padding: 2px 6px; }
        .noterm-breadcrumb:hover { background: alpha(@theme_surface, 0.9); }
        .noterm-meta { opacity: 0.75; font-size: 11px; }
        .noterm-status { opacity: 0.7; font-size: 11px; padding: 2px 4px; }
        .noterm-icon {
            font-family: "JetBrainsMono Nerd Font", "IosevkaTerm Nerd Font Mono", "MesloLGS Nerd Font Mono", monospace;
        }
    "#;
    let full = format!("{}\n{}", rdm_common::theme::load_theme_css(), extra);
    css.load_from_data(&full);
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_USER + 1,
    );
}

fn navigate_to(state: &Rc<RefCell<UiState>>, path: PathBuf) {
    {
        let mut s = state.borrow_mut();
        let truncate_at = s.nav_pos + 1;
        s.nav_history.truncate(truncate_at);
        s.nav_history.push(path.clone());
        s.nav_pos = s.nav_history.len() - 1;
        s.cwd = path;
        s.cwd_label.set_text(&s.cwd.display().to_string());
        s.back_btn.set_sensitive(s.nav_pos > 0);
        s.forward_btn.set_sensitive(false);
    }
    rebuild_breadcrumb(state);
    refresh_ls_view(state);
    update_status_bar(state);
}

fn navigate_back(state: &Rc<RefCell<UiState>>) {
    {
        let mut s = state.borrow_mut();
        if s.nav_pos == 0 {
            return;
        }
        s.nav_pos -= 1;
        let path = s.nav_history[s.nav_pos].clone();
        s.cwd = path;
        s.cwd_label.set_text(&s.cwd.display().to_string());
        s.back_btn.set_sensitive(s.nav_pos > 0);
        s.forward_btn.set_sensitive(true);
    }
    rebuild_breadcrumb(state);
    refresh_ls_view(state);
    update_status_bar(state);
}

fn navigate_forward(state: &Rc<RefCell<UiState>>) {
    {
        let mut s = state.borrow_mut();
        if s.nav_pos + 1 >= s.nav_history.len() {
            return;
        }
        s.nav_pos += 1;
        let path = s.nav_history[s.nav_pos].clone();
        s.cwd = path;
        s.cwd_label.set_text(&s.cwd.display().to_string());
        s.back_btn.set_sensitive(true);
        s.forward_btn
            .set_sensitive(s.nav_pos + 1 < s.nav_history.len());
    }
    rebuild_breadcrumb(state);
    refresh_ls_view(state);
    update_status_bar(state);
}

fn update_status_bar(state: &Rc<RefCell<UiState>>) {
    let (cwd, show_hidden, selected_count, sort_field, sort_order, folders_first, batch_mode) = {
        let s = state.borrow();
        (
            s.cwd.clone(),
            s.show_hidden,
            s.selected_paths.len(),
            s.sort_field,
            s.sort_order,
            s.folders_first,
            s.batch_select_mode,
        )
    };
    let mut total = 0usize;
    let mut hidden = 0usize;
    let mut dirs = 0usize;
    let mut files = 0usize;
    if let Ok(rd) = std::fs::read_dir(&cwd) {
        for item in rd.flatten() {
            total += 1;
            let name = item.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                hidden += 1;
            }
            if let Ok(ft) = item.file_type() {
                if ft.is_dir() {
                    dirs += 1;
                } else {
                    files += 1;
                }
            }
        }
    }
    let visible = if show_hidden {
        total
    } else {
        total.saturating_sub(hidden)
    };
    let hidden_note = if hidden > 0 && !show_hidden {
        format!("  •  {} hidden", hidden)
    } else {
        String::new()
    };
    let selected_note = if selected_count > 0 {
        format!("  •  {} selected", selected_count)
    } else {
        String::new()
    };
    let folders_note = if folders_first {
        "folders first"
    } else {
        "mixed"
    };
    let batch_note = if batch_mode { "  •  batch on" } else { "" };
    state.borrow().status_label.set_text(&format!(
        "{} items  ({} dirs, {} files){}{}{}  •  sort: {} ({}, {})",
        visible,
        dirs,
        files,
        hidden_note,
        selected_note,
        batch_note,
        sort_field.label(),
        sort_order.label(sort_field),
        folders_note,
    ));
}

fn selected_paths_vec(state: &Rc<RefCell<UiState>>) -> Vec<PathBuf> {
    state.borrow().selected_paths.iter().cloned().collect()
}

fn select_all_visible(state: &Rc<RefCell<UiState>>) {
    let (cwd, show_hidden, query, sort_field, sort_order, folders_first) = {
        let s = state.borrow();
        (
            s.cwd.clone(),
            s.show_hidden,
            s.search_query.clone(),
            s.sort_field,
            s.sort_order,
            s.folders_first,
        )
    };
    let entries = build_ls_entries_from_fs(
        &cwd,
        "ls",
        show_hidden,
        &query,
        sort_field,
        sort_order,
        folders_first,
    );
    let mut selected = HashSet::new();
    for e in entries {
        if e.name != ".." {
            selected.insert(e.path);
        }
    }
    {
        let mut s = state.borrow_mut();
        s.selected_paths = selected;
    }
    refresh_ls_view(state);
    update_status_bar(state);
}

fn open_with_system(state: &Rc<RefCell<UiState>>, path: &Path) {
    let path_buf = path.to_path_buf();
    let path_label = path_buf.display().to_string();
    let state_result = state.clone();

    gtk4::glib::spawn_future_local(async move {
        let (tx, rx) = async_channel::bounded::<Result<std::process::Output, String>>(1);
        let thread_path = path_buf.clone();
        std::thread::spawn(move || {
            let result = Command::new("xdg-open")
                .arg(&thread_path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .output()
                .map_err(|e| e.to_string());
            let _ = tx.send_blocking(result);
        });

        let result = match rx.recv().await {
            Ok(v) => v,
            Err(_) => {
                state_result
                    .borrow()
                    .status_label
                    .set_text("Open failed: could not receive xdg-open result");
                return;
            }
        };

        match result {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let reason = if stderr.is_empty() {
                    format!("xdg-open exited with status {}", out.status)
                } else {
                    stderr
                };
                state_result
                    .borrow()
                    .status_label
                    .set_text(&format!("Open failed for {}: {}", path_label, reason));
                log::warn!("xdg-open failed for {}: {}", path_label, reason);
            }
            Err(err) => {
                state_result
                    .borrow()
                    .status_label
                    .set_text(&format!("Open failed for {}: {}", path_label, err));
                log::warn!("Failed to run xdg-open for {}: {}", path_label, err);
            }
        }
    });
}

fn is_appimage(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("appimage"))
        .unwrap_or(false)
}

fn run_appimage(state: &Rc<RefCell<UiState>>, path: &Path) {
    match Command::new(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(_) => {}
        Err(err) => {
            state.borrow().status_label.set_text(&format!(
                "Run failed for {}: {}",
                path.display(),
                err
            ));
            log::warn!("Failed to run AppImage {}: {}", path.display(), err);
        }
    }
}

fn show_context_menu(state: &Rc<RefCell<UiState>>, widget: &Button, entry: &LsEntry) {
    let popover = Popover::new();
    popover.set_parent(widget);
    popover.set_has_arrow(true);

    let vbox = GtkBox::new(Orientation::Vertical, 2);
    vbox.set_margin_top(4);
    vbox.set_margin_bottom(4);
    vbox.set_margin_start(4);
    vbox.set_margin_end(4);

    let open_btn = Button::with_label("Open With System");
    open_btn.add_css_class("flat");
    {
        let path = entry.path.clone();
        let state_open = state.clone();
        let pop = popover.clone();
        open_btn.connect_clicked(move |_| {
            pop.popdown();
            open_with_system(&state_open, &path);
        });
    }
    vbox.append(&open_btn);

    if is_appimage(&entry.path) {
        let run_btn = Button::with_label("Run AppImage");
        run_btn.add_css_class("flat");
        {
            let path = entry.path.clone();
            let state_run = state.clone();
            let pop = popover.clone();
            run_btn.connect_clicked(move |_| {
                pop.popdown();
                run_appimage(&state_run, &path);
            });
        }
        vbox.append(&run_btn);
    }

    if entry.kind == EntryKind::Directory {
        let add_place_btn = Button::with_label("Add to Places");
        add_place_btn.add_css_class("flat");
        {
            let path = entry.path.clone();
            let state_add = state.clone();
            let pop = popover.clone();
            add_place_btn.connect_clicked(move |_| {
                pop.popdown();
                add_to_places(&state_add, &path);
            });
        }
        vbox.append(&add_place_btn);
    }

    let copy_btn = Button::with_label("Copy Path");
    copy_btn.add_css_class("flat");
    {
        let path_str = entry.path.display().to_string();
        let pop = popover.clone();
        copy_btn.connect_clicked(move |_| {
            pop.popdown();
            if let Some(display) = gtk4::gdk::Display::default() {
                display.clipboard().set_text(&path_str);
            }
        });
    }
    vbox.append(&copy_btn);

    let rename_btn = Button::with_label("Rename");
    rename_btn.add_css_class("flat");
    {
        let path = entry.path.clone();
        let state_r = state.clone();
        let pop = popover.clone();
        rename_btn.connect_clicked(move |_| {
            pop.popdown();
            show_rename_dialog(&state_r, &path);
        });
    }
    vbox.append(&rename_btn);

    let new_folder_btn = Button::with_label("New Folder Here");
    new_folder_btn.add_css_class("flat");
    {
        let state_r = state.clone();
        let pop = popover.clone();
        new_folder_btn.connect_clicked(move |_| {
            pop.popdown();
            show_new_folder_dialog(&state_r);
        });
    }
    vbox.append(&new_folder_btn);

    let trash_btn = Button::with_label("Move to Trash");
    trash_btn.add_css_class("flat");
    {
        let path = entry.path.clone();
        let state_r = state.clone();
        let pop = popover.clone();
        trash_btn.connect_clicked(move |_| {
            pop.popdown();
            let _ = Command::new("gio")
                .args(["trash", &path.display().to_string()])
                .spawn();
            refresh_ls_view(&state_r);
            update_status_bar(&state_r);
        });
    }
    vbox.append(&trash_btn);

    popover.set_child(Some(&vbox));
    popover.popup();
}

fn show_rename_dialog(state: &Rc<RefCell<UiState>>, path: &Path) {
    let dialog = gtk4::Window::builder()
        .title("Rename")
        .default_width(360)
        .build();

    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let lbl = Label::new(Some("New name:"));
    lbl.set_xalign(0.0);
    let name_entry = Entry::new();
    let current_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    name_entry.set_text(&current_name);
    name_entry.select_region(0, -1);

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    let cancel_btn = Button::with_label("Cancel");
    let ok_btn = Button::with_label("Rename");
    ok_btn.add_css_class("suggested-action");
    btn_row.append(&cancel_btn);
    btn_row.append(&ok_btn);

    vbox.append(&lbl);
    vbox.append(&name_entry);
    vbox.append(&btn_row);
    dialog.set_child(Some(&vbox));

    {
        let d = dialog.clone();
        cancel_btn.connect_clicked(move |_| d.close());
    }
    {
        let path = path.to_path_buf();
        let state_r = state.clone();
        let d = dialog.clone();
        let e = name_entry.clone();
        ok_btn.connect_clicked(move |_| {
            let new_name = e.text().to_string();
            if !new_name.trim().is_empty() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::rename(&path, parent.join(&new_name));
                    refresh_ls_view(&state_r);
                    update_status_bar(&state_r);
                }
            }
            d.close();
        });
    }
    {
        let path = path.to_path_buf();
        let state_r = state.clone();
        let d = dialog.clone();
        name_entry.connect_activate(move |e| {
            let new_name = e.text().to_string();
            if !new_name.trim().is_empty() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::rename(&path, parent.join(&new_name));
                    refresh_ls_view(&state_r);
                    update_status_bar(&state_r);
                }
            }
            d.close();
        });
    }

    dialog.present();
}

fn show_new_folder_dialog(state: &Rc<RefCell<UiState>>) {
    let cwd = state.borrow().cwd.clone();

    let dialog = gtk4::Window::builder()
        .title("New Folder")
        .default_width(360)
        .build();

    let vbox = GtkBox::new(Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let lbl = Label::new(Some("Folder name:"));
    lbl.set_xalign(0.0);
    let name_entry = Entry::new();
    name_entry.set_text("New Folder");
    name_entry.select_region(0, -1);

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    btn_row.set_halign(gtk4::Align::End);
    let cancel_btn = Button::with_label("Cancel");
    let ok_btn = Button::with_label("Create");
    ok_btn.add_css_class("suggested-action");
    btn_row.append(&cancel_btn);
    btn_row.append(&ok_btn);

    vbox.append(&lbl);
    vbox.append(&name_entry);
    vbox.append(&btn_row);
    dialog.set_child(Some(&vbox));

    {
        let d = dialog.clone();
        cancel_btn.connect_clicked(move |_| d.close());
    }
    {
        let state_r = state.clone();
        let d = dialog.clone();
        let e = name_entry.clone();
        let cwd2 = cwd.clone();
        ok_btn.connect_clicked(move |_| {
            let name = e.text().to_string();
            if !name.trim().is_empty() {
                let _ = std::fs::create_dir_all(cwd2.join(&name));
                refresh_ls_view(&state_r);
                update_status_bar(&state_r);
            }
            d.close();
        });
    }
    {
        let state_r = state.clone();
        let d = dialog.clone();
        name_entry.connect_activate(move |e| {
            let name = e.text().to_string();
            if !name.trim().is_empty() {
                let _ = std::fs::create_dir_all(cwd.join(&name));
                refresh_ls_view(&state_r);
                update_status_bar(&state_r);
            }
            d.close();
        });
    }

    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ls_entries_basic() {
        let cwd = PathBuf::from("/tmp");
        let output = "\
drwxr-xr-x 2 user user 4096 Mar  5 12:00 folder
-rw-r--r-- 1 user user  123 Mar  5 12:01 notes.md
lrwxrwxrwx 1 user user   10 Mar  5 12:02 link -> target
";
        let entries = parse_ls_entries(output, &cwd);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].kind, EntryKind::Directory);
        assert_eq!(entries[1].kind, EntryKind::File);
        assert_eq!(entries[2].kind, EntryKind::Symlink);
        assert_eq!(entries[1].path, PathBuf::from("/tmp/notes.md"));
    }

    #[test]
    fn parse_ls_entries_ignores_total_and_empty_lines() {
        let cwd = PathBuf::from("/tmp");
        let output = "total 8\n\n-rw-r--r-- 1 u g 1 Mar 5 12:00 a.txt\n";
        let entries = parse_ls_entries(output, &cwd);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.txt");
    }

    #[test]
    fn file_type_detection() {
        assert!(is_image(Path::new("pic.png")));
        assert!(is_video(Path::new("clip.mp4")));
        assert!(is_text(Path::new("readme.md")));
        assert!(!is_text(Path::new("archive.tar.gz")));
        assert!(is_previewable(Path::new("image.webp")));
        assert!(is_previewable(Path::new("movie.mkv")));
        assert!(is_previewable(Path::new("main.rs")));
        assert!(!is_previewable(Path::new("binary.bin")));
    }

    #[test]
    fn icon_mapping_by_mode() {
        let dir = LsEntry {
            name: "d".to_string(),
            path: PathBuf::from("d"),
            kind: EntryKind::Directory,
            perms: "drwxr-xr-x".to_string(),
            size: "4096".to_string(),
            size_bytes: 4096,
            modified: "Mar 5 12:00".to_string(),
            modified_epoch: 1,
        };
        let file = LsEntry {
            name: "f".to_string(),
            path: PathBuf::from("f.md"),
            kind: EntryKind::File,
            perms: "-rw-r--r--".to_string(),
            size: "1".to_string(),
            size_bytes: 1,
            modified: "Mar 5 12:00".to_string(),
            modified_epoch: 2,
        };
        assert_eq!(icon_for_entry(RenderMode::Raw, &dir), "");
        assert_eq!(icon_for_entry(RenderMode::Text, &dir), "");
        assert_eq!(icon_for_entry(RenderMode::Icons, &dir), "📁");
        assert_eq!(icon_for_entry(RenderMode::Nerd, &dir), "\u{f115}");
        assert_eq!(icon_for_entry(RenderMode::Icons, &file), "📄");
        assert_eq!(icon_for_entry(RenderMode::Nerd, &file), "\u{f15c}");
    }

    #[test]
    fn resolve_cd_target_handles_absolute_relative_and_home() {
        let cwd = Path::new("/home/test/work");
        assert_eq!(
            resolve_cd_target(cwd, "sub"),
            PathBuf::from("/home/test/work/sub")
        );
        assert_eq!(
            resolve_cd_target(cwd, "/var/tmp"),
            PathBuf::from("/var/tmp")
        );
        let home = dirs::home_dir().unwrap_or_else(|| cwd.to_path_buf());
        assert_eq!(resolve_cd_target(cwd, "~"), home);
    }

    #[test]
    fn enhanced_ls_includes_parent_as_first_item() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rdm-noterm-ls-{}", nanos));
        std::fs::create_dir_all(root.join("child")).expect("mkdir");
        std::fs::write(root.join("a.txt"), "x").expect("write");

        let entries = build_ls_entries_from_fs(
            &root,
            "ls",
            false,
            "",
            SortField::Name,
            SortOrder::Asc,
            true,
        );
        assert!(!entries.is_empty());
        assert_eq!(entries[0].name, "..");
        assert_eq!(entries[0].kind, EntryKind::Directory);

        let _ = std::fs::remove_dir_all(root);
    }
}
