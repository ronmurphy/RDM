use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub categories: Vec<String>,
}

pub fn load_desktop_entries() -> Vec<AppEntry> {
    let mut entries = Vec::new();
    let dirs = desktop_dirs();

    for dir in dirs {
        if let Ok(read_dir) = std::fs::read_dir(&dir) {
            for file in read_dir.flatten() {
                let path = file.path();
                if path.extension().and_then(|e| e.to_str()) == Some("desktop") {
                    if let Some(entry) = parse_desktop_file(&path) {
                        // Dedupe by name
                        if !entries.iter().any(|e: &AppEntry| e.name == entry.name) {
                            entries.push(entry);
                        }
                    }
                }
            }
        }
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    entries
}

fn desktop_dirs() -> Vec<PathBuf> {
    let mut search_dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];

    if let Ok(home) = std::env::var("HOME") {
        search_dirs.push(PathBuf::from(home).join(".local/share/applications"));
    }

    // XDG_DATA_DIRS
    if let Ok(xdg) = std::env::var("XDG_DATA_DIRS") {
        for dir in xdg.split(':') {
            let p = PathBuf::from(dir).join("applications");
            if !search_dirs.contains(&p) {
                search_dirs.push(p);
            }
        }
    }

    search_dirs
}

fn parse_desktop_file(path: &PathBuf) -> Option<AppEntry> {
    let content = std::fs::read_to_string(path).ok()?;

    let mut name = None;
    let mut exec = None;
    let mut comment = None;
    let mut icon = None;
    let mut categories = Vec::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut in_desktop_entry = false;

    for line in content.lines() {
        let line = line.trim();

        if line == "[Desktop Entry]" {
            in_desktop_entry = true;
            continue;
        }
        if line.starts_with('[') && line != "[Desktop Entry]" {
            if in_desktop_entry {
                break; // Done with [Desktop Entry] section
            }
            continue;
        }

        if !in_desktop_entry {
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "Name" => name = Some(value.to_string()),
                "Exec" => exec = Some(value.to_string()),
                "Comment" => comment = Some(value.to_string()),
                "Icon" => icon = Some(value.to_string()),
                "Categories" => {
                    categories = value
                        .split(';')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string())
                        .collect();
                }
                "NoDisplay" => no_display = value == "true",
                "Hidden" => hidden = value == "true",
                _ => {}
            }
        }
    }

    if no_display || hidden {
        return None;
    }

    Some(AppEntry {
        name: name?,
        exec: exec?,
        comment,
        icon,
        categories,
    })
}

/// Map freedesktop categories to a display group name.
pub fn map_category(categories: &[String]) -> &'static str {
    for cat in categories {
        match cat.as_str() {
            "AudioVideo" | "Audio" | "Video" | "Music" | "Player" | "Recorder" => return "Media",
            "Development" | "IDE" | "TextEditor" | "Debugger" | "WebDevelopment" => {
                return "Development"
            }
            "Game" | "ActionGame" | "AdventureGame" | "ArcadeGame" | "BoardGame" | "BlocksGame"
            | "CardGame" | "LogicGame" | "RolePlaying" | "Simulation" | "SportsGame"
            | "StrategyGame" => return "Games",
            "Graphics" | "Photography" | "2DGraphics" | "3DGraphics" | "RasterGraphics"
            | "VectorGraphics" | "Scanning" => return "Graphics",
            "Network" | "WebBrowser" | "Email" | "Chat" | "InstantMessaging" | "IRCClient"
            | "P2P" | "RemoteAccess" => return "Internet",
            "Office" | "WordProcessor" | "Spreadsheet" | "Presentation" | "Calendar"
            | "ProjectManagement" => return "Office",
            "Science" | "Math" | "Engineering" | "Astronomy" | "Biology" | "Chemistry"
            | "Physics" | "Education" => return "Science",
            "Settings" | "HardwareSettings" | "DesktopSettings" | "Accessibility" => {
                return "Settings"
            }
            "System" | "FileManager" | "TerminalEmulator" | "Monitor" | "PackageManager"
            | "Emulator" => return "System",
            "Utility" | "Archiving" | "Calculator" | "Clock" | "TextTools" | "FileTools"
            | "Compression" => return "Utilities",
            _ => {}
        }
    }
    "Other"
}

/// Group app entries by their primary mapped category. Returns a sorted map.
pub fn categorize_entries(entries: &[AppEntry]) -> BTreeMap<String, Vec<AppEntry>> {
    let mut map: BTreeMap<String, Vec<AppEntry>> = BTreeMap::new();
    for entry in entries {
        let category = map_category(&entry.categories).to_string();
        map.entry(category).or_default().push(entry.clone());
    }
    // Sort entries within each category
    for entries in map.values_mut() {
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_category_internet() {
        let cats = vec!["Network".to_string(), "WebBrowser".to_string()];
        assert_eq!(map_category(&cats), "Internet");
    }

    #[test]
    fn test_map_category_development() {
        let cats = vec!["Development".to_string(), "IDE".to_string()];
        assert_eq!(map_category(&cats), "Development");
    }

    #[test]
    fn test_map_category_fallback() {
        let cats = vec!["SomethingWeird".to_string()];
        assert_eq!(map_category(&cats), "Other");
    }

    #[test]
    fn test_map_category_empty() {
        let cats: Vec<String> = vec![];
        assert_eq!(map_category(&cats), "Other");
    }

    #[test]
    fn test_categorize_entries() {
        let entries = vec![
            AppEntry {
                name: "Firefox".into(),
                exec: "firefox".into(),
                comment: None,
                icon: Some("firefox".into()),
                categories: vec!["Network".into(), "WebBrowser".into()],
            },
            AppEntry {
                name: "GIMP".into(),
                exec: "gimp".into(),
                comment: None,
                icon: Some("gimp".into()),
                categories: vec!["Graphics".into()],
            },
            AppEntry {
                name: "Code".into(),
                exec: "code".into(),
                comment: None,
                icon: Some("code".into()),
                categories: vec!["Development".into(), "TextEditor".into()],
            },
        ];

        let categorized = categorize_entries(&entries);
        assert_eq!(categorized.len(), 3);
        assert!(categorized.contains_key("Internet"));
        assert!(categorized.contains_key("Graphics"));
        assert!(categorized.contains_key("Development"));
        assert_eq!(categorized["Internet"].len(), 1);
        assert_eq!(categorized["Internet"][0].name, "Firefox");
    }
}
