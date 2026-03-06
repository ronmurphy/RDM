use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// Freedesktop Notifications D-Bus introspection XML
const INTROSPECTION_XML: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="Notify">
      <arg direction="in"  type="s"    name="app_name"/>
      <arg direction="in"  type="u"    name="replaces_id"/>
      <arg direction="in"  type="s"    name="app_icon"/>
      <arg direction="in"  type="s"    name="summary"/>
      <arg direction="in"  type="s"    name="body"/>
      <arg direction="in"  type="as"   name="actions"/>
      <arg direction="in"  type="a{sv}" name="hints"/>
      <arg direction="in"  type="i"    name="expire_timeout"/>
      <arg direction="out" type="u"    name="id"/>
    </method>
    <method name="CloseNotification">
      <arg direction="in"  type="u"    name="id"/>
    </method>
    <method name="GetCapabilities">
      <arg direction="out" type="as"   name="capabilities"/>
    </method>
    <method name="GetServerInformation">
      <arg direction="out" type="s"    name="name"/>
      <arg direction="out" type="s"    name="vendor"/>
      <arg direction="out" type="s"    name="version"/>
      <arg direction="out" type="s"    name="spec_version"/>
    </method>
    <signal name="NotificationClosed">
      <arg type="u" name="id"/>
      <arg type="u" name="reason"/>
    </signal>
    <signal name="ActionInvoked">
      <arg type="u" name="id"/>
      <arg type="s" name="action_key"/>
    </signal>
  </interface>
</node>
"#;

/// Parsed notification from D-Bus Notify call
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    pub timeout: i32,
}

/// Callback type for when a Notify is received
pub type NotifyCallback = Rc<dyn Fn(Notification)>;
/// Callback type for when CloseNotification is received
pub type CloseCallback = Rc<dyn Fn(u32)>;

/// Holds D-Bus resources that must remain alive for the notification service.
/// Dropping this will release the bus name and unregister the object.
pub struct NotificationServiceHandle {
    _connection: gio::DBusConnection,
    _registration_id: gio::RegistrationId,
    _owner_id: gio::OwnerId,
}

/// Register the org.freedesktop.Notifications service on the session bus.
/// Gets the bus synchronously, registers the object, then owns the name.
/// Returns a handle that MUST be kept alive for the service to persist.
pub fn register_notification_service(
    next_id: Rc<RefCell<u32>>,
    on_notify: NotifyCallback,
    on_close: CloseCallback,
) -> Option<NotificationServiceHandle> {
    let connection = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(c) => c,
        Err(e) => {
            log::error!("Failed to connect to session bus: {}", e);
            return None;
        }
    };

    let node_info = gio::DBusNodeInfo::for_xml(INTROSPECTION_XML)
        .expect("Failed to parse notification introspection XML");
    let interface_info = node_info
        .lookup_interface("org.freedesktop.Notifications")
        .expect("Interface not found in XML");

    // Register object first, then claim the name
    let registration_id = match connection
        .register_object("/org/freedesktop/Notifications", &interface_info)
        .method_call(
            move |_conn, _sender, _path, _iface, method, params, invocation| {
                handle_method_call(method, params, invocation, &next_id, &on_notify, &on_close);
            },
        )
        .build()
    {
        Ok(id) => {
            log::info!("Registered D-Bus object at /org/freedesktop/Notifications");
            id
        }
        Err(e) => {
            log::error!("Failed to register D-Bus object: {}", e);
            return None;
        }
    };

    // Now claim the well-known name
    let owner_id = gio::bus_own_name_on_connection(
        &connection,
        "org.freedesktop.Notifications",
        gio::BusNameOwnerFlags::REPLACE,
        |_conn, name| {
            log::info!("Acquired bus name {}", name);
        },
        |_conn, name| {
            log::warn!("Lost bus name {}", name);
        },
    );

    log::info!("Notification D-Bus service ready");

    Some(NotificationServiceHandle {
        _connection: connection,
        _registration_id: registration_id,
        _owner_id: owner_id,
    })
}

fn handle_method_call(
    method: &str,
    params: glib::Variant,
    invocation: gio::DBusMethodInvocation,
    next_id: &Rc<RefCell<u32>>,
    on_notify: &NotifyCallback,
    on_close: &CloseCallback,
) {
    match method {
        "Notify" => {
            // (susssasa{sv}i)
            let app_name: String = params.child_value(0).get().unwrap_or_default();
            let replaces_id: u32 = params.child_value(1).get().unwrap_or(0);
            let _app_icon: String = params.child_value(2).get().unwrap_or_default();
            let summary: String = params.child_value(3).get().unwrap_or_default();
            let body: String = params.child_value(4).get().unwrap_or_default();
            // skip actions [5] and hints [6]
            let timeout: i32 = params.child_value(7).get().unwrap_or(-1);

            let id = if replaces_id > 0 {
                replaces_id
            } else {
                let mut nid = next_id.borrow_mut();
                *nid += 1;
                *nid
            };

            log::info!("Notify: [{}] {} — {}", app_name, summary, body);

            on_notify(Notification {
                id,
                app_name,
                summary,
                body,
                timeout,
            });

            invocation.return_value(Some(&(id,).to_variant()));
        }
        "CloseNotification" => {
            let id: u32 = params.child_value(0).get().unwrap_or(0);
            on_close(id);
            invocation.return_value(None);
        }
        "GetCapabilities" => {
            let caps: Vec<String> = vec!["body".into()];
            invocation.return_value(Some(&(caps,).to_variant()));
        }
        "GetServerInformation" => {
            let info = (
                "rdm-notify".to_string(),
                "RDM".to_string(),
                "0.1".to_string(),
                "1.2".to_string(),
            );
            invocation.return_value(Some(&info.to_variant()));
        }
        _ => {
            log::warn!("Unknown method: {}", method);
            invocation.return_error(
                gio::IOErrorEnum::Failed,
                &format!("Unknown method: {}", method),
            );
        }
    }
}
