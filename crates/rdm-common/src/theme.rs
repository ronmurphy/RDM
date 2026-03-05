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
    pub struct ThemeFiles {
        pub meta: &'static str,
        pub style: &'static str,
    }

    const TOKYO_NIGHT: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/tokyo-night/theme.toml"),
        style: include_str!("../themes/tokyo-night/style.css"),
    };

    const UBUNTU: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/ubuntu/theme.toml"),
        style: include_str!("../themes/ubuntu/style.css"),
    };

    const WINDOWS_10: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/windows-10/theme.toml"),
        style: include_str!("../themes/windows-10/style.css"),
    };

    const MACOS: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/macos/theme.toml"),
        style: include_str!("../themes/macos/style.css"),
    };

    const NORD: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/nord/theme.toml"),
        style: include_str!("../themes/nord/style.css"),
    };

    const CATPPUCCIN_MOCHA: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/catppuccin-mocha/theme.toml"),
        style: include_str!("../themes/catppuccin-mocha/style.css"),
    };

    const GRUVBOX_DARK: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/gruvbox-dark/theme.toml"),
        style: include_str!("../themes/gruvbox-dark/style.css"),
    };

    const DRACULA: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/dracula/theme.toml"),
        style: include_str!("../themes/dracula/style.css"),
    };

    const SOLARIZED_DARK: ThemeFiles = ThemeFiles {
        meta: include_str!("../themes/solarized-dark/theme.toml"),
        style: include_str!("../themes/solarized-dark/style.css"),
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
/// Returns the full style.css content.
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
fn load_theme_css_for(theme_name: &str) -> String {
    if let Some(css) = resolve_style(theme_name) {
        return css;
    }
    // Fallback to tokyo-night
    resolve_style("tokyo-night").unwrap_or_default()
}

/// Resolve style.css: user dir first, then built-in fallback.
fn resolve_style(theme_name: &str) -> Option<String> {
    // 1. User theme directory
    let user_path = config::config_dir()
        .join("themes")
        .join(theme_name)
        .join("style.css");
    if let Ok(contents) = std::fs::read_to_string(&user_path) {
        return Some(contents);
    }
    // 2. Built-in fallback
    builtin::get(theme_name).map(|f| f.style.to_string())
}
