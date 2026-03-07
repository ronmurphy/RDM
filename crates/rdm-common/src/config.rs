use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::display::DisplayConfig;

pub const CURRENT_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RdmConfig {
    /// Schema version for rdm.toml migration/compat checks.
    #[serde(default = "default_legacy_config_schema_version", alias = "version")]
    pub schema_version: u32,
    #[serde(default)]
    pub panel: PanelConfig,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub snap: SnapConfig,
    #[serde(default)]
    pub wallpaper: WallpaperConfig,
    #[serde(default)]
    pub menu: MenuConfig,
    #[serde(default)]
    pub appearance: AppearanceConfig,
    #[serde(default)]
    pub displays: Vec<DisplayConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PanelConfig {
    #[serde(default = "default_panel_height")]
    pub height: i32,
    #[serde(default = "default_position")]
    pub position: String,
    #[serde(default = "default_true")]
    pub show_clock: bool,
    #[serde(default = "default_true")]
    pub show_workspaces: bool,
    #[serde(default = "default_clock_format")]
    pub clock_format: String,
    /// Taskbar display mode: "text", "icons", "nerd"
    #[serde(default = "default_taskbar_mode")]
    pub taskbar_mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LauncherConfig {
    #[serde(default = "default_launcher_width")]
    pub width: i32,
    #[serde(default = "default_launcher_height")]
    pub height: i32,
    #[serde(default = "default_launcher_ui_mode")]
    pub ui_mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SnapConfig {
    #[serde(default = "default_snap_threshold")]
    pub edge_threshold: i32,
    #[serde(default = "default_true")]
    pub show_preview: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MenuConfig {
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default = "default_launcher_position")]
    pub launcher_position: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppearanceConfig {
    #[serde(default = "default_theme")]
    pub theme: String,
}

fn default_legacy_config_schema_version() -> u32 {
    // Missing field means pre-versioned config.
    0
}

fn default_config_schema_version() -> u32 {
    CURRENT_CONFIG_SCHEMA_VERSION
}

fn default_panel_height() -> i32 {
    32
}
fn default_position() -> String {
    "top".into()
}
fn default_true() -> bool {
    true
}
fn default_clock_format() -> String {
    "%H:%M  %b %d".into()
}
fn default_launcher_width() -> i32 {
    500
}
fn default_launcher_height() -> i32 {
    400
}
fn default_launcher_ui_mode() -> String {
    "winxp_classic".into()
}
fn default_snap_threshold() -> i32 {
    20
}
fn default_taskbar_mode() -> String {
    "icons".into()
}
fn default_wallpaper_path() -> String {
    String::new()
}
fn default_wallpaper_mode() -> String {
    "fill".into()
}
fn default_wallpaper_color() -> String {
    "#1a1b26".into()
}
fn default_launcher_position() -> String {
    "center".into()
}
fn default_theme() -> String {
    "tokyo-night".into()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WallpaperConfig {
    #[serde(default = "default_wallpaper_path")]
    pub path: String,
    /// Mode: fill, center, stretch, fit, tile
    #[serde(default = "default_wallpaper_mode")]
    pub mode: String,
    /// Fallback solid color
    #[serde(default = "default_wallpaper_color")]
    pub color: String,
}

impl Default for WallpaperConfig {
    fn default() -> Self {
        Self {
            path: default_wallpaper_path(),
            mode: default_wallpaper_mode(),
            color: default_wallpaper_color(),
        }
    }
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            height: default_panel_height(),
            position: default_position(),
            show_clock: true,
            show_workspaces: true,
            clock_format: default_clock_format(),
            taskbar_mode: default_taskbar_mode(),
        }
    }
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            width: default_launcher_width(),
            height: default_launcher_height(),
            ui_mode: default_launcher_ui_mode(),
        }
    }
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            edge_threshold: default_snap_threshold(),
            show_preview: true,
        }
    }
}

impl Default for MenuConfig {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            launcher_position: default_launcher_position(),
        }
    }
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
        }
    }
}

impl Default for RdmConfig {
    fn default() -> Self {
        Self {
            schema_version: default_config_schema_version(),
            panel: PanelConfig::default(),
            launcher: LauncherConfig::default(),
            snap: SnapConfig::default(),
            wallpaper: WallpaperConfig::default(),
            menu: MenuConfig::default(),
            appearance: AppearanceConfig::default(),
            displays: Vec::new(),
        }
    }
}

impl RdmConfig {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let mut cfg: Self = toml::from_str(&contents).unwrap_or_default();

                if cfg.schema_version > CURRENT_CONFIG_SCHEMA_VERSION {
                    log::warn!(
                        "Config schema version {} is newer than supported {}; loading with best effort",
                        cfg.schema_version,
                        CURRENT_CONFIG_SCHEMA_VERSION
                    );
                } else if cfg.schema_version < CURRENT_CONFIG_SCHEMA_VERSION {
                    cfg = migrate_config(cfg);
                }

                cfg
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let dir = config_dir();
        std::fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(config_path(), contents)?;
        Ok(())
    }
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("rdm")
}

pub fn config_path() -> PathBuf {
    config_dir().join("rdm.toml")
}

fn migrate_config(mut cfg: RdmConfig) -> RdmConfig {
    let start = cfg.schema_version;
    while cfg.schema_version < CURRENT_CONFIG_SCHEMA_VERSION {
        cfg = match cfg.schema_version {
            0 => migrate_config_v0_to_v1(cfg),
            v => {
                log::warn!(
                    "No migration step defined for config schema {}; forcing {}",
                    v,
                    CURRENT_CONFIG_SCHEMA_VERSION
                );
                let mut forced = cfg;
                forced.schema_version = CURRENT_CONFIG_SCHEMA_VERSION;
                forced
            }
        };
    }

    log::info!(
        "Upgraded config schema from {} to {}",
        start,
        cfg.schema_version
    );
    cfg
}

fn migrate_config_v0_to_v1(mut cfg: RdmConfig) -> RdmConfig {
    // v1 introduces explicit schema_version while preserving legacy semantics.
    cfg.schema_version = 1;
    cfg
}

#[allow(dead_code)]
fn migrate_config_v1_to_v2(mut cfg: RdmConfig) -> RdmConfig {
    // Reserved for future schema bump.
    cfg.schema_version = 2;
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_without_schema_migrates() {
        let toml = r#"
[appearance]
theme = "tokyo-night"

[panel]
height = 40
"#;
        let cfg: RdmConfig = toml::from_str(toml).expect("parse legacy config");
        assert_eq!(cfg.schema_version, 0);

        let migrated = migrate_config(cfg);
        assert_eq!(migrated.schema_version, CURRENT_CONFIG_SCHEMA_VERSION);
        assert_eq!(migrated.panel.height, 40);
        assert_eq!(migrated.appearance.theme, "tokyo-night");
    }

    #[test]
    fn config_roundtrip_includes_schema_version() {
        let cfg = RdmConfig::default();
        let encoded = toml::to_string_pretty(&cfg).expect("serialize config");
        assert!(encoded.contains("schema_version = 1"));
    }
}
