# Pi 5 deployment

> Load this when doing Pi setup or working on the systemd/kiosk layer.

## Status
- [x] Pi 5 on Debian trixie, updated, SSH enabled
- [x] NVMe mounted at `/` (`/dev/sda2`, 458GB)
- [x] `agentd` system user created
- [x] CerebroCortex 0.5.1 in venv at `/opt/cerebro-venv`
- [x] `agentd` binary native-compiled on Pi, deployed to `/usr/local/bin/agentd`
- [x] systemd unit installed, enabled, running
- [x] `install.sh` bootstrap script in repo root
- [ ] cage + webview for local KVM kiosk display (deferred)

## Hardware
- Raspberry Pi 5 — Cortex-A76, 8GB RAM
- NVMe drive (entire system, workspace, event log)
- Debian trixie headless (no desktop)

## Key paths on Pi
| Path | Purpose |
|------|---------|
| `/usr/local/bin/agentd` | daemon binary (7MB Rust) |
| `/usr/local/bin/cerebro-mcp` | wrapper → `/opt/cerebro-venv/bin/cerebro-mcp` |
| `/opt/cerebro-venv/` | CerebroCortex Python venv (isolated from system packages) |
| `/etc/agentd/plugins.toml` | plugin config (copied from `config/plugins.pi.toml`) |
| `/etc/agentd/policy.toml` | approval policy |
| `/etc/agentd/env` | `ANTHROPIC_API_KEY=...` — chmod 600, root-owned |
| `/var/lib/agentd/{workspace,events,cerebro}` | runtime data, owned by `agentd` user |
| `/usr/local/share/agentd/ui/` | frontend static files (HTML/CSS/JS) |
| `/var/pip-tmp/` | pip temp + cache dir — avoids filling 2GB `/tmp` tmpfs |

## Building on Pi (ALWAYS native, never cross-compile)
```bash
cd ~/ApexOS/agentd && ~/.cargo/bin/cargo build --release
sudo cp target/release/agentd /usr/local/bin/agentd
sudo systemctl restart agentd
```
Cross-compiling x86_64 → Pi gives "Exec format error". Pi 5 Cortex-A76 builds in ~2 min.

## Deploying a code change
```bash
# 1. push from dev laptop
git push

# 2. on Pi (~/ApexOS is a proper git clone of github.com/buckster123/ApexOS)
cd ~/ApexOS && git pull

# Rust changes: rebuild binary
cd agentd && ~/.cargo/bin/cargo build --release -p agentd
sudo cp agentd/target/release/agentd /usr/local/bin/agentd

# UI-only changes: copy static files (no rebuild needed)
sudo cp -r ui/. /usr/local/share/agentd/ui/

sudo systemctl restart agentd
sudo journalctl -u agentd -n 20 --no-pager
```

## Debian trixie quirks
- `pip3` not installed by default — `sudo apt install python3-pip`
- PEP 668 enforced: use `--break-system-packages` or venv (venv preferred)
- System packages (rpds-py, PyYAML) conflict with pip if not using venv
- `/tmp` is tmpfs 2GB — large pip installs must use `TMPDIR=/var/pip-tmp`

## EnvironmentFile format
`/etc/agentd/env` must be plain `KEY=VALUE` (no `export`, no quotes unless value has spaces):
```
ANTHROPIC_API_KEY=sk-ant-api03-...
```

## Kiosk display (deferred)
- `cage` (single-app Wayland compositor) + `chromium --kiosk http://localhost:8787`
- `cage-kiosk.service` in `deploy/` — install after UI is built
- Needs: `sudo apt install cage chromium`
