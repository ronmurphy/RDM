use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box as GtkBox, Button, CssProvider, DropDown, Entry, Label,
    Orientation, Paned, Picture, Popover, ScrolledWindow, StringList, Switch, TextView,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

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
    modified: String,
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
    nav_history: Vec<PathBuf>,
    nav_pos: usize,
    back_btn: Button,
    forward_btn: Button,
    status_label: Label,
    icon_size: u32,
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
    let hidden_label = Label::new(Some("Hidden"));
    let hidden_switch = Switch::new();
    nav_row.append(&back_btn);
    nav_row.append(&forward_btn);
    nav_row.append(&breadcrumb_box);
    nav_row.append(&search_entry);
    nav_row.append(&hidden_label);
    nav_row.append(&hidden_switch);
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

    let (places_box, place_buttons) = build_places_sidebar();
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
        nav_history: vec![initial_cwd],
        nav_pos: 0,
        back_btn: back_btn.clone(),
        forward_btn: forward_btn.clone(),
        status_label: status_label.clone(),
        icon_size: initial_icon_size,
    }));

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
        let state_hidden = state.clone();
        hidden_switch.connect_active_notify(move |sw| {
            state_hidden.borrow_mut().show_hidden = sw.is_active();
            refresh_ls_view(&state_hidden);
        });
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
        let state_open = state.clone();
        open_system_btn.connect_clicked(move |_| {
            if let Some(path) = state_open.borrow().selected_path.clone() {
                let _ = Command::new("xdg-open").arg(path).spawn();
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

    for (btn, target) in place_buttons {
        let state_btn = state.clone();
        btn.connect_clicked(move |_| {
            if target.is_dir() {
                navigate_to(&state_btn, target.clone());
            }
        });
    }

    rebuild_breadcrumb(&state);
    refresh_ls_view(&state);
    update_status_bar(&state);
    load_css();
    window.present();
}

fn run_command(state: &Rc<RefCell<UiState>>, cmd: &str) {
    append_block_label(state, &format!("$ {}", cmd), "noterm-command");

    if cmd == "pwd" {
        let cwd = state.borrow().cwd.display().to_string();
        append_block_text(state, &format!("{}\n", cwd));
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
            append_block_text(state, "");
        } else {
            drop(s);
            append_block_text(state, "cd: target is not a directory\n");
        }
        return;
    }

    let cwd = state.borrow().cwd.clone();
    let mode = state.borrow().mode;
    let (show_hidden, query) = {
        let s = state.borrow();
        (s.show_hidden, s.search_query.clone())
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
                append_block_text(&state_result, "Failed to receive command output\n");
                return;
            }
        };

        match output {
            Ok(out) => {
                let mut combined = String::new();
                combined.push_str(&String::from_utf8_lossy(&out.stdout));
                combined.push_str(&String::from_utf8_lossy(&out.stderr));

                if mode != RenderMode::Raw && cmd_owned.starts_with("ls") && out.status.success() {
                    let entries = build_ls_entries_from_fs(&cwd, &cmd_owned, show_hidden, &query);
                    if entries.is_empty() {
                        let parsed = parse_ls_entries(&combined, &cwd);
                        if parsed.is_empty() {
                            append_block_text(&state_result, &combined);
                        } else {
                            append_ls_block(&state_result, parsed);
                        }
                    } else {
                        append_ls_block(&state_result, entries);
                    }
                } else {
                    append_block_text(&state_result, &combined);
                }
            }
            Err(e) => {
                append_block_text(
                    &state_result,
                    &format!("Failed to execute command: {}\n", e),
                );
            }
        }
    });
}

fn append_block_label(state: &Rc<RefCell<UiState>>, text: &str, class_name: &str) {
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class(class_name);
    state.borrow().output_box.append(&label);
}

fn append_block_text(state: &Rc<RefCell<UiState>>, text: &str) {
    let view = TextView::new();
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_monospace(true);
    view.add_css_class("noterm-output");
    view.buffer().set_text(text);
    state.borrow().output_box.append(&view);
}

fn append_ls_block(state: &Rc<RefCell<UiState>>, entries: Vec<LsEntry>) {
    let flow = build_ls_flow(state, entries);
    state.borrow().output_box.append(&flow);
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
        // Icons mode + image file: show a scaled thumbnail.
        if mode == RenderMode::Icons && is_image(&entry.path) {
            let sz = icon_size as i32;
            match gtk4::gdk_pixbuf::Pixbuf::from_file_at_scale(&entry.path, sz, sz, true) {
                Ok(pb) => {
                    let pic = Picture::new();
                    pic.set_width_request(sz);
                    pic.set_height_request(sz);
                    pic.set_halign(gtk4::Align::Center);
                    pic.set_content_fit(gtk4::ContentFit::Contain);
                    pic.set_pixbuf(Some(&pb));
                    h.append(&pic);
                }
                Err(_) => {
                    let icon_label = Label::new(None);
                    let pt = icon_size * 3 / 4;
                    icon_label.set_markup(&format!("<span font=\"{}\">🖼️</span>", pt));
                    icon_label.set_halign(gtk4::Align::Center);
                    icon_label.add_css_class("noterm-icon");
                    h.append(&icon_label);
                }
            }
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
            let click = gtk4::GestureClick::new();
            click.connect_pressed(move |_, n_press, _, _| {
                if n_press == 2 {
                    let _ = Command::new("xdg-open").arg(&path_dbl).spawn();
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
    let (cwd, mode, show_hidden, query) = {
        let s = state.borrow();
        (s.cwd.clone(), s.mode, s.show_hidden, s.search_query.clone())
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
        let entries = build_ls_entries_from_fs(&cwd, "ls", show_hidden, &query);
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

fn build_places_sidebar() -> (GtkBox, Vec<(Button, PathBuf)>) {
    let box_widget = GtkBox::new(Orientation::Vertical, 6);
    box_widget.set_width_request(140);
    let title = Label::new(Some("Places"));
    title.set_xalign(0.0);
    box_widget.append(&title);

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let docs = home.join("Documents");
    let dl = home.join("Downloads");
    let desk = home.join("Desktop");
    let places = vec![
        ("Home", home.clone()),
        ("Desktop", desk),
        ("Documents", docs),
        ("Downloads", dl),
        ("Root", PathBuf::from("/")),
    ];

    let mut out = Vec::new();
    for (name, path) in places {
        if name == "Root" || path.exists() {
            let btn = Button::with_label(name);
            btn.set_halign(gtk4::Align::Fill);
            box_widget.append(&btn);
            out.push((btn, path));
        }
    }

    (box_widget, out)
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
            modified,
        });
    }
    entries
}

fn build_ls_entries_from_fs(cwd: &Path, cmd: &str, show_hidden: bool, query: &str) -> Vec<LsEntry> {
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
        modified: "-".to_string(),
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
            modified: modified_short(&md),
        });
    }

    if entries.len() > 1 {
        entries[1..].sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }
    entries
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
    } else if is_text(path) {
        "📄"
    } else {
        "📦"
    }
}

fn nerd_for_file(path: &Path) -> &'static str {
    if is_image(path) {
        "\u{f71e}"
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
    is_image(path) || is_text(path)
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
            log::warn!("Failed to create config directory for NoTerm icon size: {}", e);
            return;
        }
    }
    if let Err(e) = std::fs::write(&path, format!("{}\n", size)) {
        log::warn!("Failed to save NoTerm icon size to {}: {}", path.display(), e);
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
    s.preview_image.set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
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
                s.preview_image.set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
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

    if is_text(path) {
        s.preview_image.set_pixbuf(None::<&gtk4::gdk_pixbuf::Pixbuf>);
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
        s.forward_btn.set_sensitive(s.nav_pos + 1 < s.nav_history.len());
    }
    rebuild_breadcrumb(state);
    refresh_ls_view(state);
    update_status_bar(state);
}

fn update_status_bar(state: &Rc<RefCell<UiState>>) {
    let (cwd, show_hidden) = {
        let s = state.borrow();
        (s.cwd.clone(), s.show_hidden)
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
    let visible = if show_hidden { total } else { total.saturating_sub(hidden) };
    let hidden_note = if hidden > 0 && !show_hidden {
        format!("  •  {} hidden", hidden)
    } else {
        String::new()
    };
    state.borrow().status_label.set_text(&format!(
        "{} items  ({} dirs, {} files){}",
        visible, dirs, files, hidden_note
    ));
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
        let pop = popover.clone();
        open_btn.connect_clicked(move |_| {
            pop.popdown();
            let _ = Command::new("xdg-open").arg(&path).spawn();
        });
    }
    vbox.append(&open_btn);

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
        assert!(is_text(Path::new("readme.md")));
        assert!(!is_text(Path::new("archive.tar.gz")));
        assert!(is_previewable(Path::new("image.webp")));
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
            modified: "Mar 5 12:00".to_string(),
        };
        let file = LsEntry {
            name: "f".to_string(),
            path: PathBuf::from("f.md"),
            kind: EntryKind::File,
            perms: "-rw-r--r--".to_string(),
            size: "1".to_string(),
            modified: "Mar 5 12:00".to_string(),
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

        let entries = build_ls_entries_from_fs(&root, "ls", false, "");
        assert!(!entries.is_empty());
        assert_eq!(entries[0].name, "..");
        assert_eq!(entries[0].kind, EntryKind::Directory);

        let _ = std::fs::remove_dir_all(root);
    }
}
