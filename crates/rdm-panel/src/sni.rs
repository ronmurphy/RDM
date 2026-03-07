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
    let sni_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
    sni_box.add_css_class("sni-tray");

    // For monitors added after the first: register the new box via the
    // existing channel so the async loop populates it with current items.
    if SNI_INITIALIZED.get() {
        let weak = sni_box.downgrade();
        SNI_TX.with(|cell| {
            if let Some(tx) = cell.borrow().as_ref() {
                let _ = tx.send_blocking(SniEvent::BoxAdded { weak });
            }
        });
        return sni_box;
    }

    let conn = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("SNI: session D-Bus unavailable: {e}");
            return sni_box;
        }
    };

    let (tx, rx) = async_channel::bounded::<SniEvent>(64);

    // Store sender so subsequent monitor panels can join the same loop.
    SNI_TX.with(|cell| *cell.borrow_mut() = Some(tx.clone()));

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
                        let (service, obj_path) =
                            normalize_sni_service(&arg, &sender_str);
                        let key = format!("{service}{obj_path}");
                        {
                            let mut items = items_m.lock().unwrap();
                            if items.contains(&key) {
                                invocation.return_value(None);
                                return;
                            }
                            items.push(key.clone());
                        }
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
        gio::BusNameOwnerFlags::NONE,
        |_conn, name| log::info!("SNI: acquired bus name {name}"),
        |_conn, name| log::warn!("SNI: lost bus name {name}"),
    );

    // Register ourselves as a StatusNotifierHost.
    let host_name = format!(
        "org.kde.StatusNotifierHost.rdm{}",
        std::process::id()
    );
    let host_name_copy = host_name.clone();
    let host_own_id = gio::bus_own_name_on_connection(
        &conn,
        &host_name,
        gio::BusNameOwnerFlags::NONE,
        move |conn_inner, _| {
            let hn = host_name_copy.clone();
            conn_inner.call(
                Some("org.kde.StatusNotifierWatcher"),
                "/StatusNotifierWatcher",
                "org.kde.StatusNotifierWatcher",
                "RegisterStatusNotifierHost",
                Some(&(hn.as_str(),).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |result| {
                    if let Err(e) = result {
                        log::warn!("SNI: RegisterStatusNotifierHost failed: {e}");
                    }
                },
            );
        },
        |_, _| {},
    );

    // Subscribe to StatusNotifierItemRegistered signals so we also pick up
    // items registered before we started (or via a foreign watcher).
    let tx_sig = tx.clone();
    let sig_id = conn.signal_subscribe(
        Some("org.kde.StatusNotifierWatcher"),
        Some("org.kde.StatusNotifierWatcher"),
        Some("StatusNotifierItemRegistered"),
        Some("/StatusNotifierWatcher"),
        None,
        gio::DBusSignalFlags::NONE,
        move |_conn, _sender, _path, _iface, _signal, params| {
            if let Some(key) = params.child_value(0).get::<String>() {
                let (service, obj_path) = normalize_sni_service(&key, "");
                let _ = tx_sig.send_blocking(SniEvent::ItemAdded { service, obj_path });
            }
        },
    );

    // Keep IDs + connection alive for the process lifetime.
    SNI_RESOURCES.with(|r| {
        let mut r = r.borrow_mut();
        r.push(Box::new(own_id));
        r.push(Box::new(host_own_id));
        r.push(Box::new(sig_id));
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
        while let Ok(event) = rx.recv().await {
            match event {
                SniEvent::ItemAdded { service, obj_path } => {
                    let key = format!("{service}{obj_path}");
                    if items.borrow().contains_key(&key) {
                        continue;
                    }
                    let Some((proxy, watch_id)) =
                        create_sni_proxy(&conn_ev, &service, &obj_path, &tx_remove, &key).await
                    else {
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
                    items.borrow_mut().insert(key, SniItem { proxy, _watch_id: watch_id });
                }
                SniEvent::ItemRemoved { key } => {
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
        |_, _, _| {},
        move |_, _| {
            let _ = tx_w.send_blocking(SniEvent::ItemRemoved { key: key_w.clone() });
        },
    );

    Some((proxy, Box::new(watch_id)))
}

/// Build a tray button wired up to an existing SNI proxy.
/// Safe to call multiple times for the same proxy (one button per panel box).
fn make_sni_button(proxy: &gio::DBusProxy, conn: &gio::DBusConnection) -> gtk4::Button {
    let btn = gtk4::Button::new();
    btn.set_has_frame(false);
    btn.add_css_class("tray-btn");
    btn.add_css_class("sni-item");

    refresh_sni_icon(&btn, proxy);

    if let Some(title) = proxy
        .cached_property("Title")
        .and_then(|v| v.get::<String>())
        .filter(|s| !s.is_empty())
    {
        btn.set_tooltip_text(Some(&title));
    }

    // Left-click → Activate(x, y).
    let proxy_c = proxy.clone();
    btn.connect_clicked(move |b| {
        let alloc = b.allocation();
        let x = alloc.x() + alloc.width() / 2;
        let y = alloc.y() + alloc.height() / 2;
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
    let menu_path = proxy
        .cached_property("Menu")
        .and_then(|v| v.str().map(|s| s.to_string()))
        .filter(|s| !s.is_empty());
    let service = proxy
        .name()
        .map(|n| n.to_string())
        .unwrap_or_default();
    let conn_r = conn.clone();
    let proxy_r = proxy.clone();
    let gesture = gtk4::GestureClick::new();
    gesture.set_button(3);
    gesture.set_propagation_phase(gtk4::PropagationPhase::Capture);
    gesture.connect_pressed(move |g, _n, x, y| {
        g.set_state(gtk4::EventSequenceState::Claimed);
        let Some(btn_widget) = g.widget() else { return };
        let btn_ref = btn_widget.downcast_ref::<gtk4::Button>().unwrap();
        if let Some(ref path) = menu_path {
            dbusmenu::show(&conn_r, &service, path, btn_ref);
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
            proxy_r.call(
                "ContextMenu",
                Some(&(rx, ry).to_variant()),
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
                |_| {},
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
    // 1. Named icon from the icon theme.
    if let Some(name) = proxy
        .cached_property("IconName")
        .and_then(|v| v.get::<String>())
        .filter(|s| !s.is_empty())
    {
        let img = gtk4::Image::from_icon_name(&name);
        img.set_pixel_size(16);
        btn.set_child(Some(&img));
        return;
    }

    // 2. Raw pixmap data (ARGB32, big-endian).
    if let Some(img) = proxy
        .cached_property("IconPixmap")
        .as_ref()
        .and_then(pixmap_to_image)
    {
        btn.set_child(Some(&img));
        return;
    }

    // 3. Fallback.
    let img = gtk4::Image::from_icon_name("image-missing");
    img.set_pixel_size(16);
    btn.set_child(Some(&img));
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
