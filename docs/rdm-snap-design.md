# rdm-snap Design Document

## Overview

rdm-snap is an optional tiling layer for the RDM desktop. It sits on top of labwc, which handles all floating window management, compositing, and rendering. rdm-snap adds keyboard-driven tiling without replacing or fighting labwc — it is purely additive.

The goal is a hybrid desktop: floating by default (labwc), with tiling available on demand (rdm-snap). Users who never enable rdm-snap see no difference in their desktop. Users who enable it get a tiling workflow similar to Sway or Hyprland, but without switching compositors.

This fills a real gap — there is no good hybrid floating+tiling compositor for Wayland right now.

---

## How It Fits Into RDM

rdm-snap is toggled via rdm-settings. When disabled:
- rdm-snap is not running
- labwc has no rdm-snap keybindings loaded
- The user's desktop is pure labwc floating

When enabled:
- rdm-snap runs as a daemon
- labwc loads rdm-snap's keybindings via an include file
- rdm-snap manages tiling layout state and sends geometry commands to labwc

---

## Configuration Toggle Design

### The Include File Approach

labwc's `rc.xml` supports `<include>` directives. rdm-snap ships its required keybindings as a separate file:

```
~/.config/labwc/rdm-snap-keybinds.xml
```

When rdm-snap is **enabled**, rdm-settings adds or symlinks this include into `rc.xml` and sends a reconfigure signal to labwc.

When rdm-snap is **disabled**, rdm-settings removes the include and reconfigures labwc. The keybinds file itself stays on disk — it just isn't referenced.

This means:
- The main `rc.xml` is never heavily modified
- Enabling/disabling is clean and reversible
- The keybinds file can be version-controlled and shipped with RDM

### rdm-settings Responsibility

rdm-settings handles:
1. Writing/removing the `<include>` line in `rc.xml`
2. Starting/stopping the rdm-snap process (via the autostart system)
3. Triggering `labwc --reconfigure` after making changes

The user sees a single toggle: "Enable tiling window manager (rdm-snap)".

---

## Architecture

```
labwc (compositor)
    |
    |-- rc.xml keybinds call: rdm-snap <command>
    |
rdm-snap daemon
    |-- Wayland client: wlr-foreign-toplevel-management (window tracking)
    |-- IPC client: labwc socket (send MoveTo / ResizeTo actions)
    |-- State: layout tree, tiled/floating classification per window
```

### Keyboard Input

rdm-snap does not capture global keyboard shortcuts itself — that would require compositor-level access. Instead, labwc's keybind system calls rdm-snap commands directly:

```xml
<keybind key="Super-h">
    <action name="Execute">
        <command>rdm-snap tile left</command>
    </action>
</keybind>
```

rdm-snap exposes a simple CLI interface. The daemon receives these commands via a local socket or named pipe and acts on them.

### Window Tracking

rdm-snap connects to the Wayland display and uses `wlr-foreign-toplevel-management` to:
- Get the list of all current toplevels on startup
- Receive events when windows are created, closed, activated, or change state
- Know each window's app-id, title, and current state (maximized, minimized, etc.)

### Moving and Resizing Windows

labwc has an IPC socket. rdm-snap sends labwc actions to move and resize the focused window:
- `MoveTo x y` — moves the focused window to absolute coordinates
- `ResizeTo w h` — resizes the focused window

For each tiling operation, rdm-snap:
1. Calculates the full layout (positions and sizes of all tiled windows)
2. Iterates through tiled windows, focuses each one via foreign-toplevel, sends MoveTo/ResizeTo
3. Returns focus to the originally active window

This is slightly round-about but works within Wayland's client constraints.

---

## Tiled vs Floating Classification

Not every window should be tiled. rdm-snap needs to decide per-window.

### Auto-classification Rules

Windows are **floating by default** unless:
- The user explicitly tiles them (via keybind)
- A user-configured rule matches their app-id (e.g., "always tile terminal windows")

Windows that should **never be tiled** (stay floating always):
- Transient/dialog surfaces — labwc already marks these differently, they may not appear via foreign-toplevel at all
- Windows below a minimum size threshold (tiny utility windows, tooltips)
- Windows the user has manually moved with the mouse (see Manual Interaction section)

### Per-Window Toggle

The user can always explicitly float or tile any window:
- `rdm-snap float` — removes the focused window from tiling, lets labwc handle it
- `rdm-snap tile` — pulls the focused window into tiling layout

---

## Layout

### Initial Layout Algorithm: Master/Stack

Start with a classic master/stack layout:
- One master window on the left (configurable size, default 55% of screen width)
- Remaining tiled windows stacked on the right

This is the most common layout and the easiest to reason about. Other layouts (grid, BSP, thirds) can be added later.

### Layout Recalculation

Whenever tiling state changes (window added, removed, resized), rdm-snap recalculates all tiled window geometries and pushes the new positions to labwc. The calculation accounts for:
- Screen/output dimensions (from Wayland output info)
- Gap size (configurable — inner gaps between windows, outer gap from screen edge)
- Number of tiled windows

### Multiple Outputs

Each output (monitor) maintains its own independent layout state. rdm-snap tracks which output a window is on via its geometry.

---

## The Nuanced Bits

### New Windows Appearing Mid-Session

When a new toplevel appears via foreign-toplevel, it may not be ready to receive geometry commands immediately — it's still negotiating its initial size with the compositor.

Approach:
- Listen for the `toplevel_done` or equivalent signal that indicates the window is fully mapped
- Apply a short delay before sending the first MoveTo/ResizeTo (may need tuning)
- Only auto-tile new windows if the user has enabled "auto-tile new windows" in config — otherwise they start floating and the user tiles them manually

### Manual Mouse Interaction (The Hard Part)

If a user drags a tiled window with the mouse, labwc moves it. rdm-snap has no reliable way to intercept this. The layout state is now stale.

Options:
1. **Do nothing** — the window drifts floating, rdm-snap's layout still shows the old slot as empty. Next tiling action fills it. This is the simplest behavior.
2. **Detect drift** — periodically check window positions against expected positions, if a tiled window has moved significantly, reclassify it as floating. This is polling-based and a bit fragile.
3. **User-initiated resync** — provide a `rdm-snap reset` command that rebuilds the layout from current window state. The user calls this if things get out of sync.

**Recommendation:** Start with option 1 + option 3. Keep it simple. If a user is using rdm-snap, they're opting into keyboard-driven workflow — mouse dragging tiled windows is an edge case. Document that if you drag a tiled window, use `rdm-snap float` first or `rdm-snap reset` after.

### Initial State Bootstrap (Session Already Running)

When rdm-snap is enabled mid-session, existing windows are already open. rdm-snap needs to build an initial layout from whatever is on screen.

Approach:
- On startup, enumerate all toplevels via foreign-toplevel
- Start with all windows as **floating** (don't rearrange anything uninvited)
- Let the user pull windows into tiling manually, or provide a `rdm-snap tile-all` command that tiles everything currently open

This is the least surprising behavior — enabling rdm-snap doesn't immediately rearrange the user's desktop.

### labwc Reconfigure During Session

When the user enables rdm-snap via rdm-settings, labwc is reconfigured to load the keybinds. The sequence:
1. rdm-settings writes the include into rc.xml
2. rdm-settings starts the rdm-snap daemon
3. rdm-settings sends `labwc --reconfigure`
4. labwc reloads config, new keybinds are live
5. rdm-snap daemon connects to Wayland, enumerates existing windows, starts idle

No reboot or logout needed.

---

## rdm-snap Config (rdm-settings UI)

Settings that make sense to expose:

| Setting | Default | Description |
|---|---|---|
| Enable tiling | off | Master toggle |
| Gap size (inner) | 8px | Gap between tiled windows |
| Gap size (outer) | 8px | Gap between windows and screen edge |
| Master width | 55% | Master pane width as percentage of screen |
| Auto-tile new windows | off | Automatically pull new windows into tiling |
| App-id rules | none | Per-app always-tile or always-float rules |

---

## Keybind Scheme (Suggested Defaults)

These go into `rdm-snap-keybinds.xml`:

| Keybind | Command | Action |
|---|---|---|
| Super+h | `rdm-snap focus left` | Focus window to the left |
| Super+l | `rdm-snap focus right` | Focus window to the right |
| Super+j | `rdm-snap focus down` | Focus window below |
| Super+k | `rdm-snap focus up` | Focus window above |
| Super+Shift+h | `rdm-snap move left` | Move window left in layout |
| Super+Shift+l | `rdm-snap move right` | Move window right in layout |
| Super+Return | `rdm-snap swap-master` | Promote focused window to master |
| Super+t | `rdm-snap tile` | Pull focused window into tiling |
| Super+f | `rdm-snap float` | Send focused window back to floating |
| Super+r | `rdm-snap reset` | Rebuild layout from current window state |
| Super+comma | `rdm-snap shrink-master` | Shrink master pane |
| Super+period | `rdm-snap grow-master` | Grow master pane |

These mirror dwm/Sway conventions so users coming from tiling WMs feel at home.

---

## What rdm-snap Does NOT Do

To keep scope reasonable:
- Does not replace labwc or act as a compositor
- Does not capture global shortcuts itself (labwc does that)
- Does not manage virtual desktops/workspaces (labwc handles those)
- Does not render anything visual (labwc handles decorations and gaps rendering)
- Does not handle multi-seat or unusual Wayland extensions

---

## Open Questions for Review

1. Should auto-tile new windows be on or off by default? Off feels safer but on feels more like a tiling WM.
2. Should there be a "scratchpad" concept (hidden floating window, toggled with a keybind)?
3. Should layouts beyond master/stack be in scope for the first version?
4. How should the user switch between layout modes if multiple are supported?
5. Is the CLI-per-command approach (labwc calls `rdm-snap tile left`) preferable to a persistent socket the keybind commands send to? The persistent socket is faster but the CLI approach is simpler to build first.

Answers:
1. have a "tile all" keybind in the rdm-snap config that labwc reads in if it is enabled.
2. unsure what a 'scratchpad' is.
3. fir the first version, master/stack is the target goal, we can add in bsp and others later on, and use rdm-settings to set the default behaviour for rdm-snap
4. use rdm-settings, we can have a drop down "Master/stack", "BSP", etc.  it can set a string that is sent to rdm-snap and then it becomes a kind of "if-then" for the snapping behaviour.
5. honestly the power users would want the cli usage, so we should add that is as a fallback and for the power users, but we can use the socket as the main, or, we can use just the cli and shell it quietly when keypresses are done.