use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RdmConfig {
    #[serde(default)]
    pub panel: PanelConfig,
    #[serde(default)]
    pub launcher: LauncherConfig,
    #[serde(default)]
    pub snap: SnapConfig,
    #[serde(default)]
    pub wallpaper: WallpaperConfig,
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
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SnapConfig {
    #[serde(default = "default_snap_threshold")]
    pub edge_threshold: i32,
    #[serde(default = "default_true")]
    pub show_preview: bool,
}

fn default_panel_height() -> i32 { 32 }
fn default_position() -> String { "top".into() }
fn default_true() -> bool { true }
fn default_clock_format() -> String { "%H:%M  %b %d".into() }
fn default_launcher_width() -> i32 { 500 }
fn default_launcher_height() -> i32 { 400 }
fn default_snap_threshold() -> i32 { 20 }
fn default_taskbar_mode() -> String { "icons".into() }
fn default_wallpaper_path() -> String { String::new() }
fn default_wallpaper_mode() -> String { "fill".into() }
fn default_wallpaper_color() -> String { "#1a1b26".into() }

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

impl Default for RdmConfig {
    fn default() -> Self {
        Self {
            panel: PanelConfig::default(),
            launcher: LauncherConfig::default(),
            snap: SnapConfig::default(),
            wallpaper: WallpaperConfig::default(),
        }
    }
}

impl RdmConfig {
    pub fn load() -> Self {
        let path = config_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
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
