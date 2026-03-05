#!/bin/bash
# install.sh — Build and install RDM Desktop Environment (QML edition)
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
echo "  ║     RDM Desktop — Installer (QML)    ║"
echo "  ║     Rust Desktop Manager for Wayland  ║"
echo "  ╚══════════════════════════════════════╝"
echo -e "${NC}"

# ─── Check prerequisites ───────────────────────────────────────

info "Checking prerequisites..."

missing=()

command -v cargo  >/dev/null 2>&1 || missing+=("rust/cargo")
command -v labwc  >/dev/null 2>&1 || missing+=("labwc")
command -v swaybg >/dev/null 2>&1 || missing+=("swaybg")
command -v mako   >/dev/null 2>&1 || missing+=("mako")
command -v foot   >/dev/null 2>&1 || missing+=("foot")

# Check for Qt (try Qt6 first, then Qt5)
qt_found=false
if pkg-config --exists Qt6Quick 2>/dev/null; then
    ok "Qt6 Quick"
    qt_found=true
elif pkg-config --exists Qt5Quick 2>/dev/null; then
    ok "Qt5 Quick"
    qt_found=true
fi

if ! $qt_found; then
    missing+=("qt6-declarative (Qt Quick/QML)")
fi

# layer-shell-qt: pkg-config name varies by distro and Qt version
# Arch/KDE Plasma 6: "LayerShellQt", some older builds: "LayerShellQtInterface"
if pkg-config --exists LayerShellQt 2>/dev/null || pkg-config --exists LayerShellQtInterface 2>/dev/null; then
    ok "layer-shell-qt"
else
    missing+=("layer-shell-qt")
fi

if [ ${#missing[@]} -gt 0 ]; then
    err "Missing dependencies: ${missing[*]}"
    echo ""
    echo "  Install on Arch Linux:"
    echo "    sudo pacman -S labwc swaybg swaylock mako foot rust qt6-base qt6-declarative qt6-wayland layer-shell-qt networkmanager"
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

ok "rdm-panel, rdm-launcher, rdm-session, rdm-snap, rdm-watermark, rdm-settings"

# ─── Install scripts ───────────────────────────────────────────

info "Installing scripts..."

sudo install -Dm755 scripts/rdm-start   "$PREFIX/bin/rdm-start"
sudo install -Dm755 scripts/rdm-reload  "$PREFIX/bin/rdm-reload"

ok "rdm-start, rdm-reload"

# ─── Install session entry ─────────────────────────────────────

info "Registering RDM as a Wayland session..."

sudo install -Dm644 config/rdm.desktop /usr/share/wayland-sessions/rdm.desktop

ok "Session entry: /usr/share/wayland-sessions/rdm.desktop"

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
