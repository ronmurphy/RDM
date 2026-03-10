![sprite-animation](https://github.com/user-attachments/assets/0c752074-4b2a-476a-a073-7b5c54ec3ffc)

# RDM — Rust Desktop Manager

A lightweight, modular Wayland desktop environment built from scratch in **Rust + GTK4**. RDM runs on [labwc](https://labwc.github.io/) (a wlroots-based compositor) and provides everything you need for a daily-driver desktop: panel, app launcher, notifications, clipboard manager, screen lock, idle management, settings GUI, theming, and a plugin system — all in ~15k lines of Rust.

![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-2021_edition-orange)
![Wayland](https://img.shields.io/badge/display-Wayland-blueviolet)
![Compositor](https://img.shields.io/badge/compositor-labwc-green)

---

## Screenshots

> *Coming soon*

---

## Features

| Category | What You Get |
|----------|-------------|
| **Panel / Taskbar** | Top or bottom bar with app launcher button, running-window taskbar (icons, nerd-font, or text mode), clock + calendar popup, and system tray (battery, WiFi, session controls) |
| **App Launcher** | Full-screen overlay search (Super key), scans `.desktop` files, keyboard-driven |
| **Notifications** | Built-in freedesktop D-Bus notification daemon with themed popups |
| **Clipboard Manager** | Panel plugin — stores text + image history, one-click paste-back |
| **System Monitor** | Panel plugin — live CPU / RAM / temperature readout |
| **Screen Lock** | Super+L locks via swaylock; auto-lock after timeout; lock-before-sleep |
| **Idle Management** | Screen blanking + DPMS via swayidle; audio/video playback inhibits idle |
| **Screenshots** | Region, full-screen, or current-monitor capture (grim + slurp), auto-copied to clipboard |
| **Settings App** | GTK4 GUI — Appearance, Panel, Wallpaper, Displays (drag arrangement), Plugins, Theme Editor, Diagnostics |
| **9 Built-in Themes** | Tokyo Night, Catppuccin Mocha, Nord, Dracula, Gruvbox, Ubuntu, macOS, Solarized Dark, Windows 11 |
| **Theme Editor** | Visual color picker to create and save custom themes |
| **Plugin System** | Panel plugins as `.so` shared libraries — enable/disable/reorder in Settings |
| **Dock** | Optional app dock (auto-hide, configurable) |
| **NoTerm** | Beginner-friendly terminal + file browser hybrid with clickable tiles and inline previews |
| **Hot Reload** | Rebuild a component, run `rdm-reload`, changes apply instantly — no logout needed |
| **Session Manager** | Autostart management, crash recovery, PID tracking |
| **XDG Portal** | Screen sharing / capture works out of the box (OBS, Discord, etc.) |
| **Polkit Agent** | Authentication dialogs for privileged operations |
| **Volume / Media Keys** | Volume control (WirePlumber) and media playback (playerctl) |

---

## Installation

### 1. Install Dependencies

<details>
<summary><b>Arch Linux</b></summary>

```bash
sudo pacman -S labwc swaybg swayidle swaylock foot grim slurp wl-clipboard \
  wlr-randr wireplumber playerctl networkmanager polkit-gnome \
  rust gtk4 gtk4-layer-shell

# Recommended (AUR):
yay -S sway-audio-idle-inhibit   # inhibit idle while audio plays
```

</details>

<details>
<summary><b>Fedora 40+</b></summary>

```bash
sudo dnf install labwc swaybg swayidle swaylock foot grim slurp wl-clipboard \
  wlr-randr wireplumber playerctl NetworkManager polkit-gnome \
  rust cargo gtk4-devel gtk4-layer-shell-devel
```

</details>

<details>
<summary><b>Debian 13 (Trixie) / Ubuntu 24.04+</b></summary>

```bash
sudo apt install labwc swaybg swayidle swaylock foot grim slurp wl-clipboard \
  wlr-randr wireplumber playerctl network-manager policykit-1-gnome \
  rustc cargo libgtk-4-dev libgtk4-layer-shell-dev
```

</details>

> **Optional but recommended:** Install a [Nerd Font](https://www.nerdfonts.com/) (e.g. JetBrainsMono Nerd Font) for the nerd taskbar mode.

### 2. Clone & Install

```bash
git clone https://github.com/ronmurphy/RDM.git
cd RDM
./install.sh
```

The install script:
- Builds all binaries and plugins in release mode
- Installs binaries to `/usr/local/bin/`
- Installs panel plugins to `/usr/local/lib/rdm/plugins/`
- Registers RDM as a Wayland session for your display manager
- Installs D-Bus service, icons, `.desktop` entries
- Copies default configs to `~/.config/rdm/` (won't overwrite existing)
- Sets up labwc keybindings and xdg-desktop-portal config

### 3. Log In

Log out, then select **"RDM Desktop"** from your display manager (SDDM, GDM, etc.).

Or from a TTY:
```bash
exec rdm-start
```

### Uninstall

```bash
./uninstall.sh
```

---

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Super` (tap) | App launcher |
| `Super + Enter` | Terminal (foot) |
| `Super + L` | Lock screen |
| `Super + Q` | Close window |
| `Super + F` | Toggle maximize |
| `Super + Arrow` | Snap window (half-screen) |
| `Super + S` | Screenshot (region) |
| `Super + Shift + S` | Screenshot (all monitors) |
| `Super + Alt + S` | Screenshot (current monitor) |
| `Print` | Screenshot (all monitors) |
| `Super + 1–4` | Switch workspace |
| `Super + Shift + 1–4` | Move window to workspace |
| `Volume / Mute keys` | Volume control |

---

## Architecture

```
Display Manager
  └── rdm-start           # sets env vars, deploys configs, exec labwc
        └── labwc          # Wayland compositor
              └── rdm-session    # reads session.toml, spawns & monitors:
                    ├── rdm-panel        # panel + taskbar + tray + plugins
                    ├── rdm-notify       # notification daemon
                    ├── rdm-watermark    # version label on desktop
                    ├── swaybg           # wallpaper
                    ├── swayidle         # screen blank + auto-lock
                    ├── rdm-idle-inhibit # audio playback detection
                    └── polkit-gnome     # auth agent
```

### Components

| Crate | Type | Purpose |
|-------|------|---------|
| `rdm-session` | binary | Session/process manager with crash recovery and hot reload |
| `rdm-panel` | binary | Panel bar — taskbar, clock, tray, plugin host |
| `rdm-launcher` | binary | Overlay app launcher |
| `rdm-notify` | binary | Freedesktop D-Bus notification daemon |
| `rdm-settings` | binary | GTK4 settings GUI (7 pages) |
| `rdm-dock` | binary | Optional app dock |
| `rdm-noterm` | binary | Terminal + file browser hybrid |
| `rdm-snap` | binary | Window snap daemon (stub — labwc handles snapping) |
| `rdm-watermark` | binary | Desktop version watermark |
| `rdm-common` | library | Shared config, themes, CSS system |
| `rdm-panel-api` | library | Plugin ABI for panel plugins |
| `rdm-panel-clipboard` | plugin | Clipboard history manager |
| `rdm-panel-sysmon` | plugin | CPU / RAM / temp monitor |
| `rdm-panel-hello` | plugin | Example/template plugin |

### Config Files

| File | Location | Purpose |
|------|----------|---------|
| `rdm.toml` | `~/.config/rdm/` | Panel, launcher, wallpaper, idle, plugin settings |
| `session.toml` | `~/.config/rdm/` | Autostart process list |
| `rc.xml` | `~/.config/labwc/` | Keybindings and compositor config |

### Plugin System

Panel plugins are shared libraries (`.so`) with a C ABI. Drop a `.so` into `~/.local/share/rdm/plugins/` or `/usr/local/lib/rdm/plugins/`, then enable it in **Settings → Plugins**. See [plugins/plugin-dev.txt](plugins/plugin-dev.txt) for the developer guide.

---

## Development

```bash
# Build everything
cargo build --release

# Build just plugins
./build-plugins.sh --release

# Hot reload after changes (no logout needed)
cargo build --release -p rdm-panel
sudo install -m755 target/release/rdm-panel /usr/local/bin/
rdm-reload
```

---

## Project Status

RDM is a fully functional Wayland desktop environment. See [progress.md](progress.md) for detailed technical documentation and architecture notes.

### Still on the roadmap
- Brightness slider in tray
- Workspace indicator / switcher widget
- Application pinning in taskbar
- Visual snap zone overlays
- Screen recording

---

## License

MIT
