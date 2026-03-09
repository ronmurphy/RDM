#!/bin/bash
# rdm-plugin-install — Install one or more RDM panel plugin .so files
#
# Usage:
#   rdm-plugin-install file.so [file2.so ...]
#
# Intended to be called by rdm-plugin-install.desktop via drag-and-drop,
# but works equally well from the terminal.
#
# Installs to: ~/.local/share/rdm/plugins/

set -euo pipefail

DEST="${XDG_DATA_HOME:-$HOME/.local/share}/rdm/plugins"
NOTIFY_CMD=""

# Prefer notify-send for desktop notifications, fall back to zenity, then plain stderr.
if command -v notify-send >/dev/null 2>&1; then
    NOTIFY_CMD="notify-send"
elif command -v zenity >/dev/null 2>&1; then
    NOTIFY_CMD="zenity"
fi

notify_ok() {
    local msg="$1"
    if [[ "$NOTIFY_CMD" == "notify-send" ]]; then
        notify-send -i dialog-information "RDM Plugin Installer" "$msg"
    elif [[ "$NOTIFY_CMD" == "zenity" ]]; then
        zenity --info --title="RDM Plugin Installer" --text="$msg" 2>/dev/null || true
    else
        echo "[rdm-plugin-install] $msg" >&2
    fi
}

notify_err() {
    local msg="$1"
    if [[ "$NOTIFY_CMD" == "notify-send" ]]; then
        notify-send -i dialog-error "RDM Plugin Installer" "$msg"
    elif [[ "$NOTIFY_CMD" == "zenity" ]]; then
        zenity --error --title="RDM Plugin Installer" --text="$msg" 2>/dev/null || true
    else
        echo "[rdm-plugin-install] ERROR: $msg" >&2
    fi
}

# ── Validate arguments ────────────────────────────────────────────────────────

if [[ $# -eq 0 ]]; then
    notify_err "No .so file provided.\n\nDrag a plugin .so file onto the RDM Plugin Installer icon."
    exit 1
fi

# ── Create destination ────────────────────────────────────────────────────────

mkdir -p "$DEST"

# ── Install each file ─────────────────────────────────────────────────────────

INSTALLED=()
ERRORS=()

for src in "$@"; do
    # Strip file:// URI prefix if dragged from a file manager
    src="${src#file://}"
    # URL-decode %20 etc. (simple pass for most common cases)
    src="$(python3 -c "import sys, urllib.parse; print(urllib.parse.unquote(sys.argv[1]))" "$src" 2>/dev/null || echo "$src")"

    filename="$(basename "$src")"

    # Validate: must be a .so file
    if [[ "$filename" != *.so ]]; then
        ERRORS+=("'$filename' is not a .so file — skipped")
        continue
    fi

    # Validate: file must exist and be a regular file
    if [[ ! -f "$src" ]]; then
        ERRORS+=("'$filename' not found or not a regular file — skipped")
        continue
    fi

    dest_path="$DEST/$filename"

    if cp "$src" "$dest_path"; then
        chmod 755 "$dest_path"
        INSTALLED+=("$filename")
    else
        ERRORS+=("Failed to copy '$filename' to $DEST")
    fi
done

# ── Report results ────────────────────────────────────────────────────────────

if [[ ${#INSTALLED[@]} -gt 0 && ${#ERRORS[@]} -eq 0 ]]; then
    FILES="$(printf '%s\n' "${INSTALLED[@]}")"
    notify_ok "Installed ${#INSTALLED[@]} plugin(s) to $DEST:\n\n$FILES\n\nAdd a [[panel.plugins]] entry to ~/.config/rdm/rdm.toml then run: rdm-reload"

elif [[ ${#INSTALLED[@]} -gt 0 && ${#ERRORS[@]} -gt 0 ]]; then
    FILES="$(printf '%s\n' "${INSTALLED[@]}")"
    ERRS="$(printf '%s\n' "${ERRORS[@]}")"
    notify_ok "Installed:\n$FILES\n\nWarnings:\n$ERRS"

elif [[ ${#ERRORS[@]} -gt 0 ]]; then
    ERRS="$(printf '%s\n' "${ERRORS[@]}")"
    notify_err "Installation failed:\n\n$ERRS"
    exit 1
fi
