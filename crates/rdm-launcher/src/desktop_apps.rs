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
                    categories = value.split(';').filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
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
