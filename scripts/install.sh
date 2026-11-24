#!/bin/sh
# Install Alice Agent from source (or from a prebuilt binary).
#
#   scripts/install.sh                 build with cargo and install to ~/.local/bin
#   scripts/install.sh --bin ./alice   install a prebuilt binary
#   scripts/install.sh --uninstall     remove the binary and the default service
#
# PREFIX (default ~/.local) can be overridden: PREFIX=/usr/local scripts/install.sh
set -eu

PREFIX="${PREFIX:-$HOME/.local}"
DEST="$PREFIX/bin/alice"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64|Linux-x86_64|Linux-aarch64) ;;
  *) echo "warning: untested platform $(uname -s)-$(uname -m)" >&2 ;;
esac

if [ "${1:-}" = "--uninstall" ]; then
  if [ -x "$DEST" ]; then
    "$DEST" stop 2>/dev/null || true
    "$DEST" service uninstall 2>/dev/null || true
    rm -f "$DEST"
    echo "Removed $DEST. Your data in ~/.alice was kept (delete it yourself if you want)."
  else
    echo "alice is not installed at $DEST"
  fi
  exit 0
fi

if [ "${1:-}" = "--bin" ]; then
  SRC="${2:?usage: install.sh --bin <path>}"
else
  command -v cargo >/dev/null 2>&1 || { echo "cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
  cd "$(dirname "$0")/.."
  cargo build --release --locked
  SRC=target/release/alice
fi

mkdir -p "$PREFIX/bin"
# Replace atomically so a running server keeps its old binary until restart.
cp "$SRC" "$DEST.tmp"
chmod 755 "$DEST.tmp"
mv "$DEST.tmp" "$DEST"
echo "Installed $DEST ($("$DEST" --version))"

case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) echo "Add $PREFIX/bin to your PATH, e.g.: echo 'export PATH=\"$PREFIX/bin:\$PATH\"' >> ~/.profile" ;;
esac

if "$DEST" status 2>/dev/null | grep -q running; then
  echo "A server is running; restart it to use the new version: alice restart"
else
  echo "Next: alice setup && alice start && alice chat"
fi
