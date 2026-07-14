#!/usr/bin/env sh
# 從 GitHub Release 安裝預編譯 binary；不需要 Rust、Cargo 或 Git。
set -eu

RELEASE_BASE_URL="${FREECLAUDE_RELEASE_BASE_URL:-https://github.com/mushroomTW/FreeClaudeDesktop/releases/latest/download}"
INSTALL_DIR="${FREECLAUDE_INSTALL_DIR:-$HOME/.local/share/freeclaude}"
BIN_DIR="${FREECLAUDE_BIN_DIR:-$HOME/.local/bin}"
WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT INT TERM

command -v curl >/dev/null 2>&1 || { echo "找不到 curl。" >&2; exit 1; }
command -v tar >/dev/null 2>&1 || { echo "找不到 tar。" >&2; exit 1; }

case "$(uname -s)" in
  Linux) OS="unknown-linux-gnu" ;;
  Darwin) OS="apple-darwin" ;;
  *) echo "不支援的作業系統：$(uname -s)" >&2; exit 1 ;;
esac

case "$(uname -m)" in
  x86_64|amd64) ARCH="x86_64" ;;
  aarch64|arm64) ARCH="aarch64" ;;
  *) echo "不支援的 CPU 架構：$(uname -m)" >&2; exit 1 ;;
esac

TARGET="$ARCH-$OS"
ARCHIVE="freeclaude-$TARGET.tar.gz"
curl -fL --retry 3 "$RELEASE_BASE_URL/$ARCHIVE" -o "$WORK_DIR/$ARCHIVE"
curl -fL --retry 3 "$RELEASE_BASE_URL/checksums.txt" -o "$WORK_DIR/checksums.txt"

expected_checksum=$(awk -v archive="$ARCHIVE" '$2 == archive || $2 == "*" archive { print $1; exit }' "$WORK_DIR/checksums.txt")
[ -n "$expected_checksum" ] || { echo "checksums.txt 不包含 $ARCHIVE。" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then
  actual_checksum=$(sha256sum "$WORK_DIR/$ARCHIVE" | awk '{print $1}')
else
  actual_checksum=$(shasum -a 256 "$WORK_DIR/$ARCHIVE" | awk '{print $1}')
fi
[ "$expected_checksum" = "$actual_checksum" ] || { echo "下載檔案的 SHA-256 驗證失敗。" >&2; exit 1; }

tar -xzf "$WORK_DIR/$ARCHIVE" -C "$WORK_DIR"

mkdir -p "$INSTALL_DIR" "$BIN_DIR"
install -m 755 "$WORK_DIR/freeclaude" "$INSTALL_DIR/freeclaude"
install -m 755 "$WORK_DIR/freeclaude-proxy" "$INSTALL_DIR/freeclaude-proxy"
ln -sfn "$INSTALL_DIR/freeclaude" "$BIN_DIR/freeclaude"

echo "FreeClaudeDesktop 已安裝至：$INSTALL_DIR"
if ! command -v freeclaude >/dev/null 2>&1; then
  echo "請將 $BIN_DIR 加入 PATH，然後執行：freeclaude install"
else
  echo "下一步：freeclaude install"
fi
