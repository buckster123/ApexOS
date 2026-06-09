#!/usr/bin/env bash
# ╔══════════════════════════════════════════════════════════════════╗
# ║              ApexOS — One-shot installer                         ║
# ║  Tested on: Raspberry Pi 5, Debian trixie / Raspberry Pi OS     ║
# ╚══════════════════════════════════════════════════════════════════╝
#
# Quick install (fresh Pi, clones repo automatically):
#   curl -fsSL https://raw.githubusercontent.com/buckster123/ApexOS/main/install.sh | sudo bash
#
# From a local clone:
#   sudo bash install.sh [OPTIONS]
#
# Options:
#   -y / --yes          Non-interactive (skip prompts, accept defaults)
#   --no-kiosk          Skip cage Wayland kiosk (headless/remote-only use)
#   --no-voice          Skip whisper + piper + wake word
#   --api-key=KEY       Set Anthropic API key non-interactively
#   --repo-dir=PATH     Use a local repo instead of cloning
#
# Safe to re-run — every step is idempotent.
# ──────────────────────────────────────────────────────────────────

set -euo pipefail
trap 'echo -e "\n${RED}  ✗ Install failed at line $LINENO — check output above.${NC}" >&2' ERR

# ── colours ────────────────────────────────────────────────────────────────────
if [[ -t 1 ]]; then
  RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
  CYAN='\033[0;36m'; BOLD='\033[1m'; DIM='\033[2m'; NC='\033[0m'
else
  RED=''; GREEN=''; YELLOW=''; CYAN=''; BOLD=''; DIM=''; NC=''
fi

ok()     { echo -e "${GREEN}  ✓${NC} $*"; }
info()   { echo -e "${CYAN}  →${NC} $*"; }
warn()   { echo -e "${YELLOW}  ⚠${NC} $*"; }
die()    { echo -e "${RED}  ✗${NC} $*" >&2; exit 1; }
ask()    { echo -e "${BOLD}  ?${NC} $*"; }
hdr()    { echo -e "\n${CYAN}${BOLD}━━━  $*  ━━━${NC}\n"; }
dim()    { echo -e "${DIM}    $*${NC}"; }

# ── args ───────────────────────────────────────────────────────────────────────
YES=false; SKIP_KIOSK=false; SKIP_VOICE=false; API_KEY=""; REPO_DIR=""

for arg in "$@"; do
  case "$arg" in
    -y|--yes)         YES=true ;;
    --no-kiosk)       SKIP_KIOSK=true ;;
    --no-voice)       SKIP_VOICE=true ;;
    --api-key=*)      API_KEY="${arg#*=}" ;;
    --repo-dir=*)     REPO_DIR="${arg#*=}" ;;
    *) warn "unknown option: $arg" ;;
  esac
done

confirm() {
  # confirm <message> — returns 0 (yes) or 1 (no); auto-yes if YES=true
  if $YES; then return 0; fi
  ask "$1 [Y/n] "
  read -r reply </dev/tty
  [[ "${reply:-y}" =~ ^[Yy]$ ]]
}

# ── must be root ───────────────────────────────────────────────────────────────
[[ $EUID -eq 0 ]] || die "Run as root: sudo bash install.sh"

BUILD_USER="${SUDO_USER:-root}"
BUILD_HOME=$(getent passwd "$BUILD_USER" | cut -d: -f6)

# ── banner ─────────────────────────────────────────────────────────────────────
echo ""
echo -e "${CYAN}${BOLD}"
echo "  ██████╗ ██████╗ ███████╗██╗  ██╗ ██████╗ ███████╗"
echo "  ██╔══██╗██╔══██╗██╔════╝╚██╗██╔╝██╔═══██╗██╔════╝"
echo "  ███████║██████╔╝█████╗   ╚███╔╝ ██║   ██║███████╗"
echo "  ██╔══██║██╔═══╝ ██╔══╝   ██╔██╗ ██║   ██║╚════██║"
echo "  ██║  ██║██║     ███████╗██╔╝ ██╗╚██████╔╝███████║"
echo "  ╚═╝  ╚═╝╚═╝     ╚══════╝╚═╝  ╚═╝ ╚═════╝ ╚══════╝"
echo -e "${NC}"
echo -e "${DIM}  Agent-first OS daemon for Raspberry Pi 5${NC}"
echo ""

# ── hardware detection ─────────────────────────────────────────────────────────
hdr "Detecting hardware"

PI_MODEL="unknown"
if [[ -f /proc/device-tree/model ]]; then
  PI_MODEL=$(tr -d '\0' < /proc/device-tree/model)
  ok "Board: $PI_MODEL"
else
  warn "Not a Raspberry Pi (or /proc/device-tree not available) — continuing anyway"
fi

# Audio capture
HAS_MIC=false
if command -v arecord &>/dev/null && arecord -l 2>/dev/null | grep -q "card"; then
  MIC_INFO=$(arecord -l 2>/dev/null | grep "card" | head -1)
  ok "Audio input: $MIC_INFO"
  HAS_MIC=true
else
  warn "No audio capture device found yet"
  dim "Plug in a USB mic and re-run, or connect one later"
fi

# Audio output
HAS_SPEAKER=false
if command -v aplay &>/dev/null && aplay -l 2>/dev/null | grep -q "card"; then
  HAS_SPEAKER=true
  ok "Audio output: found"
fi

# Camera
HAS_CAMERA=false
if command -v rpicam-hello &>/dev/null; then
  if rpicam-hello --list-cameras 2>/dev/null | grep -qiE "available|imx|ov"; then
    ok "Camera: detected"
    HAS_CAMERA=true
  else
    warn "rpicam-hello found but no camera module detected"
  fi
else
  warn "No camera tools (rpicam-apps not installed)"
fi

# Voice capability
DO_VOICE=false
if ! $SKIP_VOICE; then
  if $HAS_MIC || $HAS_SPEAKER; then
    if confirm "Set up voice I/O? (whisper STT + piper TTS + wake word)"; then
      DO_VOICE=true
    fi
  else
    dim "Skipping voice setup (no audio hardware detected)"
  fi
fi

# Kiosk
DO_KIOSK=false
if ! $SKIP_KIOSK; then
  if confirm "Set up Wayland kiosk display? (cage — requires monitor on HDMI)"; then
    DO_KIOSK=true
  fi
fi

echo ""
info "Install plan:"
dim "  agentd daemon + CerebroCortex + apexos-tools  →  always"
$DO_VOICE  && dim "  whisper.cpp + piper + apex-wake               →  yes" \
           || dim "  voice I/O                                     →  skipped"
$DO_KIOSK  && dim "  cage Wayland kiosk                            →  yes" \
           || dim "  cage kiosk                                    →  skipped"
$HAS_CAMERA && dim "  camera API (/api/snapshot)                   →  yes"
echo ""
confirm "Proceed?" || { echo "Aborted."; exit 0; }

# ── locate / clone repo ────────────────────────────────────────────────────────
hdr "Repository"

if [[ -z "$REPO_DIR" ]]; then
  SCRIPT_SRC="${BASH_SOURCE[0]:-}"
  if [[ -n "$SCRIPT_SRC" ]] && [[ -f "$(dirname "$SCRIPT_SRC")/agentd/Cargo.toml" ]]; then
    REPO_DIR="$(cd "$(dirname "$SCRIPT_SRC")" && pwd)"
    ok "Using local repo at $REPO_DIR"
  else
    REPO_DIR="/opt/ApexOS"
    if [[ -d "$REPO_DIR/.git" ]]; then
      info "Updating existing clone at $REPO_DIR …"
      git -C "$REPO_DIR" pull --ff-only
    else
      info "Cloning ApexOS to $REPO_DIR …"
      git clone --depth=1 https://github.com/buckster123/ApexOS.git "$REPO_DIR"
    fi
    ok "Repo ready at $REPO_DIR"
  fi
fi
[[ -f "$REPO_DIR/agentd/Cargo.toml" ]] || die "Repo not found at $REPO_DIR"

# ── system packages ────────────────────────────────────────────────────────────
hdr "System packages"

BASE_PKGS=(
  curl git build-essential pkg-config
  libssl-dev python3 python3-pip python3-venv
  libnotify-bin notify-send
  alsa-utils espeak-ng
)
VOICE_PKGS=(cmake ffmpeg)
KIOSK_PKGS=(cage seatd)
CAMERA_PKGS=(rpicam-apps)

TO_INSTALL=("${BASE_PKGS[@]}")
$DO_VOICE  && TO_INSTALL+=("${VOICE_PKGS[@]}")
$DO_KIOSK  && TO_INSTALL+=("${KIOSK_PKGS[@]}")
$HAS_CAMERA && TO_INSTALL+=("${CAMERA_PKGS[@]}")

info "Updating package lists …"
apt-get update -qq

info "Installing packages …"
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  "${TO_INSTALL[@]}" 2>&1 | grep -E "^(Setting up|Unpacking|Err:|E:)" || true
ok "System packages ready"

# ── users and directories ──────────────────────────────────────────────────────
hdr "Users and directories"

if ! id agentd &>/dev/null; then
  useradd --system --no-create-home --shell /usr/sbin/nologin agentd
  ok "Created agentd system user"
else
  ok "agentd user exists"
fi

# agentd needs audio group for TTS/notifications
usermod -aG audio agentd 2>/dev/null || true

mkdir -p /etc/agentd
mkdir -p /var/lib/agentd/{workspace,events,cerebro,ui,whisper,piper}
mkdir -p /var/pip-tmp/cache
mkdir -p /opt/cerebro-venv

chown -R agentd:agentd /var/lib/agentd
chmod 700 /var/lib/agentd
chmod 750 /etc/agentd

# Power control sudoers
cat > /etc/sudoers.d/agentd-power << 'EOF'
agentd ALL=(ALL) NOPASSWD: /bin/systemctl reboot, /bin/systemctl poweroff
EOF
chmod 440 /etc/sudoers.d/agentd-power
ok "Directories, permissions, sudoers ready"

# ── Rust toolchain ─────────────────────────────────────────────────────────────
hdr "Rust toolchain"

CARGO="$BUILD_HOME/.cargo/bin/cargo"
if [[ ! -x "$CARGO" ]]; then
  info "Installing Rust for $BUILD_USER …"
  sudo -u "$BUILD_USER" bash -c \
    'curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path' \
    2>&1 | grep -E "(stable|installed|error)" || true
fi
[[ -x "$CARGO" ]] || die "Rust install failed — cargo not found at $CARGO"
ok "Rust: $($CARGO --version)"

# ── Build agentd ───────────────────────────────────────────────────────────────
hdr "Building agentd"
info "This takes ~2 min on Pi 5 …"

cd "$REPO_DIR/agentd"
sudo -u "$BUILD_USER" "$CARGO" build --release 2>&1 \
  | grep -E "(Compiling agentd|Finished|^error)" || true

BINARY="$REPO_DIR/agentd/target/release/agentd"
[[ -x "$BINARY" ]] || die "Build failed — binary not found at $BINARY"
install -m 755 "$BINARY" /usr/local/bin/agentd
ok "agentd built and installed ($(du -sh "$BINARY" | cut -f1))"

# ── Build apexos-tools ─────────────────────────────────────────────────────────
hdr "Building apexos-tools"

cd "$REPO_DIR/tools"
sudo -u "$BUILD_USER" "$CARGO" build --release 2>&1 \
  | grep -E "(Compiling apexos|Finished|^error)" || true

TOOLS_BIN="$REPO_DIR/tools/target/release/apexos-tools"
[[ -x "$TOOLS_BIN" ]] || die "apexos-tools build failed"
install -m 755 "$TOOLS_BIN" /usr/local/bin/apexos-tools

# Wrapper expected by plugins.toml
cat > /usr/local/bin/apexos-tools-mcp << 'EOF'
#!/bin/bash
exec /usr/local/bin/apexos-tools "$@"
EOF
chmod +x /usr/local/bin/apexos-tools-mcp
ok "apexos-tools built and installed"

# ── CerebroCortex ─────────────────────────────────────────────────────────────
hdr "CerebroCortex memory system"

if [[ ! -x /opt/cerebro-venv/bin/cerebro-mcp ]]; then
  python3 -m venv /opt/cerebro-venv

  CEREBRO_SRC=""
  for candidate in \
      "$REPO_DIR/../CerebroCortex" \
      "$BUILD_HOME/CerebroCortex" \
      "/opt/CerebroCortex"; do
    [[ -f "$candidate/pyproject.toml" ]] && CEREBRO_SRC="$candidate" && break
  done

  if [[ -n "$CEREBRO_SRC" ]]; then
    info "Installing from local source: $CEREBRO_SRC"
  else
    info "Cloning CerebroCortex from GitHub …"
    CEREBRO_SRC=$(mktemp -d)/CerebroCortex
    git clone --depth=1 https://github.com/buckster123/CerebroCortex "$CEREBRO_SRC"
  fi

  TMPDIR=/var/pip-tmp /opt/cerebro-venv/bin/pip install \
    --cache-dir /var/pip-tmp/cache -q "$CEREBRO_SRC[all]" 2>&1 \
    | grep -E "(Successfully|error|ERROR)" || true
fi

[[ -x /opt/cerebro-venv/bin/cerebro-mcp ]] || die "CerebroCortex install failed"
cat > /usr/local/bin/cerebro-mcp << 'EOF'
#!/bin/bash
exec /opt/cerebro-venv/bin/cerebro-mcp "$@"
EOF
chmod +x /usr/local/bin/cerebro-mcp
ok "CerebroCortex ready ($(/opt/cerebro-venv/bin/cerebro-mcp --version 2>&1 | grep -oE 'v?[0-9]+\.[0-9.]+' | head -1 || echo 'installed'))"

# ── Config and UI ──────────────────────────────────────────────────────────────
hdr "Configuration"

# Use Pi-specific plugins.toml if present
if [[ -f "$REPO_DIR/agentd/config/plugins.pi.toml" ]]; then
  install -m 644 "$REPO_DIR/agentd/config/plugins.pi.toml" /etc/agentd/plugins.toml
else
  install -m 644 "$REPO_DIR/agentd/config/plugins.toml" /etc/agentd/plugins.toml
fi
install -m 644 "$REPO_DIR/agentd/config/policy.toml" /etc/agentd/policy.toml

# Soul.md — only write if not already customised
if [[ ! -f /etc/agentd/soul.md ]]; then
  install -m 644 "$REPO_DIR/agentd/config/soul.md" /etc/agentd/soul.md
  chown agentd:agentd /etc/agentd/soul.md
fi

# UI static files
cp -r "$REPO_DIR/ui/." /var/lib/agentd/ui/
chown -R agentd:agentd /var/lib/agentd/ui

# systemd services
for svc in agentd.service cerebro-api.service; do
  SVC_SRC="$REPO_DIR/agentd/deploy/$svc"
  [[ -f "$SVC_SRC" ]] && install -m 644 "$SVC_SRC" /etc/systemd/system/
done
ok "Config files, UI, and services installed"

# ── Mesh: Avahi mDNS advertisement ────────────────────────────────────────────
hdr "Mesh discovery (mDNS)"
apt-get install -y -q avahi-daemon avahi-utils
# Service advertisement file — announces _apexos._tcp on port 8787
install -m 644 "$REPO_DIR/deploy/apexos.avahi.service" /etc/avahi/services/apexos.service
systemctl enable --now avahi-daemon
# Empty peers.toml if not already present (agentd will manage it at runtime)
if [[ ! -f /etc/agentd/peers.toml ]]; then
  echo "# ApexOS mesh peers" > /etc/agentd/peers.toml
  chown agentd:agentd /etc/agentd/peers.toml
fi
ok "Avahi advertising _apexos._tcp · peers.toml ready"

# ── Cage kiosk ─────────────────────────────────────────────────────────────────
if $DO_KIOSK; then
  hdr "Cage Wayland kiosk"

  if ! id agentos-kiosk &>/dev/null; then
    useradd --create-home --shell /bin/bash agentos-kiosk
  fi
  for grp in video render input tty audio; do
    getent group "$grp" &>/dev/null && usermod -aG "$grp" agentos-kiosk
  done

  # seatd: allow agentos-kiosk to use seat0
  getent group seat &>/dev/null || groupadd seat
  usermod -aG seat agentos-kiosk
  systemctl enable --now seatd 2>/dev/null || true

  install -m 644 "$REPO_DIR/agentd/deploy/cage-kiosk.service" \
    /etc/systemd/system/cage-kiosk.service
  systemctl daemon-reload
  systemctl enable cage-kiosk.service
  ok "Cage kiosk installed (starts at boot when monitor detected)"
fi

# ── Voice I/O ──────────────────────────────────────────────────────────────────
if $DO_VOICE; then
  hdr "Voice I/O (whisper + piper + wake word)"

  # ── whisper.cpp ──
  WHISPER_DIR=/opt/whisper.cpp
  WHISPER_DATA=/var/lib/agentd/whisper
  WHISPER_MODEL=ggml-tiny.en.bin
  WAKE_MODEL=ggml-base.en.bin

  if [[ ! -d "$WHISPER_DIR/.git" ]]; then
    info "Cloning whisper.cpp …"
    git clone --depth=1 https://github.com/ggerganov/whisper.cpp "$WHISPER_DIR"
  fi
  chown -R "$BUILD_USER":"$BUILD_USER" "$WHISPER_DIR" 2>/dev/null || true

  if [[ ! -x "$WHISPER_DIR/build/bin/whisper-cli" ]]; then
    info "Building whisper.cpp (this takes ~5 min on Pi 5) …"
    sudo -u "$BUILD_USER" bash -c "
      cd $WHISPER_DIR
      cmake -B build -DCMAKE_BUILD_TYPE=Release -DWHISPER_NO_AVX=ON 2>&1 | tail -3
      cmake --build build -j4 --config Release 2>&1 | tail -5
    "
  fi
  ln -sf "$WHISPER_DIR/build/bin/whisper-cli" /usr/local/bin/whisper-cpp

  for model in "$WHISPER_MODEL" "$WAKE_MODEL"; do
    if [[ ! -f "$WHISPER_DATA/$model" ]]; then
      info "Downloading whisper model: $model …"
      (cd "$WHISPER_DIR" && bash models/download-ggml-model.sh "${model#ggml-}" 2>&1 | tail -3)
      cp "$WHISPER_DIR/models/$model" "$WHISPER_DATA/"
    fi
  done
  chown -R agentd:agentd "$WHISPER_DATA"
  ok "whisper-cpp ready (tiny.en + base.en)"

  # ── piper ──
  PIPER_DIR=/opt/piper
  PIPER_DATA=/var/lib/agentd/piper
  PIPER_VOICE=en_US-lessac-medium

  if [[ ! -f "$PIPER_DIR/piper" ]]; then
    info "Downloading piper (arm64) …"
    TMP=$(mktemp -d)
    wget -q -O "$TMP/piper.tar.gz" \
      "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_aarch64.tar.gz"
    tar -xzf "$TMP/piper.tar.gz" -C "$TMP"
    mkdir -p "$PIPER_DIR"
    cp -r "$TMP/piper/." "$PIPER_DIR/"
    rm -rf "$TMP"
  fi
  ln -sf "$PIPER_DIR/piper" /usr/local/bin/piper

  if [[ ! -f "$PIPER_DATA/${PIPER_VOICE}.onnx" ]]; then
    info "Downloading piper voice: $PIPER_VOICE (~63MB) …"
    BASE="https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium"
    TMP=$(mktemp -d)
    wget -q -O "$TMP/${PIPER_VOICE}.onnx"      "$BASE/${PIPER_VOICE}.onnx"
    wget -q -O "$TMP/${PIPER_VOICE}.onnx.json" "$BASE/${PIPER_VOICE}.onnx.json"
    cp "$TMP/${PIPER_VOICE}.onnx"      "$PIPER_DATA/"
    cp "$TMP/${PIPER_VOICE}.onnx.json" "$PIPER_DATA/"
    rm -rf "$TMP"
  fi
  chown -R agentd:agentd "$PIPER_DATA"
  ok "piper ready (${PIPER_VOICE})"

  # ── apex-wake service ──
  mkdir -p /opt/apex-wake
  install -m 755 "$REPO_DIR/deploy/apex-wake.py" /opt/apex-wake/wake.py
  install -m 644 "$REPO_DIR/deploy/apex-wake.service" /etc/systemd/system/apex-wake.service

  # Detect ALSA capture device
  ALSA_DEV="plughw:0,0"
  if command -v arecord &>/dev/null; then
    CARD=$(arecord -l 2>/dev/null | grep "^card" | head -1 | grep -oP "card \K[0-9]+")
    [[ -n "$CARD" ]] && ALSA_DEV="plughw:${CARD},0"
  fi

  # Voice env vars
  VOICE_VARS=(
    "WHISPER_MODEL=${WHISPER_DATA}/ggml-tiny.en.bin"
    "WHISPER_BIN=/usr/local/bin/whisper-cpp"
    "WAKE_WHISPER_MODEL=${WHISPER_DATA}/ggml-base.en.bin"
    "PIPER_MODEL=${PIPER_DATA}/${PIPER_VOICE}.onnx"
    "ALSA_CAPTURE_DEVICE=${ALSA_DEV}"
  )
  for kv in "${VOICE_VARS[@]}"; do
    KEY="${kv%%=*}"
    grep -q "^${KEY}=" /etc/agentd/env 2>/dev/null \
      && sed -i "s|^${KEY}=.*|${kv}|" /etc/agentd/env \
      || echo "$kv" >> /etc/agentd/env
  done

  systemctl daemon-reload
  systemctl enable apex-wake.service
  ok "apex-wake service installed (wake phrase: 'apex')"
fi

# ── Anthropic API key ──────────────────────────────────────────────────────────
hdr "Anthropic API key"

# Touch env file if it doesn't exist
touch /etc/agentd/env
chmod 600 /etc/agentd/env

if [[ -n "$API_KEY" ]]; then
  grep -q "^ANTHROPIC_API_KEY=" /etc/agentd/env \
    && sed -i "s|^ANTHROPIC_API_KEY=.*|ANTHROPIC_API_KEY=${API_KEY}|" /etc/agentd/env \
    || echo "ANTHROPIC_API_KEY=${API_KEY}" >> /etc/agentd/env
  ok "API key set"
elif ! grep -q "^ANTHROPIC_API_KEY=sk-" /etc/agentd/env 2>/dev/null; then
  if $YES; then
    warn "No API key provided — add it later via the UI or:"
    dim "  echo 'ANTHROPIC_API_KEY=sk-ant-...' | sudo tee -a /etc/agentd/env"
  else
    echo ""
    echo -e "  Get a free key at ${CYAN}https://console.anthropic.com${NC}"
    ask "Paste your Anthropic API key (sk-ant-...) or press Enter to skip:"
    read -r KEY_INPUT </dev/tty || true
    if [[ "$KEY_INPUT" == sk-ant-* ]]; then
      grep -q "^ANTHROPIC_API_KEY=" /etc/agentd/env \
        && sed -i "s|^ANTHROPIC_API_KEY=.*|ANTHROPIC_API_KEY=${KEY_INPUT}|" /etc/agentd/env \
        || echo "ANTHROPIC_API_KEY=${KEY_INPUT}" >> /etc/agentd/env
      ok "API key saved"
    else
      warn "Skipped — enter the key through the UI on first launch"
    fi
  fi
else
  ok "API key already configured"
fi

# ── Start services ─────────────────────────────────────────────────────────────
hdr "Starting services"

systemctl daemon-reload
systemctl enable agentd
systemctl restart agentd
sleep 3

if systemctl is-active --quiet agentd; then
  ok "agentd is running"
else
  warn "agentd failed to start — last 20 log lines:"
  journalctl -u agentd -n 20 --no-pager 2>/dev/null | sed 's/^/    /'
  die "Fix the error above and re-run, or start manually: sudo systemctl start agentd"
fi

$DO_VOICE && systemctl enable --now apex-wake 2>/dev/null && ok "apex-wake running"

# ── Done ───────────────────────────────────────────────────────────────────────
PI_IP=$(hostname -I 2>/dev/null | awk '{print $1}')

echo ""
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GREEN}${BOLD}  ApexOS is live!${NC}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo ""
echo -e "  Desktop UI →  ${CYAN}http://${PI_IP:-<pi-ip>}:8787${NC}"
echo -e "  CLI skin   →  ${CYAN}http://${PI_IP:-<pi-ip>}:8787/index.html${NC}"
echo ""
if ! grep -q "^ANTHROPIC_API_KEY=sk-" /etc/agentd/env 2>/dev/null; then
  echo -e "  ${YELLOW}⚠  API key not set — open the UI and enter it in the key dialogue${NC}"
  echo ""
fi
$DO_VOICE && echo -e "  Voice      →  say ${BOLD}\"apex\"${NC} to wake, or ${BOLD}Ctrl+Space${NC} in UI"
$DO_KIOSK && echo -e "  Kiosk      →  connect an HDMI monitor and reboot"
echo ""
echo -e "${DIM}  Logs: journalctl -u agentd -f${NC}"
echo -e "${DIM}  Docs: https://github.com/buckster123/ApexOS${NC}"
echo ""
