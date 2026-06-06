# Pi 5 deployment

> Load this when doing Pi setup, cross-compilation, or first deploy.

## Status
- [ ] Pi 5 on RaspiOS Lite, updated, SSH enabled
- [ ] NVMe mounted, workspace path chosen
- [ ] `agentd` + `agentos-kiosk` users created
- [ ] CerebroCortex installed on Pi
- [ ] `agentd` binary cross-compiled and deployed
- [ ] systemd units installed and enabled
- [ ] cage + chromium installed for kiosk display

## Hardware
- Raspberry Pi 5
- NVMe drive (workspace + event log)
- RaspiOS Lite (no desktop — cage provides the only Wayland compositor)

## Cross-compilation
Target: `aarch64-unknown-linux-gnu`
```
cargo build --release --target aarch64-unknown-linux-gnu
```

## Paths on Pi
| Path | Purpose |
|------|---------|
| `/usr/local/bin/agentd` | daemon binary |
| `/etc/agentd/` | config (plugins.toml, policy.toml) |
| `/var/lib/agentd/workspace/` | agent workspace (AGENTD_WORKSPACE) |
| `/var/lib/agentd/events/` | event log (AGENTD_LOG) |

## systemd units
`agentd/deploy/agentd.service` + `cage-kiosk.service` — copy to `/etc/systemd/system/`

## Notes
<!-- Fill in: NVMe mount point, SSH key, CerebroCortex install path on Pi, any RaspiOS quirks -->
