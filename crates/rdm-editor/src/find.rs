// sourceview5::prelude re-exports gtk4::prelude.
use sourceview5::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation, SearchBar, SearchEntry};
use sourceview5::{Buffer, SearchContext, SearchSettings};

#[derive(Clone)]
pub struct FindBar {
    pub widget:  SearchBar,
    search_ctx:  std::rc::Rc<std::cell::RefCell<Option<SearchContext>>>,
    entry:       SearchEntry,
}

impl FindBar {
    pub fn new() -> Self {
        let bar = SearchBar::new();
        bar.add_css_class("editor-find-bar");
        bar.set_show_close_button(true);

        let hbox = GtkBox::new(Orientation::Horizontal, 6);
        hbox.set_margin_start(8);
        hbox.set_margin_end(8);
        hbox.set_margin_top(4);
        hbox.set_margin_bottom(4);

        let entry = SearchEntry::new();
        entry.set_placeholder_text(Some("Find…"));
        entry.set_hexpand(true);

        let prev_btn = Button::with_label("◀");
        let next_btn = Button::with_label("▶");

        let match_lbl = Label::new(Some(""));
        match_lbl.add_css_class("editor-statusbar-item");

        hbox.append(&entry);
        hbox.append(&prev_btn);
        hbox.append(&next_btn);
        hbox.append(&match_lbl);
        bar.set_child(Some(&hbox));

        let search_ctx: std::rc::Rc<std::cell::RefCell<Option<SearchContext>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));

        {
            let ctx_rc = search_ctx.clone();
            next_btn.connect_clicked(move |_| {
                if let Some(ctx) = ctx_rc.borrow().as_ref() { find_next(ctx); }
            });
        }
        {
            let ctx_rc = search_ctx.clone();
            prev_btn.connect_clicked(move |_| {
                if let Some(ctx) = ctx_rc.borrow().as_ref() { find_prev(ctx); }
            });
        }
        {
            let ctx_rc = search_ctx.clone();
            let lbl = match_lbl.clone();
            entry.connect_search_changed(move |e| {
                let text = e.text().to_string();
                if let Some(ctx) = ctx_rc.borrow().as_ref() {
                    ctx.settings().set_search_text(if text.is_empty() { None } else { Some(&text) });
                    let count = ctx.occurrences_count();
                    let match_text = if text.is_empty() {
                        String::new()
                    } else if count > 0 {
                        format!("{} matches", count)
                    } else {
                        "No matches".to_string()
                    };
                    lbl.set_text(&match_text);
                }
            });
        }
        {
            let ctx_rc = search_ctx.clone();
            entry.connect_activate(move |_| {
                if let Some(ctx) = ctx_rc.borrow().as_ref() { find_next(ctx); }
            });
        }

        let fb = Self { widget: bar, search_ctx, entry: entry.clone() };
        fb.widget.connect_entry(&fb.entry);
        fb
    }

    pub fn set_buffer(&self, buffer: &Buffer) {
        let settings = SearchSettings::new();
        settings.set_wrap_around(true);
        settings.set_case_sensitive(false);
        let ctx = SearchContext::new(buffer, Some(&settings));
        let text = self.entry.text().to_string();
        if !text.is_empty() {
            settings.set_search_text(Some(&text));
        }
        *self.search_ctx.borrow_mut() = Some(ctx);
    }

    pub fn reveal(&self) {
        self.widget.set_search_mode(true);
        self.entry.grab_focus();
    }

    pub fn hide(&self) { self.widget.set_search_mode(false); }

    pub fn toggle(&self) {
        if self.widget.is_search_mode() { self.hide(); } else { self.reveal(); }
    }
}

fn find_next(ctx: &SearchContext) {
    let buf = ctx.buffer();
    let mark = buf.get_insert();
    let cursor = buf.iter_at_mark(&mark);
    // sourceview5: forward(start_iter) -> Option<(match_start, match_end, wrapped)>
    if let Some((start, end, _wrapped)) = ctx.forward(&cursor) {
        buf.select_range(&start, &end);
    }
}

fn find_prev(ctx: &SearchContext) {
    let buf = ctx.buffer();
    let mark = buf.get_insert();
    let cursor = buf.iter_at_mark(&mark);
    if let Some((start, end, _wrapped)) = ctx.backward(&cursor) {
        buf.select_range(&start, &end);
    }
}
