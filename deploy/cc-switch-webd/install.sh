#!/usr/bin/env bash
set -euo pipefail

START_SERVICE=1
CHECK_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --no-start) START_SERVICE=0 ;;
    --check) CHECK_ONLY=1 ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd -P)"

if [ ! -x "$ROOT_DIR/bin/cc-switch-webd" ]; then
  echo "missing binary: $ROOT_DIR/bin/cc-switch-webd" >&2
  exit 1
fi
if [ ! -f "$ROOT_DIR/webui/index.html" ]; then
  echo "missing WebUI: $ROOT_DIR/webui/index.html" >&2
  exit 1
fi
if [ "$CHECK_ONLY" -eq 1 ]; then
  echo "cc-switch-webd archive layout ok"
  exit 0
fi

if [ "$(id -u)" -ne 0 ]; then
  echo "install.sh must run as root" >&2
  exit 1
fi

install_runtime_deps() {
  if ! command -v apt-get >/dev/null 2>&1; then
    return
  fi

  local missing=0
  if command -v ldd >/dev/null 2>&1; then
    if ldd "$ROOT_DIR/bin/cc-switch-webd" | grep -q 'not found'; then
      missing=1
    fi
  fi
  if [ "$missing" -eq 0 ]; then
    return
  fi

  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  apt-get install -y --no-install-recommends \
    ca-certificates \
    libwebkit2gtk-4.1-0 \
    libjavascriptcoregtk-4.1-0 \
    libsoup-3.0-0
}

verify_runtime_deps() {
  if command -v ldd >/dev/null 2>&1; then
    if ldd "$ROOT_DIR/bin/cc-switch-webd" | grep 'not found' >&2; then
      echo "missing cc-switch-webd runtime libraries" >&2
      exit 1
    fi
  fi
}

install_runtime_deps
verify_runtime_deps

id -u cc-switch-webd >/dev/null 2>&1 || useradd --system --home-dir /var/lib/cc-switch-webd --shell /usr/sbin/nologin cc-switch-webd

install -d -m 0755 /etc/cc-switch-webd
install -d -m 0750 -o cc-switch-webd -g cc-switch-webd /var/lib/cc-switch-webd
install -d -m 0750 -o cc-switch-webd -g cc-switch-webd /var/log/cc-switch-webd
install -d -m 0755 /usr/share/cc-switch-webd/webui
install -d -m 0755 /usr/local/bin

install -m 0755 "$ROOT_DIR/bin/cc-switch-webd" /usr/local/bin/cc-switch-webd
find /usr/share/cc-switch-webd/webui -mindepth 1 -maxdepth 1 -exec rm -rf {} +
cp -a "$ROOT_DIR/webui/." /usr/share/cc-switch-webd/webui/
chown -R root:root /usr/share/cc-switch-webd/webui

if [ ! -f /etc/cc-switch-webd/config.toml ]; then
  install -m 0640 -o root -g cc-switch-webd "$SCRIPT_DIR/config.example.toml" /etc/cc-switch-webd/config.toml
fi
if [ ! -f /etc/cc-switch-webd/env ]; then
  install -m 0640 -o root -g cc-switch-webd "$SCRIPT_DIR/env.example" /etc/cc-switch-webd/env
fi

ensure_config_defaults() {
  local config=/etc/cc-switch-webd/config.toml
  local tmp
  tmp="$(mktemp)"
  awk '
    BEGIN {
      count = split("session_ttl_seconds=31536000|turnstile_enabled=false|turnstile_required=false|turnstile_site_key=\"0x4AAAAAADPfCPB_O-N3j6ON\"|turnstile_expected_hostname=\"661313.xyz\"|turnstile_expected_action=\"login\"|turnstile_verify_url=\"https://challenges.cloudflare.com/turnstile/v0/siteverify\"", entries, "|")
      for (i = 1; i <= count; i++) {
        split(entries[i], pair, "=")
        key = pair[1]
        value = substr(entries[i], length(key) + 2)
        order[i] = key
        values[key] = value
      }
    }
    function emit_missing(    i, key) {
      for (i = 1; i <= count; i++) {
        key = order[i]
        if (!seen[key]) {
          print key " = " values[key]
          seen[key] = 1
        }
      }
    }
    /^[[:space:]]*\[security\][[:space:]]*$/ {
      in_security = 1
      saw_security = 1
      print
      next
    }
    in_security && /^[[:space:]]*\[/ {
      emit_missing()
      in_security = 0
    }
    in_security {
      for (i = 1; i <= count; i++) {
        key = order[i]
        if ($0 ~ "^[[:space:]]*" key "[[:space:]]*=") {
          seen[key] = 1
          if (key == "session_ttl_seconds" && $0 ~ "=[[:space:]]*43200[[:space:]]*$") {
            print "session_ttl_seconds = 31536000"
            next
          }
        }
      }
    }
    { print }
    END {
      if (in_security) {
        emit_missing()
      } else if (!saw_security) {
        print ""
        print "[security]"
        emit_missing()
      }
    }
  ' "$config" > "$tmp"
  cat "$tmp" > "$config"
  rm -f "$tmp"
  chown root:cc-switch-webd "$config"
  chmod 0640 "$config"
}

ensure_config_defaults

install -m 0644 "$SCRIPT_DIR/cc-switch-webd.service" /etc/systemd/system/cc-switch-webd.service
install -m 0644 "$SCRIPT_DIR/logrotate.conf" /etc/logrotate.d/cc-switch-webd

systemctl daemon-reload
systemctl enable cc-switch-webd.service

if [ "$START_SERVICE" -eq 1 ]; then
  systemctl restart cc-switch-webd.service
fi

echo "cc-switch-webd installed"
echo "initialize admin with: CC_SWITCH_WEBD_ADMIN_PASSWORD=... cc-switch-webd admin init --config /etc/cc-switch-webd/config.toml"
