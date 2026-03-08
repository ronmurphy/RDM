use sourceview5::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, Dialog, Entry, Label, Orientation, ResponseType,
};

/// Show a "Go to Line" dialog and jump the cursor in `buffer` to the chosen line.
/// `parent` is the application window for dialog parenting.
pub fn show_goto_dialog(parent: &gtk4::ApplicationWindow, buffer: &sourceview5::Buffer) {
    let dialog = Dialog::builder()
        .title("Go to Line")
        .transient_for(parent)
        .modal(true)
        .use_header_bar(1)
        .build();

    dialog.add_button("Cancel", ResponseType::Cancel);
    let go_btn = dialog.add_button("Go", ResponseType::Accept);
    go_btn.add_css_class("suggested-action");
    dialog.set_default_response(ResponseType::Accept);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_start(16);
    content.set_margin_end(16);
    content.set_margin_top(8);
    content.set_margin_bottom(8);

    let hbox = GtkBox::new(Orientation::Horizontal, 8);
    hbox.set_halign(Align::Center);

    let lbl = Label::new(Some("Line:"));
    let entry = Entry::new();
    entry.set_width_chars(8);
    entry.set_activates_default(true);
    entry.set_input_purpose(gtk4::InputPurpose::Digits);

    // Pre-fill with current line number.
    let mark = buffer.get_insert();
    let iter = buffer.iter_at_mark(&mark);
    entry.set_text(&(iter.line() + 1).to_string());
    entry.select_region(0, -1);

    hbox.append(&lbl);
    hbox.append(&entry);
    content.append(&hbox);
    content.show();

    let buf = buffer.clone();
    let entry_clone = entry.clone();
    dialog.connect_response(move |dlg, resp| {
        if resp == ResponseType::Accept {
            let text = entry_clone.text();
            if let Ok(line_num) = text.parse::<i32>() {
                let line = (line_num - 1).max(0);
                let mut target = buf.iter_at_line(line).unwrap_or_else(|| buf.end_iter());
                // Move to first non-whitespace character on the line.
                while !target.ends_line() && target.char().is_whitespace() {
                    target.forward_char();
                }
                buf.place_cursor(&target);
                // Scroll to the cursor — handled by the view via buffer signals.
            }
        }
        dlg.close();
    });

    dialog.present();
}
