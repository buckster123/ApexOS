# ApexOS — Installation Manual

Three paths depending on how deep you want to go.

---

## Path 1 — Pi Imager (zero-touch, recommended for fresh hardware)

1. Download [Raspberry Pi Imager](https://www.raspberrypi.com/software/) v1.8.5+
2. Choose **Raspberry Pi OS Lite (64-bit)** as the OS
3. Click **Edit Settings** (gear icon)
   - Hostname: `apexos`
   - Username: `apexos` + password of your choice
   - WiFi credentials if not using ethernet
   - Timezone / locale
   - Enable SSH
4. Under **Options → Run custom script on first boot**, paste:
   ```
   https://raw.githubusercontent.com/buckster123/ApexOS/main/firstrun.sh
   ```
5. *Optional — pre-load your API key so the install is truly hands-free:*
   After writing the card but before booting, mount the boot partition and create:
   ```
   echo "APEX_API_KEY=sk-ant-YOUR_KEY" > /Volumes/bootfs/apexos.env
   ```
6. Flash → insert card → boot

The Pi will network-wait, clone the repo, run the installer (~15 min on Pi 5), and reboot. Access at `http://apexos.local:8787`.

---

## Path 2 — One-liner (Pi already running, SSH in)

```bash
curl -fsSL https://raw.githubusercontent.com/buckster123/ApexOS/main/install.sh | sudo bash
```

The installer auto-clones the repo to `/opt/ApexOS`, detects hardware, asks a few questions, and starts the service. API key can be entered during install or later through the UI.

**Flags:**
| Flag | Effect |
|------|--------|
| `--yes` / `-y` | Non-interactive, accept all defaults |
| `--no-kiosk` | Skip Wayland cage kiosk |
| `--no-voice` | Skip whisper + piper + wake word |
| `--api-key=sk-ant-...` | Set API key non-interactively |

---

## Path 3 — Manual (full control, nerds only)

### Prerequisites

- Raspberry Pi 5 (4GB or 8GB), Debian trixie or Raspberry Pi OS Lite 64-bit
- NVMe SSD strongly recommended (the event log and models are write-heavy)
- Network access during install

### 1. System packages

```bash
sudo apt-get update
sudo apt-get install -y \
  curl git build-essential pkg-config libssl-dev \
  python3 python3-pip python3-venv \
  alsa-utils espeak-ng libnotify-bin \
  ffmpeg cmake                          # voice only
  cage seatd                            # kiosk only
  rpicam-apps                           # camera only
```

### 2. Rust

```bash
curl -fsSL https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### 3. Build and install agentd

```bash
git clone https://github.com/buckster123/ApexOS.git
cd ApexOS/agentd
cargo build --release
sudo install -m 755 target/release/agentd /usr/local/bin/agentd
```

### 4. Build and install apexos-tools

```bash
cd ../tools
cargo build --release
sudo install -m 755 target/release/apexos-tools /usr/local/bin/apexos-tools
sudo tee /usr/local/bin/apexos-tools-mcp << 'EOF'
#!/bin/bash
exec /usr/local/bin/apexos-tools "$@"
EOF
sudo chmod +x /usr/local/bin/apexos-tools-mcp
```

### 5. CerebroCortex

```bash
sudo python3 -m venv /opt/cerebro-venv
sudo /opt/cerebro-venv/bin/pip install "path/to/CerebroCortex[all]"
# or from GitHub:
# sudo /opt/cerebro-venv/bin/pip install "git+https://github.com/buckster123/CerebroCortex[all]"

sudo tee /usr/local/bin/cerebro-mcp << 'EOF'
#!/bin/bash
exec /opt/cerebro-venv/bin/cerebro-mcp "$@"
EOF
sudo chmod +x /usr/local/bin/cerebro-mcp
```

### 6. Users and directories

```bash
sudo useradd --system --no-create-home --shell /usr/sbin/nologin agentd
sudo usermod -aG audio agentd
sudo mkdir -p /etc/agentd /var/lib/agentd/{workspace,events,cerebro,ui,whisper,piper}
sudo chown -R agentd:agentd /var/lib/agentd
sudo chmod 700 /var/lib/agentd
sudo chmod 750 /etc/agentd
```

### 7. Config files

```bash
cd ApexOS
sudo cp agentd/config/plugins.pi.toml /etc/agentd/plugins.toml
sudo cp agentd/config/policy.toml     /etc/agentd/policy.toml
sudo cp agentd/config/soul.md         /etc/agentd/soul.md
sudo chown agentd:agentd /etc/agentd/soul.md
sudo cp -r ui/. /var/lib/agentd/ui/
sudo chown -R agentd:agentd /var/lib/agentd/ui
```

### 8. API key

```bash
echo "ANTHROPIC_API_KEY=sk-ant-YOUR_KEY" | sudo tee /etc/agentd/env
sudo chmod 600 /etc/agentd/env
```

### 9. systemd service

```bash
sudo cp agentd/deploy/agentd.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now agentd
```

### 10. Cage kiosk (optional — local HDMI display)

```bash
sudo useradd --create-home --shell /bin/bash agentos-kiosk
sudo usermod -aG video,render,input,tty,audio agentos-kiosk
sudo groupadd -f seat && sudo usermod -aG seat agentos-kiosk
sudo systemctl enable --now seatd
sudo cp agentd/deploy/cage-kiosk.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable cage-kiosk.service
```

### 11. Voice I/O (optional)

```bash
# whisper.cpp
sudo git clone --depth=1 https://github.com/ggerganov/whisper.cpp /opt/whisper.cpp
cd /opt/whisper.cpp
cmake -B build -DCMAKE_BUILD_TYPE=Release && cmake --build build -j4
bash models/download-ggml-model.sh tiny.en
bash models/download-ggml-model.sh base.en
sudo cp models/ggml-tiny.en.bin models/ggml-base.en.bin /var/lib/agentd/whisper/
sudo ln -sf /opt/whisper.cpp/build/bin/whisper-cli /usr/local/bin/whisper-cpp

# piper
wget -q -O /tmp/piper.tar.gz \
  https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_aarch64.tar.gz
sudo tar -xzf /tmp/piper.tar.gz -C /opt/
sudo ln -sf /opt/piper/piper /usr/local/bin/piper

# lessac-medium voice
sudo wget -q -P /var/lib/agentd/piper \
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx
sudo wget -q -P /var/lib/agentd/piper \
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium/en_US-lessac-medium.onnx.json

# apex-wake service
sudo mkdir -p /opt/apex-wake
sudo cp deploy/apex-wake.py /opt/apex-wake/wake.py
sudo cp deploy/apex-wake.service /etc/systemd/system/
sudo systemctl daemon-reload && sudo systemctl enable --now apex-wake

# Add voice env vars
sudo tee -a /etc/agentd/env << 'EOF'
WHISPER_MODEL=/var/lib/agentd/whisper/ggml-tiny.en.bin
WHISPER_BIN=/usr/local/bin/whisper-cpp
WAKE_WHISPER_MODEL=/var/lib/agentd/whisper/ggml-base.en.bin
PIPER_MODEL=/var/lib/agentd/piper/en_US-lessac-medium.onnx
ALSA_CAPTURE_DEVICE=plughw:2,0   # change card number to match your device
EOF
```

---

## File map

| Path | Contents |
|------|----------|
| `/usr/local/bin/agentd` | Main daemon binary |
| `/usr/local/bin/cerebro-mcp` | CerebroCortex wrapper |
| `/usr/local/bin/apexos-tools` | MCP tools binary |
| `/usr/local/bin/whisper-cpp` | STT engine |
| `/usr/local/bin/piper` | TTS engine |
| `/etc/agentd/env` | Secrets (API key, env vars) — chmod 600 |
| `/etc/agentd/soul.md` | Agent system prompt — live-editable |
| `/etc/agentd/plugins.toml` | MCP plugin registry — live-reloadable |
| `/etc/agentd/policy.toml` | Approval rules |
| `/var/lib/agentd/ui/` | Frontend static files |
| `/var/lib/agentd/events/` | JSONL event log (date-rolling) |
| `/var/lib/agentd/cerebro/` | CerebroCortex database |
| `/var/lib/agentd/whisper/` | Whisper models |
| `/var/lib/agentd/piper/` | Piper voice models |
| `/opt/apex-wake/wake.py` | Wake word detection loop |

## Ports

| Port | Service |
|------|---------|
| 8787 | agentd WebSocket + HTTP UI |
| 8767 | cerebro-api (memory browser) |
| 8080 | SensorHead dashboard (if running) |

## Useful commands

```bash
# Service control
sudo systemctl status agentd apex-wake cage-kiosk
sudo journalctl -u agentd -f          # live logs
sudo systemctl restart agentd         # after config changes

# Update
cd /opt/ApexOS && git pull
cd agentd && cargo build --release
sudo systemctl stop agentd
sudo cp target/release/agentd /usr/local/bin/
sudo systemctl start agentd
sudo cp -r ../ui/. /var/lib/agentd/ui/  # UI-only changes don't need a restart

# Wake word tuning
sudo journalctl -u apex-wake -f       # see what it's hearing
# Change phrase: add WAKE_PHRASE=yourword to /etc/agentd/env, restart apex-wake

# Audio device discovery
arecord -l                            # list capture devices
aplay -l                              # list playback devices
```
