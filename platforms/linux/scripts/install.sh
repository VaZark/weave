#!/bin/sh
# Installs Weave's GUI so the desktop (GNOME Resources, the shell) shows it as an app:
# the binary, its icon and its .desktop file. Usage: install.sh [PREFIX] (default ~/.local).
# Run `cargo build --release -p weave-player-gui` first.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../.." && pwd)
prefix=${1:-$HOME/.local}
bin=${WEAVE_GUI_BIN:-$root/target/release/weave-player-gui}
id=io.github.vazark.Weave

install -Dm755 "$bin" "$prefix/bin/weave-player-gui"
install -Dm644 "$here/../icon/$id.svg" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
mkdir -p "$prefix/share/applications"
"$bin" --print-desktop-entry > "$prefix/share/applications/$id.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qt "$prefix/share/icons/hicolor" || true
echo "Installed to $prefix. Make sure $prefix/bin is on PATH."
