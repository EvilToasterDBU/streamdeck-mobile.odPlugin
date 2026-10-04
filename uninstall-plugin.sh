#!/usr/bin/env bash
set -euo pipefail
CONFIG_DIR="${OPENDECK_CONFIG_DIR:-$HOME/.config/opendeck}"
rm -rf "$CONFIG_DIR/plugins/com.toshtaru.streamdeck-mobile.sdPlugin"
echo "Removed Stream Deck Mobile plugin."
