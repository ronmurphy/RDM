// Clock module — formatting logic extracted for the QML panel.
// The actual timer is handled by a QML Timer that calls
// PanelBackend::update_clock() every second.

use chrono::Local;

/// Format the current time using the given format string.
pub fn format_now(format: &str) -> String {
    Local::now().format(format).to_string()
}
