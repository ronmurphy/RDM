#!/bin/bash
# build-plugins.sh — Build all RDM panel plugins and stage them into plugins/
#
# Usage:
#   ./build-plugins.sh              # debug build (fast)
#   ./build-plugins.sh --release    # release build (optimised)
#
# Output:  plugins/<name>.so
#
# The staged .so files can then be:
#   • Copied manually to ~/.local/share/rdm/plugins/
#   • Installed system-wide to /usr/local/lib/rdm/plugins/
#   • Drag-and-dropped onto the plugin installer (see plugins/README_INSTALL.txt)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

BOLD='\033[1m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}==>${NC} ${BOLD}$1${NC}"; }
ok()   { echo -e "${GREEN}  ✓${NC} $1"; }

# ── Parse args ────────────────────────────────────────────────────────────────

RELEASE=0
for arg in "$@"; do
    case "$arg" in
        --release|-r) RELEASE=1 ;;
        --help|-h)
            echo "Usage: $0 [--release]"
            echo "  Builds all rdm-panel-* plugin crates and copies .so files to plugins/"
            exit 0
            ;;
    esac
done

if [[ $RELEASE -eq 1 ]]; then
    CARGO_ARGS="--release"
    TARGET_DIR="$SCRIPT_DIR/target/release"
    BUILD_TYPE="release"
else
    CARGO_ARGS=""
    TARGET_DIR="$SCRIPT_DIR/target/debug"
    BUILD_TYPE="debug"
fi

# ── Discover plugin crates ────────────────────────────────────────────────────
# Anything under crates/ whose name starts with rdm-panel- (but not rdm-panel
# itself or rdm-panel-api which is not a plugin).

PLUGINS=()
for dir in "$SCRIPT_DIR"/crates/rdm-panel-*/; do
    name="$(basename "$dir")"
    # skip the ABI crate — it's a library, not a loadable plugin
    [[ "$name" == "rdm-panel-api" ]] && continue
    PLUGINS+=("$name")
done

if [[ ${#PLUGINS[@]} -eq 0 ]]; then
    echo "No plugin crates found under crates/rdm-panel-*/"
    exit 0
fi

echo -e "${BOLD}"
echo "  ╔══════════════════════════════════════╗"
echo "  ║     RDM Panel — Plugin Builder       ║"
echo "  ╚══════════════════════════════════════╝"
echo -e "${NC}"
info "Build type: $BUILD_TYPE"
info "Plugins found: ${PLUGINS[*]}"

# ── Build ─────────────────────────────────────────────────────────────────────

info "Building plugins..."
for crate in "${PLUGINS[@]}"; do
    echo -e "  building ${BOLD}$crate${NC}..."
    cargo build $CARGO_ARGS -p "$crate" 2>&1 | grep -E "^error|Compiling $crate|Finished" || true
done
ok "Build complete"

# ── Stage into plugins/ ───────────────────────────────────────────────────────

PLUGINS_DIR="$SCRIPT_DIR/plugins"
mkdir -p "$PLUGINS_DIR"

info "Staging .so files → plugins/"
STAGED=0
for crate in "${PLUGINS[@]}"; do
    # Cargo names the .so after the lib name (underscores, not hyphens)
    lib_name="${crate//-/_}"
    so_src="$TARGET_DIR/lib${lib_name}.so"
    so_dst="$PLUGINS_DIR/lib${lib_name}.so"

    if [[ -f "$so_src" ]]; then
        cp "$so_src" "$so_dst"
        ok "plugins/lib${lib_name}.so  ($(du -h "$so_dst" | cut -f1))"
        STAGED=$((STAGED + 1))
    else
        echo -e "  ⚠  lib${lib_name}.so not found in $TARGET_DIR (build may have failed)"
    fi
done

echo ""
echo -e "${GREEN}${BOLD}  ✓ $STAGED plugin(s) staged in plugins/${NC}"
echo ""
echo "  To install for the current user:"
echo "    mkdir -p ~/.local/share/rdm/plugins"
echo "    cp plugins/*.so ~/.local/share/rdm/plugins/"
echo ""
echo "  Or drag a .so onto:  plugins/rdm-plugin-install.desktop"
echo "  See:                 plugins/README_INSTALL.txt"
echo ""
