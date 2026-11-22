#!/usr/bin/env bash
# Clean Uninstaller for locally installed Tomoyo on Linux.
set -euo pipefail

BIN_DIR="${TOMOYO_BIN_DIR:-$HOME/.local/bin}"
XDG_DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
XDG_CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA_DIR="${TOMOYO_DATA_DIR:-$XDG_DATA/tomoyo}"
PURGE=0

for arg in "$@"; do
  case "$arg" in
    --purge) PURGE=1 ;;
    --bin-dir=*) BIN_DIR="${arg#*=}" ;;
    --data-dir=*) DATA_DIR="${arg#*=}" ;;
    -h|--help)
      echo "Usage: $0 [--purge] [--bin-dir=PATH] [--data-dir=PATH]"
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user stop tomoyo-launcher.service tomoyo-tray.service >/dev/null 2>&1 || true
fi
for proc in tomoyo tomoyo-webview-host tomoyo-capture; do
  pkill -x "$proc" >/dev/null 2>&1 || true
done
sleep 0.3

for b in tomoyo tomoyo-webview-host tomoyo-capture tomoyo-cli tomoyo-plugin-hello tomoyo-plugin-ocr tomoyo-plugin-notes; do
  rm -f "$BIN_DIR/$b"
done

rm -f "$XDG_DATA/applications/tomoyo.desktop" \
  "$XDG_CFG/autostart/tomoyo.desktop" \
  "$XDG_DATA/icons/hicolor/256x256/apps/tomoyo.png" \
  "$XDG_DATA/icons/hicolor/scalable/apps/tomoyo.svg"

if [ "$PURGE" -eq 1 ]; then
  rm -rf "$DATA_DIR"
else
  rm -rf "$DATA_DIR/fonts" "$DATA_DIR/assets" "$DATA_DIR/registry" "$DATA_DIR/uninstall.sh"
  for p in example-hello example-python example-wasm example-webview-json builtin-ocr official-notes; do
    rm -rf "$DATA_DIR/plugins/$p"
  done
fi

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$XDG_DATA/applications" >/dev/null 2>&1 || true
echo "[OK] Tomoyo uninstalled cleanly$([ "$PURGE" -eq 1 ] && echo ' (including user data)' || echo ' (user config & clipboard.db preserved)')."
