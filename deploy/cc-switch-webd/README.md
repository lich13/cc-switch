# cc-switch-webd Linux deployment

## Install

```bash
sudo deploy/install.sh
sudo CC_SWITCH_WEBD_ADMIN_PASSWORD='change-me-long-password' \
  cc-switch-webd admin init --config /etc/cc-switch-webd/config.toml
sudo systemctl restart cc-switch-webd
```

## Update

```bash
sudo deploy/update.sh latest
```

For an explicit cloud overwrite from a release tag:

```bash
sudo CC_SWITCH_WEBD_ARCH=x86_64 deploy/update.sh v3.16.3-lich13.1
# or: sudo CC_SWITCH_WEBD_ARCH=arm64 deploy/update.sh v3.16.3-lich13.1
```

`update.sh` downloads `cc-switch-webd-linux-$CC_SWITCH_WEBD_ARCH.tar.gz`,
verifies the matching `.sha256`, and runs `deploy/install.sh`. The install step
overwrites `/usr/local/bin/cc-switch-webd`, `/usr/share/cc-switch-webd/webui`,
the systemd unit, and logrotate config. Existing `/etc/cc-switch-webd/config.toml`
and `/etc/cc-switch-webd/env` are preserved and missing security defaults are
added.

## Rollback

```bash
sudo deploy/rollback.sh <known-good-webd-tag>
```

## Verify

```bash
systemctl is-active cc-switch-webd
curl -fsS http://127.0.0.1:15722/healthz
curl -fsS http://127.0.0.1:15722/readyz
curl -fsS http://127.0.0.1:15722/api/public/settings
nginx -t
curl -fsS https://example.com/cc-switch/
curl -sS -o /dev/null -w '%{http_code}\n' https://example.com/cc-switch/v1/messages
```

## Logs

```bash
journalctl -u cc-switch-webd -n 200 --no-pager
tail -n 200 /var/log/cc-switch-webd/webd.log
```

## Turnstile

The daemon reads Turnstile from `/etc/cc-switch-webd/config.toml` and
`/etc/cc-switch-webd/env`. Keep `CC_SWITCH_WEBD_TURNSTILE_SECRET_KEY` in the
env file and do not put it in logs or release notes.

The example `nginx.conf` proxies only the management WebUI under `/cc-switch/`.
Keep model proxy routes such as `/v1/messages` and `/v1/responses` off the
public management domain unless you intentionally design separate authentication
for them.
