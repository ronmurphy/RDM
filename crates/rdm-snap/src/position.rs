use crate::layout::WindowGeometry;
use std::process::Command;

/// Marker comments we use to find our injected keybind block in rc.xml.
const MARKER_BEGIN: &str = "<!-- rdm-snap-action-begin -->";
const MARKER_END: &str = "<!-- rdm-snap-action-end -->";

fn rc_xml_path() -> std::path::PathBuf {
    let dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/labwc"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
    dir.join("rc.xml")
}

/// Inject or replace the F24 keybind block inside rc.xml, then reconfigure
/// labwc and press F24 via wtype to apply the geometry.
///
/// `output_name` ensures the window is sent to the correct output before
/// positioning, so windows stay on their monitor.
pub fn apply_one(geo: &WindowGeometry, output_name: &str) -> Result<(), String> {
    let keybind_block = format!(
        "{}\n    <keybind key=\"F24\">\n      \
         <action name=\"MoveToOutput\" output=\"{}\" />\n      \
         <action name=\"Unmaximize\" />\n      \
         <action name=\"MoveTo\" x=\"{}\" y=\"{}\" />\n      \
         <action name=\"ResizeTo\" width=\"{}\" height=\"{}\" />\n    \
         </keybind>\n    {}",
        MARKER_BEGIN, output_name, geo.x, geo.y, geo.width, geo.height, MARKER_END,
    );

    inject_block(&keybind_block)?;

    // Reconfigure labwc to pick up the changed rc.xml
    let status = Command::new("labwc").arg("--reconfigure").status()
        .map_err(|e| format!("labwc --reconfigure failed: {}", e))?;
    if !status.success() {
        return Err("labwc --reconfigure returned non-zero".into());
    }

    // Small delay for labwc to reload
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Press F24 via wtype to trigger the MoveTo + ResizeTo
    let status = Command::new("wtype").args(["-k", "F24"]).status()
        .map_err(|e| format!("wtype failed (is wtype installed?): {}", e))?;
    if !status.success() {
        return Err("wtype returned non-zero".into());
    }

    // Small delay for the action to take effect
    std::thread::sleep(std::time::Duration::from_millis(30));

    Ok(())
}

/// Replace the F24 keybind with empty markers (keeps markers for next use).
pub fn clear_actions() {
    let empty = format!("{}\n    {}", MARKER_BEGIN, MARKER_END);
    let _ = inject_block(&empty);
    let _ = Command::new("labwc").arg("--reconfigure").status();
}

/// Fully remove the markers from rc.xml (only on daemon quit).
pub fn remove_markers() {
    if let Ok(content) = std::fs::read_to_string(rc_xml_path()) {
        if let Some(cleaned) = strip_block(&content) {
            let _ = std::fs::write(rc_xml_path(), cleaned);
            let _ = Command::new("labwc").arg("--reconfigure").status();
        }
    }
}

/// Ensure our marker block exists in rc.xml (empty initially).
/// Called once at daemon startup.
pub fn ensure_markers_in_rc_xml() -> bool {
    let path = rc_xml_path();
    let Ok(content) = std::fs::read_to_string(&path) else { return false };

    if content.contains(MARKER_BEGIN) {
        return false; // Already present
    }

    // Insert empty marker block just before </keyboard>
    let new_content = content.replace(
        "</keyboard>",
        &format!("    {}\n    {}\n  </keyboard>", MARKER_BEGIN, MARKER_END),
    );

    if new_content == content {
        log::warn!("Could not find </keyboard> in rc.xml to insert markers");
        return false;
    }

    if let Err(e) = std::fs::write(&path, &new_content) {
        log::error!("Failed to write rc.xml markers: {}", e);
        return false;
    }

    log::info!("Inserted rdm-snap action markers into rc.xml");
    true
}

/// Replace the content between our markers with a new block.
fn inject_block(block: &str) -> Result<(), String> {
    let path = rc_xml_path();
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read rc.xml: {}", e))?;

    let Some(start) = content.find(MARKER_BEGIN) else {
        return Err("Marker not found in rc.xml — run ensure_markers_in_rc_xml first".into());
    };
    let Some(end) = content.find(MARKER_END) else {
        return Err("End marker not found in rc.xml".into());
    };

    let end_of_marker = end + MARKER_END.len();
    let mut new_content = String::with_capacity(content.len());
    new_content.push_str(&content[..start]);
    new_content.push_str(block);
    new_content.push_str(&content[end_of_marker..]);

    std::fs::write(&path, &new_content)
        .map_err(|e| format!("Failed to write rc.xml: {}", e))?;

    Ok(())
}

/// Strip the marker block entirely (for cleanup on quit).
fn strip_block(content: &str) -> Option<String> {
    let start = content.find(MARKER_BEGIN)?;
    let end = content.find(MARKER_END)? + MARKER_END.len();
    // Also strip any trailing newline
    let end = if content[end..].starts_with('\n') { end + 1 } else { end };
    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..start]);
    result.push_str(&content[end..]);
    Some(result)
}
