/// DBusMenu (com.canonical.dbusmenu) context-menu renderer.
///
/// Shows a GTK4 Popover populated from a remote menu object.  The host
/// fetches the menu layout via D-Bus and renders it with GTK widgets;
/// on item activation it sends an Event back to the remote app.
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

/// Show the DBusMenu for a tray item as a GTK4 Popover anchored to `parent`.
///
/// * `conn`       – the session D-Bus connection
/// * `service`    – bus name of the SNI service (e.g. `":1.77"`)
/// * `menu_path`  – object path of the menu (from SNI `Menu` property, e.g. `"/MenuBar"`)
/// * `parent`     – the tray button to anchor the popover to
pub fn show(
    conn: &gio::DBusConnection,
    service: &str,
    menu_path: &str,
    parent: &gtk4::Button,
) {
    let conn = conn.clone();
    let service = service.to_string();
    let menu_path = menu_path.to_string();
    let parent_weak = parent.downgrade();

    // GetLayout(parentId=0, recursionDepth=-1, propertyNames=[])
    let args = (0i32, -1i32, Vec::<String>::new()).to_variant();

    let conn2 = conn.clone();
    let service2 = service.clone();
    let menu_path2 = menu_path.clone();
    conn.call(
        Some(&service),
        &menu_path,
        "com.canonical.dbusmenu",
        "GetLayout",
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        5000,
        gio::Cancellable::NONE,
        move |result| {
            let conn = conn2;
            let service = service2;
            let menu_path = menu_path2;
            let Some(parent) = parent_weak.upgrade() else { return };
            let Ok(result) = result else {
                log::warn!("DBusMenu: GetLayout failed for {service}{menu_path}");
                return;
            };

            // result type: (u (i a{sv} av))
            //   child_value(0) = u          (revision, ignored)
            //   child_value(1) = (i a{sv} av)  (root item)
            //     .child_value(0) = i         (root id, usually 0)
            //     .child_value(1) = a{sv}     (root props, usually empty)
            //     .child_value(2) = av        (top-level menu children)
            let root = result.child_value(1); // (i a{sv} av)
            let children = root.child_value(2); // av

            let popover = gtk4::Popover::new();
            popover.set_has_arrow(false);
            popover.add_css_class("sni-context-menu");

            let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
            vbox.add_css_class("sni-menu-box");
            popover.set_child(Some(&vbox));
            popover.set_parent(&parent);

            let n = children.n_children();
            for i in 0..n {
                // Each element of av is type "v"; unwrap to get (i a{sv} av).
                let item_v = children.child_value(i);
                let Some(item) = item_v.get::<glib::Variant>() else { continue };

                let id: i32 = item.child_value(0).get().unwrap_or(0);
                let props = item.child_value(1);

                let item_type = dict_str(&props, "type").unwrap_or_else(|| "standard".into());
                let visible = dict_bool(&props, "visible").unwrap_or(true);
                let enabled = dict_bool(&props, "enabled").unwrap_or(true);
                let label = dict_str(&props, "label").unwrap_or_default();

                if !visible {
                    continue;
                }

                if item_type == "separator" {
                    vbox.append(&gtk4::Separator::new(gtk4::Orientation::Horizontal));
                    continue;
                }

                if label.is_empty() {
                    continue;
                }

                let row = gtk4::Button::with_label(&label);
                row.set_use_underline(true);
                row.set_has_frame(false);
                row.set_sensitive(enabled);
                row.add_css_class("sni-menu-item");

                let conn_c = conn.clone();
                let svc_c = service.clone();
                let path_c = menu_path.clone();
                let popover_weak = popover.downgrade();
                row.connect_clicked(move |_| {
                    if let Some(p) = popover_weak.upgrade() {
                        p.popdown();
                    }
                    send_event(&conn_c, &svc_c, &path_c, id, "clicked");
                });

                vbox.append(&row);
            }

            popover.popup();
        },
    );
}

/// Send a DBusMenu Event (e.g. "clicked") for a menu item.
fn send_event(conn: &gio::DBusConnection, service: &str, menu_path: &str, id: i32, event: &str) {
    // Event(id: INT32, eventId: STRING, data: VARIANT, timestamp: UINT32)
    // data for "clicked" is conventionally 0i32 wrapped as v.
    let timestamp = (glib::real_time() / 1000) as u32;
    // Put 0i32.to_variant() (type "i") directly in the tuple so ToVariant wraps it as "v".
    let args = (id, event.to_string(), 0i32.to_variant(), timestamp).to_variant();
    conn.call(
        Some(service),
        menu_path,
        "com.canonical.dbusmenu",
        "Event",
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        -1,
        gio::Cancellable::NONE,
        |_| {},
    );
}

/// Extract a string value from an `a{sv}` properties variant.
fn dict_str(props: &glib::Variant, key: &str) -> Option<String> {
    let n = props.n_children();
    for i in 0..n {
        let entry = props.child_value(i);
        let k: String = entry.child_value(0).get()?;
        if k == key {
            // Value is type "v"; get the inner variant, then extract as String.
            let inner = entry.child_value(1).get::<glib::Variant>()?;
            return inner.get::<String>();
        }
    }
    None
}

/// Extract a bool value from an `a{sv}` properties variant.
fn dict_bool(props: &glib::Variant, key: &str) -> Option<bool> {
    let n = props.n_children();
    for i in 0..n {
        let entry = props.child_value(i);
        let k: String = entry.child_value(0).get()?;
        if k == key {
            let inner = entry.child_value(1).get::<glib::Variant>()?;
            return inner.get::<bool>();
        }
    }
    None
}
