# Session Note

Date: 2026-03-06
Repo: `/home/brad/Documents/RDM`

## What Was Changed

1. WiFi tray scan moved to async behavior:
- `crates/rdm-panel/src/wifi.rs`
- Menu now shows `Scanning...` immediately.
- Added top `Rescan` action.
- Refresh updates all registered WiFi submenus (multi-monitor friendly).

2. Initial panel startup guard for non-RDM sessions:
- `crates/rdm-panel/src/main.rs`
- `rdm-panel` now exits early unless one of these env vars contains `RDM`:
  - `XDG_CURRENT_DESKTOP`
  - `XDG_SESSION_DESKTOP`
  - `DESKTOP_SESSION`

3. Follow-up hardening after Budgie still showed the panel:
- `crates/rdm-panel/src/main.rs`
- `scripts/rdm-start`
- `is_rdm_session()` now requires all of:
  - `RDM_SESSION` is truthy (`1`, `true`, `yes`)
  - `XDG_SESSION_TYPE=wayland`
  - one of `XDG_CURRENT_DESKTOP` / `XDG_SESSION_DESKTOP` / `DESKTOP_SESSION` contains `RDM`
- `rdm-start` now exports:
  - `RDM_SESSION=1`
  - `XDG_CURRENT_DESKTOP=RDM`
  - `XDG_SESSION_DESKTOP=RDM`
  - `DESKTOP_SESSION=RDM`
  - `XDG_SESSION_TYPE=wayland`

4. Session-manager compatibility fix:
- `crates/rdm-session/src/main.rs`
- All managed child processes are now spawned with `RDM_SESSION=1`.
- Reason: `rdm-reload` restarts children but does not restart `rdm-session`, so older manager env could otherwise block new panel guard.

5. Parent-process ancestry guard added:
- `crates/rdm-panel/src/main.rs`
- `rdm-panel` now also requires its parent chain to include `rdm-session`.
- This prevents manual/foreign-DE launches even if env vars are spoofed.
- Startup warning now logs a parent-chain summary on failure.

## Why

Friend reported concern that panel can start in other DEs (GNOME/Plasma) and keep running even when RDM/labwc assumptions are not met.

## What Failed

- After the initial env-var-only guard, booting into Budgie still showed `rdm-panel` (unexpected).
- In an already-running RDM session started before env changes, `rdm-reload` caused `rdm-panel` to exit because `RDM_SESSION` was not present in `rdm-session` environment.

## Current Expected Behavior

- In RDM session launched via `rdm-start`: panel starts normally via `rdm-session`.
- In non-RDM session (e.g., Budgie): panel logs warning and exits without starting UI.
- Manual launch from terminal (`RDM_SESSION=1 rdm-panel`) is expected to fail now unless parent chain includes `rdm-session`.

## Validation Done

- `cargo check -p rdm-panel` passes after follow-up hardening.
- `cargo check -p rdm-session` passes after forced `RDM_SESSION` child env change.
- Confirmed at runtime: `RDM_SESSION=1 rdm-panel` can start panel before parent-chain guard; after parent-chain guard this manual launch is blocked (expected).
- Runtime confirmation for the hardened guard is still pending (needs new Budgie test after reinstall/restart).

## Next Debug Steps (When You Return)

1. Build/install updated binaries/scripts (`rdm-panel`, `rdm-session`, `rdm-start`).
2. Run cross-session sequence:
- Boot Budgie (Test A): confirm `rdm-panel` does not appear.
- Boot RDM (Test B): confirm panel appears normally.
- Boot Budgie again (Test C): confirm `rdm-panel` does not appear.
3. In RDM session:
- Confirm panel starts.
- Open WiFi menu: should show `Scanning...` first, then results.
- Confirm `Rescan` works.
4. If panel still starts in another DE:
- Print env values in that DE:
  - `echo $RDM_SESSION`
  - `echo $XDG_SESSION_TYPE`
  - `echo $XDG_CURRENT_DESKTOP`
  - `echo $XDG_SESSION_DESKTOP`
  - `echo $DESKTOP_SESSION`
- Check panel logs for startup warning (should include all vars above).
- Confirm which process launched `rdm-panel`:
  - `pgrep -af rdm-panel`
  - `ps -o ppid= -p <rdm-panel-pid> | xargs -I{} ps -fp {}`
- Check for non-RDM autostart entries:
  - `~/.config/autostart/*.desktop`
  - DE-specific startup config or user systemd services.

## Notes for Follow-up

- If false negatives happen (panel blocked in valid RDM variants), we may need to relax one marker or set it consistently in all RDM entrypoints.
- The Wayland capability guard (`zwlr_foreign_toplevel_manager_v1`) is already present as a second startup gate.
- If Budgie still launches the panel after this hardening, root cause is likely external autostart or stale installed binary/script.
