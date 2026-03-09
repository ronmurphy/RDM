#!/bin/bash
# buildall.sh — Build and install each RDM crate one at a time
# "Nuclear option" — rebuild everything from scratch, crate by crate
# Usage: ./buildall.sh

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-/usr/local}"

info()  { echo -e "${BLUE}==>${NC} ${BOLD}$1${NC}"; }
ok()    { echo -e "${GREEN}  ✓${NC} $1"; }
err()   { echo -e "${RED}  ✗ $1${NC}"; exit 1; }

cd "$SCRIPT_DIR"

# Auto-increment build number
BUILD_FILE="$SCRIPT_DIR/.build_number"
if [ -f "$BUILD_FILE" ]; then
    BUILD_NUM=$(cat "$BUILD_FILE")
else
    BUILD_NUM=0
fi
BUILD_NUM=$((BUILD_NUM + 1))
echo "$BUILD_NUM" > "$BUILD_FILE"
info "Build #$BUILD_NUM"

info "Running cargo check workspace gate..."
cargo check --workspace
ok "cargo check passed"

echo -e "${BOLD}"
echo "  ╔══════════════════════════════════════╗"
echo "  ║     RDM Desktop — Build All          ║"
echo "  ║  Crate-by-crate rebuild + install    ║"
echo "  ╚══════════════════════════════════════╝"
echo -e "${NC}"

CRATES=(
    rdm-session
    rdm-panel
    rdm-dock
    rdm-launcher
    rdm-noterm
    rdm-settings
    rdm-snap
    rdm-watermark
    rdm-notify
#    rdm-editor
)

# i may discontinue Editor, so let's not build it for now. it's a bit of a pain to maintain and test, and i don't use it myself. if there's demand for it in the future, i can always add it back.

TOTAL=${#CRATES[@]}
COUNT=0

for crate in "${CRATES[@]}"; do
    COUNT=$((COUNT + 1))
    info "[$COUNT/$TOTAL] Building $crate..."
    cargo build --release -p "$crate"
    ok "Built $crate"

    info "[$COUNT/$TOTAL] Installing $crate -> $PREFIX/bin/$crate"
    sudo install -Dm755 "target/release/$crate" "$PREFIX/bin/$crate"
    ok "Installed $crate"
    echo ""
done

# Install scripts
info "Installing scripts..."
sudo install -Dm755 scripts/rdm-start      "$PREFIX/bin/rdm-start"
sudo install -Dm755 scripts/rdm-reload     "$PREFIX/bin/rdm-reload"
sudo install -Dm755 scripts/rdm-screenshot "$PREFIX/bin/rdm-screenshot"
sudo install -Dm755 scripts/rdm-volume     "$PREFIX/bin/rdm-volume"
ok "rdm-start, rdm-reload, rdm-screenshot, rdm-volume"

echo ""
echo -e "${GREEN}${BOLD}  ✓ All $TOTAL crates built and installed.${NC}"
echo ""
echo "  Reload running session:  rdm-reload"
echo ""
