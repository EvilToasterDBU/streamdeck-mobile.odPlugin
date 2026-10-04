#!/usr/bin/env bash
# Builds the release binary and packages the plugin as an .odPlugin archive
# (dist/com.toshtaru.streamdeck-mobile-v<version>.odPlugin).
#
# An .odPlugin file is a plain zip whose only top-level entry is the
# com.toshtaru.streamdeck-mobile.sdPlugin folder. That is what OpenDeck's
# Plugins -> "Install from file" expects: it finds the *.sdPlugin folder in the
# archive and extracts everything into its plugins directory, so nothing else
# (README, scripts, ...) may sit next to that folder.
#
# Environment:
#   TARGET      target triple of the bundled binary (default x86_64-unknown-linux-gnu)
#   SKIP_BUILD  set to 1 to package an already built target/release binary
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

TARGET="${TARGET:-x86_64-unknown-linux-gnu}"
PLUGIN_ID="com.toshtaru.streamdeck-mobile"
BUNDLE_NAME="$PLUGIN_ID.sdPlugin"
BUNDLE="$ROOT/$BUNDLE_NAME"
BIN_NAME="opendeck-streamdeck-mobile"
VERSION="$(tr -d '[:space:]' < "$ROOT/VERSION")"
DIST="$ROOT/dist"
ARCHIVE="$DIST/$PLUGIN_ID-v$VERSION.odPlugin"

# VERSION, Cargo.toml and the manifest must agree (install-plugin.sh enforces
# the same thing); a mismatch here means a release would ship a wrong version.
CARGO_VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n1)"
MANIFEST_VERSION="$(sed -n 's/.*"Version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$BUNDLE/manifest.json" | head -n1)"
if [[ "$CARGO_VERSION" != "$VERSION" || "$MANIFEST_VERSION" != "$VERSION" ]]; then
  echo "ERROR: version mismatch: VERSION=$VERSION Cargo.toml=$CARGO_VERSION manifest.json=$MANIFEST_VERSION" >&2
  exit 1
fi

if [[ "${SKIP_BUILD:-0}" != "1" ]]; then
  printf '%s\n' '==> cargo build --release --locked'
  cargo build --release --locked
fi

SRC_BIN="$ROOT/target/release/$BIN_NAME"
if [[ ! -x "$SRC_BIN" ]]; then
  echo "ERROR: release binary was not produced: $SRC_BIN" >&2
  exit 1
fi

# Assemble the bundle in a scratch directory so the working tree stays clean
# and the archive only ever contains the target binary.
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
cp -a "$BUNDLE" "$STAGE/$BUNDLE_NAME"
rm -rf "$STAGE/$BUNDLE_NAME/bin"
mkdir -p "$STAGE/$BUNDLE_NAME/bin/$TARGET"
cp -f "$SRC_BIN" "$STAGE/$BUNDLE_NAME/bin/$TARGET/$BIN_NAME"
chmod 755 "$STAGE/$BUNDLE_NAME/bin/$TARGET/$BIN_NAME"
cp -f "$ROOT/LICENSE" "$STAGE/$BUNDLE_NAME/LICENSE"

mkdir -p "$DIST"
rm -f "$ARCHIVE" "$ARCHIVE.sha256"
# -X drops uid/gid/timestamp extras but keeps the unix mode bits, which
# OpenDeck's extractor applies (the binary must stay executable).
(cd "$STAGE" && zip -qrX "$ARCHIVE" "$BUNDLE_NAME")

# Sanity checks on what was just produced.
if zipinfo -1 "$ARCHIVE" | grep -qv "^$BUNDLE_NAME/"; then
  echo "ERROR: archive contains entries outside $BUNDLE_NAME/" >&2
  exit 1
fi
if ! zipinfo "$ARCHIVE" "$BUNDLE_NAME/bin/$TARGET/$BIN_NAME" | grep -q '^-rwxr-xr-x'; then
  echo "ERROR: binary is not executable inside the archive" >&2
  exit 1
fi

(cd "$DIST" && sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256")

echo
printf '%s\n' '==> Build complete'
echo "Version: $VERSION"
echo "Archive: $ARCHIVE"
echo "SHA-256: $ARCHIVE.sha256"
