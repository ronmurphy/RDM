use gtk4::prelude::*;
use std::process::Command;

/// A scanned WiFi network
#[derive(Clone, Debug)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub in_use: bool,
}

/// Scan available WiFi networks via nmcli
pub fn scan_networks() -> Vec<WifiNetwork> {
    let output = Command::new("nmcli")
        .args(["-t", "-f", "SSID,SIGNAL,SECURITY,IN-USE", "dev", "wifi", "list"])
        .output();

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            log::error!("nmcli scan failed: {}", e);
            return Vec::new();
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut seen = std::collections::HashSet::new();
    let mut networks = Vec::new();

    for line in stdout.lines() {
        // Format: SSID:SIGNAL:SECURITY:IN-USE
        // SSID can contain colons, so split from the right
        let parts: Vec<&str> = line.rsplitn(4, ':').collect();
        if parts.len() < 4 {
            continue;
        }
        // rsplitn reverses: [IN-USE, SECURITY, SIGNAL, SSID]
        let in_use = parts[0].trim() == "*";
        let security = parts[1].trim().to_string();
        let signal: u8 = parts[2].trim().parse().unwrap_or(0);
        let ssid = parts[3].trim().to_string();

        if ssid.is_empty() || !seen.insert(ssid.clone()) {
            continue;
        }

        networks.push(WifiNetwork {
            ssid,
            signal,
            security,
            in_use,
        });
    }

    // Sort: connected first, then by signal strength descending
    networks.sort_by(|a, b| {
        b.in_use.cmp(&a.in_use).then(b.signal.cmp(&a.signal))
    });

    networks
}

/// Check if a connection profile already exists (password saved)
fn is_known_network(ssid: &str) -> bool {
    Command::new("nmcli")
        .args(["-t", "-f", "NAME", "con", "show"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .any(|line| line.trim() == ssid)
        })
        .unwrap_or(false)
}

/// Connect to a known network (password already saved)
fn connect_known(ssid: &str) -> Result<(), String> {
    let output = Command::new("nmcli")
        .args(["con", "up", ssid])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Connect to a new network with a password, saving credentials
fn connect_new(ssid: &str, password: &str) -> Result<(), String> {
    let output = Command::new("nmcli")
        .args(["dev", "wifi", "connect", ssid, "password", password])
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// Nerd font icon for WiFi signal strength
fn wifi_signal_icon(signal: u8, in_use: bool) -> &'static str {
    if in_use {
        "\u{f05a9}" // 󰖩 wifi-check
    } else {
        match signal {
            0..=25  => "\u{f092b}", // 󰤫 wifi-strength-1
            26..=50 => "\u{f092d}", // 󰤭 wifi-strength-2
            51..=75 => "\u{f092f}", // 󰤯 wifi-strength-3
            _       => "\u{f0928}", // 󰤨 wifi-strength-4
        }
    }
}

/// Build the WiFi submenu and register actions.
/// Returns the submenu to be inserted into the tray menu.
pub fn build_wifi_submenu(app: &gtk4::Application) -> gtk4::gio::Menu {
    let submenu = gtk4::gio::Menu::new();
    populate_wifi_menu(&submenu);

    // Action: connect to a WiFi network (parameter = SSID)
    let wifi_action = gtk4::gio::SimpleAction::new(
        "wifi-connect",
        Some(&String::static_variant_type()),
    );

    wifi_action.connect_activate(|_, param| {
        let ssid = param
            .and_then(|v| v.get::<String>())
            .unwrap_or_default();
        if ssid.is_empty() {
            return;
        }

        if is_known_network(&ssid) {
            // Known network — connect directly
            match connect_known(&ssid) {
                Ok(()) => log::info!("Connected to {}", ssid),
                Err(e) => log::error!("Failed to connect to {}: {}", ssid, e),
            }
        } else {
            // Unknown network — show password dialog
            show_password_dialog(&ssid);
        }
    });
    app.add_action(&wifi_action);

    // Action: refresh WiFi list
    let refresh_action = gtk4::gio::SimpleAction::new("wifi-refresh", None);
    let submenu_ref = submenu.clone();
    refresh_action.connect_activate(move |_, _| {
        populate_wifi_menu(&submenu_ref);
    });
    app.add_action(&refresh_action);

    // Auto-refresh every 30 seconds
    let submenu_clone = submenu.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(30), move || {
        populate_wifi_menu(&submenu_clone);
        gtk4::glib::ControlFlow::Continue
    });

    submenu
}

/// Populate/refresh the WiFi submenu with current scan results
fn populate_wifi_menu(menu: &gtk4::gio::Menu) {
    menu.remove_all();

    let networks = scan_networks();

    if networks.is_empty() {
        menu.append(Some("No networks found"), Some("app.wifi-refresh"));
        return;
    }

    for net in networks.iter().take(15) {
        let icon = wifi_signal_icon(net.signal, net.in_use);
        let connected_mark = if net.in_use { " \u{2713}" } else { "" }; // ✓
        let lock = if net.security.contains("WPA") || net.security.contains("WEP") {
            " \u{f033e}" // 󰌾 lock
        } else {
            ""
        };
        let label = format!("{}  {}{}{}", icon, net.ssid, lock, connected_mark);

        let action = format!("app.wifi-connect");
        let item = gtk4::gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some(&action), Some(&net.ssid.to_variant()));
        menu.append_item(&item);
    }

    // Separator + refresh option
    let refresh_section = gtk4::gio::Menu::new();
    refresh_section.append(Some("\u{f0450}  Refresh"), Some("app.wifi-refresh")); // 󰑐
    menu.append_section(None, &refresh_section);
}

/// Show a GTK4 dialog asking for the WiFi password
fn show_password_dialog(ssid: &str) {
    let ssid_owned = ssid.to_string();

    // Build a window as the password dialog (layer-shell compatible)
    let dialog = gtk4::Window::builder()
        .title(&format!("Connect to {}", ssid))
        .default_width(350)
        .default_height(160)
        .resizable(false)
        .build();

    let vbox = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    vbox.set_margin_top(16);
    vbox.set_margin_bottom(16);
    vbox.set_margin_start(16);
    vbox.set_margin_end(16);

    let label = gtk4::Label::new(Some(&format!("Password for \"{}\"", ssid)));
    label.add_css_class("wifi-dialog-title");
    vbox.append(&label);

    let entry = gtk4::PasswordEntry::new();
    entry.set_show_peek_icon(true);
    entry.set_placeholder_text(Some("Enter WiFi password"));
    vbox.append(&entry);

    let btn_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    btn_box.set_halign(gtk4::Align::End);

    let cancel_btn = gtk4::Button::with_label("Cancel");
    let connect_btn = gtk4::Button::with_label("Connect");
    connect_btn.add_css_class("suggested-action");

    btn_box.append(&cancel_btn);
    btn_box.append(&connect_btn);
    vbox.append(&btn_box);

    dialog.set_child(Some(&vbox));

    // Cancel
    let dialog_cancel = dialog.clone();
    cancel_btn.connect_clicked(move |_| {
        dialog_cancel.close();
    });

    // Connect
    let dialog_connect = dialog.clone();
    let entry_clone = entry.clone();
    connect_btn.connect_clicked(move |_| {
        let password = entry_clone.text().to_string();
        if password.is_empty() {
            return;
        }
        let ssid = ssid_owned.clone();
        let dlg = dialog_connect.clone();

        // Run connection in a thread to not block the UI
        gtk4::glib::spawn_future_local(async move {
            let (tx, rx) = async_channel::bounded::<Result<(), String>>(1);
            let ssid_thread = ssid.clone();
            let pw_thread = password.clone();
            std::thread::spawn(move || {
                let result = connect_new(&ssid_thread, &pw_thread);
                let _ = tx.send_blocking(result);
            });

            match rx.recv().await {
                Ok(Ok(())) => {
                    log::info!("Connected to {}", ssid);
                    dlg.close();
                }
                Ok(Err(e)) => {
                    log::error!("WiFi connection failed: {}", e);
                    show_error_in_dialog(&dlg, &format!("Failed: {}", e));
                }
                Err(_) => {
                    log::error!("WiFi connection channel closed");
                    dlg.close();
                }
            }
        });
    });

    // Enter key activates connect
    let connect_btn_clone = connect_btn.clone();
    entry.connect_activate(move |_| {
        connect_btn_clone.emit_clicked();
    });

    dialog.present();
}

fn show_error_in_dialog(dialog: &gtk4::Window, msg: &str) {
    // Find the vbox and append an error label
    if let Some(child) = dialog.child() {
        if let Some(vbox) = child.downcast_ref::<gtk4::Box>() {
            // Remove previous error if any
            let mut child = vbox.first_child();
            while let Some(c) = child {
                let next = c.next_sibling();
                if c.has_css_class("wifi-error") {
                    vbox.remove(&c);
                }
                child = next;
            }

            let err_label = gtk4::Label::new(Some(msg));
            err_label.add_css_class("wifi-error");
            // Insert before the button box (last child)
            vbox.append(&err_label);
        }
    }
}
