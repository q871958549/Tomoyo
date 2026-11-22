#!/usr/bin/env bash
# Local Linux Installation Script for Tomoyo (Arch / CachyOS / KDE Plasma / Freedesktop).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="$REPO_ROOT/target/release"
BIN_DIR="${TOMOYO_BIN_DIR:-$HOME/.local/bin}"
XDG_DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
XDG_CFG="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA_DIR="${TOMOYO_DATA_DIR:-$XDG_DATA/tomoyo}"

NO_LAUNCH=0
ENABLE_AUTOSTART=0
BUILD_FIRST=0
for arg in "$@"; do
  case "$arg" in
    --no-launch) NO_LAUNCH=1 ;;
    --enable-autostart) ENABLE_AUTOSTART=1 ;;
    --build) BUILD_FIRST=1 ;;
    --bin-dir=*) BIN_DIR="${arg#*=}" ;;
    --data-dir=*) DATA_DIR="${arg#*=}" ;;
    -h|--help)
      echo "Usage: $0 [--build] [--no-launch] [--enable-autostart] [--bin-dir=PATH] [--data-dir=PATH]"
      exit 0
      ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

if [ "$BUILD_FIRST" -eq 1 ]; then
  (cd "$REPO_ROOT/frontend" && npm run build)
  cargo build --manifest-path "$REPO_ROOT/Cargo.toml" --release --workspace
fi

if [ ! -x "$RELEASE_DIR/tomoyo" ]; then
  echo "Release binary not found at $RELEASE_DIR/tomoyo. Run 'cargo build --release --workspace' or pass --build." >&2
  exit 1
fi

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$REPO_ROOT/Cargo.toml" | head -n1)"
VERSION="${VERSION:-2.3.0}"
echo "=== Installing Tomoyo v$VERSION -> $BIN_DIR & $DATA_DIR ==="

# Stop any running Tomoyo instances safely before overwriting binaries
if command -v systemctl >/dev/null 2>&1; then
  systemctl --user stop tomoyo-launcher.service tomoyo-tray.service >/dev/null 2>&1 || true
  systemctl --user reset-failed tomoyo-launcher.service tomoyo-tray.service >/dev/null 2>&1 || true
  AUTOSTART_CGROUP="/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/app.slice/app-tomoyo@autostart.service/cgroup.procs"
  if [ -r "$AUTOSTART_CGROUP" ]; then
    foreign_procs=""
    while IFS= read -r pid; do
      [ -n "$pid" ] || continue
      comm="$(cat "/proc/$pid/comm" 2>/dev/null || true)"
      case "$comm" in
        ""|tomoyo|tomoyo-*) ;;
        *) foreign_procs="${foreign_procs}${comm}($pid) " ;;
      esac
    done < "$AUTOSTART_CGROUP"
    if [ -z "$foreign_procs" ]; then
      systemctl --user stop app-tomoyo@autostart.service >/dev/null 2>&1 || true
    else
      echo "[WARN] app-tomoyo@autostart.service still holds legacy child processes: $foreign_procs" >&2
      echo "       Leaving those apps running; restart them once to clear the old autostart cgroup." >&2
    fi
  fi
fi
for proc in tomoyo tomoyo-webview-host tomoyo-capture; do
  pkill -x "$proc" >/dev/null 2>&1 || true
done
sleep 0.3

mkdir -p "$BIN_DIR" "$DATA_DIR/fonts" "$DATA_DIR/assets" "$DATA_DIR/registry" \
  "$XDG_DATA/applications" \
  "$XDG_DATA/icons/hicolor/256x256/apps" \
  "$XDG_DATA/icons/hicolor/scalable/apps"

BINS=(
  tomoyo
  tomoyo-webview-host
  tomoyo-capture
  tomoyo-cli
  tomoyo-plugin-hello
  tomoyo-plugin-ocr
  tomoyo-plugin-notes
)
for b in "${BINS[@]}"; do
  install -Dm755 "$RELEASE_DIR/$b" "$BIN_DIR/$b"
done

install -Dm644 "$REPO_ROOT/LICENSE-MIT" "$DATA_DIR/LICENSE-MIT"
install -Dm644 "$REPO_ROOT/LICENSE-APACHE" "$DATA_DIR/LICENSE-APACHE"
install -Dm644 "$REPO_ROOT/README.md" "$DATA_DIR/README.md"
install -Dm644 "$REPO_ROOT"/assets/fonts/*.ttf "$DATA_DIR/fonts/"
install -Dm644 "$REPO_ROOT/assets/tomoyo-icon.png" "$DATA_DIR/assets/tomoyo-icon.png"
install -Dm644 "$REPO_ROOT/registry/index.json" "$DATA_DIR/registry/index.json"
install -Dm755 "$REPO_ROOT/scripts/uninstall-local.sh" "$DATA_DIR/uninstall.sh"

for p in example-hello example-python example-wasm example-webview-json builtin-ocr official-notes; do
  rm -rf "$DATA_DIR/plugins/$p"
  mkdir -p "$DATA_DIR/plugins/$p"
  cp -a "$REPO_ROOT/plugins/$p/." "$DATA_DIR/plugins/$p/"
  rm -rf "$DATA_DIR/plugins/$p/target" "$DATA_DIR/plugins/$p/__pycache__"
done

install -Dm644 "$REPO_ROOT/assets/tomoyo-icon.png" "$XDG_DATA/icons/hicolor/256x256/apps/tomoyo.png"
install -Dm644 "$REPO_ROOT/assets/tomoyo-icon.svg" "$XDG_DATA/icons/hicolor/scalable/apps/tomoyo.svg"

DESKTOP_DST="$XDG_DATA/applications/tomoyo.desktop"
sed \
  -e "s|^Exec=tomoyo\$|Exec=$BIN_DIR/tomoyo|" \
  -e "s|^Exec=tomoyo --silent\$|Exec=$BIN_DIR/tomoyo --silent|" \
  -e "s|^Exec=tomoyo-webview-host --settings\$|Exec=$BIN_DIR/tomoyo-webview-host --settings|" \
  "$REPO_ROOT/packaging/linux/tomoyo.desktop" > "$DESKTOP_DST"
chmod 644 "$DESKTOP_DST"

if [ "$ENABLE_AUTOSTART" -eq 1 ]; then
  mkdir -p "$XDG_CFG/autostart"
  cat > "$XDG_CFG/autostart/tomoyo.desktop" <<EOF
[Desktop Entry]
Type=Application
Version=1.0
Name=Tomoyo
Comment=Low-memory open desktop launcher
Exec="$BIN_DIR/tomoyo" --silent
Icon=tomoyo
Terminal=false
Categories=Utility;
X-KDE-autostart-after=panel
X-GNOME-Autostart-enabled=true
EOF
fi

command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$XDG_DATA/applications" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -f -t "$XDG_DATA/icons/hicolor" >/dev/null 2>&1 || true

echo "[OK] Installed Tomoyo v$VERSION binaries -> $BIN_DIR"
echo "[OK] Desktop entry -> $DESKTOP_DST"

if [ "$NO_LAUNCH" -eq 0 ]; then
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user reset-failed tomoyo-launcher.service >/dev/null 2>&1 || true
  fi
  if ! (command -v systemd-run >/dev/null 2>&1 \
    && systemd-run --user --unit=tomoyo-launcher --collect --quiet --slice=app.slice \
      --description="Tomoyo" --property=ExitType=cgroup --working-directory="$HOME" \
      -- "$BIN_DIR/tomoyo" --silent >/dev/null 2>&1); then
    nohup "$BIN_DIR/tomoyo" --silent </dev/null >/dev/null 2>&1 &
  fi
  sleep 1.0
  if pid=$(pgrep -x tomoyo | head -n1); then
    echo "[OK] Tomoyo running in tray (PID=$pid). Check global hotkey or tray icon to open."
  else
    echo "[WARN] Tomoyo did not stay running; inspect $XDG_DATA/tomoyo/logs/tomoyo.log" >&2
  fi
fi
