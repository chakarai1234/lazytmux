#!/usr/bin/env sh
set -eu

PROJECT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BIN_DIR="$HOME/.local/bin"
BIN_NAME="lazytmux"
RELEASE_BIN="$PROJECT_DIR/target/release/$BIN_NAME"
TMP_BIN="$BIN_DIR/$BIN_NAME.tmp"
DEST_BIN="$BIN_DIR/$BIN_NAME"

if ! command -v cargo >/dev/null 2>&1; then
  printf '%s\n' "cargo is required but was not found in PATH." >&2
  exit 1
fi

cd "$PROJECT_DIR"
cargo build --release

mkdir -p "$BIN_DIR"
install -m 755 "$RELEASE_BIN" "$TMP_BIN"
mv -f "$TMP_BIN" "$DEST_BIN"

printf '%s\n' "Installed $BIN_NAME to $DEST_BIN"
printf '%s\n' "Make sure $BIN_DIR is in your PATH."
