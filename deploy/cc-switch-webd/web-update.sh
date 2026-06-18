#!/usr/bin/env bash
set -euo pipefail

SOURCE="${1:-}"
if [ -z "$SOURCE" ]; then
  echo "usage: web-update.sh <webui-dir-or-cc-switch-webd-archive.tar.gz>" >&2
  exit 2
fi
if [ "$(id -u)" -ne 0 ]; then
  echo "web-update.sh must run as root" >&2
  exit 1
fi

TMP_DIR=""
cleanup() {
  if [ -n "$TMP_DIR" ]; then
    rm -rf "$TMP_DIR"
  fi
}
trap cleanup EXIT

if [ -d "$SOURCE" ]; then
  WEBUI_DIR="$SOURCE"
else
  TMP_DIR="$(mktemp -d)"
  tar -xzf "$SOURCE" -C "$TMP_DIR"
  WEBUI_INDEX="$(find "$TMP_DIR" -path '*/webui/index.html' -type f | head -1)"
  if [ -z "$WEBUI_INDEX" ]; then
    echo "archive does not contain webui/index.html: $SOURCE" >&2
    exit 1
  fi
  WEBUI_DIR="$(dirname "$WEBUI_INDEX")"
fi

if [ ! -f "$WEBUI_DIR/index.html" ]; then
  echo "missing index.html in $WEBUI_DIR" >&2
  exit 1
fi

find /usr/share/cc-switch-webd/webui -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -a "$WEBUI_DIR/." /usr/share/cc-switch-webd/webui/
systemctl reload-or-restart cc-switch-webd.service
echo "cc-switch-webd WebUI updated"
