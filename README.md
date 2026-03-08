![sprite-animation](https://github.com/user-attachments/assets/0c752074-4b2a-476a-a073-7b5c54ec3ffc)

# RDM — Rust Desktop Manager

A lightweight, modular Wayland desktop environment built from scratch in Rust. RDM sits on top of [labwc](https://labwc.github.io/) (a wlroots-based compositor) and provides a full desktop shell: panel/taskbar, app launcher, system tray, settings app, wallpaper management, notifications, session management, and NoTerm (a beginner-friendly terminal/files view) — with **9 built-in color themes** and a visual theme editor for creating your own.

![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-2021_edition-orange)
![Wayland](https://img.shields.io/badge/display-Wayland-blueviolet)
![Compositor](https://img.shields.io/badge/compositor-labwc-green)

---

## Screenshots

> *Coming soon — RDM is under active development.*

---

## What It Does

- **Panel/Taskbar** — Top (or bottom) bar with app launcher button, running-window taskbar, clock with calendar popup, and system tray. Three taskbar modes:
  - **Icons** — GTK icon theme icons for each open window
  - **Nerd** — Nerd Font glyphs with icon-derived colors extracted from each app's icon
  - **Text** — Window title buttons (classic style)
- **App Launcher** — Overlay search dialog (Super key) that scans `.desktop` files and launches apps. Includes a dedicated Settings button for quick access to RDM Settings
- **System Tray** — Unified menu button combining:
  - Battery indicator with charge level, Nerd Font icons, and color coding
  - WiFi submenu — scans networks via NetworkManager, connect to known/new networks with password dialog
  - Session controls — Lock, Logout, Reboot, Shutdown
- **Wallpaper** — Managed via `swaybg`, configurable through the settings app (image path, fill mode, solid color fallback)
- **Settings App** — GTK4 GUI to configure panel options (taskbar mode, position, height, clock), wallpaper (image, mode, background color), display arrangement, and a **Theme Editor** for creating custom color themes. Changes apply via hot reload.
- **NoTerm (`rdm-noterm`)** — Guided terminal + file browser hybrid with:
  - command input at the bottom (Enter runs + clears)
  - enhanced clickable `ls` tiles with `..` as first item and single-click folder navigation
  - raw/text/icons/nerd display modes
  - built-in preview for common text/image files
  - preview drawer hidden by default, slide-out on selection, with close `X`
  - remembered display mode across launches (`~/.config/rdm/noterm-mode`)
- **Hot Reload** — Rebuild any component, run `rdm-reload`, and see changes instantly without restarting the compositor or losing your windows
- **Session Manager** — Manages autostart processes, automatic crash recovery, PID tracking, SIGUSR1-driven hot reload
- **Version Watermark** — Subtle build version label on the desktop (layer-shell bottom)
- **Window Snapping** — Provided by labwc's built-in snapping (half-screen, maximize, corners) with keyboard shortcuts
- **Notifications** — Built-in notification daemon (`rdm-notify`) implementing the freedesktop D-Bus notification spec
- **Screenshots** — Multi-monitor screenshot tool (`rdm-screenshot`) using grim + slurp, saves to `~/Pictures/Screenshots/` and copies to clipboard
- **Volume & Media Keys** — Multimedia key support for volume control (via WirePlumber) and media playback (via playerctl)

## What It Can't Do (Yet)

- Brightness slider in the tray
- Visual snap zone previews (quarter/thirds tiling overlays)
- Multi-monitor configuration UI
- Workspace indicator / switcher widget in the panel
- Application pinning in the taskbar
- Drag-and-drop window reordering
- Screen recording

---

## Architecture

RDM is a Cargo workspace with 9 crates:

| Crate | Binary | Purpose |
|-------|--------|---------|
| `rdm-session` | `rdm-session` | Process manager — starts/stops/restarts all shell components, handles hot reload via SIGUSR1 |
| `rdm-panel` | `rdm-panel` | Panel bar — taskbar, clock, system tray (battery, wifi, power), launcher button |
| `rdm-launcher` | `rdm-launcher` | Overlay app launcher — searches `.desktop` files, keyboard-driven |
| `rdm-notify` | `rdm-notify` | Notification daemon — freedesktop D-Bus notifications with GTK4 layer-shell popups |
| `rdm-settings` | `rdm-settings` | GTK4 settings GUI — panel, wallpaper, displays, and theme editor |
| `rdm-noterm` | `rdm-noterm` | Guided terminal/files app with enhanced `ls`, click navigation, and inline previews |
| `rdm-watermark` | `rdm-watermark` | Version watermark on desktop background |
| `rdm-snap` | `rdm-snap` | Snap daemon (stub — labwc handles snapping natively for now) |
| `rdm-common` | *(library)* | Shared config types, load/save, build info, 3-layer CSS theme system |

### Runtime Dependencies (not Rust crates)

| Program | Role |
|---------|------|
| [labwc](https://labwc.github.io/) | Wayland compositor (wlroots-based) |
| [swaybg](https://github.com/swaywm/swaybg) | Wallpaper renderer |
| [foot](https://codeberg.org/dnkl/foot) | Default terminal emulator |
| [grim](https://sr.ht/~emersion/grim/) | Screenshot capture (Wayland) |
| [slurp](https://github.com/emersion/slurp) | Region selection for screenshots |
| [wl-clipboard](https://github.com/bugaevc/wl-clipboard) | Clipboard support (screenshot copy) |
| [WirePlumber](https://pipewire.pages.freedesktop.org/wireplumber/) | Volume control via `wpctl` |
| [playerctl](https://github.com/altdesktop/playerctl) | Media playback control |
| NetworkManager | WiFi management (via `nmcli`) |

### How It Starts

```
Display Manager (SDDM, etc.)
  └── rdm-start          (sets XDG vars, writes labwc autostart, exec labwc)
        └── labwc         (Wayland compositor)
              └── rdm-session   (reads session.toml, spawns all children)
                    ├── rdm-panel       (panel + taskbar + tray)
                    ├── rdm-notify      (notification daemon)
                    ├── rdm-watermark   (version label)
                    └── swaybg          (wallpaper, args from rdm.toml)
```

### Config Files

All config lives in `~/.config/rdm/`:

| File | Purpose |
|------|---------|
| `rdm.toml` | Panel settings, launcher size, snap config, wallpaper config |
| `session.toml` | Autostart process list for rdm-session |

labwc config lives in `~/.config/labwc/rc.xml` (keybindings, snapping, theme).

---

## Installation

### Prerequisites (Arch Linux)

```bash
# Compositor and Wayland tools
sudo pacman -S labwc swaybg foot

# Screenshot & media tools
sudo pacman -S grim slurp wl-clipboard wireplumber playerctl

# Build dependencies
sudo pacman -S rust cargo gtk4 gtk4-layer-shell gtksourceview5 webkit2gtk-6.0

# Runtime dependencies
sudo pacman -S networkmanager

# Recommended: a Nerd Font for the "nerd" taskbar mode
# Install from AUR or https://www.nerdfonts.com/
# e.g., JetBrainsMono Nerd Font, IosevkaTerm Nerd Font Mono
```

### Clone and Install

```bash
git clone https://github.com/ronmurphy/RDM.git
cd RDM
chmod +x install.sh
./install.sh
```

The install script will:
1. Build all crates in release mode
2. Install binaries to `/usr/local/bin/`
3. Install the `rdm-start`, `rdm-reload`, and `rdm-screenshot` scripts
4. Register RDM as a session in your display manager (`/usr/share/wayland-sessions/rdm.desktop`)
5. Install D-Bus activation service for `rdm-notify`
6. Copy default configs to `~/.config/rdm/` (won't overwrite existing)
7. Copy labwc config to `~/.config/labwc/rc.xml` (won't overwrite existing)

Then **log out** and select **"RDM Desktop"** from your display manager's session chooser.

### Starting from a TTY

```bash
exec rdm-start
```

### Uninstall

```bash
chmod +x uninstall.sh
./uninstall.sh
```

---

## Usage

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Super` (tap) | Open app launcher |
| `Super + Enter` | Open terminal (foot) |
| `Super + S` | Screenshot (region select) |
| `Super + Shift + S` | Screenshot (all monitors) |
| `Print Screen` | Screenshot (all monitors) |
| `Super + Left/Right/Up/Down` | Snap window to half-screen |
| `Super + F` | Toggle maximize |
| `Super + Q` | Close window |
| `Super + 1-4` | Switch to workspace 1-4 |
| `Super + Shift + 1-4` | Move window to workspace 1-4 |
| `Volume Up/Down/Mute` | Adjust volume (multimedia keys) |
| `Play/Next/Prev` | Media playback control |

### Development Workflow

RDM supports hot reload for rapid development:

```bash
# Edit any crate's source code, then:
cargo build --release
sudo install -m755 target/release/rdm-panel /usr/local/bin/
rdm-reload
# Panel restarts with new binary — no logout needed
```

### Settings

Run `rdm-settings` from the launcher or terminal to open the settings GUI. Changes are saved to `~/.config/rdm/rdm.toml` and applied via hot reload.

---

## Project Status

See [progress.md](progress.md) for detailed technical documentation of what has been built and how each component works.

---

## License

MIT
