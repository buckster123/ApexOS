#!/bin/bash
# ApexOS firstrun — automatically installs ApexOS on first Pi boot.
#
# ── HOW TO USE WITH RASPBERRY PI IMAGER ───────────────────────────
#
# 1. Open Raspberry Pi Imager (v1.8.5 or later)
# 2. Choose OS: Raspberry Pi OS Lite (64-bit)  ← recommended
# 3. Click the ⚙ gear / "Edit Settings" button
# 4. Under "General":
#      ✓ Set hostname: apexos
#      ✓ Set username: apexos    password: (your choice)
#      ✓ Configure WiFi (if not using ethernet)
#      ✓ Set locale/timezone
# 5. Under "Services":
#      ✓ Enable SSH (password or key auth)
# 6. Under "Options" → "Run custom script on first boot":
#      URL: https://raw.githubusercontent.com/buckster123/ApexOS/main/firstrun.sh
#      (or leave blank and run manually — see below)
# 7. Save → Write → Boot
#
# The Pi will reboot once during install. Total time: ~15 min on Pi 5.
# Access at http://apexos.local:8787 when complete.
#
# ── MANUAL ALTERNATIVE ────────────────────────────────────────────
# After flashing and SSH-ing in:
#   curl -fsSL https://raw.githubusercontent.com/buckster123/ApexOS/main/install.sh | sudo bash
#
# ── PASSING AN API KEY ────────────────────────────────────────────
# Set APEX_API_KEY in the Pi Imager environment or in /boot/apexos.env before boot:
#   echo "APEX_API_KEY=sk-ant-..." | sudo tee /boot/firmware/apexos.env
#
# ─────────────────────────────────────────────────────────────────

set -euo pipefail
exec > /var/log/apexos-firstrun.log 2>&1

echo "[apexos-firstrun] $(date) — starting"

# ── wait for network ───────────────────────────────────────────────
echo "[apexos-firstrun] waiting for network …"
for i in $(seq 1 18); do
  ping -c1 -W3 github.com &>/dev/null && break
  echo "[apexos-firstrun] attempt $i/18 — no network yet, retrying in 10s"
  sleep 10
done
ping -c1 -W3 github.com &>/dev/null || {
  echo "[apexos-firstrun] ERROR: no network after 3 min — aborting"
  exit 1
}
echo "[apexos-firstrun] network OK"

# ── pick up optional API key from boot partition ───────────────────
API_KEY_ARG=""
for keyfile in /boot/apexos.env /boot/firmware/apexos.env; do
  if [[ -f "$keyfile" ]]; then
    KEY=$(grep -oP 'APEX_API_KEY=\K\S+' "$keyfile" 2>/dev/null || true)
    [[ -n "$KEY" ]] && API_KEY_ARG="--api-key=${KEY}"
    rm -f "$keyfile"  # consume it so it doesn't sit on the boot partition
    echo "[apexos-firstrun] found API key in $keyfile"
    break
  fi
done

# ── ensure git is available ────────────────────────────────────────
apt-get update -qq
apt-get install -y --no-install-recommends git curl ca-certificates

# ── clone and run installer ────────────────────────────────────────
echo "[apexos-firstrun] cloning ApexOS …"
if [[ -d /opt/ApexOS/.git ]]; then
  git -C /opt/ApexOS pull --ff-only
else
  git clone --depth=1 https://github.com/buckster123/ApexOS.git /opt/ApexOS
fi

echo "[apexos-firstrun] running install.sh …"
bash /opt/ApexOS/install.sh --yes --no-kiosk ${API_KEY_ARG}

# ── mark firstrun complete ─────────────────────────────────────────
touch /boot/firmware/apexos-installed 2>/dev/null || true
echo "[apexos-firstrun] $(date) — complete, rebooting …"
reboot
