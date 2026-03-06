use serde::{Deserialize, Serialize};

use crate::config;

// ─── Types ───────────────────────────────────────────────────────

/// Metadata about an available theme
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ThemeMeta {
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
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
                    themes.push(meta);
                }
            }
        }
    }

    themes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    themes
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
        std::fs::read_to_string(&user_shared)
            .unwrap_or_else(|_| builtin::SHARED_STYLE.to_string())
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
