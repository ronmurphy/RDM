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

# ── --plugins mode ────────────────────────────────────────────────────────────
# Build only plugin cdylib crates and stage .so files into plugins/

if [[ "${1:-}" == "--plugins" ]]; then
    RELEASE=0
    [[ "${2:-}" == "--release" ]] && RELEASE=1

    if [[ $RELEASE -eq 1 ]]; then
        CARGO_ARGS="--release"
        TARGET_DIR="$SCRIPT_DIR/target/release"
        BUILD_TYPE="release"
    else
        CARGO_ARGS=""
        TARGET_DIR="$SCRIPT_DIR/target/debug"
        BUILD_TYPE="debug"
    fi

    echo -e "${BOLD}"
    echo "  ╔══════════════════════════════════════╗"
    echo "  ║     RDM Desktop — Plugin Builder     ║"
    echo "  ╚══════════════════════════════════════╝"
    echo -e "${NC}"
    info "Build type: $BUILD_TYPE"

    PLUGIN_CRATES=()
    for dir in "$SCRIPT_DIR"/crates/rdm-panel-*/; do
        name="$(basename "$dir")"
        [[ "$name" == "rdm-panel-api" ]] && continue
        PLUGIN_CRATES+=("$name")
    done

    if [[ ${#PLUGIN_CRATES[@]} -eq 0 ]]; then
        echo "No plugin crates found under crates/rdm-panel-*/"
        exit 0
    fi

    info "Plugins found: ${PLUGIN_CRATES[*]}"

    PLUGINS_DIR="$SCRIPT_DIR/plugins"
    mkdir -p "$PLUGINS_DIR"

    STAGED=0
    for crate in "${PLUGIN_CRATES[@]}"; do
        info "Building $crate ($BUILD_TYPE)..."
        cargo build $CARGO_ARGS -p "$crate"
        ok "Built $crate"

        lib_name="${crate//-/_}"
        so_src="$TARGET_DIR/lib${lib_name}.so"
        so_dst="$PLUGINS_DIR/lib${lib_name}.so"

        if [[ -f "$so_src" ]]; then
            cp "$so_src" "$so_dst"
            ok "Staged → plugins/lib${lib_name}.so  ($(du -h "$so_dst" | cut -f1))"
            STAGED=$((STAGED + 1))
        else
            err "lib${lib_name}.so not found in $TARGET_DIR"
        fi
    done

    echo ""
    echo -e "${GREEN}${BOLD}  ✓ $STAGED plugin(s) built and staged in plugins/${NC}"
    echo ""
    echo "  To install for the current user:"
    echo "    mkdir -p ~/.local/share/rdm/plugins"
    echo "    cp plugins/*.so ~/.local/share/rdm/plugins/"
    echo ""
    exit 0
fi

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
