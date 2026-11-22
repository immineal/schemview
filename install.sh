#!/bin/sh
# Install schemview for the current user (Linux) and make it the default app for
# .litematic, .schematic and .schem files.
#
#   ./install.sh              install (uses ./schemview if present, otherwise builds with cargo)
#   ./install.sh --uninstall  remove it again
set -eu
cd "$(dirname "$0")"
BIN_DIR="$HOME/.local/bin"
D="${XDG_DATA_HOME:-$HOME/.local/share}"
TYPES="application/x-litematic application/x-minecraft-schematic application/x-sponge-schematic"

refresh() {
    update-mime-database "$D/mime" 2>/dev/null || true
    update-desktop-database "$D/applications" 2>/dev/null || true
    gtk-update-icon-cache -q -t "$D/icons/hicolor" 2>/dev/null || true
    command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$BIN_DIR/schemview" "$D/applications/schemview.desktop" \
          "$D/icons/hicolor/scalable/apps/schemview.svg" "$D/mime/packages/schemview.xml"
    refresh
    echo "schemview removed"
    exit 0
fi

if [ -x ./schemview ]; then
    BIN=./schemview                     # release archive
else
    cargo build --release
    BIN="$(cargo metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p')/release/schemview"
fi

install -Dm755 "$BIN" "$BIN_DIR/schemview"
install -Dm644 desktop/schemview.svg "$D/icons/hicolor/scalable/apps/schemview.svg"
install -Dm644 desktop/schemview-mime.xml "$D/mime/packages/schemview.xml"
mkdir -p "$D/applications"
sed "s|^Exec=schemview|Exec=$BIN_DIR/schemview|; s|^TryExec=schemview|TryExec=$BIN_DIR/schemview|" \
    desktop/schemview.desktop > "$D/applications/schemview.desktop"
refresh
if command -v xdg-mime >/dev/null; then
    for t in $TYPES; do xdg-mime default schemview.desktop "$t"; done
fi
echo "installed: $BIN_DIR/schemview"
