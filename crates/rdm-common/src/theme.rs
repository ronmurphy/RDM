use serde::{Deserialize, Serialize};

use crate::config;
pub const CURRENT_THEME_META_SCHEMA_VERSION: u32 = 1;
pub const CURRENT_LAYOUT_SCHEMA_VERSION: u32 = 1;

// ─── Types ───────────────────────────────────────────────────────

/// Metadata about an available theme
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThemeMeta {
    #[serde(
        default = "default_legacy_theme_meta_schema_version",
        alias = "version"
    )]
    pub schema_version: u32,
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
}

/// Theme-scoped layout profile.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThemeLayout {
    #[serde(default = "default_legacy_layout_schema_version", alias = "version")]
    pub schema_version: u32,
    #[serde(default)]
    pub panel: PanelLayout,
    #[serde(default)]
    pub launcher: LauncherLayout,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PanelLayout {
    #[serde(default = "default_left")]
    pub launcher: String,
    #[serde(default = "default_center")]
    pub taskbar: String,
    #[serde(default = "default_right")]
    pub clock: String,
    #[serde(default = "default_right")]
    pub sys_popup: String,
    #[serde(default = "default_right")]
    pub tray: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LauncherLayout {
    #[serde(default = "default_right")]
    pub favorites_side: String,
    #[serde(default = "default_left")]
    pub settings_side: String,
}

fn default_left() -> String {
    "left".to_string()
}
fn default_legacy_theme_meta_schema_version() -> u32 {
    0
}
fn default_legacy_layout_schema_version() -> u32 {
    0
}
fn default_theme_meta_schema_version() -> u32 {
    CURRENT_THEME_META_SCHEMA_VERSION
}
fn default_layout_schema_version() -> u32 {
    CURRENT_LAYOUT_SCHEMA_VERSION
}
fn default_center() -> String {
    "center".to_string()
}
fn default_right() -> String {
    "right".to_string()
}

impl Default for ThemeLayout {
    fn default() -> Self {
        Self {
            schema_version: default_layout_schema_version(),
            panel: PanelLayout::default(),
            launcher: LauncherLayout::default(),
        }
    }
}

impl Default for PanelLayout {
    fn default() -> Self {
        Self {
            launcher: default_left(),
            taskbar: default_center(),
            clock: default_right(),
            sys_popup: default_right(),
            tray: default_right(),
        }
    }
}

impl Default for LauncherLayout {
    fn default() -> Self {
        Self {
            favorites_side: default_right(),
            settings_side: default_left(),
        }
    }
}

// ─── Built-in themes ─────────────────────────────────────────────

mod builtin {
    /// Per-theme files: colors + overrides.
    /// The shared style.css lives outside theme folders.
    pub struct ThemeFiles {
        pub meta: &'static str,
        pub colors: &'static str,
        pub overrides: &'static str,
    }

    /// Shared structural CSS loaded between colors and overrides.
    pub const SHARED_STYLE: &str = include_str!("../themes/style.css");

    const TOKYO_NIGHT: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/tokyo-night/theme.toml"),
        colors: include_str!("../themes/tokyo-night/colors.css"),
        overrides: include_str!("../themes/tokyo-night/overrides.css"),
    };

    const UBUNTU: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/ubuntu/theme.toml"),
        colors: include_str!("../themes/ubuntu/colors.css"),
        overrides: include_str!("../themes/ubuntu/overrides.css"),
    };

    const WINDOWS_10: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/windows-10/theme.toml"),
        colors: include_str!("../themes/windows-10/colors.css"),
        overrides: include_str!("../themes/windows-10/overrides.css"),
    };

    const MACOS: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/macos/theme.toml"),
        colors: include_str!("../themes/macos/colors.css"),
        overrides: include_str!("../themes/macos/overrides.css"),
    };

    const NORD: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/nord/theme.toml"),
        colors: include_str!("../themes/nord/colors.css"),
        overrides: include_str!("../themes/nord/overrides.css"),
    };

    const CATPPUCCIN_MOCHA: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/catppuccin-mocha/theme.toml"),
        colors: include_str!("../themes/catppuccin-mocha/colors.css"),
        overrides: include_str!("../themes/catppuccin-mocha/overrides.css"),
    };

    const GRUVBOX_DARK: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/gruvbox-dark/theme.toml"),
        colors: include_str!("../themes/gruvbox-dark/colors.css"),
        overrides: include_str!("../themes/gruvbox-dark/overrides.css"),
    };

    const DRACULA: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/dracula/theme.toml"),
        colors: include_str!("../themes/dracula/colors.css"),
        overrides: include_str!("../themes/dracula/overrides.css"),
    };

    const SOLARIZED_DARK: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/solarized-dark/theme.toml"),
        colors: include_str!("../themes/solarized-dark/colors.css"),
        overrides: include_str!("../themes/solarized-dark/overrides.css"),
    };

    pub const ALL: &[(&str, &ThemeFiles)] = &[
        ("tokyo-night", &TOKYO_NIGHT),
        ("ubuntu", &UBUNTU),
        ("windows-10", &WINDOWS_10),
        ("macos", &MACOS),
        ("nord", &NORD),
        ("catppuccin-mocha", &CATPPUCCIN_MOCHA),
        ("gruvbox-dark", &GRUVBOX_DARK),
        ("dracula", &DRACULA),
        ("solarized-dark", &SOLARIZED_DARK),
    ];

    pub fn get(theme: &str) -> Option<&'static ThemeFiles> {
        ALL.iter()
            .find(|(name, _)| *name == theme)
            .map(|(_, files)| *files)
    }

    pub fn list_names() -> impl Iterator<Item = &'static str> {
        ALL.iter().map(|(name, _)| *name)
    }
}

// ─── Public API ──────────────────────────────────────────────────

/// Load the complete CSS for the active theme.
///
/// Cascades in order: **colors → shared style → overrides**
/// so that theme overrides always win.
///
/// Falls back to "tokyo-night" if the configured theme is not found.
pub fn load_theme_css() -> String {
    let theme = config::RdmConfig::load().appearance.theme;
    load_theme_css_for(&theme)
}

/// Load layout for the active theme, or defaults if none exists.
pub fn load_active_theme_layout() -> ThemeLayout {
    let theme = config::RdmConfig::load().appearance.theme;
    load_theme_layout_for(&theme)
}

/// List all available themes (built-in + user), deduplicated by name.
pub fn list_themes() -> Vec<ThemeMeta> {
    let mut themes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // User themes first (they override built-in by name)
    let user_dir = config::config_dir().join("themes");
    if let Ok(entries) = std::fs::read_dir(&user_dir) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                let name = entry.file_name().to_string_lossy().to_string();
                let meta_path = entry.path().join("theme.toml");
                if let Ok(contents) = std::fs::read_to_string(&meta_path) {
                    if let Ok(meta) = toml::from_str::<ThemeMeta>(&contents) {
                        let meta = migrate_theme_meta(meta, &name);
                        seen.insert(name);
                        themes.push(meta);
                    }
                }
            }
        }
    }

    // Built-in themes (skip if user already provides same name)
    for name in builtin::list_names() {
        if !seen.contains(name) {
            if let Some(files) = builtin::get(name) {
                if let Ok(meta) = toml::from_str::<ThemeMeta>(files.meta) {
                    themes.push(migrate_theme_meta(meta, name));
                }
            }
        }
    }

    themes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    themes
}

/// Load layout profile for a specific theme.
pub fn load_theme_layout_for(theme_name: &str) -> ThemeLayout {
    let user_dir = config::config_dir().join("themes").join(theme_name);
    let path = user_dir.join("layout.toml");
    let layout = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str::<ThemeLayout>(&s).ok())
        .unwrap_or_default();
    migrate_theme_layout(layout, theme_name)
}

// ─── Internal ────────────────────────────────────────────────────

/// Load CSS for a specific theme name.
/// Assembles: colors.css + style.css + overrides.css
fn load_theme_css_for(theme_name: &str) -> String {
    if let Some(css) = assemble_theme(theme_name) {
        return css;
    }
    // Fallback to tokyo-night
    assemble_theme("tokyo-night").unwrap_or_default()
}

/// Assemble the three CSS layers for a theme.
///
/// Load order:
///   1. `<theme>/colors.css`   — @define-color palette
///   2. `themes/style.css`     — shared structural rules
///   3. `<theme>/overrides.css` — optional per-theme tweaks
///
/// For user themes, looks in `~/.config/rdm/themes/<name>/`.
/// Falls back to built-in if user files are missing.
fn assemble_theme(theme_name: &str) -> Option<String> {
    let user_dir = config::config_dir().join("themes").join(theme_name);

    // ── 1. Colors ────────────────────────────────────────────
    let colors = try_read(&user_dir, "colors.css")
        .or_else(|| builtin::get(theme_name).map(|f| f.colors.to_string()))?;

    // ── 2. Shared style ──────────────────────────────────────
    // User can override the shared style by placing style.css
    // in the theme root (~/.config/rdm/themes/style.css).
    let shared_style = {
        let user_shared = config::config_dir().join("themes").join("style.css");
        std::fs::read_to_string(&user_shared).unwrap_or_else(|_| builtin::SHARED_STYLE.to_string())
    };

    // ── 3. Overrides ─────────────────────────────────────────
    let overrides = try_read(&user_dir, "overrides.css")
        .or_else(|| builtin::get(theme_name).map(|f| f.overrides.to_string()))
        .unwrap_or_default();

    Some(format!("{}\n{}\n{}", colors, shared_style, overrides))
}

/// Try reading a file from a user theme directory.
fn try_read(dir: &std::path::Path, filename: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(filename)).ok()
}

// ─── Theme Editor helpers ────────────────────────────────────────

/// A single `@define-color name #hex;` entry parsed from colors.css.
#[derive(Debug, Clone)]
pub struct ThemeColor {
    pub var_name: String,
    pub value: String, // hex string, e.g. "#1a1b26"
}

/// Load the color palette for a given theme name.
///
/// Returns parsed `@define-color` entries from colors.css.
/// Tries user dir first, then built-in.
pub fn load_theme_colors(theme_name: &str) -> Vec<ThemeColor> {
    let user_dir = config::config_dir().join("themes").join(theme_name);
    let css = try_read(&user_dir, "colors.css")
        .or_else(|| builtin::get(theme_name).map(|f| f.colors.to_string()))
        .unwrap_or_default();
    parse_colors_css(&css)
}

/// Parse `@define-color` lines from raw CSS text.
fn parse_colors_css(css: &str) -> Vec<ThemeColor> {
    let mut colors = Vec::new();
    for line in css.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("@define-color ") {
            // Format: "name value;"
            let rest = rest.trim_end_matches(';').trim();
            if let Some(idx) = rest.find(char::is_whitespace) {
                let var_name = rest[..idx].to_string();
                let value = rest[idx..].trim().to_string();
                // Only include concrete hex colors (skip @references)
                if value.starts_with('#') {
                    colors.push(ThemeColor { var_name, value });
                }
            }
        }
    }
    colors
}

/// Generate colors.css content from a list of ThemeColor entries.
pub fn serialize_colors_css(colors: &[ThemeColor], comment: &str) -> String {
    let mut out = format!("/* {} */\n", comment);
    for c in colors {
        out.push_str(&format!("@define-color {} {};\n", c.var_name, c.value));
    }
    out
}

/// Save a user theme to `~/.config/rdm/themes/<slug>/`.
///
/// Creates the directory, writes theme.toml and colors.css.
/// The theme will use the shared style.css and empty overrides.
pub fn save_user_theme(
    slug: &str,
    display_name: &str,
    colors: &[ThemeColor],
    layout: Option<&ThemeLayout>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = config::config_dir().join("themes").join(slug);
    std::fs::create_dir_all(&dir)?;

    // theme.toml
    let meta = ThemeMeta {
        schema_version: default_theme_meta_schema_version(),
        name: slug.to_string(),
        display_name: display_name.to_string(),
        author: "User".to_string(),
        description: format!("Custom theme based on user edits"),
    };
    let toml_str = toml::to_string_pretty(&meta)?;
    std::fs::write(dir.join("theme.toml"), toml_str)?;

    // colors.css
    let css = serialize_colors_css(colors, &format!("{} — Color Palette", display_name));
    std::fs::write(dir.join("colors.css"), css)?;

    // overrides.css (empty if doesn't exist)
    let overrides_path = dir.join("overrides.css");
    if !overrides_path.exists() {
        std::fs::write(&overrides_path, "/* Custom overrides */\n")?;
    }

    // layout.toml
    let layout = layout.cloned().unwrap_or_default();
    let layout_toml = toml::to_string_pretty(&layout)?;
    std::fs::write(dir.join("layout.toml"), layout_toml)?;

    Ok(())
}

/// Get theme names as slugs (for the base-theme selector).
pub fn list_theme_slugs() -> Vec<(String, String)> {
    list_themes()
        .into_iter()
        .map(|t| (t.name, t.display_name))
        .collect()
}

fn migrate_theme_meta(mut meta: ThemeMeta, theme_name: &str) -> ThemeMeta {
    if meta.schema_version > CURRENT_THEME_META_SCHEMA_VERSION {
        log::warn!(
            "Theme meta '{}' schema {} is newer than supported {}; loading with best effort",
            theme_name,
            meta.schema_version,
            CURRENT_THEME_META_SCHEMA_VERSION
        );
        return meta;
    }

    let start = meta.schema_version;
    while meta.schema_version < CURRENT_THEME_META_SCHEMA_VERSION {
        meta = match meta.schema_version {
            0 => migrate_theme_meta_v0_to_v1(meta),
            v => {
                log::warn!(
                    "No migration step defined for theme meta '{}' schema {}; forcing {}",
                    theme_name,
                    v,
                    CURRENT_THEME_META_SCHEMA_VERSION
                );
                let mut forced = meta;
                forced.schema_version = CURRENT_THEME_META_SCHEMA_VERSION;
                forced
            }
        };
    }

    if start != meta.schema_version {
        log::info!(
            "Upgraded theme meta '{}' schema {} -> {}",
            theme_name,
            start,
            meta.schema_version
        );
    }
    meta
}

fn migrate_theme_layout(mut layout: ThemeLayout, theme_name: &str) -> ThemeLayout {
    if layout.schema_version > CURRENT_LAYOUT_SCHEMA_VERSION {
        log::warn!(
            "Theme layout '{}' schema {} is newer than supported {}; loading with best effort",
            theme_name,
            layout.schema_version,
            CURRENT_LAYOUT_SCHEMA_VERSION
        );
        return layout;
    }

    let start = layout.schema_version;
    while layout.schema_version < CURRENT_LAYOUT_SCHEMA_VERSION {
        layout = match layout.schema_version {
            0 => migrate_layout_v0_to_v1(layout),
            v => {
                log::warn!(
                    "No migration step defined for theme layout '{}' schema {}; forcing {}",
                    theme_name,
                    v,
                    CURRENT_LAYOUT_SCHEMA_VERSION
                );
                let mut forced = layout;
                forced.schema_version = CURRENT_LAYOUT_SCHEMA_VERSION;
                forced
            }
        };
    }

    if start != layout.schema_version {
        log::info!(
            "Upgraded theme layout '{}' schema {} -> {}",
            theme_name,
            start,
            layout.schema_version
        );
    }
    layout
}

fn migrate_theme_meta_v0_to_v1(mut meta: ThemeMeta) -> ThemeMeta {
    meta.schema_version = 1;
    meta
}

fn migrate_layout_v0_to_v1(mut layout: ThemeLayout) -> ThemeLayout {
    layout.schema_version = 1;
    layout
}

#[allow(dead_code)]
fn migrate_theme_meta_v1_to_v2(mut meta: ThemeMeta) -> ThemeMeta {
    // Reserved for future schema bump.
    meta.schema_version = 2;
    meta
}

#[allow(dead_code)]
fn migrate_layout_v1_to_v2(mut layout: ThemeLayout) -> ThemeLayout {
    // Reserved for future schema bump.
    layout.schema_version = 2;
    layout
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct XdgConfigGuard {
        previous: Option<std::ffi::OsString>,
        root: std::path::PathBuf,
    }

    impl Drop for XdgConfigGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => {
                    // SAFETY: test process uses a global mutex to serialize env var mutation.
                    unsafe { std::env::set_var("XDG_CONFIG_HOME", v) };
                }
                None => {
                    // SAFETY: test process uses a global mutex to serialize env var mutation.
                    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
                }
            }
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn with_temp_xdg_config_home() -> XdgConfigGuard {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("rdm-common-test-{}", nanos));
        fs::create_dir_all(&root).expect("create temp root");
        let previous = std::env::var_os("XDG_CONFIG_HOME");
        // SAFETY: test process uses a global mutex to serialize env var mutation.
        unsafe { std::env::set_var("XDG_CONFIG_HOME", &root) };
        XdgConfigGuard { previous, root }
    }

    #[test]
    fn legacy_layout_without_schema_migrates() {
        let toml = r#"
[panel]
launcher = "right"
taskbar = "center"
clock = "left"
tray = "right"
"#;
        let parsed: ThemeLayout = toml::from_str(toml).expect("parse legacy layout");
        assert_eq!(parsed.schema_version, 0);

        let migrated = migrate_theme_layout(parsed, "test-theme");
        assert_eq!(migrated.schema_version, CURRENT_LAYOUT_SCHEMA_VERSION);
        assert_eq!(migrated.panel.launcher, "right");
        assert_eq!(migrated.panel.clock, "left");
        assert_eq!(migrated.panel.sys_popup, "right");
    }

    #[test]
    fn save_and_load_theme_layout_roundtrip() {
        let _env_guard = ENV_LOCK.lock().expect("env lock");
        let _xdg = with_temp_xdg_config_home();

        let slug = "smoke-layout";
        let layout = ThemeLayout {
            schema_version: CURRENT_LAYOUT_SCHEMA_VERSION,
            panel: PanelLayout {
                launcher: "right".to_string(),
                taskbar: "left".to_string(),
                clock: "center".to_string(),
                sys_popup: "left".to_string(),
                tray: "right".to_string(),
            },
            launcher: LauncherLayout {
                favorites_side: "left".to_string(),
                settings_side: "right".to_string(),
            },
        };
        let colors = vec![
            ThemeColor {
                var_name: "theme_bg".to_string(),
                value: "#101010".to_string(),
            },
            ThemeColor {
                var_name: "theme_fg".to_string(),
                value: "#f0f0f0".to_string(),
            },
        ];

        save_user_theme(slug, "Smoke Layout", &colors, Some(&layout)).expect("save theme");
        let loaded = load_theme_layout_for(slug);

        assert_eq!(loaded.schema_version, CURRENT_LAYOUT_SCHEMA_VERSION);
        assert_eq!(loaded.panel.launcher, "right");
        assert_eq!(loaded.panel.taskbar, "left");
        assert_eq!(loaded.panel.clock, "center");
        assert_eq!(loaded.panel.sys_popup, "left");
        assert_eq!(loaded.launcher.favorites_side, "left");
        assert_eq!(loaded.launcher.settings_side, "right");
    }
}
