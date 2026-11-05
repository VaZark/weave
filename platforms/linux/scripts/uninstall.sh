#!/bin/sh
# Removes what install.sh put in PREFIX (default ~/.local): the binary, icon and .desktop file.
# Your library and settings (~/.local/share/weave, ~/.config) are left alone.
# Usage: uninstall.sh [PREFIX]
set -eu
prefix=${1:-$HOME/.local}
id=io.github.vazark.Weave

rm -f "$prefix/bin/weave-player-gui" \
  "$prefix/share/icons/hicolor/scalable/apps/$id.svg" \
  "$prefix/share/applications/$id.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -qt "$prefix/share/icons/hicolor" || true
echo "Uninstalled from $prefix."
