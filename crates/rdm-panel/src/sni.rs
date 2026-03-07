/// StatusNotifierItem (SNI) system tray host.
///
/// Registers `org.kde.StatusNotifierWatcher` on the session bus so apps can
/// embed their tray icons in our panel.  D-Bus callbacks are Send+Sync, so
/// shared watcher state uses `Arc<Mutex<_>>`.  Icon buttons are created on the
/// GTK main thread via an `async_channel`.
use crate::dbusmenu;
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const WATCHER_XML: &str = r#"<node>
  <interface name="org.kde.StatusNotifierWatcher">
    <method name="RegisterStatusNotifierItem">
      <arg name="service" type="s" direction="in"/>
    </method>
    <method name="RegisterStatusNotifierHost">
      <arg name="service" type="s" direction="in"/>
    </method>
    <property name="RegisteredStatusNotifierItems" type="as" access="read"/>
    <property name="IsStatusNotifierHostRegistered" type="b" access="read"/>
    <property name="ProtocolVersion" type="i" access="read"/>
    <signal name="StatusNotifierItemRegistered">
      <arg name="service" type="s"/>
    </signal>
    <signal name="StatusNotifierItemUnregistered">
      <arg name="service" type="s"/>
    </signal>
    <signal name="StatusNotifierHostRegistered"/>
  </interface>
</node>"#;

enum SniEvent {
    ItemAdded { service: String, obj_path: String },
    ItemRemoved { key: String },
    /// A new panel tray box was created (second/third monitor joining late).
    BoxAdded { weak: glib::WeakRef<gtk4::Box> },
}

/// One registered SNI item: its proxy and the name-vanish watcher.
struct SniItem {
    proxy: gio::DBusProxy,
    _watch_id: Box<dyn std::any::Any>,
}

/// Per-monitor tray box with the buttons it currently shows.
struct SniBox {
    weak: glib::WeakRef<gtk4::Box>,
    /// item key → button shown in this box
    buttons: HashMap<String, gtk4::Button>,
}

thread_local! {
    /// Keeps GIO watch/own IDs and the connection alive for the process lifetime.
    static SNI_RESOURCES: RefCell<Vec<Box<dyn std::any::Any>>> =
        const { RefCell::new(Vec::new()) };
    static SNI_INITIALIZED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    /// Sender for the SNI event channel — stored so subsequent monitor panels
    /// can send BoxAdded events into the same loop.
    static SNI_TX: RefCell<Option<async_channel::Sender<SniEvent>>> =
        const { RefCell::new(None) };
}

/// Build and return a `gtk4::Box` that will populate itself with SNI icon
/// buttons as apps register their tray items.  Safe to call once per monitor;
/// the watcher is only registered the first time.  Subsequent calls register
/// the new box so it also receives icons.
pub fn setup_sni_tray() -> gtk4::Box {
    log::info!("SNI: setup_sni_tray called");
    let sni_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    sni_box.add_css_class("sni-tray");

    // For monitors added after the first: register the new box via the
    // existing channel so the async loop populates it with current items.
    if SNI_INITIALIZED.get() {
        // log::debug!("SNI: watcher already initialized, registering additional tray box");
        let weak = sni_box.downgrade();
        SNI_TX.with(|cell| {
            if let Some(tx) = cell.borrow().as_ref() {
                let _ = tx.send_blocking(SniEvent::BoxAdded { weak });
            }
        });
        return sni_box;
    }

    let conn = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(c) => {
            // log::debug!("SNI: connected to session D-Bus");
            c
        }
        Err(e) => {
            log::warn!("SNI: session D-Bus unavailable: {e}");
            return sni_box;
        }
    };

    let (tx, rx) = async_channel::bounded::<SniEvent>(64);

    // Store sender so subsequent monitor panels can join the same loop.
    SNI_TX.with(|cell| *cell.borrow_mut() = Some(tx.clone()));

    // Fallback path: discover SNI providers directly from the bus so icons can
    // still appear even when another broken watcher owns the bus name.
    // Run this deferred so panel startup isn't blocked by bus probing.
    let conn_seed = conn.clone();
    let tx_seed = tx.clone();
    glib::idle_add_local_once(move || {
        // log::debug!("SNI: starting deferred bus-seed probe");
        seed_items_from_bus_names(&conn_seed, &tx_seed);
    });

    // Shared watcher state (needs Send+Sync for register_object closures).
    let registered_items: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let host_registered: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    // Parse the interface description.
    let node = match gio::DBusNodeInfo::for_xml(WATCHER_XML) {
        Ok(n) => n,
        Err(e) => {
            log::error!("SNI: bad watcher XML: {e}");
            return sni_box;
        }
    };
    let iface = node
        .lookup_interface("org.kde.StatusNotifierWatcher")
        .expect("interface declared in XML");

    let items_m = registered_items.clone();
    let host_m = host_registered.clone();
    let conn_m = conn.clone();
    let tx_m = tx.clone();
    let items_g = registered_items.clone();
    let host_g = host_registered.clone();

    let reg_result = conn
        .register_object("/StatusNotifierWatcher", &iface)
        // ── method_call ──────────────────────────────────────────────────────
        .method_call(
            move |_conn: gio::DBusConnection,
                  sender: Option<&str>,
                  _path: &str,
                  _iface: Option<&str>,
                  method: &str,
                  params: glib::Variant,
                  invocation: gio::DBusMethodInvocation| {
                let sender_str = sender.unwrap_or("").to_string();
                match method {
                    "RegisterStatusNotifierItem" => {
                        let arg =
                            params.child_value(0).get::<String>().unwrap_or_default();
                        // log::debug!(
                        //     "SNI: RegisterStatusNotifierItem sender={sender_str} arg={arg}"
                        // );
                        let (service, obj_path) =
                            normalize_sni_service(&arg, &sender_str);
                        let key = format!("{service}{obj_path}");
                        {
                            let mut items = items_m.lock().unwrap();
                            if items.contains(&key) {
                                // log::debug!("SNI: item already registered, skipping key={key}");
                                invocation.return_value(None);
                                return;
                            }
                            items.push(key.clone());
                        }
                        log::info!("SNI: registered item key={key}");
                        let _ = conn_m.emit_signal(
                            None,
                            "/StatusNotifierWatcher",
                            "org.kde.StatusNotifierWatcher",
                            "StatusNotifierItemRegistered",
                            Some(&(key.as_str(),).to_variant()),
                        );
                        let _ =
                            tx_m.send_blocking(SniEvent::ItemAdded { service, obj_path });
                        invocation.return_value(None);
                    }
                    "RegisterStatusNotifierHost" => {
                        *host_m.lock().unwrap() = true;
                        // log::debug!("SNI: RegisterStatusNotifierHost");
                        let _ = conn_m.emit_signal(
                            None,
                            "/StatusNotifierWatcher",
                            "org.kde.StatusNotifierWatcher",
                            "StatusNotifierHostRegistered",
                            None,
                        );
                        invocation.return_value(None);
                    }
                    _ => invocation.return_value(None),
                }
            },
        )
        // ── get_property ─────────────────────────────────────────────────────
        .property(
            move |_conn: gio::DBusConnection,
                  _sender: Option<&str>,
                  _path: &str,
                  _iface: &str,
                  prop: &str|
                  -> glib::Variant {
                match prop {
                    "RegisteredStatusNotifierItems" => {
                        let items = items_g.lock().unwrap();
                        let keys: Vec<&str> =
                            items.iter().map(|s| s.as_str()).collect();
                        keys.to_variant()
                    }
                    "IsStatusNotifierHostRegistered" => {
                        (*host_g.lock().unwrap()).to_variant()
                    }
                    "ProtocolVersion" => 0i32.to_variant(),
                    _ => ().to_variant(),
                }
            },
        )
        // ── set_property ─────────────────────────────────────────────────────
        .set_property(
            |_: gio::DBusConnection,
             _: Option<&str>,
             _: &str,
             _: &str,
             _: &str,
             _: glib::Variant| false,
        )
        .build();

    match reg_result {
        Ok(_) => log::info!("SNI: watcher object registered at /StatusNotifierWatcher"),
        Err(e) => {
            log::error!("SNI: register_object failed: {e}");
            return sni_box;
        }
    }

    // Own org.kde.StatusNotifierWatcher.
    let own_id = gio::bus_own_name_on_connection(
        &conn,
        "org.kde.StatusNotifierWatcher",
        gio::BusNameOwnerFlags::REPLACE,
        |conn_inner, name| {
            log::info!("SNI: acquired bus name {name}");
            let _ = conn_inner.emit_signal(
                None,
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "StatusNotifierHostRegistered",
                None,
            );
        },
        |_conn, name| log::warn!("SNI: lost bus name {name}"),
    );

    // Register ourselves as a StatusNotifierHost.
    let host_name = format!(
        "org.kde.StatusNotifierHost.rdm{}",
        std::process::id()
    );
    let host_name_copy = host_name.clone();
    let tx_host = tx.clone();
    let host_own_id = gio::bus_own_name_on_connection(
        &conn,
        &host_name,
        gio::BusNameOwnerFlags::NONE,
        move |conn_inner, _| {
            let hn = host_name_copy.clone();
            schedule_host_registration(conn_inner.clone(), hn, tx_host.clone());
        },
        |_, _| {},
    );

    // Re-attempt host registration whenever the watcher owner appears/changes.
    let host_name_for_watch = host_name.clone();
    let tx_host_watch = tx.clone();
    let watcher_watch_id = gio::bus_watch_name_on_connection(
        &conn,
        "org.kde.StatusNotifierWatcher",
        gio::BusNameWatcherFlags::NONE,
        move |conn_inner, name, owner| {
            // log::debug!("SNI: watcher appeared name={name} owner={owner}");
            schedule_host_registration(
                conn_inner.clone(),
                host_name_for_watch.clone(),
                tx_host_watch.clone(),
            );
        },
        move |_, name| {
            log::warn!("SNI: watcher vanished name={name}");
        },
    );

    // Subscribe to StatusNotifierItemRegistered signals so we also pick up
    // items registered before we started (or via a foreign watcher).
    let tx_sig = tx.clone();
    let sig_id = conn.signal_subscribe(
        Some("org.kde.StatusNotifierWatcher"),
        Some("org.kde.StatusNotifierWatcher"),
        Some("StatusNotifierItemRegistered"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |_conn, _sender, _path, _iface, _signal, params| {
            if let Some(key) = params.child_value(0).get::<String>() {
                // log::debug!("SNI: observed StatusNotifierItemRegistered signal key={key}");
                let (service, obj_path) = normalize_sni_service(&key, "");
                let _ = tx_sig.send_blocking(SniEvent::ItemAdded { service, obj_path });
            }
        },
    );

    // Also discover SNI providers as services appear on the bus.
    let tx_name = tx.clone();
    let conn_name = conn.clone();
    let name_sig_id = conn.signal_subscribe(
        Some("org.freedesktop.DBus"),
        Some("org.freedesktop.DBus"),
        Some("NameOwnerChanged"),
        Some("/org/freedesktop/DBus"),
        None,
        gio::DBusSignalFlags::NONE,
        move |_conn, _sender, _path, _iface, _signal, params| {
            let name = params.child_value(0).get::<String>().unwrap_or_default();
            let old_owner = params.child_value(1).get::<String>().unwrap_or_default();
            let new_owner = params.child_value(2).get::<String>().unwrap_or_default();
            if new_owner.is_empty() || new_owner == old_owner {
                return;
            }
            maybe_probe_service_for_sni(&conn_name, &name, &tx_name);
        },
    );

    // Keep IDs + connection alive for the process lifetime.
    SNI_RESOURCES.with(|r| {
        let mut r = r.borrow_mut();
        r.push(Box::new(own_id));
        r.push(Box::new(host_own_id));
        r.push(Box::new(watcher_watch_id));
        r.push(Box::new(sig_id));
        r.push(Box::new(name_sig_id));
        r.push(Box::new(conn.clone()));
    });

    SNI_INITIALIZED.set(true);

    // GTK side: receive events and manage icon buttons across all monitor panels.
    // `boxes` holds every panel's tray box (one per monitor).
    // `items` holds the proxy + watcher for each registered SNI item.
    let boxes: Rc<RefCell<Vec<SniBox>>> = Rc::new(RefCell::new(vec![SniBox {
        weak: sni_box.downgrade(),
        buttons: HashMap::new(),
    }]));
    let items: Rc<RefCell<HashMap<String, SniItem>>> = Rc::new(RefCell::new(HashMap::new()));
    let conn_ev = conn.clone();
    let tx_remove = tx.clone();

    glib::spawn_future_local(async move {
        // log::debug!("SNI: GTK event loop started");
        while let Ok(event) = rx.recv().await {
            match event {
                SniEvent::ItemAdded { service, obj_path } => {
                    let key = format!("{service}{obj_path}");
                    // log::debug!("SNI: ItemAdded key={key}");
                    if items.borrow().contains_key(&key) {
                        // log::debug!("SNI: ItemAdded ignored, already present key={key}");
                        continue;
                    }
                    let Some((proxy, watch_id)) =
                        create_sni_proxy(&conn_ev, &service, &obj_path, &tx_remove, &key).await
                    else {
                        log::warn!("SNI: failed to create proxy key={key}");
                        continue;
                    };
                    // Add a button to every live panel box.
                    let mut boxes_ref = boxes.borrow_mut();
                    for sni_box in boxes_ref.iter_mut() {
                        let Some(b) = sni_box.weak.upgrade() else { continue };
                        let btn = make_sni_button(&proxy, &conn_ev);
                        b.append(&btn);
                        sni_box.buttons.insert(key.clone(), btn);
                    }
                    log::info!("SNI: item active key={key}");
                    items.borrow_mut().insert(key, SniItem { proxy, _watch_id: watch_id });
                }
                SniEvent::ItemRemoved { key } => {
                    // log::debug!("SNI: ItemRemoved key={key}");
                    if items.borrow_mut().remove(&key).is_some() {
                        for sni_box in boxes.borrow_mut().iter_mut() {
                            if let Some(btn) = sni_box.buttons.remove(&key) {
                                if let Some(b) = sni_box.weak.upgrade() {
                                    b.remove(&btn);
                                }
                            }
                        }
                    }
                }
                SniEvent::BoxAdded { weak } => {
                    // A new monitor panel joined — populate it with existing items.
                    // log::debug!("SNI: BoxAdded event");
                    let Some(b) = weak.upgrade() else { continue };
                    let mut buttons = HashMap::new();
                    for (key, item) in items.borrow().iter() {
                        let btn = make_sni_button(&item.proxy, &conn_ev);
                        b.append(&btn);
                        buttons.insert(key.clone(), btn);
                    }
                    boxes.borrow_mut().push(SniBox { weak, buttons });
                }
            }
        }
    });

    sni_box
}

/// Resolve a raw registration string into (service_name, object_path).
///
/// Apps may pass:
/// - Just an object path (`/StatusNotifierItem`) → use the D-Bus sender as the name.
/// - `bus.name/obj/path` → split on the first `/`.
/// - Just a bus name (`org.example.App`) → assume path `/StatusNotifierItem`.
fn normalize_sni_service(arg: &str, sender: &str) -> (String, String) {
    if arg.starts_with('/') {
        (sender.to_string(), arg.to_string())
    } else if let Some(slash) = arg.find('/') {
        (arg[..slash].to_string(), arg[slash..].to_string())
    } else {
        (arg.to_string(), "/StatusNotifierItem".to_string())
    }
}

/// Create the D-Bus proxy for an SNI item and set up the name-vanish watcher.
/// Returns `(proxy, watch_id)` on success.
async fn create_sni_proxy(
    conn: &gio::DBusConnection,
    service: &str,
    obj_path: &str,
    tx_remove: &async_channel::Sender<SniEvent>,
    key: &str,
) -> Option<(gio::DBusProxy, Box<dyn std::any::Any>)> {
    // log::debug!("SNI: creating proxy service={service} path={obj_path}");
    let proxy = gio::DBusProxy::new_future(
        conn,
        gio::DBusProxyFlags::NONE,
        None,
        Some(service),
        obj_path,
        "org.kde.StatusNotifierItem",
    )
    .await
    .map_err(|e| log::warn!("SNI: proxy for {service}{obj_path}: {e}"))
    .ok()?;

    let tx_w = tx_remove.clone();
    let key_w = key.to_string();
    let watch_id = gio::bus_watch_name_on_connection(
        conn,
        service,
        gio::BusNameWatcherFlags::NONE,
        |_, name, _| { let _ = name; }, // log::debug!("SNI: service appeared {name}"),
        move |_, _| {
            log::info!("SNI: service vanished key={key_w}");
            let _ = tx_w.send_blocking(SniEvent::ItemRemoved { key: key_w.clone() });
        },
    );

    Some((proxy, Box::new(watch_id)))
}

/// Build a tray button wired up to an existing SNI proxy.
/// Safe to call multiple times for the same proxy (one button per panel box).
fn make_sni_button(proxy: &gio::DBusProxy, conn: &gio::DBusConnection) -> gtk4::Button {
    // log::debug!(
    //     "SNI: make button service={:?} path={}",
    //     proxy.name(),
    //     proxy.object_path()
    // );
    let btn = gtk4::Button::new();
    btn.set_has_frame(false);
    btn.set_size_request(18, 18);
    btn.add_css_class("tray-btn");
    btn.add_css_class("sni-item");

    refresh_sni_icon(&btn, proxy);

    if let Some(title) = sni_string_property(proxy, "Title").filter(|s| !s.is_empty())
    {
        btn.set_tooltip_text(Some(&title));
    }

    // Left-click → Activate(x, y).
    let proxy_c = proxy.clone();
    btn.connect_clicked(move |b| {
        let alloc = b.allocation();
        let x = alloc.x() + alloc.width() / 2;
        let y = alloc.y() + alloc.height() / 2;
        // log::debug!("SNI: left-click activate x={x} y={y}");
        proxy_c.call(
            "Activate",
            Some(&(x, y).to_variant()),
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |_| {},
        );
    });

    // Right-click: prefer DBusMenu if the item advertises a Menu path;
    // fall back to the SNI ContextMenu D-Bus call otherwise.
    let conn_r = conn.clone();
    let proxy_r = proxy.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
    gesture.connect_pressed(move |g, _n, x, y| {
        g.set_state(gtk4::EventSequenceState::Claimed);
        let Some(btn_widget) = g.widget() else { return };
        let btn_ref = btn_widget.downcast_ref::<gtk4::Button>().unwrap();
        let menu_path = sni_string_property(&proxy_r, "Menu").filter(|s| !s.is_empty());
        let service = proxy_r
            .name_owner()
            .or_else(|| proxy_r.name())
            .map(|n| n.to_string())
            .unwrap_or_default();

        if let Some(path) = menu_path.filter(|_: &String| !service.is_empty()) {
            // log::debug!("SNI: right-click using DBusMenu service={service} path={path}");
            dbusmenu::show(&conn_r, &service, &path, btn_ref);
        } else {
            // Fall back: let the app draw its own menu.
            let (rx, ry) = g
                .widget()
                .and_then(|w| {
                    let root = w.root()?;
                    let pt = gtk4::graphene::Point::new(x as f32, y as f32);
                    w.compute_point(root.upcast_ref::<gtk4::Widget>(), &pt)
                        .map(|p| (p.x() as i32, p.y() as i32))
                })
                .unwrap_or((x as i32, y as i32));
            let proxy_fallback = proxy_r.clone();
            // log::debug!("SNI: right-click using ContextMenu fallback x={rx} y={ry}");
            proxy_r.call(
                "ContextMenu",
                Some(&(rx, ry).to_variant()),
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                move |res| {
                    // Some items don't implement ContextMenu and expect
                    // SecondaryActivate for right-click behavior.
                    if res.is_err() {
                        // log::debug!("SNI: ContextMenu failed, trying SecondaryActivate");
                        proxy_fallback.call(
                            "SecondaryActivate",
                            Some(&(rx, ry).to_variant()),
                            gio::DBusCallFlags::NONE,
                            -1,
                            gio::Cancellable::NONE,
                            |_| {},
                        );
                    }
                },
            );
        }
    });
    btn.add_controller(gesture);

    // Update icon when the item's properties change.
    let btn_w = btn.downgrade();
    proxy.connect_local("g-properties-changed", false, move |vals| {
        let proxy: gio::DBusProxy = vals[0].get().unwrap();
        if let Some(btn) = btn_w.upgrade() {
            refresh_sni_icon(&btn, &proxy);
        }
        None
    });

    btn
}

fn refresh_sni_icon(btn: &gtk4::Button, proxy: &gio::DBusProxy) {
    // SNI may provide a custom icon directory; add it to the icon theme search
    // path before we try icon-name lookups.
    if let Some(icon_theme_path) = sni_string_property(proxy, "IconThemePath").filter(|s| !s.is_empty())
    {
        // log::debug!("SNI: applying IconThemePath={icon_theme_path}");
        if let Some(display) = gtk4::gdk::Display::default() {
            let theme = gtk4::IconTheme::for_display(&display);
            theme.add_search_path(&icon_theme_path);
        }
    }

    // 1. Named icon from the icon theme.
    if let Some(name) = sni_string_property(proxy, "IconName").filter(|s| !s.is_empty())
    {
        // log::debug!("SNI: trying IconName={name}");
        if let Some(img) = image_from_icon_name_or_path(&name) {
            btn.set_child(Some(&img));
            // log::debug!("SNI: icon resolved via IconName");
            return;
        }
    }

    // 1b. Some items publish only AttentionIconName.
    if let Some(name) = sni_string_property(proxy, "AttentionIconName").filter(|s| !s.is_empty())
    {
        // log::debug!("SNI: trying AttentionIconName={name}");
        if let Some(img) = image_from_icon_name_or_path(&name) {
            btn.set_child(Some(&img));
            // log::debug!("SNI: icon resolved via AttentionIconName");
            return;
        }
    }

    // 2. Raw pixmap data (ARGB32, big-endian).
    if let Some(icon_pixmap) = sni_variant_property(proxy, "IconPixmap") {
        if let Some(img) = pixmap_to_image(&icon_pixmap) {
            btn.set_child(Some(&img));
            // log::debug!("SNI: icon resolved via IconPixmap");
            return;
        }
    }

    // 2b. Attention pixmap fallback.
    if let Some(attn_pixmap) = sni_variant_property(proxy, "AttentionIconPixmap") {
        if let Some(img) = pixmap_to_image(&attn_pixmap) {
            btn.set_child(Some(&img));
            // log::debug!("SNI: icon resolved via AttentionIconPixmap");
            return;
        }
    }

    // 3. Fallback to a visible glyph if icon theme misses image-missing.
    let img = gtk4::Image::from_icon_name("image-missing");
    img.set_pixel_size(16);
    if img.paintable().is_some() {
        btn.set_child(Some(&img));
        // log::debug!("SNI: icon fallback image-missing");
    } else {
        let lbl = gtk4::Label::new(Some("•"));
        lbl.set_width_chars(1);
        btn.set_child(Some(&lbl));
        // log::debug!("SNI: icon fallback bullet");
    }
}

fn sni_string_property(proxy: &gio::DBusProxy, prop: &str) -> Option<String> {
    if let Some(v) = proxy.cached_property(prop) {
        if let Some(s) = variant_to_string(&v).filter(|s| !s.is_empty()) {
            return Some(s);
        }
    }
    let v = sni_variant_property(proxy, prop)?;
    variant_to_string(&v).filter(|s| !s.is_empty())
}

fn sni_variant_property(proxy: &gio::DBusProxy, prop: &str) -> Option<glib::Variant> {
    if let Some(v) = proxy.cached_property(prop) {
        // log::debug!("SNI: property cache hit {prop}");
        return Some(v);
    }
    // log::debug!("SNI: property cache miss {prop}, using Properties.Get");
    let args = ("org.kde.StatusNotifierItem", prop).to_variant();
    let result = proxy
        .connection()
        .call_sync(
            proxy.name().as_deref(),
            proxy.object_path().as_ref(),
            "org.freedesktop.DBus.Properties",
            "Get",
            Some(&args),
            None,
            gio::DBusCallFlags::NONE,
            200,
            gio::Cancellable::NONE,
        )
        .map_err(|e| {
            // log::debug!("SNI: Properties.Get failed for {prop}: {e}");
            e
        })
        .ok()?;
    let out = result.child_value(0).get::<glib::Variant>();
    if out.is_none() {
        // log::debug!("SNI: Properties.Get returned empty variant for {prop}");
    }
    out
}

fn variant_to_string(v: &glib::Variant) -> Option<String> {
    v.get::<String>()
        .or_else(|| v.str().map(|s| s.to_string()))
}

fn image_from_icon_name_or_path(name: &str) -> Option<gtk4::Image> {
    if name.starts_with('/') {
        let tex = gtk4::gdk::Texture::from_filename(name).ok()?;
        let img = gtk4::Image::from_paintable(Some(&tex));
        img.set_pixel_size(16);
        return Some(img);
    }
    if let Some(path) = name.strip_prefix("file://") {
        let tex = gtk4::gdk::Texture::from_filename(path).ok()?;
        let img = gtk4::Image::from_paintable(Some(&tex));
        img.set_pixel_size(16);
        return Some(img);
    }

    let img = gtk4::Image::from_icon_name(name);
    img.set_pixel_size(16);
    if img.paintable().is_some() {
        Some(img)
    } else {
        None
    }
}

fn register_status_notifier_host(conn: &gio::DBusConnection, host_name: &str) -> Option<&'static str> {
    const WATCHER_PATHS: [&str; 3] = [
        "/StatusNotifierWatcher",
        "/org/StatusNotifierWatcher",
        "/org/kde/StatusNotifierWatcher",
    ];
    for path in WATCHER_PATHS {
        let res = conn.call_sync(
            Some("org.kde.StatusNotifierWatcher"),
            path,
            "org.kde.StatusNotifierWatcher",
            "RegisterStatusNotifierHost",
            Some(&(host_name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            100,  // 100ms timeout — fail fast, retry loop will try again
            gio::Cancellable::NONE,
        );
        match res {
            Ok(_) => return Some(path),
            Err(e) => { let _ = e; }, // log::debug!("SNI: RegisterStatusNotifierHost failed at {path}: {e}"),
        }
    }
    None
}

fn schedule_host_registration(
    conn: gio::DBusConnection,
    host_name: String,
    tx: async_channel::Sender<SniEvent>,
) {
    let attempts = Rc::new(std::cell::Cell::new(0u8));
    let attempts_c = attempts.clone();
    glib::timeout_add_local(Duration::from_millis(500), move || {
        let n = attempts_c.get().saturating_add(1);
        attempts_c.set(n);

        if let Some(path) = register_status_notifier_host(&conn, &host_name) {
            log::info!("SNI: host registered against watcher path={path}");
            seed_registered_items(&conn, path, &tx);
            return glib::ControlFlow::Break;
        }

        if n >= 10 {
            log::warn!("SNI: host registration retries exhausted");
            return glib::ControlFlow::Break;
        }

        glib::ControlFlow::Continue
    });
}

fn seed_items_from_bus_names(conn: &gio::DBusConnection, tx: &async_channel::Sender<SniEvent>) {
    let Ok(result) = conn.call_sync(
        Some("org.freedesktop.DBus"),
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "ListNames",
        None,
        None,
        gio::DBusCallFlags::NONE,
        200,
        gio::Cancellable::NONE,
    ) else {
        // log::debug!("SNI: ListNames failed, skipping bus-seed fallback");
        return;
    };

    let Some(names) = result.child_value(0).get::<Vec<String>>() else {
        // log::debug!("SNI: ListNames result parse failed");
        return;
    };

    let names = Rc::new(names);
    let idx = Rc::new(std::cell::Cell::new(0usize));
    let names_c = names.clone();
    let idx_c = idx.clone();
    let conn_c = conn.clone();
    let tx_c = tx.clone();
    glib::timeout_add_local(Duration::from_millis(8), move || {
        let i = idx_c.get();
        if i >= names_c.len() {
            // log::debug!("SNI: deferred bus-seed probe complete ({} names)", names_c.len());
            return glib::ControlFlow::Break;
        }
        idx_c.set(i + 1);
        maybe_probe_service_for_sni(&conn_c, &names_c[i], &tx_c);
        glib::ControlFlow::Continue
    });
}

fn maybe_probe_service_for_sni(
    conn: &gio::DBusConnection,
    service: &str,
    tx: &async_channel::Sender<SniEvent>,
) {
    // Cheap filter: ignore known non-app/system buses.
    if service.starts_with("org.freedesktop.DBus")
        || service.starts_with("org.gtk.")
        || service.starts_with("org.kde.StatusNotifierWatcher")
    {
        return;
    }

    const CANDIDATE_PATHS: [&str; 4] = [
        "/StatusNotifierItem",
        "/org/ayatana/NotificationItem",
        "/org/kde/StatusNotifierItem",
        "/org/StatusNotifierItem",
    ];
    for path in CANDIDATE_PATHS {
        let args = ("org.kde.StatusNotifierItem",).to_variant();
        let ok = conn
            .call_sync(
                Some(service),
                path,
                "org.freedesktop.DBus.Properties",
                "GetAll",
                Some(&args),
                None,
                gio::DBusCallFlags::NONE,
                80,
                gio::Cancellable::NONE,
            )
            .is_ok();
        if ok {
            // log::debug!("SNI: direct-probe found item service={service} path={path}");
            let _ = tx.send_blocking(SniEvent::ItemAdded {
                service: service.to_string(),
                obj_path: path.to_string(),
            });
            break;
        }
    }
}

fn seed_registered_items(
    conn: &gio::DBusConnection,
    watcher_path: &str,
    tx: &async_channel::Sender<SniEvent>,
) {
    let args = (
        "org.kde.StatusNotifierWatcher",
        "RegisteredStatusNotifierItems",
    )
        .to_variant();
    let Ok(result) = conn.call_sync(
        Some("org.kde.StatusNotifierWatcher"),
        watcher_path,
        "org.freedesktop.DBus.Properties",
        "Get",
        Some(&args),
        None,
        gio::DBusCallFlags::NONE,
        1000,
        gio::Cancellable::NONE,
    ) else {
        // log::debug!("SNI: could not read RegisteredStatusNotifierItems from {watcher_path}");
        return;
    };

    let Some(items_v) = result.child_value(0).get::<glib::Variant>() else {
        // log::debug!("SNI: watcher returned no RegisteredStatusNotifierItems variant");
        return;
    };
    let Some(items) = items_v.get::<Vec<String>>() else {
        // log::debug!("SNI: watcher RegisteredStatusNotifierItems had unexpected type");
        return;
    };

    for key in items {
        // log::debug!("SNI: seeding existing item key={key}");
        let (service, obj_path) = normalize_sni_service(&key, "");
        let _ = tx.send_blocking(SniEvent::ItemAdded { service, obj_path });
    }
}

/// Convert an SNI `a(iiay)` pixmap variant to a `gtk4::Image`.
/// Picks the largest available size.
fn pixmap_to_image(variant: &glib::Variant) -> Option<gtk4::Image> {
    let n = variant.n_children();
    let mut best: Option<(i32, i32, Vec<u8>)> = None;

    for i in 0..n {
        let entry = variant.child_value(i);
        let w: i32 = entry.child_value(0).get()?;
        let h: i32 = entry.child_value(1).get()?;
        let data: Vec<u8> = entry.child_value(2).get()?;
        if best.as_ref().map_or(true, |(bw, bh, _)| w * h > bw * bh) {
            best = Some((w, h, data));
        }
    }

    let (w, h, argb) = best?;
    if argb.len() < (w * h * 4) as usize {
        return None;
    }

    // ARGB big-endian [A, R, G, B] → RGBA [R, G, B, A]
    let mut rgba = Vec::with_capacity(argb.len());
    for px in argb.chunks_exact(4) {
        rgba.push(px[1]); // R
        rgba.push(px[2]); // G
        rgba.push(px[3]); // B
        rgba.push(px[0]); // A
    }

    let bytes = glib::Bytes::from(&rgba);
    let texture = gtk4::gdk::MemoryTexture::new(
        w,
        h,
        gtk4::gdk::MemoryFormat::R8g8b8a8,
        &bytes,
        (w * 4) as usize,
    );
    let img = gtk4::Image::from_paintable(Some(&texture));
    img.set_pixel_size(16);
    Some(img)
}
