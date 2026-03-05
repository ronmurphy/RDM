use serde::{Deserialize, Serialize};

/// A display mode reported by wlr-randr (runtime only, not persisted)
#[derive(Debug, Clone)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh: f64,
    pub preferred: bool,
    pub current: bool,
}

/// Live information about a connected output, parsed from wlr-randr
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub modes: Vec<DisplayMode>,
    pub position: (i32, i32),
    pub transform: String,
    pub scale: f64,
}

/// Persistent per-display configuration stored in rdm.toml under [[displays]]
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DisplayConfig {
    pub name: String,
    #[serde(default = "default_display_enabled")]
    pub enabled: bool,
    /// Format: "WIDTHxHEIGHT@RATE", e.g. "1920x1080@60". Empty = auto.
    #[serde(default)]
    pub mode: String,
    /// Format: "X,Y", e.g. "0,0". Empty = auto.
    #[serde(default)]
    pub position: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default = "default_transform")]
    pub transform: String,
}

fn default_display_enabled() -> bool {
    true
}
fn default_scale() -> f64 {
    1.0
}
fn default_transform() -> String {
    "normal".into()
}

/// Query connected displays by running `wlr-randr` and parsing its output.
pub fn query_displays() -> Result<Vec<DisplayInfo>, String> {
    let output = std::process::Command::new("wlr-randr")
        .output()
        .map_err(|e| format!("Failed to run wlr-randr: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("wlr-randr failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_wlr_randr_output(&stdout))
}

/// Parse wlr-randr text output into DisplayInfo structs.
///
/// Expected format:
/// ```text
/// DP-1 "Dell Inc. DELL P2419H ABC123" (DP-1)
///   Enabled: yes
///   Modes:
///     1920x1080 px, 60.000000 Hz (preferred, current)
///     1920x1080 px, 59.940000 Hz
///   Position: 0,0
///   Transform: normal
///   Scale: 1.000000
///   Adaptive Sync: disabled
/// ```
pub fn parse_wlr_randr_output(text: &str) -> Vec<DisplayInfo> {
    let mut displays = Vec::new();
    let mut current: Option<DisplayInfo> = None;
    let mut in_modes = false;

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }

        // No indent = new output header
        if !line.starts_with(' ') {
            // Push previous display if any
            if let Some(display) = current.take() {
                displays.push(display);
            }
            in_modes = false;

            // Parse output name (first word) and description (in quotes)
            let name = line.split_whitespace().next().unwrap_or("").to_string();
            let description = line
                .find('"')
                .and_then(|start| {
                    line[start + 1..]
                        .find('"')
                        .map(|end| line[start + 1..start + 1 + end].to_string())
                })
                .unwrap_or_default();

            current = Some(DisplayInfo {
                name,
                description,
                enabled: true,
                modes: Vec::new(),
                position: (0, 0),
                transform: "normal".to_string(),
                scale: 1.0,
            });
            continue;
        }

        let trimmed = line.trim();

        // Four-space indent (mode entry) while we're in modes section
        if in_modes && line.starts_with("    ") {
            if let Some(ref mut display) = current {
                if let Some(mode) = parse_mode_line(trimmed) {
                    display.modes.push(mode);
                }
            }
            continue;
        }

        // Two-space indent = property line
        if let Some(ref mut display) = current {
            if trimmed == "Modes:" {
                in_modes = true;
            } else if let Some(val) = trimmed.strip_prefix("Enabled: ") {
                display.enabled = val.trim() == "yes";
                in_modes = false;
            } else if let Some(val) = trimmed.strip_prefix("Position: ") {
                in_modes = false;
                let parts: Vec<&str> = val.split(',').collect();
                if parts.len() == 2 {
                    display.position = (
                        parts[0].trim().parse().unwrap_or(0),
                        parts[1].trim().parse().unwrap_or(0),
                    );
                }
            } else if let Some(val) = trimmed.strip_prefix("Transform: ") {
                in_modes = false;
                display.transform = val.trim().to_string();
            } else if let Some(val) = trimmed.strip_prefix("Scale: ") {
                in_modes = false;
                display.scale = val.trim().parse().unwrap_or(1.0);
            } else if trimmed.starts_with("Adaptive Sync:") {
                in_modes = false;
            }
        }
    }

    // Push last display
    if let Some(display) = current {
        displays.push(display);
    }

    displays
}

/// Parse a single mode line like "1920x1080 px, 60.000000 Hz (preferred, current)"
fn parse_mode_line(line: &str) -> Option<DisplayMode> {
    // Split on " px, " to separate resolution from refresh
    let (res_part, rest) = line.split_once(" px, ")?;

    // Parse resolution: "1920x1080"
    let (w_str, h_str) = res_part.split_once('x')?;
    let width: u32 = w_str.trim().parse().ok()?;
    let height: u32 = h_str.trim().parse().ok()?;

    // Parse refresh: "60.000000 Hz (preferred, current)" or "59.940000 Hz"
    let hz_end = rest.find(" Hz")?;
    let refresh: f64 = rest[..hz_end].trim().parse().ok()?;

    let flags = rest.get(hz_end + 3..).unwrap_or("");
    let preferred = flags.contains("preferred");
    let current = flags.contains("current");

    Some(DisplayMode {
        width,
        height,
        refresh,
        preferred,
        current,
    })
}

/// Apply a set of display configs by running wlr-randr commands.
pub fn apply_display_config(configs: &[DisplayConfig]) -> Result<(), String> {
    for config in configs {
        let mut args = vec!["--output".to_string(), config.name.clone()];

        if !config.enabled {
            args.push("--off".to_string());
        } else {
            args.push("--on".to_string());

            if !config.mode.is_empty() {
                args.push("--mode".to_string());
                args.push(config.mode.clone());
            }
            if !config.position.is_empty() {
                args.push("--pos".to_string());
                args.push(config.position.clone());
            }
            args.push("--scale".to_string());
            args.push(config.scale.to_string());
            args.push("--transform".to_string());
            args.push(config.transform.clone());
        }

        log::info!("wlr-randr {}", args.join(" "));

        let output = std::process::Command::new("wlr-randr")
            .args(&args)
            .output()
            .map_err(|e| format!("Failed to run wlr-randr: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "wlr-randr failed for {}: {}",
                config.name,
                stderr.trim()
            ));
        }
    }
    Ok(())
}

/// Merge detected displays with saved config.
/// Displays not in saved config get defaults from current state.
/// Saved configs for disconnected displays are dropped.
pub fn merge_with_saved(detected: &[DisplayInfo], saved: &[DisplayConfig]) -> Vec<DisplayConfig> {
    detected
        .iter()
        .map(|info| {
            if let Some(saved) = saved.iter().find(|c| c.name == info.name) {
                saved.clone()
            } else {
                DisplayConfig {
                    name: info.name.clone(),
                    enabled: info.enabled,
                    mode: info
                        .modes
                        .iter()
                        .find(|m| m.current)
                        .map(|m| format!("{}x{}@{:.0}", m.width, m.height, m.refresh))
                        .unwrap_or_default(),
                    position: format!("{},{}", info.position.0, info.position.1),
                    scale: info.scale,
                    transform: info.transform.clone(),
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_display() {
        let input = r#"DP-1 "Dell Inc. DELL P2419H ABC123" (DP-1)
  Enabled: yes
  Modes:
    1920x1080 px, 60.000000 Hz (preferred, current)
    1920x1080 px, 59.940000 Hz
    1680x1050 px, 59.954000 Hz
  Position: 0,0
  Transform: normal
  Scale: 1.000000
  Adaptive Sync: disabled
"#;
        let displays = parse_wlr_randr_output(input);
        assert_eq!(displays.len(), 1);

        let d = &displays[0];
        assert_eq!(d.name, "DP-1");
        assert_eq!(d.description, "Dell Inc. DELL P2419H ABC123");
        assert!(d.enabled);
        assert_eq!(d.modes.len(), 3);
        assert_eq!(d.position, (0, 0));
        assert_eq!(d.transform, "normal");
        assert!((d.scale - 1.0).abs() < f64::EPSILON);

        assert_eq!(d.modes[0].width, 1920);
        assert_eq!(d.modes[0].height, 1080);
        assert!((d.modes[0].refresh - 60.0).abs() < 0.001);
        assert!(d.modes[0].preferred);
        assert!(d.modes[0].current);

        assert!(!d.modes[1].preferred);
        assert!(!d.modes[1].current);
    }

    #[test]
    fn test_parse_multiple_displays() {
        let input = r#"DP-1 "Dell Inc. P2419H" (DP-1)
  Enabled: yes
  Modes:
    1920x1080 px, 60.000000 Hz (preferred, current)
  Position: 0,0
  Transform: normal
  Scale: 1.000000
  Adaptive Sync: disabled
HDMI-A-1 "Samsung Electric Company C27F390" (HDMI-A-1)
  Enabled: yes
  Modes:
    1920x1080 px, 60.000000 Hz (preferred, current)
    1920x1080 px, 50.000000 Hz
  Position: 1920,0
  Transform: normal
  Scale: 1.000000
  Adaptive Sync: disabled
"#;
        let displays = parse_wlr_randr_output(input);
        assert_eq!(displays.len(), 2);

        assert_eq!(displays[0].name, "DP-1");
        assert_eq!(displays[1].name, "HDMI-A-1");
        assert_eq!(displays[1].position, (1920, 0));
        assert_eq!(displays[1].modes.len(), 2);
    }

    #[test]
    fn test_parse_disabled_display() {
        let input = r#"DP-2 "LG Display" (DP-2)
  Enabled: no
  Modes:
    2560x1440 px, 144.000000 Hz (preferred)
    2560x1440 px, 60.000000 Hz
  Position: 0,0
  Transform: normal
  Scale: 1.000000
  Adaptive Sync: disabled
"#;
        let displays = parse_wlr_randr_output(input);
        assert_eq!(displays.len(), 1);
        assert!(!displays[0].enabled);
        assert_eq!(displays[0].modes.len(), 2);
        assert!(displays[0].modes[0].preferred);
        assert!(!displays[0].modes[0].current);
    }

    #[test]
    fn test_merge_with_saved() {
        let detected = vec![
            DisplayInfo {
                name: "DP-1".into(),
                description: "Dell".into(),
                enabled: true,
                modes: vec![DisplayMode {
                    width: 1920,
                    height: 1080,
                    refresh: 60.0,
                    preferred: true,
                    current: true,
                }],
                position: (0, 0),
                transform: "normal".into(),
                scale: 1.0,
            },
            DisplayInfo {
                name: "HDMI-A-1".into(),
                description: "Samsung".into(),
                enabled: true,
                modes: vec![DisplayMode {
                    width: 2560,
                    height: 1440,
                    refresh: 144.0,
                    preferred: true,
                    current: true,
                }],
                position: (1920, 0),
                transform: "normal".into(),
                scale: 1.0,
            },
        ];

        let saved = vec![DisplayConfig {
            name: "DP-1".into(),
            enabled: true,
            mode: "1920x1080@60".into(),
            position: "0,0".into(),
            scale: 1.5,
            transform: "normal".into(),
        }];

        let merged = merge_with_saved(&detected, &saved);
        assert_eq!(merged.len(), 2);

        // DP-1 uses saved config (scale 1.5)
        assert_eq!(merged[0].name, "DP-1");
        assert!((merged[0].scale - 1.5).abs() < f64::EPSILON);

        // HDMI-A-1 gets defaults from detected state
        assert_eq!(merged[1].name, "HDMI-A-1");
        assert_eq!(merged[1].mode, "2560x1440@144");
        assert_eq!(merged[1].position, "1920,0");
    }
}
