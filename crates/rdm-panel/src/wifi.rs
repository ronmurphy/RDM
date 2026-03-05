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

    // Limit to 15 results
    networks.truncate(15);
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

/// Format a network into a display label for the QML menu.
pub fn format_network_label(net: &WifiNetwork) -> String {
    let icon = wifi_signal_icon(net.signal, net.in_use);
    let connected_mark = if net.in_use { " \u{2713}" } else { "" }; // ✓
    let lock = if net.security.contains("WPA") || net.security.contains("WEP") {
        " \u{f033e}" // 󰌾 lock
    } else {
        ""
    };
    format!("{}  {}{}{}", icon, net.ssid, lock, connected_mark)
}

/// Connect to a WiFi network by SSID.
/// Known networks connect directly; unknown networks are attempted without
/// a password (a proper password dialog should be shown by the QML layer for
/// secured networks, but as a fallback we log a warning).
pub fn connect_network(ssid: &str) {
    if is_known_network(ssid) {
        match connect_known(ssid) {
            Ok(()) => log::info!("Connected to {}", ssid),
            Err(e) => log::error!("Failed to connect to {}: {}", ssid, e),
        }
    } else {
        log::warn!("Unknown network '{}' — password may be required", ssid);
        // Attempt connection; NetworkManager will prompt via its own agent
        // if one is running, otherwise this will fail for secured networks.
        match connect_known(ssid) {
            Ok(()) => log::info!("Connected to {}", ssid),
            Err(e) => log::error!("Failed to connect to {}: {}", ssid, e),
        }
    }
}
