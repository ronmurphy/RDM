#!/bin/bash
# install.sh — Build and install RDM Desktop Environment
# Usage: ./install.sh
# Requires: rust/cargo, sudo access

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-/usr/local}"

info()  { echo -e "${BLUE}==>${NC} ${BOLD}$1${NC}"; }
ok()    { echo -e "${GREEN}  ✓${NC} $1"; }
err()   { echo -e "${RED}  ✗ $1${NC}"; }

echo -e "${BOLD}"
echo "  ╔══════════════════════════════════════╗"
echo "  ║     RDM Desktop — Installer          ║"
echo "  ║     Rust Desktop Manager for Wayland  ║"
echo "  ╚══════════════════════════════════════╝"
echo -e "${NC}"

# ─── Check prerequisites ───────────────────────────────────────

info "Checking prerequisites..."

missing=()

command -v cargo  >/dev/null 2>&1 || missing+=("rust/cargo")
command -v labwc  >/dev/null 2>&1 || missing+=("labwc")
command -v swaybg >/dev/null 2>&1 || missing+=("swaybg")
command -v foot   >/dev/null 2>&1 || missing+=("foot")
command -v grim   >/dev/null 2>&1 || missing+=("grim")
command -v slurp  >/dev/null 2>&1 || missing+=("slurp")
command -v wpctl  >/dev/null 2>&1 || missing+=("wireplumber")

if pkg-config --exists gtk4 2>/dev/null; then
    ok "gtk4"
else
    missing+=("gtk4")
fi

if pkg-config --exists gtk4-layer-shell-0 2>/dev/null; then
    ok "gtk4-layer-shell"
else
    missing+=("gtk4-layer-shell")
fi

if pkg-config --exists gtksourceview-5 2>/dev/null; then
    ok "gtksourceview5"
else
    missing+=("gtksourceview5")
fi

if pkg-config --exists webkitgtk-6.0 2>/dev/null; then
    ok "webkit2gtk-6.0"
else
    missing+=("webkit2gtk-6.0 (optional, for rdm-editor preview)")
fi

if [ ${#missing[@]} -gt 0 ]; then
    err "Missing dependencies: ${missing[*]}"
    echo ""
    echo "  Install on Arch Linux:"
    echo "    sudo pacman -S labwc swaybg foot rust gtk4 gtk4-layer-shell gtksourceview5 webkit2gtk-6.0 grim slurp wl-clipboard wireplumber networkmanager"
    echo ""
    echo "  Then re-run this script."
    exit 1
fi

ok "All prerequisites found"

# ─── Build ──────────────────────────────────────────────────────

info "Building RDM Desktop (release mode)..."
cd "$SCRIPT_DIR"
cargo build --release

ok "Build complete"

# ─── Install binaries ──────────────────────────────────────────

info "Installing binaries to $PREFIX/bin/ (requires sudo)..."

sudo install -Dm755 target/release/rdm-panel     "$PREFIX/bin/rdm-panel"
sudo install -Dm755 target/release/rdm-launcher   "$PREFIX/bin/rdm-launcher"
sudo install -Dm755 target/release/rdm-session    "$PREFIX/bin/rdm-session"
sudo install -Dm755 target/release/rdm-snap       "$PREFIX/bin/rdm-snap"
sudo install -Dm755 target/release/rdm-watermark  "$PREFIX/bin/rdm-watermark"
sudo install -Dm755 target/release/rdm-settings   "$PREFIX/bin/rdm-settings"
sudo install -Dm755 target/release/rdm-notify    "$PREFIX/bin/rdm-notify"
sudo install -Dm755 target/release/rdm-dock      "$PREFIX/bin/rdm-dock"
#sudo install -Dm755 target/release/rdm-editor    "$PREFIX/bin/rdm-editor"#

ok "rdm-panel, rdm-launcher, rdm-session, rdm-snap, rdm-watermark, rdm-settings, rdm-notify, rdm-dock, rdm-editor"

# ─── Install scripts ───────────────────────────────────────────

info "Installing scripts..."

sudo install -Dm755 scripts/rdm-start      "$PREFIX/bin/rdm-start"
sudo install -Dm755 scripts/rdm-reload     "$PREFIX/bin/rdm-reload"
sudo install -Dm755 scripts/rdm-screenshot "$PREFIX/bin/rdm-screenshot"
sudo install -Dm755 scripts/rdm-volume     "$PREFIX/bin/rdm-volume"
sudo install -Dm755 plugins/rdm-plugin-install.sh "$PREFIX/bin/rdm-plugin-install"

ok "rdm-start, rdm-reload, rdm-screenshot, rdm-volume, rdm-plugin-install"

# ─── Install session entry ─────────────────────────────────────

info "Registering RDM as a Wayland session..."

sudo install -Dm644 config/rdm.desktop           /usr/share/wayland-sessions/rdm.desktop
sudo install -Dm644 config/rdm-editor.desktop    /usr/share/applications/rdm-editor.desktop
sudo install -Dm644 config/rdm-settings.desktop  /usr/share/applications/rdm-settings.desktop
sudo install -Dm644 config/rdm-noterm.desktop    /usr/share/applications/rdm-noterm.desktop
sudo install -Dm644 config/rdm-launcher.desktop  /usr/share/applications/rdm-launcher.desktop

ok "Session entry: /usr/share/wayland-sessions/rdm.desktop"
ok "App entries:   rdm-editor, rdm-settings, rdm-noterm, rdm-launcher"

# ─── Install icons ─────────────────────────────────────────

info "Installing icons..."

sudo install -Dm644 assets/rdm-settings.svg  /usr/share/icons/hicolor/scalable/apps/rdm-settings.svg
sudo install -Dm644 assets/rdm-noterm.svg    /usr/share/icons/hicolor/scalable/apps/rdm-noterm.svg
sudo install -Dm644 assets/rdm-launcher.svg  /usr/share/icons/hicolor/scalable/apps/rdm-launcher.svg
sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true

ok "rdm-settings.svg, rdm-noterm.svg, rdm-launcher.svg → /usr/share/icons/hicolor/scalable/apps/"

# ─── Install D-Bus service for rdm-notify ──────────────────────

info "Installing D-Bus activation service for rdm-notify..."

DBUS_SERVICES="${XDG_DATA_HOME:-$HOME/.local/share}/dbus-1/services"
mkdir -p "$DBUS_SERVICES"
# Write with the correct install prefix
cat > "$DBUS_SERVICES/org.freedesktop.Notifications.service" <<DBUSEOF
[D-BUS Service]
Name=org.freedesktop.Notifications
Exec=$PREFIX/bin/rdm-notify
DBUSEOF

ok "D-Bus service: $DBUS_SERVICES/org.freedesktop.Notifications.service"

# ─── Copy default configs ──────────────────────────────────────

info "Setting up default configuration..."

RDM_CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/rdm"
mkdir -p "$RDM_CONFIG"

if [ ! -f "$RDM_CONFIG/rdm.toml" ]; then
    cp config/rdm.toml "$RDM_CONFIG/"
    ok "Copied rdm.toml → $RDM_CONFIG/"
else
    ok "rdm.toml already exists (not overwriting)"
fi

if [ ! -f "$RDM_CONFIG/session.toml" ]; then
    cp config/session.toml "$RDM_CONFIG/"
    ok "Copied session.toml → $RDM_CONFIG/"
else
    ok "session.toml already exists (not overwriting)"
fi

# ─── Copy labwc config ─────────────────────────────────────────

LABWC_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/labwc"
mkdir -p "$LABWC_DIR"

if [ ! -f "$LABWC_DIR/rc.xml" ]; then
    cp config/labwc-rc.xml "$LABWC_DIR/rc.xml"
    ok "Copied labwc-rc.xml → $LABWC_DIR/rc.xml"
else
    ok "labwc rc.xml already exists (not overwriting)"
fi

# ─── Done ───────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}  ✓ RDM Desktop installed successfully!${NC}"
echo ""
echo "  Next steps:"
echo "    1. Log out of your current session"
echo "    2. Select \"RDM Desktop\" from your display manager (SDDM, GDM, etc.)"
echo "    3. Or from a TTY:  exec rdm-start"
echo ""
echo "  After making code changes:"
echo "    cargo build --release && sudo install -m755 target/release/<crate> $PREFIX/bin/"
echo "    rdm-reload"
echo ""
echo "  Settings:  rdm-settings"
echo "  Uninstall: ./uninstall.sh"
echo ""
