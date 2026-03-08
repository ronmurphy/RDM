//! AI Chat panel — a dedicated WebView window with persistent session storage.
//!
//! Each AI service gets its own persistent cookie/localStorage profile stored in
//! ~/.local/share/rdm-editor/ai-session/<service>/
//! so the user logs in once and stays logged in, exactly like a browser profile.

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Entry, Label, Orientation, Window,
};
use webkit6::prelude::*;
use webkit6::{WebView, WebsiteDataManager, NetworkSession};
use std::path::PathBuf;

/// Known AI services.
struct AiService {
    label: &'static str,
    url:   &'static str,
    dir:   &'static str, // subdirectory under ai-session/
}

const SERVICES: &[AiService] = &[
    AiService { label: "Claude",  url: "https://claude.ai",          dir: "claude"  },
    AiService { label: "ChatGPT", url: "https://chatgpt.com",        dir: "chatgpt" },
    AiService { label: "Gemini",  url: "https://gemini.google.com",  dir: "gemini"  },
    AiService { label: "Codex",   url: "https://github.com/copilot", dir: "codex"   },
];

/// Base directory for AI session data.
fn session_base() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("~/.local/share"))
        .join("rdm-editor")
        .join("ai-session")
}

/// Create a WebView with a persistent session stored in `session_dir`.
fn make_web_view(session_dir: PathBuf) -> WebView {
    let _ = std::fs::create_dir_all(&session_dir);

    // NetworkSession with persistent data manager — cookies & localStorage persist.
    let data_dir  = session_dir.to_string_lossy().to_string();
    let cache_dir = session_dir.join("cache").to_string_lossy().to_string();

    let network_session = NetworkSession::new(Some(&data_dir), Some(&cache_dir));
    WebView::builder()
        .network_session(&network_session)
        .build()
}

/// Open the AI panel window.
pub fn show_ai_panel(parent: &gtk4::ApplicationWindow) {
    let win = Window::builder()
        .title("AI Chat — rdm-editor")
        .transient_for(parent)
        .modal(false)
        .default_width(900)
        .default_height(720)
        .resizable(true)
        .build();

    let root = GtkBox::new(Orientation::Vertical, 0);

    // ── Service selector toolbar ──────────────────────────────────
    let toolbar = GtkBox::new(Orientation::Horizontal, 4);
    toolbar.set_margin_start(8);
    toolbar.set_margin_end(8);
    toolbar.set_margin_top(6);
    toolbar.set_margin_bottom(4);

    let service_lbl = Label::new(Some("Open:"));
    service_lbl.set_margin_end(4);
    toolbar.append(&service_lbl);

    // ── URL bar ───────────────────────────────────────────────────
    let url_entry = Entry::new();
    url_entry.set_hexpand(true);
    url_entry.set_placeholder_text(Some("URL"));
    url_entry.add_css_class("monospace");

    let go_btn = Button::with_label("Go");

    // ── Build WebView (starts with Claude, uses Claude's session dir) ──
    // We create a fresh WebView when switching services to use the correct
    // persistent session. The current WebView is replaced in the container.
    let initial_service = &SERVICES[0];
    let web_view_cell = std::rc::Rc::new(std::cell::RefCell::new(
        make_web_view(session_base().join(initial_service.dir))
    ));
    web_view_cell.borrow().set_hexpand(true);
    web_view_cell.borrow().set_vexpand(true);

    // Container for the WebView (so we can swap it out on service switch).
    let wv_container = GtkBox::new(Orientation::Vertical, 0);
    wv_container.set_hexpand(true);
    wv_container.set_vexpand(true);
    wv_container.append(&*web_view_cell.borrow());

    // Load initial URL.
    web_view_cell.borrow().load_uri(initial_service.url);
    url_entry.set_text(initial_service.url);

    // Service buttons.
    for svc in SERVICES {
        let btn = Button::with_label(svc.label);
        btn.set_tooltip_text(Some(svc.url));

        let wv_cell   = web_view_cell.clone();
        let container = wv_container.clone();
        let url_e     = url_entry.clone();
        let svc_dir   = session_base().join(svc.dir);
        let svc_url   = svc.url;

        btn.connect_clicked(move |_| {
            // Remove old WebView.
            let old = wv_cell.borrow().clone();
            container.remove(&old);
            // Create new WebView with service-specific session.
            let new_wv = make_web_view(svc_dir.clone());
            new_wv.set_hexpand(true);
            new_wv.set_vexpand(true);
            new_wv.load_uri(svc_url);
            container.append(&new_wv);
            *wv_cell.borrow_mut() = new_wv;
            url_e.set_text(svc_url);
        });

        toolbar.append(&btn);
    }

    // Separator between service buttons and URL bar.
    let sep_box = GtkBox::new(Orientation::Horizontal, 0);
    sep_box.set_hexpand(true);
    toolbar.append(&sep_box);
    toolbar.append(&url_entry);
    toolbar.append(&go_btn);

    // Go button / Enter in URL bar navigates.
    {
        let wv = web_view_cell.clone();
        let entry = url_entry.clone();
        go_btn.connect_clicked(move |_| {
            let mut url = entry.text().to_string();
            if !url.contains("://") { url = format!("https://{}", url); }
            wv.borrow().load_uri(&url);
        });
    }
    {
        let wv = web_view_cell.clone();
        url_entry.connect_activate(move |e| {
            let mut url = e.text().to_string();
            if !url.contains("://") { url = format!("https://{}", url); }
            wv.borrow().load_uri(&url);
        });
    }

    // Update URL bar when WebView navigates.
    {
        let entry = url_entry.clone();
        web_view_cell.borrow().connect_load_changed(move |wv, _| {
            if let Some(uri) = wv.uri() {
                entry.set_text(&uri);
            }
        });
    }

    root.append(&toolbar);
    root.append(&wv_container);
    win.set_child(Some(&root));
    win.present();
}

/// Format the current file/selection content as markdown with a diff-request footer,
/// and return the string ready to be placed on the clipboard.
pub fn format_for_ai(filename: &str, lang: &str, code: &str, question: &str) -> String {
    let question_block = if question.trim().is_empty() {
        String::new()
    } else {
        format!("{}\n\n", question.trim())
    };

    format!(
        "{question_block}\
File: `{filename}`\n\
\n\
```{lang}\n\
{code}\n\
```\n\
\n\
Please respond **only** with a unified git diff in this format:\n\
\n\
```diff\n\
--- a/{filename}\n\
+++ b/{filename}\n\
@@ -line,count +line,count @@\n\
 context line\n\
-removed line\n\
+added line\n\
```\n\
\n\
Do not include any explanation outside the diff block.",
        question_block = question_block,
        filename = filename,
        lang = lang,
        code = code,
    )
}
