mod dbus;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

const DEFAULT_TIMEOUT_MS: u32 = 5000;
const NOTIFICATION_WIDTH: i32 = 350;
const NOTIFICATION_GAP: i32 = 8;
const TOP_MARGIN: i32 = 8;

struct ActiveNotification {
    window: gtk4::Window,
    timeout_source: Option<glib::SourceId>,
}

struct NotificationState {
    next_id: u32,
    active: HashMap<u32, ActiveNotification>,
    order: Vec<u32>,
}

fn main() {
    env_logger::init();
    log::info!("Starting RDM Notification Daemon");

    let app = gtk4::Application::builder()
        .application_id("org.rdm.notify")
        .flags(gtk4::gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(build_daemon);
    app.run();
}

fn build_daemon(app: &gtk4::Application) {
    load_css();

    // Keep the app alive without a visible window
    let _hold = app.hold();

    let state = Rc::new(RefCell::new(NotificationState {
        next_id: 0,
        active: HashMap::new(),
        order: Vec::new(),
    }));

    let app_ref = app.clone();
    let next_id = Rc::new(RefCell::new(0u32));

    let state_notify = state.clone();
    let on_notify: dbus::NotifyCallback = Rc::new(move |notif| {
        show_notification(&app_ref, &state_notify, notif);
    });

    let state_close = state.clone();
    let on_close: dbus::CloseCallback = Rc::new(move |id| {
        dismiss_notification(&state_close, id);
    });

    dbus::register_notification_service(next_id, on_notify, on_close);

    log::info!("Notification daemon ready");
}

fn load_css() {
    let css = gtk4::CssProvider::new();
    css.load_from_data(&rdm_common::theme::load_theme_css());
    gtk4::style_context_add_provider_for_display(
        &gtk4::gdk::Display::default().expect("No display"),
        &css,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn show_notification(
    app: &gtk4::Application,
    state: &Rc<RefCell<NotificationState>>,
    notif: dbus::Notification,
) {
    let id = notif.id;

    // If replacing, dismiss old one first
    if state.borrow().active.contains_key(&id) {
        dismiss_notification(state, id);
    }

    // Sync next_id
    {
        let mut s = state.borrow_mut();
        if id > s.next_id {
            s.next_id = id;
        }
    }

    let window = gtk4::Window::builder()
        .application(app)
        .default_width(NOTIFICATION_WIDTH)
        .resizable(false)
        .decorated(false)
        .build();

    window.init_layer_shell();
    window.set_layer(gtk4_layer_shell::Layer::Overlay);
    window.set_anchor(gtk4_layer_shell::Edge::Top, true);
    window.set_anchor(gtk4_layer_shell::Edge::Right, true);
    window.set_anchor(gtk4_layer_shell::Edge::Left, false);
    window.set_anchor(gtk4_layer_shell::Edge::Bottom, false);
    window.set_exclusive_zone(0);
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
    window.set_namespace("rdm-notify");

    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.add_css_class("notification");

    if !notif.app_name.is_empty() {
        let app_label = gtk4::Label::new(Some(&notif.app_name));
        app_label.add_css_class("notification-app");
        app_label.set_halign(gtk4::Align::Start);
        app_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        container.append(&app_label);
    }

    let summary_label = gtk4::Label::new(Some(&notif.summary));
    summary_label.add_css_class("notification-summary");
    summary_label.set_halign(gtk4::Align::Start);
    summary_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    summary_label.set_max_width_chars(40);
    container.append(&summary_label);

    if !notif.body.is_empty() {
        let body_label = gtk4::Label::new(Some(&notif.body));
        body_label.add_css_class("notification-body");
        body_label.set_halign(gtk4::Align::Start);
        body_label.set_wrap(true);
        body_label.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
        body_label.set_max_width_chars(45);
        container.append(&body_label);
    }

    window.set_child(Some(&container));

    // Click to dismiss
    let click = gtk4::GestureClick::new();
    let state_click = state.clone();
    click.connect_released(move |gesture, _, _, _| {
        gesture.set_state(gtk4::EventSequenceState::Claimed);
        dismiss_notification(&state_click, id);
    });
    window.add_controller(click);

    // Add to state
    {
        let mut s = state.borrow_mut();
        s.order.push(id);
        s.active.insert(id, ActiveNotification {
            window: window.clone(),
            timeout_source: None,
        });
    }

    window.present();

    // Auto-dismiss timeout
    let timeout_ms = if notif.timeout < 0 {
        DEFAULT_TIMEOUT_MS
    } else if notif.timeout == 0 {
        0
    } else {
        notif.timeout as u32
    };

    if timeout_ms > 0 {
        let state_timeout = state.clone();
        let source = glib::timeout_add_local_once(
            std::time::Duration::from_millis(timeout_ms as u64),
            move || {
                dismiss_notification(&state_timeout, id);
            },
        );

        let mut s = state.borrow_mut();
        if let Some(entry) = s.active.get_mut(&id) {
            entry.timeout_source = Some(source);
        }
    }

    restack(state);
}

fn dismiss_notification(state: &Rc<RefCell<NotificationState>>, id: u32) {
    let mut s = state.borrow_mut();
    if let Some(entry) = s.active.remove(&id) {
        if let Some(source) = entry.timeout_source {
            source.remove();
        }
        entry.window.close();
    }
    s.order.retain(|&oid| oid != id);
    drop(s);

    restack(state);
}

fn restack(state: &Rc<RefCell<NotificationState>>) {
    let s = state.borrow();
    let mut y_offset = TOP_MARGIN;

    for &id in &s.order {
        if let Some(entry) = s.active.get(&id) {
            entry.window.set_margin(gtk4_layer_shell::Edge::Top, y_offset);
            let (_, natural) = entry.window.preferred_size();
            let h = if natural.height() > 0 { natural.height() } else { 80 };
            y_offset += h + NOTIFICATION_GAP;
        }
    }
}
