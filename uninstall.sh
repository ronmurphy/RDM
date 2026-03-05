#!/bin/bash
# uninstall.sh — Remove RDM Desktop Environment
# Usage: ./uninstall.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

PREFIX="${PREFIX:-/usr/local}"

info()  { echo -e "${BLUE}==>${NC} ${BOLD}$1${NC}"; }
ok()    { echo -e "${GREEN}  ✓${NC} $1"; }

echo -e "${BOLD}"
echo "  ╔══════════════════════════════════════╗"
echo "  ║     RDM Desktop — Uninstaller        ║"
echo "  ╚══════════════════════════════════════╝"
echo -e "${NC}"

echo "This will remove RDM binaries, scripts, and the session entry."
echo "Your config files in ~/.config/rdm/ will NOT be deleted."
echo ""
read -rp "Continue? [y/N] " confirm
if [[ ! "$confirm" =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
fi

# ─── Remove binaries ───────────────────────────────────────────

info "Removing binaries from $PREFIX/bin/..."

BINARIES=(
    rdm-panel
    rdm-launcher
    rdm-session
    rdm-snap
    rdm-watermark
    rdm-settings
    rdm-notify
    rdm-start
    rdm-reload
    rdm-screenshot
)

for bin in "${BINARIES[@]}"; do
    if [ -f "$PREFIX/bin/$bin" ]; then
        sudo rm -f "$PREFIX/bin/$bin"
        ok "Removed $bin"
    fi
done

# ─── Remove session entry ──────────────────────────────────────

info "Removing session entry..."

if [ -f /usr/share/wayland-sessions/rdm.desktop ]; then
    sudo rm -f /usr/share/wayland-sessions/rdm.desktop
    ok "Removed /usr/share/wayland-sessions/rdm.desktop"
fi

# ─── Remove D-Bus service ─────────────────────────────────────────

info "Removing D-Bus service file..."

DBUS_SERVICE="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services/org.freedesktop.Notifications.service"
if [ -f "$DBUS_SERVICE" ]; then
    rm -f "$DBUS_SERVICE"
    ok "Removed $DBUS_SERVICE"
fi

# ─── Done ───────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}  ✓ RDM Desktop uninstalled.${NC}"
echo ""
echo "  Your config files are preserved at:"
echo "    ~/.config/rdm/rdm.toml"
echo "    ~/.config/rdm/session.toml"
echo ""
echo "  To also remove configs:  rm -rf ~/.config/rdm"
echo "  To remove labwc config:  rm ~/.config/labwc/rc.xml"
echo ""
