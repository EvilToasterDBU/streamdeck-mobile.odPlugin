#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONFIG_DIR="${OPENDECK_CONFIG_DIR:-$HOME/.config/opendeck}"
PLUGIN_DIR="$CONFIG_DIR/plugins"
BUNDLE="$ROOT/com.toshtaru.streamdeck-mobile.sdPlugin"
DEST="$PLUGIN_DIR/com.toshtaru.streamdeck-mobile.sdPlugin"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
SRC_BIN="$ROOT/target/release/opendeck-streamdeck-mobile"
BIN="$BUNDLE/bin/$TARGET/opendeck-streamdeck-mobile"
EXPECTED="$(tr -d '[:space:]' < "$ROOT/VERSION")"

usage(){ cat <<USAGE
Usage: $0 [--build] [--reset-pairing] [--verify]
  --build          Force a clean release rebuild (recommended)
  --reset-pairing  Delete saved Mobile trust
  --verify         Verify installed manifest, binary and running process
USAGE
}
BUILD=0; RESET=0; VERIFY=0
for a in "$@"; do case "$a" in
  --build) BUILD=1;; --reset-pairing) RESET=1;; --verify) VERIFY=1;; -h|--help) usage; exit 0;;
  *) echo "Unknown option: $a" >&2; exit 2;; esac; done

command -v cargo >/dev/null 2>&1 || { echo "ERROR: cargo not found" >&2; exit 1; }
command -v strings >/dev/null 2>&1 || { echo "ERROR: strings not found" >&2; exit 1; }
[[ -f "$BUNDLE/manifest.json" ]] || { echo "ERROR: bundle missing: $BUNDLE" >&2; exit 1; }

# Never trust a pre-existing target/release binary. This is what caused version
# skew between the source archive and the binary actually launched by OpenDeck.
echo "==> Cleaning previous release binary"
rm -f "$SRC_BIN"
rm -rf "$ROOT/target/release/.fingerprint/opendeck-streamdeck-mobile" "$ROOT/target/release/build/opendeck-streamdeck-mobile-*" 2>/dev/null || true

echo "==> cargo clean -p opendeck-streamdeck-mobile"
(cd "$ROOT" && cargo clean -p opendeck-streamdeck-mobile >/dev/null 2>&1 || true)
echo "==> cargo build --release"
(cd "$ROOT" && cargo build --release)
[[ -x "$SRC_BIN" ]] || { echo "ERROR: no release binary produced" >&2; exit 1; }

mkdir -p "$(dirname "$BIN")"
cp -f "$SRC_BIN" "$BIN"
chmod 755 "$BIN"

# Stop an old instance before replacing the installed executable.
pkill -f '/com.toshtaru.streamdeck-mobile.sdPlugin/bin/.*opendeck-streamdeck-mobile' 2>/dev/null || true
sleep 0.2

mkdir -p "$PLUGIN_DIR"
rm -rf "$DEST"
cp -a "$BUNDLE" "$DEST"
chmod 755 "$DEST/bin/$TARGET/opendeck-streamdeck-mobile"

if (( RESET )); then
  rm -f "$CONFIG_DIR/streamdeck-mobile.json"
  echo "==> Pairing state reset"
fi

INST_MANIFEST="$(sed -n 's/.*"Version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$DEST/manifest.json" | head -n1)"
CARGO_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)"
[[ "$INST_MANIFEST" == "$EXPECTED" ]] || { echo "ERROR: manifest version $INST_MANIFEST != $EXPECTED" >&2; exit 1; }
[[ "$CARGO_VERSION" == "$EXPECTED" ]] || { echo "ERROR: Cargo version $CARGO_VERSION != $EXPECTED" >&2; exit 1; }

# The package version is compiled into the Rust binary by env!(CARGO_PKG_VERSION).
# Release builds (strip+lto) pack adjacent &str literals with no separator, so a
# whole-line match against `strings` output is unreliable; search for the
# version as a substring instead. `strings` runs to completion into a variable
# (rather than piping straight into `grep -q`) so its early exit can't trigger
# a SIGPIPE that `pipefail` would otherwise misreport as a real failure.
BINARY_STRINGS="$(strings "$DEST/bin/$TARGET/opendeck-streamdeck-mobile")"
if ! grep -qF "$EXPECTED" <<< "$BINARY_STRINGS"; then
  echo "ERROR: binary version '$EXPECTED' not found in embedded strings" >&2
  echo "Binary: $DEST/bin/$TARGET/opendeck-streamdeck-mobile" >&2
  exit 1
fi

echo
echo "OpenDeck Stream Deck Mobile installed: v$EXPECTED"
echo "  bundle : $DEST"
echo "  binary : $DEST/bin/$TARGET/opendeck-streamdeck-mobile"
echo "  VSD2   : 2.14.0"

if (( VERIFY )); then
  echo "==> Installed verification OK"
  pgrep -af 'opendeck-streamdeck-mobile' || echo "  running process: not running (restart OpenDeck)"
fi
