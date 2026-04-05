use std::process::Command;

const MARKER_BEGIN: &str = "<!-- rdm-snap-keybinds-begin -->";
const MARKER_END: &str = "<!-- rdm-snap-keybinds-end -->";

const KEYBINDS_BLOCK: &str = r#"<!-- rdm-snap-keybinds-begin -->
    <!-- Tile / float focused window -->
    <keybind key="W-t">
      <action name="Execute" command="rdm-snap tile" />
    </keybind>
    <keybind key="W-S-t">
      <action name="Execute" command="rdm-snap float" />
    </keybind>

    <!-- Tile all open windows -->
    <keybind key="W-A-t">
      <action name="Execute" command="rdm-snap tile-all" />
    </keybind>

    <!-- Focus navigation (vim-style) -->
    <keybind key="W-A-h">
      <action name="Execute" command="rdm-snap focus left" />
    </keybind>
    <keybind key="W-A-l">
      <action name="Execute" command="rdm-snap focus right" />
    </keybind>
    <keybind key="W-A-k">
      <action name="Execute" command="rdm-snap focus up" />
    </keybind>
    <keybind key="W-A-j">
      <action name="Execute" command="rdm-snap focus down" />
    </keybind>

    <!-- Swap focused window with master -->
    <keybind key="W-S-m">
      <action name="Execute" command="rdm-snap swap-master" />
    </keybind>

    <!-- Grow / shrink master pane -->
    <keybind key="W-bracketright">
      <action name="Execute" command="rdm-snap grow-master" />
    </keybind>
    <keybind key="W-bracketleft">
      <action name="Execute" command="rdm-snap shrink-master" />
    </keybind>

    <!-- Reset layout -->
    <keybind key="W-S-r">
      <action name="Execute" command="rdm-snap reset" />
    </keybind>
    <!-- rdm-snap-keybinds-end -->"#;

fn rc_xml_path() -> std::path::PathBuf {
    let dir = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".config/labwc"))
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
    dir.join("rc.xml")
}

/// Returns true if snap keybinds are present in rc.xml.
pub fn are_enabled() -> bool {
    std::fs::read_to_string(rc_xml_path())
        .map(|c| c.contains(MARKER_BEGIN))
        .unwrap_or(false)
}

/// Inject snap keybinds into rc.xml (before </keyboard>).
/// Returns true if keybinds were added (false if already present).
pub fn enable() -> bool {
    let path = rc_xml_path();
    let Ok(content) = std::fs::read_to_string(&path) else { return false };

    if content.contains(MARKER_BEGIN) {
        return false; // Already present
    }

    let new = content.replace(
        "</keyboard>",
        &format!("{}\n  </keyboard>", KEYBINDS_BLOCK),
    );

    if new == content {
        eprintln!("rdm-snap: could not find </keyboard> in rc.xml");
        return false;
    }

    if std::fs::write(&path, &new).is_err() {
        return false;
    }

    let _ = Command::new("labwc").arg("--reconfigure").status();
    true
}

/// Remove snap keybinds from rc.xml.
/// Returns true if keybinds were removed (false if not present).
pub fn disable() -> bool {
    let path = rc_xml_path();
    let Ok(content) = std::fs::read_to_string(&path) else { return false };

    let Some(start) = content.find(MARKER_BEGIN) else { return false };
    let Some(end_pos) = content.find(MARKER_END) else { return false };
    let end = end_pos + MARKER_END.len();

    // Also strip the trailing newline
    let end = if content[end..].starts_with('\n') { end + 1 } else { end };

    let mut result = String::with_capacity(content.len());
    result.push_str(&content[..start]);
    result.push_str(&content[end..]);

    if std::fs::write(&path, &result).is_err() {
        return false;
    }

    let _ = Command::new("labwc").arg("--reconfigure").status();
    true
}
