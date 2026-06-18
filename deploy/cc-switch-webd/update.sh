#!/usr/bin/env bash
set -euo pipefail

REPO="${CC_SWITCH_WEBD_REPO:-lich13/cc-switch}"
VERSION="${1:-latest}"
ARCH="${CC_SWITCH_WEBD_ARCH:-x86_64}"
ASSET="cc-switch-webd-linux-${ARCH}.tar.gz"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if [ "$(id -u)" -ne 0 ]; then
  echo "update.sh must run as root" >&2
  exit 1
fi

if [ "$VERSION" = "latest" ]; then
  BASE_URL="https://github.com/${REPO}/releases/latest/download"
else
  BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
fi

curl -fL "${BASE_URL}/${ASSET}" -o "$TMP_DIR/$ASSET"
curl -fL "${BASE_URL}/${ASSET}.sha256" -o "$TMP_DIR/$ASSET.sha256"

(cd "$TMP_DIR" && sha256sum -c "$ASSET.sha256")
tar -xzf "$TMP_DIR/$ASSET" -C "$TMP_DIR"

ROOT="$(find "$TMP_DIR" -maxdepth 1 -type d -name 'cc-switch-webd-linux-*' | head -1)"
if [ -z "$ROOT" ]; then
  echo "extracted archive root not found" >&2
  exit 1
fi

exec "$ROOT/deploy/install.sh"
