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
- [rdm-notify — Notification Daemon](#rdm-notify--notification-daemon)
- [rdm-watermark — Version Watermark](#rdm-watermark--version-watermark)
- [rdm-settings — Settings GUI](#rdm-settings--settings-gui)
- [rdm-noterm — Guided Terminal + File Browser](#rdm-noterm--guided-terminal--file-browser)
- [rdm-snap — Snap Daemon (Stub)](#rdm-snap--snap-daemon-stub)
- [Hot Reload System](#hot-reload-system)
- [Theming](#theming)
- [Key Libraries & Protocols](#key-libraries--protocols)
- [Known Issues & Gotchas](#known-issues--gotchas)
- [Future Work](#future-work)

---

## Overview

RDM (Rust Desktop Manager) is a Wayland desktop environment that runs on top of **labwc**, a wlroots-based compositor. The project is a Cargo workspace with 9 crates — 8 binaries and 1 shared library.

All GUI components use **GTK4** with **gtk4-layer-shell** to create shell surfaces (panels, overlays, desktop widgets). The taskbar uses the **wlr-foreign-toplevel-management** Wayland protocol directly via `wayland-client` to track open windows.

The color theme uses a **3-layer CSS architecture** (colors → shared style → overrides) supporting 9 built-in themes and user-created themes via a visual theme editor.

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
│   ├── rdm-reload             # Sends SIGUSR1 to rdm-session for hot reload
│   └── rdm-screenshot         # Multi-monitor screenshot tool (grim + slurp)
├── install.sh                 # Main install script
├── uninstall.sh               # Uninstall script
├── crates/
│   ├── rdm-common/            # Shared config types, build info
│   ├── rdm-session/           # Session/process manager (tokio async)
│   ├── rdm-panel/             # Panel bar (taskbar, clock, tray, wifi)
│   ├── rdm-launcher/          # Overlay app launcher
│   ├── rdm-notify/            # Notification daemon (freedesktop D-Bus)
│   ├── rdm-noterm/            # Guided terminal + file browser hybrid
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
2. Spawns each process (rdm-panel, rdm-watermark, rdm-notify, swaybg, etc.)
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
  - `Nerd` — `gtk4::Label` with a Nerd Font glyph (`nerd_glyph_for()` maps app_id to Unicode glyphs). Each label gets icon-derived coloring extracted from the app's icon via `extract_icon_color()` (dominant-color pixbuf sampling), applied as inline CSS at priority 802. Colors are cached in a `ColorCache` HashMap.

**Action channel:** A `std::sync::mpsc` channel allows the GTK thread to send `ToplevelAction::Activate(id)` or `Close(id)` back to the Wayland thread, which calls the corresponding protocol methods.

### Clock (clock.rs)

A `gtk4::MenuButton` with a popover containing a date header and `gtk4::Calendar` widget. The button label updates every second via `glib::timeout_add_seconds_local(1, ...)` using `chrono` with a configurable format string (default: `"%H:%M  %b %d"`). Clicking the clock opens the calendar popup. Styled with the `.tray-btn` class to match the system tray.

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

- Top bar with a dedicated **Settings button** (gear icon, launches `rdm-settings`) and search entry
- Scrolled list of `.desktop` file entries (loaded via `freedesktop-desktop-entry` crate)
- Filters in real-time as you type
- Enter activates the selected entry; Escape closes
- Strips `%f`, `%u`, `%F`, `%U` field codes from Exec lines before launching
- Spawns processes with zombie reaping via background thread

The launcher is opened by the panel's "Apps" button (spawns `rdm-launcher` process) or by tapping the Super key (labwc keybinding in `rc.xml` fires on release so Super+key combos aren't intercepted).

---

## rdm-notify — Notification Daemon

**Path:** `crates/rdm-notify/`
**Binary:** `rdm-notify`
**Modules:** main.rs, dbus.rs

A native notification daemon that implements the **org.freedesktop.Notifications** D-Bus interface. Replaces external daemons like mako or dunst.

### D-Bus Integration (dbus.rs)

- Connects to the session bus via `gio::bus_get_sync()`
- Registers an object at `/org/freedesktop/Notifications` implementing:
  - `Notify` — receives notification (app_name, summary, body, timeout), assigns IDs, calls GTK callback
  - `CloseNotification` — programmatic dismissal
  - `GetCapabilities` — returns `["body"]`
  - `GetServerInformation` — returns `rdm-notify / RDM / 0.1 / 1.2`
- Owns the bus name `org.freedesktop.Notifications` with `REPLACE` flag
- D-Bus activation service file allows auto-start if rdm-notify isn't already running

### Notification Display (main.rs)

- Each notification creates a new GTK4 layer-shell window on `Layer::Overlay`
- Anchored to top-right corner, stacked vertically with 8px gaps
- Shows app name, summary, and body text with theme CSS classes
- Click-to-dismiss gesture handler
- Auto-dismiss timeout (default 5 seconds, respects per-notification timeout)
- `restack()` repositions all visible notifications when one is dismissed

### Lifetime Management

The GTK application hold guard and D-Bus service handle are both leaked via `std::mem::forget()` to ensure they persist for the entire program lifetime. Without this, the app would exit immediately since there are no visible windows, and the bus name would be released.

---

## rdm-watermark — Version Watermark

**Path:** `crates/rdm-watermark/`
**Binary:** `rdm-watermark`

A tiny layer-shell surface on `Layer::Bottom` (above wallpaper, below all windows), anchored to the bottom-right corner. Displays the build version string at 25% opacity. Non-interactive, zero exclusive zone.

---

## rdm-settings — Settings GUI

**Path:** `crates/rdm-settings/`
**Binary:** `rdm-settings`

A regular GTK4 window (not layer-shell) with a `Stack` + `StackSidebar` layout. Five pages:

### Appearance Page
- Theme selector dropdown (lists all built-in + user themes)
- Description display for the selected theme

### Panel Page
- Taskbar Mode dropdown (icons / text / nerd)
- Panel Position dropdown (top / bottom)
- Panel Height spin button (24–64)
- Show Clock toggle
- Clock Format text entry
- Launcher Position dropdown (center / panel / full)

### Wallpaper Page
- Image path display + Browse button (`FileChooserNative`) + Clear button
- Mode dropdown (fill / center / stretch / fit / tile)
- Background Color text entry (hex)

### Displays Page
- Interactive drag-and-drop display arrangement canvas (Cairo-rendered)
- Per-monitor controls: resolution, refresh rate, position, enable/disable
- Applies configuration via `wlr-randr`

### Theme Editor Page
- Base Theme dropdown to pick a starting palette
- Theme Name entry (auto-fills `<base>-custom`)
- Scrollable color swatch grid showing all `@define-color` variables
- Click any swatch to open a GTK4 `ColorDialog` color picker
- Save button writes to `~/.config/rdm/themes/<slug>/` (theme.toml + colors.css + overrides.css)
- Saved themes instantly appear in the Appearance theme selector

### Apply Flow
1. User clicks "Apply"
2. Config is saved to `~/.config/rdm/rdm.toml` via `RdmConfig::save()`
3. `apply_changes()` runs `rdm-reload` (shell script)
4. `rdm-reload` sends SIGUSR1 to `rdm-session`
5. Session manager stops all children, waits 800ms, restarts with fresh config
6. `rdm-panel` reads new taskbar mode, position, etc. from `rdm.toml`
7. `swaybg` gets new wallpaper args built from `rdm.toml`

---

## rdm-noterm — Guided Terminal + File Browser

**Path:** `crates/rdm-noterm/`  
**Binary:** `rdm-noterm`

NoTerm is a beginner-friendly "not a terminal" app that combines command execution with clickable file navigation.

### Core Behavior

- Command entry is at the bottom.
- Enter or Run executes in the current directory and clears the input.
- `cd <path>` and `pwd` are handled directly.
- Other commands run via `sh -lc` in current directory.

### Enhanced `ls` View

- `ls` output can render as raw text, text tiles, emoji icons, or nerd icons.
- Tiles are clickable:
  - first item is always `..` (parent directory)
  - single-click directory enters it
  - single-click previewable file opens inline preview
  - double-click non-previewable file shell-opens via `xdg-open`
- Search filter and hidden-file toggle apply to enhanced `ls`.

### Preview Drawer

- Preview panel is hidden by default.
- Clicking previewable content reveals a slide-out drawer from the right.
- Drawer targets a wide preview split (approximately 75% preview / 25% output).
- Top-left `X` closes the drawer and returns to navigation.
- Image preview uses contain-fit drawing in a `DrawingArea`.
- Text preview reads and truncates large files for safety.

### Persistence

- Mode selection (`raw/text/icons/nerd`) is persisted to:
  - `~/.config/rdm/noterm-mode`
- On startup, NoTerm restores the last selected mode.

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

RDM uses a **3-layer CSS architecture** for theming, managed by `rdm-common/src/theme.rs`:

### CSS Cascade Order

1. **`colors.css`** — `@define-color` palette (~18 variables per theme)
2. **`style.css`** — Shared structural CSS (~460 lines, one copy for all themes)
3. **`overrides.css`** — Per-theme tweaks (border radius, light/dark panel, etc.)

The assembled CSS is loaded at **priority 801** (`STYLE_PROVIDER_PRIORITY_USER + 1`) in all 5 GUI crates, which beats the user's `~/.config/gtk-4.0/gtk.css` (loaded at 800 by GTK). Per-widget icon colors use priority 802.

### Built-in Themes (9)

| Theme | Style | Notes |
|-------|-------|-------|
| Tokyo Night | Dark | Default — blue/purple accents |
| Ubuntu | Dark | Orange/aubergine accents |
| Windows 11 | Dark | Blue accents, hard-edge (0 radius) |
| macOS | Light | Aqua accents, pill-shaped buttons |
| Nord | Dark | Arctic blue palette |
| Catppuccin Mocha | Dark | Mauve/lavender accents |
| Gruvbox Dark | Dark | Warm orange/yellow palette |
| Dracula | Dark | Purple/pink accents |
| Solarized Dark | Dark | Blue/cyan palette |

### Theme Color Variables

| Variable | Purpose |
|----------|--------|
| `theme_bg` | Primary background |
| `theme_fg` | Primary foreground/text |
| `theme_surface` | Elevated surface background |
| `theme_border` | Borders and separators |
| `theme_active` | Active/focused state |
| `theme_accent` | Primary accent color |
| `theme_accent_hover` | Hover accent variant |
| `theme_deep_bg` | Deepest background (tray, popovers) |
| `theme_muted` | Muted/secondary text |
| `theme_subtle` | Subtle text |
| `theme_green/yellow/red/cyan/purple` | Semantic colors |
| `theme_suggested_fg` | Foreground on suggested-action buttons |
| `theme_contrast_fg` | High-contrast foreground (e.g., white) |

### User Themes

Custom themes are stored in `~/.config/rdm/themes/<name>/` with:
- `theme.toml` — name, display_name, author, description
- `colors.css` — `@define-color` palette
- `overrides.css` — optional per-theme CSS tweaks

User themes override built-in themes if they share the same slug name. The **Theme Editor** in rdm-settings provides a visual GUI for creating themes.

### Nerd Icon Colors

In Nerd taskbar mode, each app's icon color is extracted from its GTK icon theme icon using a dominant-color algorithm (pixbuf sampling). The extracted color is applied as inline CSS to the Label widget at priority 802, producing colored Nerd Font glyphs that match each application's branding. A `ColorCache` prevents redundant pixbuf lookups.

Font stack: `"Inter", "Noto Sans", sans-serif` for UI; `"JetBrainsMono Nerd Font", "IosevkaTerm Nerd Font Mono", "MesloLGS Nerd Font Mono", monospace` for Nerd icons and taskbar nerd mode.

---

## Key Libraries & Protocols

| Library | Version | Used For |
|---------|---------|----------|
| `gtk4` | 0.9 (v4_10 feature) | All GUI (uses GTK 4.x C library, ColorDialog for theme editor) |
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

2. **gtk4 0.9 with `v4_10` feature** — Enables `ColorDialog` for the theme editor. `FileChooserNative` is still used in wallpaper settings (deprecated in 4.10 but functional). CSS loaded via `CssProvider::load_from_data()`.

3. **Session manager changes require re-login** — `rdm-session` is the parent of all other processes. Hot reload restarts its children, not itself.

4. **swaybg args override** — The `args` field for swaybg in `session.toml` is ignored at runtime. `rdm-session` builds swaybg's arguments from `rdm.toml [wallpaper]` section so the settings app can change wallpapers.

5. **Battery path is hardcoded** to `/sys/class/power_supply/BAT0/`. Desktop machines without a battery will show "AC" in the tray.

6. **WiFi requires NetworkManager** — The WiFi module calls `nmcli` directly. Systems using other network managers (iwd standalone, ConnMan, etc.) won't have WiFi functionality.

---

## Future Work

- [ ] Brightness slider in tray
- [ ] Visual snap zone preview overlays
- [ ] Workspace indicator widget in panel
- [ ] Taskbar pinned apps
- [ ] Screen recording integration
- [ ] Auto-detect battery path (iterate `/sys/class/power_supply/`)
- [ ] Proper freedesktop session management (logout inhibit, etc.)
- [ ] Notification actions (button callbacks)
- [ ] Notification history / notification center
- [x] Configurable theme / user CSS override (3-layer theme system + theme editor)
- [x] Multi-monitor display arrangement in settings
- [x] Panel plugin system (cdylib `.so` plugins with C ABI)
- [x] Clipboard manager panel plugin (text + images)
- [x] System monitor panel plugin (CPU / RAM / temp)
- [x] Plugin management UI in settings (enable/disable/reorder)
- [x] Idle management (swayidle + DPMS + audio inhibit)
- [x] Screen lock (swaylock, auto-lock, lock-before-sleep)
- [x] XDG desktop portal integration (screen sharing)
- [x] Polkit authentication agent
- [x] Dock / app dock (rdm-dock)
- [x] Multi-distro install script (Arch, Fedora, Debian)

---

## Completion Status

RDM is a fully functional Wayland desktop environment as of March 2026. The following core DE features are implemented and working:

| Feature | Status | Component |
|---------|--------|-----------|
| Compositor integration | ✅ | labwc + rdm-start |
| Panel / Taskbar | ✅ | rdm-panel (3 modes: icons, nerd, text) |
| App Launcher | ✅ | rdm-launcher |
| Notifications | ✅ | rdm-notify (freedesktop D-Bus) |
| Wallpaper | ✅ | swaybg managed by rdm-session |
| Window Snapping | ✅ | labwc native (half, maximize, corners) |
| Workspaces | ✅ | labwc native (4 workspaces, Super+1-4) |
| Settings GUI | ✅ | rdm-settings (7 pages) |
| Theming | ✅ | 9 built-in themes + visual theme editor |
| Screenshots | ✅ | rdm-screenshot (region, full, output) |
| Volume/Media Keys | ✅ | rdm-volume + labwc keybinds |
| Session Manager | ✅ | rdm-session (autostart, crash recovery, hot reload) |
| Screen Lock | ✅ | swaylock (Super+L, auto-lock, lock-before-sleep) |
| Idle Management | ✅ | swayidle + rdm-idle-inhibit (audio-aware) |
| Clipboard Manager | ✅ | rdm-panel-clipboard plugin |
| System Monitor | ✅ | rdm-panel-sysmon plugin |
| Plugin System | ✅ | cdylib .so plugins with settings UI |
| Display Arrangement | ✅ | Interactive drag canvas in rdm-settings |
| XDG Portal | ✅ | Screen sharing (OBS, Discord) |
| Polkit Agent | ✅ | polkit-gnome autostart |
| Dock | ✅ | rdm-dock (optional) |
| File Manager | ✅ | rdm-noterm (terminal + clickable file browser) |
| Hot Reload | ✅ | rdm-reload (SIGUSR1, no logout needed) |
| D-Bus Activation | ✅ | rdm-notify auto-starts on demand |
| Install / Uninstall | ✅ | One-script install with prereq checks |

---

## Panel Plugin System

**Added:** March 2026

### Architecture

Panel plugins are **cdylib** shared libraries (`.so` files) loaded at runtime by `rdm-panel` via `libloading`. Each plugin must export 4 C ABI symbols:

```rust
#[no_mangle] pub extern "C" fn rdm_plugin_info() -> rdm_panel_api::PluginInfo;
#[no_mangle] pub extern "C" fn rdm_plugin_new_instance(id: u32, config_toml: *const c_char) -> *mut gtk4::Box;
#[no_mangle] pub extern "C" fn rdm_plugin_remove_instances();
#[no_mangle] pub extern "C" fn rdm_plugin_exit();
```

### Plugin Search Paths (in priority order)

1. `~/.local/share/rdm/plugins/` — user plugins
2. `/usr/local/lib/rdm/plugins/` — system plugins (from install.sh)
3. `/usr/lib/rdm/plugins/` — distro packages
4. `<exe_dir>/rdm-plugins/` — dev convenience

### Plugin Configuration

Plugins are listed in `rdm.toml` under `[[panel.plugins]]` entries:

```toml
[[panel.plugins]]
name     = "clipboard"
position = "right"

[[panel.plugins]]
name     = "sysmon"
position = "right"
```

Disabled plugins are commented out with `#` prefix (preserving config for re-enable).

### Settings UI

The **Plugins** page in rdm-settings provides:
- Discovery of all `.so` files across search paths
- Checkbox to enable/disable each plugin
- Dropdown for panel position (left / center / right)
- Up/Down buttons for load order
- Save & Reload button to apply changes

### Built-in Plugins

| Plugin | Crate | Description |
|--------|-------|-------------|
| clipboard | rdm-panel-clipboard | Clipboard history (text + images), polls `wl-paste`, paste-back via `wl-copy` |
| sysmon | rdm-panel-sysmon | CPU%, RAM%, temperature from `/sys/class/thermal/` |
| hello | rdm-panel-hello | Example/template plugin for developers |

---

## Idle & Lock System

**Added:** March 2026

### Components

- **swayidle** — Managed by rdm-session with dynamically built args from `rdm.toml [idle]`
- **swaylock** — Screen locker, triggered by timeout, keybind (Super+L), or before-sleep
- **rdm-idle-inhibit** — Script that detects audio playback and inhibits idle

### Configuration (`rdm.toml`)

```toml
[idle]
enabled = true
screen_off_secs = 300         # DPMS off after 5 min
lock_timeout_secs = 600       # Lock after 10 min (0 = no auto-lock)
lock_before_sleep = true      # Lock on suspend
idle_inhibit_on_audio = true  # Don't blank while audio plays
```

### rdm-idle-inhibit

Prefers `sway-audio-idle-inhibit` (exec's into it if available). Falls back to a polling loop that checks `pactl list sinks` for `RUNNING` state and resets the idle timer via `wlrctl` or `ydotool`.

---

## rdm-dock — Application Dock

**Path:** `crates/rdm-dock/`
**Binary:** `rdm-dock`

A layer-shell dock bar that shows running applications as icon buttons. Uses the same `wlr-foreign-toplevel-management` protocol as the panel taskbar. Can be enabled/disabled from the Settings app (Panel page dock toggle). Auto-starts via `session.toml` when enabled.
