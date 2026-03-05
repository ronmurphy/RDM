# RDM — Technical Progress & Architecture

This document is a developer-facing reference for how RDM is built, what each component does internally, and what design decisions were made. Useful for contributors, AI assistants, and anyone working on the codebase.

---

## Table of Contents

- [Overview](#overview)
- [Workspace Structure](#workspace-structure)
- [rdm-common — Shared Library](#rdm-common--shared-library)
- [rdm-session — Session Manager](#rdm-session--session-manager)
- [rdm-panel — Desktop Panel](#rdm-panel--desktop-panel)
  - [Taskbar (toplevel.rs + taskbar.rs)](#taskbar-toplevelrs--taskbarrs)
  - [Clock (clock.rs)](#clock-clockrs)
  - [System Tray (tray.rs)](#system-tray-trayrs)
  - [WiFi (wifi.rs)](#wifi-wifirs)
- [rdm-launcher — App Launcher](#rdm-launcher--app-launcher)
- [rdm-watermark — Version Watermark](#rdm-watermark--version-watermark)
- [rdm-settings — Settings GUI](#rdm-settings--settings-gui)
- [rdm-snap — Snap Daemon (Stub)](#rdm-snap--snap-daemon-stub)
- [Hot Reload System](#hot-reload-system)
- [Theming](#theming)
- [Key Libraries & Protocols](#key-libraries--protocols)
- [Known Issues & Gotchas](#known-issues--gotchas)
- [Future Work](#future-work)

---

## Overview

RDM (Rust Desktop Manager) is a Wayland desktop environment that runs on top of **labwc**, a wlroots-based compositor. The project is a Cargo workspace with 7 crates — 6 binaries and 1 shared library.

All GUI components use **GTK4** with **gtk4-layer-shell** to create shell surfaces (panels, overlays, desktop widgets). The taskbar uses the **wlr-foreign-toplevel-management** Wayland protocol directly via `wayland-client` to track open windows.

The color theme is **Tokyo Night** throughout, with CSS hardcoded in each component's `load_css()` function.

---

## Workspace Structure

```
RDM/
├── Cargo.toml                 # Workspace root — all deps defined here
├── config/
│   ├── labwc-rc.xml           # Default labwc keybindings & compositor config
│   ├── rdm.desktop            # Session entry for display managers
│   ├── rdm.toml               # Default panel/launcher/snap/wallpaper config
│   └── session.toml           # Default autostart process list
├── scripts/
│   ├── rdm-install            # Legacy install script (see install.sh)
│   ├── rdm-start              # Session entry point — sets env, execs labwc
│   └── rdm-reload             # Sends SIGUSR1 to rdm-session for hot reload
├── install.sh                 # Main install script
├── uninstall.sh               # Uninstall script
├── crates/
│   ├── rdm-common/            # Shared config types, build info
│   ├── rdm-session/           # Session/process manager (tokio async)
│   ├── rdm-panel/             # Panel bar (taskbar, clock, tray, wifi)
│   ├── rdm-launcher/          # Overlay app launcher
│   ├── rdm-watermark/         # Desktop version watermark
│   ├── rdm-settings/          # GTK4 settings app
│   └── rdm-snap/              # Window snap daemon (stub)
```

---

## rdm-common — Shared Library

**Path:** `crates/rdm-common/`

Provides shared configuration types and utility functions used by all other crates.

### Config Types (config.rs)

- `RdmConfig` — top-level config with sections: `panel`, `launcher`, `snap`, `wallpaper`
- `PanelConfig` — height, position (top/bottom), show_clock, clock_format, taskbar_mode (icons/text/nerd)
- `LauncherConfig` — width, height
- `SnapConfig` — edge_threshold, show_preview
- `WallpaperConfig` — path (image file), mode (fill/center/stretch/fit/tile), color (hex fallback)

All structs derive `Serialize` + `Deserialize` with `#[serde(default)]` on every field, so partial config files work correctly.

### Key Functions

- `RdmConfig::load()` — reads `~/.config/rdm/rdm.toml`, falls back to defaults
- `RdmConfig::save()` — writes pretty TOML back to the config file
- `config_dir()` → `~/.config/rdm/`
- `config_path()` → `~/.config/rdm/rdm.toml`
- `build_version_string()` — returns build version/date from compile-time env (via build.rs)

### Build Script (build.rs)

Sets `RDM_BUILD_DATE` environment variable at compile time using `chrono`, consumed by `build_version_string()` to show the build date in the watermark.

---

## rdm-session — Session Manager

**Path:** `crates/rdm-session/`
**Binary:** `rdm-session`
**Runtime:** tokio async

The session manager is the first process launched inside the labwc compositor (via the `autostart` file written by `rdm-start`). It:

1. Reads `~/.config/rdm/session.toml` for the autostart list
2. Spawns each process (rdm-panel, rdm-watermark, swaybg, mako, etc.)
3. Monitors children — restarts crashed processes marked `restart = true`
4. Listens for SIGUSR1 — on receipt, stops all children, waits 800ms for surfaces to release, reloads config, restarts everything

### swaybg Special Handling

When spawning `swaybg`, the session manager does **not** use the `args` field from `session.toml`. Instead it calls `build_swaybg_args()` which reads the `[wallpaper]` section from `rdm.toml` and constructs `-i`, `-m`, `-c` arguments dynamically. This allows the settings app to change the wallpaper by writing `rdm.toml` and triggering a reload.

### Signal Handling

Uses `nix::sys::signal::sigaction` to install a SIGUSR1 handler that sets an `AtomicBool` flag. The tokio main loop checks this flag every 500ms. This is signal-safe because the handler only performs an atomic store.

### PID File

Written to `~/.config/rdm/session.pid` so `rdm-reload` can find the session manager process.

---

## rdm-panel — Desktop Panel

**Path:** `crates/rdm-panel/`
**Binary:** `rdm-panel`
**Modules:** main.rs, clock.rs, taskbar.rs, toplevel.rs, tray.rs, wifi.rs

The panel is a GTK4 layer-shell surface anchored to the top (or bottom) edge. It contains:

- **Left:** App launcher button ("Apps") — spawns `rdm-launcher` on click
- **Center:** Taskbar showing running windows
- **Right:** Clock label + system tray menu button

### Layer Shell Setup

```rust
window.init_layer_shell();
window.set_layer(Layer::Top);
window.set_anchor(Edge::Left, true);
window.set_anchor(Edge::Right, true);
window.set_anchor(Edge::Top, at_top);     // or Bottom
window.auto_exclusive_zone_enable();       // reserves space
```

### Taskbar (toplevel.rs + taskbar.rs)

**toplevel.rs** — Wayland protocol client running in a dedicated thread.

- Connects to the Wayland display independently from GTK's connection
- Binds `zwlr_foreign_toplevel_manager_v1` to receive toplevel events (new window, title change, state change, closed)
- Each toplevel gets a numeric ID; events update a `HandleState` struct
- On every change, `flush_to_shared()` copies state into an `Arc<Mutex<SharedState>>` with a generation counter
- The GTK side polls this shared state every 250ms

**Critical implementation detail:** The `event_created_child!` macro must be defined for `ZwlrForeignToplevelManagerV1` → `ZwlrForeignToplevelHandleV1`. Without this, wayland-client panics when the compositor creates new toplevel handle objects. This was a major debugging issue during development.

**taskbar.rs** — GTK widget management.

- `setup_taskbar(container, mode)` starts the toplevel tracker thread and sets up a 250ms poll timer
- On each poll, compares the generation counter to avoid unnecessary updates
- Removes stale widgets (closed windows), adds new ones, updates existing
- Three display modes controlled by `TaskbarMode`:
  - `Text` — `gtk4::Button` with window title
  - `Icons` — `gtk4::Button` with a `gtk4::Image` from the icon theme (`resolve_icon_name()` maps app_id to theme icons with fallbacks)
  - `Nerd` — `gtk4::Button` with a Nerd Font glyph label (`nerd_glyph_for()` maps app_id to Unicode glyphs)

**Action channel:** A `std::sync::mpsc` channel allows the GTK thread to send `ToplevelAction::Activate(id)` or `Close(id)` back to the Wayland thread, which calls the corresponding protocol methods.

### Clock (clock.rs)

Simple `glib::timeout_add_seconds_local(1, ...)` that formats the current time with `chrono` and updates a GTK Label. Format string comes from config (default: `"%H:%M  %b %d"`).

### System Tray (tray.rs)

A single `gtk4::MenuButton` that shows a GIO `Menu` with:

1. **Battery section** — reads `/sys/class/power_supply/BAT0/capacity` and `status`. Displays 10 different Nerd Font battery icons (5 for discharging levels, 5 for charging levels). Color-coded: green (>40%), yellow (15-40%), red (<15%), blue (charging). Updates every 30s.
2. **WiFi submenu** — delegated to `wifi.rs`
3. **Session submenu** — Lock (swaylock), Logout (labwc exit), Reboot, Shutdown via `loginctl`

The button label shows the battery icon + percentage (e.g., "󰂁 85%").

### WiFi (wifi.rs)

- `scan_networks()` — calls `nmcli -t -f SSID,SIGNAL,SECURITY,IN-USE dev wifi list`, parses output
- `build_wifi_submenu()` — creates GIO menu items with signal strength Nerd Font icons (▂▄▆█ variants), lock icon for secured networks, checkmark for the currently connected network
- On click: known networks connect directly via `nmcli con up`; unknown secured networks show a GTK4 password dialog with `gtk4::PasswordEntry`
- `connect_new()` — `nmcli dev wifi connect <ssid> password <pw>` (NetworkManager auto-saves credentials)
- Uses `async_channel` for the password dialog → connection flow
- Auto-refreshes every 30s, plus a manual "Refresh" menu item

---

## rdm-launcher — App Launcher

**Path:** `crates/rdm-launcher/`
**Binary:** `rdm-launcher`

An overlay layer-shell surface (`Layer::Overlay`) with exclusive keyboard grab. Contains:

- Search entry at the top
- Scrolled list of `.desktop` file entries (loaded via `freedesktop-desktop-entry` crate)
- Filters in real-time as you type
- Enter activates the selected entry; Escape closes
- Strips `%f`, `%u`, `%F`, `%U` field codes from Exec lines before launching
- Spawns processes with zombie reaping via background thread

The launcher is opened by the panel's "Apps" button (spawns `rdm-launcher` process) or by the Super key (labwc keybinding in `rc.xml`).

---

## rdm-watermark — Version Watermark

**Path:** `crates/rdm-watermark/`
**Binary:** `rdm-watermark`

A tiny layer-shell surface on `Layer::Bottom` (above wallpaper, below all windows), anchored to the bottom-right corner. Displays the build version string at 25% opacity. Non-interactive, zero exclusive zone.

---

## rdm-settings — Settings GUI

**Path:** `crates/rdm-settings/`
**Binary:** `rdm-settings`

A regular GTK4 window (not layer-shell) with a `Stack` + `StackSidebar` layout. Two pages:

### Panel Page
- Taskbar Mode dropdown (icons / text / nerd)
- Panel Position dropdown (top / bottom)
- Panel Height spin button (24–64)
- Show Clock toggle
- Clock Format text entry

### Wallpaper Page
- Image path display + Browse button (`FileChooserNative`) + Clear button
- Mode dropdown (fill / center / stretch / fit / tile)
- Background Color text entry (hex)

### Apply Flow
1. User clicks "Apply"
2. Config is saved to `~/.config/rdm/rdm.toml` via `RdmConfig::save()`
3. `apply_changes()` runs `rdm-reload` (shell script)
4. `rdm-reload` sends SIGUSR1 to `rdm-session`
5. Session manager stops all children, waits 800ms, restarts with fresh config
6. `rdm-panel` reads new taskbar mode, position, etc. from `rdm.toml`
7. `swaybg` gets new wallpaper args built from `rdm.toml`

---

## rdm-snap — Snap Daemon (Stub)

**Path:** `crates/rdm-snap/`
**Binary:** `rdm-snap`

Currently a placeholder. labwc provides built-in edge snapping configured via `rc.xml` (half-screen left/right/up/down, maximize on top edge). The snap daemon is intended to add:

- Visual snap preview overlays (translucent rectangles showing where a window will snap)
- Quarter-tiling (corner snapping)
- Thirds support

For now, it just logs its config and parks the thread.

---

## Hot Reload System

The hot reload system allows updating any shell component without restarting the compositor or losing windows:

```
Developer edits code
    → cargo build --release
    → sudo install -m755 target/release/rdm-panel /usr/local/bin/
    → rdm-reload
        → reads PID from ~/.config/rdm/session.pid
        → sends SIGUSR1 to rdm-session
            → rdm-session: SIGTERM all children
            → wait 300ms, force-kill stragglers
            → wait 800ms for layer-shell surfaces to release
            → re-read session.toml + rdm.toml
            → spawn fresh processes (new binaries)
```

**Important:** Changes to `rdm-session` itself require a full logout/login since it's the parent process that manages everything else.

---

## Theming

All components use the **Tokyo Night** color palette, applied via GTK4 `CssProvider::load_from_data()` in each binary's `load_css()` function.

Key colors:
| Token | Hex | Usage |
|-------|-----|-------|
| Background | `#1a1b26` | Panel, launcher, popover backgrounds |
| Foreground | `#c0caf5` | Primary text |
| Subtle | `#a9b1d6` | Clock, secondary text |
| Comment | `#565f89` | App descriptions, hints |
| Selection | `#292e42` | Hover states |
| Blue | `#7aa2f7` | Launcher button, titles, accents |
| Dark blue | `#3d59a1` | Active taskbar item |
| Border | `#3b4261` | Separators, input borders |
| Green | `#9ece6a` | Battery normal |
| Yellow | `#e0af68` | Battery low |
| Red | `#f7768e` | Battery critical, errors |
| Cyan | `#7dcfff` | Battery charging |

Font stack: `"Inter", "Noto Sans", sans-serif` for UI; `"JetBrainsMono Nerd Font", "IosevkaTerm Nerd Font Mono", "MesloLGS Nerd Font Mono", monospace` for Nerd icons and taskbar nerd mode.

---

## Key Libraries & Protocols

| Library | Version | Used For |
|---------|---------|----------|
| `gtk4` | 0.9 | All GUI (uses GTK 4.x C library) |
| `gtk4-layer-shell` | 0.4 | Wayland layer-shell surfaces (panels, overlays) |
| `wayland-client` | 0.31 | Direct Wayland protocol communication |
| `wayland-protocols-wlr` | 0.3 | wlr-foreign-toplevel-management protocol |
| `tokio` | 1 | Async runtime for session manager |
| `serde` / `toml` | 1 / 0.8 | Config serialization |
| `chrono` | 0.4 | Clock formatting, build timestamps |
| `freedesktop-desktop-entry` | 0.7 | Parsing `.desktop` files for launcher |
| `async-channel` | 2 | Async communication in WiFi password dialog |
| `nix` | 0.29 | POSIX signal handling (SIGUSR1) |
| `dirs` | 6 | XDG directory resolution |

### Wayland Protocol: wlr-foreign-toplevel-management

This is the core protocol that makes the taskbar work. It's a wlroots extension (supported by labwc, sway, etc.) that lets clients:

- Receive events when windows are created, changed, or closed
- Get window title, app_id, and state (activated, maximized, minimized, fullscreen)
- Request actions: activate, close, maximize, minimize, fullscreen

RDM connects to this protocol in a **separate thread** from GTK, using its own `wayland_client::Connection`. State is shared with the GTK main thread via `Arc<Mutex<SharedState>>` with a generation counter for efficient change detection.

---

## Known Issues & Gotchas

1. **`event_created_child!` macro is required** for wayland-client 0.31 when the compositor creates new objects via `new_id` events. Without it, the client panics. This must be defined for `ZwlrForeignToplevelManagerV1` dispatching to `ZwlrForeignToplevelHandleV1`.

2. **gtk4 0.9 API differences** — Uses `CssProvider::load_from_data()` (not `load_from_string()` which is GTK 4.12+). Uses `FileChooserNative` instead of `FileDialog` (which requires GTK 4.10+/gtk4-rs 0.8+).

3. **Session manager changes require re-login** — `rdm-session` is the parent of all other processes. Hot reload restarts its children, not itself.

4. **swaybg args override** — The `args` field for swaybg in `session.toml` is ignored at runtime. `rdm-session` builds swaybg's arguments from `rdm.toml [wallpaper]` section so the settings app can change wallpapers.

5. **Battery path is hardcoded** to `/sys/class/power_supply/BAT0/`. Desktop machines without a battery will show "AC" in the tray.

6. **WiFi requires NetworkManager** — The WiFi module calls `nmcli` directly. Systems using other network managers (iwd standalone, ConnMan, etc.) won't have WiFi functionality.

---

## Future Work

- [ ] Volume / audio controls in tray (PipeWire/PulseAudio)
- [ ] Brightness slider in tray
- [ ] Visual snap zone preview overlays
- [ ] Workspace indicator widget in panel
- [ ] Multi-monitor support in settings
- [ ] Configurable theme / user CSS override
- [ ] Taskbar pinned apps
- [ ] Screenshot / screen recording integration
- [ ] Auto-detect battery path (iterate `/sys/class/power_supply/`)
- [ ] Proper freedesktop session management (logout inhibit, etc.)
