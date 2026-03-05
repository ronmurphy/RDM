# RDM — Rust Desktop Manager (QML Edition)

A lightweight, modular Wayland desktop environment built from scratch in Rust. RDM sits on top of [labwc](https://labwc.github.io/) (a wlroots-based compositor) and provides a full desktop shell: panel/taskbar, app launcher, system tray, settings app, wallpaper management, notifications, and session management — all with a cohesive **Tokyo Night** color theme.

The UI layer uses **Qt/QML** (via the `qmetaobject` Rust crate) with **layer-shell-qt** for Wayland layer-shell integration, while all business logic remains in pure Rust.

![License](https://img.shields.io/badge/license-MIT-blue)
![Rust](https://img.shields.io/badge/rust-2021_edition-orange)
![Wayland](https://img.shields.io/badge/display-Wayland-blueviolet)
![Compositor](https://img.shields.io/badge/compositor-labwc-green)
![UI](https://img.shields.io/badge/UI-Qt%2FQML-41cd52)

---

## Screenshots

> *Coming soon — RDM is under active development.*

---

## What It Does

- **Panel/Taskbar** — Top (or bottom) bar with app launcher button, running-window taskbar, clock, and system tray. Three taskbar modes:
  - **Icons** — Nerd Font glyphs based on app_id (icon theme independent)
  - **Nerd** — Nerd Font glyphs (terminal, browser, editor, etc.)
  - **Text** — Window title buttons (classic style)
- **App Launcher** — Overlay search dialog (Super key) that scans `.desktop` files and launches apps
- **System Tray** — Unified menu button combining:
  - Battery indicator with charge level, Nerd Font icons, and color coding
  - WiFi submenu — scans networks via NetworkManager, connect to known/new networks with password dialog
  - Session controls — Lock, Logout, Reboot, Shutdown
- **Wallpaper** — Managed via `swaybg`, configurable through the settings app (image path, fill mode, solid color fallback)
- **Settings App** — QML GUI to configure panel options (taskbar mode, position, height, clock) and wallpaper (image, mode, background color). Changes apply via hot reload.
- **Hot Reload** — Rebuild any component, run `rdm-reload`, and see changes instantly without restarting the compositor or losing your windows
- **Session Manager** — Manages autostart processes, automatic crash recovery, PID tracking, SIGUSR1-driven hot reload
- **Version Watermark** — Subtle build version label on the desktop (layer-shell bottom)
- **Window Snapping** — Provided by labwc's built-in snapping (half-screen, maximize, corners) with keyboard shortcuts
- **Notifications** — Via [mako](https://github.com/emersion/mako) notification daemon

## What It Can't Do (Yet)

- Volume / audio controls in the tray
- Brightness slider in the tray
- Visual snap zone previews (quarter/thirds tiling overlays)
- Multi-monitor configuration UI
- Workspace indicator / switcher widget in the panel
- Theming beyond Tokyo Night (colors are currently hardcoded in CSS)
- Application pinning in the taskbar
- Drag-and-drop window reordering
- Screen recording / screenshot tools

---

## Architecture

RDM is a Cargo workspace with 7 crates:

| Crate | Binary | Purpose |
|-------|--------|---------|
| `rdm-session` | `rdm-session` | Process manager — starts/stops/restarts all shell components, handles hot reload via SIGUSR1 |
| `rdm-panel` | `rdm-panel` | Panel bar — taskbar, clock, system tray (battery, wifi, power), launcher button |
| `rdm-launcher` | `rdm-launcher` | Overlay app launcher — searches `.desktop` files, keyboard-driven |
| `rdm-settings` | `rdm-settings` | QML settings GUI — panel config + wallpaper config |
| `rdm-watermark` | `rdm-watermark` | Version watermark on desktop background |
| `rdm-snap` | `rdm-snap` | Snap daemon (stub — labwc handles snapping natively for now) |
| `rdm-common` | *(library)* | Shared config types, load/save, build info |

### Runtime Dependencies (not Rust crates)

| Program | Role |
|---------|------|
| [labwc](https://labwc.github.io/) | Wayland compositor (wlroots-based) |
| [swaybg](https://github.com/swaywm/swaybg) | Wallpaper renderer |
| [mako](https://github.com/emersion/mako) | Notification daemon |
| [swaylock](https://github.com/swaywm/swaylock) | Screen locker |
| [foot](https://codeberg.org/dnkl/foot) | Default terminal emulator |
| [layer-shell-qt](https://invent.kde.org/plasma/layer-shell-qt) | Qt/QML layer-shell integration for Wayland |
| NetworkManager | WiFi management (via `nmcli`) |

### How It Starts

```
Display Manager (SDDM, etc.)
  └── rdm-start          (sets XDG vars, writes labwc autostart, exec labwc)
        └── labwc         (Wayland compositor)
              └── rdm-session   (reads session.toml, spawns all children)
                    ├── rdm-panel       (panel + taskbar + tray)
                    ├── rdm-watermark   (version label)
                    ├── swaybg          (wallpaper, args from rdm.toml)
                    └── mako            (notifications)
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
sudo pacman -S labwc swaybg swaylock mako foot

# Build dependencies (Qt/QML + Rust)
sudo pacman -S rust cargo qt6-base qt6-declarative qt6-wayland layer-shell-qt

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
3. Install the `rdm-start` and `rdm-reload` scripts
4. Register RDM as a session in your display manager (`/usr/share/wayland-sessions/rdm.desktop`)
5. Copy default configs to `~/.config/rdm/` (won't overwrite existing)
6. Copy labwc config to `~/.config/labwc/rc.xml` (won't overwrite existing)

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
| `Super` | Open app launcher |
| `Super + Return` | Open terminal (foot) |
| `Super + Left/Right/Up/Down` | Snap window to half-screen |
| `Super + F` | Toggle maximize |
| `Super + Q` | Close window |
| `Super + 1-4` | Switch to workspace 1-4 |
| `Super + Shift + 1-4` | Move window to workspace 1-4 |

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
