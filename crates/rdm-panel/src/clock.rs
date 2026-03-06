use chrono::{Datelike, Local, NaiveDate};
use gtk4::prelude::*;
use gtk4::{Align, Calendar, Label, MenuButton, Orientation, Popover};

/// Build the clock widget: a flat MenuButton that shows the time,
/// with a popover containing the full date and a month calendar.
pub fn build_clock_widget(format: &str) -> MenuButton {
    let btn = MenuButton::new();
    btn.add_css_class("tray-btn");
    btn.add_css_class("clock");

    // ── Popover ──────────────────────────────────────────────
    let popover = Popover::new();
    popover.add_css_class("calendar-popover");

    let vbox = gtk4::Box::new(Orientation::Vertical, 4);
    vbox.set_margin_top(12);
    vbox.set_margin_bottom(12);
    vbox.set_margin_start(12);
    vbox.set_margin_end(12);

    // Full date header  e.g. "Thursday, March 5, 2026"
    let date_label = Label::new(None);
    date_label.add_css_class("calendar-date");
    date_label.set_halign(Align::Center);
    vbox.append(&date_label);

    // Separator
    let sep = gtk4::Separator::new(Orientation::Horizontal);
    sep.set_margin_top(4);
    sep.set_margin_bottom(4);
    vbox.append(&sep);

    // GTK Calendar widget
    let calendar = Calendar::new();
    calendar.add_css_class("calendar-widget");
    vbox.append(&calendar);

    popover.set_child(Some(&vbox));
    btn.set_popover(Some(&popover));

    // When popover opens, snap the calendar back to today
    {
        let calendar = calendar.clone();
        let date_label = date_label.clone();
        popover.connect_show(move |_| {
            let today = Local::now();
            let dt = gtk4::glib::DateTime::from_local(
                today.year(),
                today.month() as i32,
                today.day() as i32,
                0,
                0,
                0.0,
            );
            if let Ok(dt) = dt {
                calendar.select_day(&dt);
            }
            date_label.set_label(&today.format("%A, %B %-d, %Y").to_string());
        });
    }

    // When the user picks a different day, update the header
    {
        let date_label = date_label.clone();
        calendar.connect_day_selected(move |cal| {
            let dt = cal.date();
            if let Some(nd) =
                NaiveDate::from_ymd_opt(dt.year(), dt.month() as u32, dt.day_of_month() as u32)
            {
                date_label.set_label(&nd.format("%A, %B %-d, %Y").to_string());
            }
        });
    }

    // ── Timer ────────────────────────────────────────────────
    let fmt = format.to_string();

    // Initial tick
    let now = Local::now();
    btn.set_label(&now.format(&fmt).to_string());
    date_label.set_label(&now.format("%A, %B %-d, %Y").to_string());

    // Update every second
    let tick = {
        let btn = btn.clone();
        let fmt = fmt.clone();
        move || {
            let now = Local::now();
            btn.set_label(&now.format(&fmt).to_string());
            gtk4::glib::ControlFlow::Continue
        }
    };
    gtk4::glib::timeout_add_seconds_local(1, tick);

    btn
}
