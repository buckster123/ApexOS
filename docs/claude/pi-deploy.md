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
- [x] cage + chromium kiosk for local KVM display — seatd backend, `/run/cage-kiosk` runtime dir

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
| `/var/lib/agentd/ui/` | frontend static files (HTML/CSS/JS) — must be under ReadWritePaths for ProtectSystem=strict |
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
sudo cp -r ui/. /var/lib/agentd/ui/

sudo systemctl restart agentd
sudo journalctl -u agentd -n 20 --no-pager
```

## Debian trixie quirks
- `pip3` not installed by default — `sudo apt install python3-pip`
- PEP 668 enforced: use `--break-system-packages` or venv (venv preferred)
- System packages (rpds-py, PyYAML) conflict with pip if not using venv
- `/tmp` is tmpfs 2GB — large pip installs must use `TMPDIR=/var/pip-tmp`

## Sensor head system packages (required for sensor-head-mcp tools)

These are system apt packages that must be installed on the Pi for the IMX500 AI camera tools:

```bash
sudo apt-get install -y python3-opencv    # detect_objects, classify_scene (cv2)
sudo apt-get install -y imx500-all        # firmware + model .rpk files in /usr/share/imx500-models/
```

`python3-opencv` is picked up automatically by SensorHead venv via the existing `system-picamera2.pth`
that already points `/usr/lib/python3/dist-packages` into the venv — no venv changes needed.

`imx500-all` installs the EfficientDet Lite0 (`imx500_network_efficientdet_lite0_pp.rpk`) and
MobileNetV2 (`imx500_network_mobilenet_v2.rpk`) model files required by `detect_objects` and
`classify_scene`. Without it, those endpoints return 500 with "Firmware file … does not exist."
Restart `sensorhead-dashboard` after installing to load the new models.

## EnvironmentFile format
`/etc/agentd/env` must be plain `KEY=VALUE` (no `export`, no quotes unless value has spaces):
```
ANTHROPIC_API_KEY=sk-ant-api03-...
```

## Kiosk display

### Setup (done by install.sh)
- `sudo apt install cage chromium seatd`
- `useradd --create-home --shell /bin/bash --user-group agentos-kiosk`
- `usermod -aG video,render,input,tty agentos-kiosk`
- Install `deploy/cage-kiosk.service` → `/etc/systemd/system/`
- `systemctl enable cage-kiosk.service` (auto-starts at boot)

### How it works
- seatd socket: `/run/seatd.sock` — group `video`, mode 0770
- agentos-kiosk is in `video` group → can connect to seatd
- `LIBSEAT_BACKEND=seatd` forces cage to use seatd (not logind)
- `RuntimeDirectory=cage-kiosk` → `/run/cage-kiosk/` owned by agentos-kiosk
- cage creates Wayland socket `wayland-0` there; chromium renders into it
- Headless (no monitor): cage still runs, no output; 3 failures → service stops

### Pitfalls found
- `PAMName=login` approach fails: logind doesn't propagate XDG_RUNTIME_DIR to the exec'd process, and `%U` in ExecStartPre=+ expands to 0 (root UID), not the service user
- seatd socket group is `video` on Debian trixie (not `seat` or `_seatd`)
- `StartLimitIntervalSec`/`StartLimitBurst` belong in `[Unit]`, not `[Service]`

## Power control (reboot / poweroff from UI)
- `agentd.service` sets `NoNewPrivileges=true`, which **blocks `sudo`** entirely
  (setuid escalation is denied: "the no new privileges flag is set"). A NOPASSWD
  sudoers rule for the agentd user is therefore dead code.
- Gateway `POST /api/power` calls `systemctl reboot|poweroff` **directly, no sudo**.
  Authorization is granted by `/etc/polkit-1/rules.d/49-agentd-power.rules`
  (installed by `install.sh`), allowing the `agentd` user the
  `org.freedesktop.login1.{reboot,reboot-multiple-sessions,power-off,power-off-multiple-sessions}`
  actions via logind.
- Non-destructive test: `systemd-run --uid=agentd -p NoNewPrivileges=yes systemctl reboot --when=+90min`
  should schedule (not deny), then `shutdown -c` / `systemctl reboot --when=cancel`.

## apex-face daemon
- `apex-face.service` MUST set `WorkingDirectory=/run/apex-face` — lgpio creates
  its `.lgd-nfy*` notify FIFO in the process CWD; default CWD `/` is unwritable by
  `apexos` and crash-loops the daemon ("xCreatePipe: Can't set permissions").
  The dir is provided by `RuntimeDirectory=apex-face`.
