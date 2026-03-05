// Taskbar utilities — Nerd Font glyph mapping and title truncation.
// The QML panel uses these through the TaskbarModel in main.rs.

/// Map app_id to a Nerd Font glyph.
pub fn nerd_glyph_for(app_id: &str) -> String {
    let lower = app_id.to_lowercase();
    let glyph = match lower.as_str() {
        // Browsers
        s if s.contains("firefox") => "\u{f269}",       // 
        s if s.contains("chrome") => "\u{f268}",        // 
        s if s.contains("chromium") => "\u{f268}",      // 
        s if s.contains("brave") => "\u{f39f}",         // 
        // Terminals
        s if s.contains("foot") => "\u{f489}",          // 
        s if s.contains("kitty") => "\u{f489}",         // 
        s if s.contains("alacritty") => "\u{f489}",     // 
        s if s.contains("terminal") => "\u{f489}",      // 
        s if s.contains("wezterm") => "\u{f489}",       // 
        s if s.contains("konsole") => "\u{f489}",       // 
        // Editors / IDEs
        s if s.contains("code") || s.contains("vscode") => "\u{e70c}", // 
        s if s.contains("neovim") || s.contains("nvim") => "\u{e62b}", // 
        s if s.contains("vim") => "\u{e62b}",           // 
        s if s.contains("emacs") => "\u{e632}",         // 
        s if s.contains("sublime") => "\u{e7aa}",       // 
        // Files
        s if s.contains("thunar") || s.contains("nautilus") || s.contains("dolphin") || s.contains("files") || s.contains("pcmanfm") => "\u{f413}", // 
        // Media
        s if s.contains("spotify") => "\u{f1bc}",       // 
        s if s.contains("vlc") => "\u{f40a}",           // 
        s if s.contains("mpv") => "\u{f40a}",           // 
        // Communication
        s if s.contains("discord") => "\u{f392}",       // 
        s if s.contains("telegram") => "\u{f2c6}",      // 
        s if s.contains("slack") => "\u{f198}",         // 
        s if s.contains("signal") => "\u{f086}",        // 
        // Games / Creative
        s if s.contains("steam") => "\u{f1b6}",         // 
        s if s.contains("gimp") => "\u{e69e}",          // 
        s if s.contains("inkscape") => "\u{e69e}",      // 
        s if s.contains("blender") => "\u{e69e}",       // 
        s if s.contains("obs") => "\u{f03d}",           // 
        // System
        s if s.contains("settings") || s.contains("control") => "\u{f013}", // 
        s if s.contains("monitor") || s.contains("htop") || s.contains("btop") => "\u{f080}", // 
        // Fallback
        _ => "\u{f2d0}",                                // 
    };
    glyph.to_string()
}

pub fn truncate_title(title: &str, max_len: usize) -> String {
    if title.len() <= max_len {
        title.to_string()
    } else {
        let mut s: String = title.chars().take(max_len - 1).collect();
        s.push('\u{2026}');
        s
    }
}
