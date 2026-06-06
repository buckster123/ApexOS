#!/usr/bin/env bash
# ApexOS bootstrap — run once on a fresh headless Raspberry Pi
#
# One-liner (public repo):
#   curl -fsSL https://raw.githubusercontent.com/buckster123/ApexOS/main/install.sh | sudo bash
#
# Or clone first then run:
#   git clone https://github.com/buckster123/ApexOS.git && sudo bash ApexOS/install.sh
#
# Safe to re-run; each step is idempotent.
set -euo pipefail

# ── colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; CYAN='\033[0;36m'; NC='\033[0m'
ok()   { echo -e "${GREEN}  ✓ $*${NC}"; }
info() { echo -e "${CYAN}  → $*${NC}"; }
warn() { echo -e "${YELLOW}  ⚠ $*${NC}"; }
die()  { echo -e "${RED}  ✗ $*${NC}" >&2; exit 1; }

# ── must be root ─────────────────────────────────────────────────────────────
[[ $EUID -eq 0 ]] || die "Run as root: sudo bash install.sh"

# ── locate repo root ─────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$SCRIPT_DIR"
[[ -f "$REPO_DIR/agentd/Cargo.toml" ]] || die "Run from the ApexOS repo root (Cargo.toml not found)"

echo ""
echo -e "${CYAN}╔══════════════════════════════════════╗${NC}"
echo -e "${CYAN}║     ApexOS — Bootstrap Installer     ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════╝${NC}"
echo ""

# ── 1. system packages ───────────────────────────────────────────────────────
echo "── 1/7  System packages"
apt-get update -qq
apt-get install -y -qq \
    curl git build-essential pkg-config \
    libssl-dev libsqlite3-dev \
    python3 python3-pip python3-venv \
    tesseract-ocr \
    2>/dev/null
ok "system packages ready"

# ── 2. agentd user + directories ─────────────────────────────────────────────
echo "── 2/7  Users and directories"

if ! id agentd &>/dev/null; then
    useradd --system --no-create-home --shell /usr/sbin/nologin agentd
    ok "created agentd system user"
else
    ok "agentd user already exists"
fi

mkdir -p /etc/agentd
mkdir -p /var/lib/agentd/{workspace,events,cerebro}
mkdir -p /var/pip-tmp                    # pip temp on NVMe, avoids 2GB tmpfs limit
mkdir -p /opt/cerebro-venv

chown -R agentd:agentd /var/lib/agentd
chmod 750 /etc/agentd
ok "directories ready"

# ── 3. Rust toolchain (for agentd user and install user) ────────────────────
echo "── 3/7  Rust toolchain"

CARGO_HOME_PATH="/home/agentd/.cargo"
RUSTUP_HOME_PATH="/home/agentd/.rustup"

# Install for the user running this script (likely root or pi user via sudo)
# We build as the current SUDO_USER if set, otherwise root
BUILD_USER="${SUDO_USER:-root}"
BUILD_HOME=$(eval echo "~$BUILD_USER")

if [[ ! -x "$BUILD_HOME/.cargo/bin/cargo" ]]; then
    info "installing Rust for $BUILD_USER …"
    sudo -u "$BUILD_USER" bash -c \
        'curl -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path 2>&1' \
        | grep -E '(installed|updated|stable|error)' || true
fi

CARGO="$BUILD_HOME/.cargo/bin/cargo"
[[ -x "$CARGO" ]] || die "Rust install failed — cargo not found at $CARGO"
RUST_VER=$($CARGO --version)
ok "Rust ready: $RUST_VER"

# ── 4. Build agentd ──────────────────────────────────────────────────────────
echo "── 4/7  Build agentd (this takes ~2 min on Pi 5)"

cd "$REPO_DIR/agentd"
sudo -u "$BUILD_USER" "$CARGO" build --release 2>&1 \
    | grep -E '(Compiling agentd|Finished|error)' || true

BINARY="$REPO_DIR/agentd/target/release/agentd"
[[ -x "$BINARY" ]] || die "Build failed — binary not found"
ok "agentd built ($(du -sh "$BINARY" | cut -f1))"

# install binary
install -m 755 "$BINARY" /usr/local/bin/agentd
ok "binary installed to /usr/local/bin/agentd"

# ── 5. CerebroCortex in venv ─────────────────────────────────────────────────
echo "── 5/7  CerebroCortex"

if [[ ! -x /opt/cerebro-venv/bin/cerebro-mcp ]]; then
    info "creating venv …"
    python3 -m venv /opt/cerebro-venv

    # Check if we have a local CerebroCortex source alongside the repo
    CEREBRO_SRC=""
    for candidate in \
            "$REPO_DIR/../CerebroCortex" \
            "$BUILD_HOME/CerebroCortex" \
            "/home/pi/CerebroCortex"; do
        if [[ -f "$candidate/pyproject.toml" ]]; then
            CEREBRO_SRC="$candidate"
            break
        fi
    done

    if [[ -n "$CEREBRO_SRC" ]]; then
        info "installing CerebroCortex from local source: $CEREBRO_SRC"
        TMPDIR=/var/pip-tmp /opt/cerebro-venv/bin/pip install \
            --cache-dir /var/pip-tmp/cache \
            "$CEREBRO_SRC[all]" 2>&1 \
            | grep -E '(Successfully|error|ERROR)' || true
    else
        # Fall back to GitHub
        CEREBRO_TMP=$(mktemp -d)
        info "cloning CerebroCortex …"
        git clone --depth 1 https://github.com/buckster123/CerebroCortex "$CEREBRO_TMP/CerebroCortex" 2>&1
        TMPDIR=/var/pip-tmp /opt/cerebro-venv/bin/pip install \
            --cache-dir /var/pip-tmp/cache \
            "$CEREBRO_TMP/CerebroCortex[all]" 2>&1 \
            | grep -E '(Successfully|error|ERROR)' || true
        rm -rf "$CEREBRO_TMP"
    fi
fi

[[ -x /opt/cerebro-venv/bin/cerebro-mcp ]] || die "CerebroCortex install failed"

# wrapper so cerebro-mcp is in PATH for the agentd service
cat > /usr/local/bin/cerebro-mcp << 'EOF'
#!/bin/bash
exec /opt/cerebro-venv/bin/cerebro-mcp "$@"
EOF
chmod +x /usr/local/bin/cerebro-mcp
ok "CerebroCortex ready ($(/opt/cerebro-venv/bin/cerebro-mcp --version 2>&1 | grep -oE 'v[0-9.]+' || echo 'installed'))"

# ── 6. Config files ──────────────────────────────────────────────────────────
echo "── 6/7  Configuration"

# plugins.toml — use Pi-specific variant if present
if [[ -f "$REPO_DIR/agentd/config/plugins.pi.toml" ]]; then
    install -m 644 "$REPO_DIR/agentd/config/plugins.pi.toml" /etc/agentd/plugins.toml
else
    install -m 644 "$REPO_DIR/agentd/config/plugins.toml" /etc/agentd/plugins.toml
fi

install -m 644 "$REPO_DIR/agentd/config/policy.toml"  /etc/agentd/policy.toml
install -m 644 "$REPO_DIR/agentd/deploy/agentd.service" /etc/systemd/system/agentd.service

# UI static files
mkdir -p /usr/local/share/agentd/ui
cp -r "$REPO_DIR/ui/." /usr/local/share/agentd/ui/
ok "UI files installed to /usr/local/share/agentd/ui"

systemctl daemon-reload
systemctl enable agentd
ok "config and service installed"

# ── 7. API key ───────────────────────────────────────────────────────────────
echo "── 7/7  Anthropic API key"

if [[ ! -s /etc/agentd/env ]]; then
    echo ""
    echo -e "${YELLOW}  Enter your Anthropic API key (sk-ant-...) and press Enter.${NC}"
    echo -e "${YELLOW}  It will be stored in /etc/agentd/env (chmod 600, root-only).${NC}"
    echo -n "  ANTHROPIC_API_KEY= "
    read -r API_KEY
    if [[ -n "$API_KEY" ]]; then
        echo "ANTHROPIC_API_KEY=${API_KEY}" > /etc/agentd/env
        chmod 600 /etc/agentd/env
        ok "API key saved"
    else
        warn "no key entered — add it later: echo 'ANTHROPIC_API_KEY=sk-ant-...' > /etc/agentd/env"
    fi
else
    ok "API key already present"
fi

# ── start ────────────────────────────────────────────────────────────────────
echo ""
info "starting agentd …"
systemctl start agentd
sleep 4

echo ""
if systemctl is-active --quiet agentd; then
    ok "agentd is running"
    journalctl -u agentd -n 8 --no-pager 2>/dev/null | sed 's/^/     /'
    echo ""
    echo -e "${GREEN}  ApexOS is live — WebSocket at ws://$(hostname -I | awk '{print $1}'):8787/ws${NC}"
else
    warn "agentd failed to start — check logs:"
    journalctl -u agentd -n 20 --no-pager 2>/dev/null | sed 's/^/     /'
    exit 1
fi
echo ""
