#!/usr/bin/env bash
# ApexOS voice setup: whisper.cpp (STT) + piper (TTS)
# Run as a user with sudo access on the Pi (not as root).
# Usage: bash deploy/setup-voice.sh
set -euo pipefail

WHISPER_DIR=/opt/whisper.cpp
WHISPER_MODEL=ggml-tiny.en.bin
WHISPER_DATA=/var/lib/agentd/whisper

PIPER_DIR=/opt/piper
PIPER_DATA=/var/lib/agentd/piper
PIPER_VOICE=en_US-lessac-medium

echo "=== [1/6] Installing system deps ==="
sudo apt-get update -qq
sudo apt-get install -y --no-install-recommends ffmpeg build-essential git

echo "=== [2/6] Building whisper.cpp ==="
if [ ! -d "$WHISPER_DIR" ]; then
  sudo git clone --depth=1 https://github.com/ggerganov/whisper.cpp "$WHISPER_DIR"
  sudo chown -R "$(whoami):$(whoami)" "$WHISPER_DIR"
fi
cd "$WHISPER_DIR"
make -j4

echo "=== [3/6] Downloading whisper tiny.en model ==="
sudo mkdir -p "$WHISPER_DATA"
sudo chown -R agentd:agentd "$WHISPER_DATA"
if [ ! -f "$WHISPER_DATA/$WHISPER_MODEL" ]; then
  bash models/download-ggml-model.sh tiny.en
  sudo cp "models/$WHISPER_MODEL" "$WHISPER_DATA/"
fi
sudo chmod 644 "$WHISPER_DATA/$WHISPER_MODEL"
sudo ln -sf "$WHISPER_DIR/main" /usr/local/bin/whisper-cpp
echo "whisper-cpp -> $(readlink /usr/local/bin/whisper-cpp)"

echo "=== [4/6] Installing piper (arm64) ==="
sudo mkdir -p "$PIPER_DIR"
if [ ! -f "$PIPER_DIR/piper" ]; then
  TMP=$(mktemp -d)
  # Piper 2023.11.14-2 arm64 release
  wget -q -O "$TMP/piper.tar.gz" \
    "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_aarch64.tar.gz"
  tar -xzf "$TMP/piper.tar.gz" -C "$TMP"
  sudo cp -r "$TMP/piper/." "$PIPER_DIR/"
  rm -rf "$TMP"
fi
sudo ln -sf "$PIPER_DIR/piper" /usr/local/bin/piper
echo "piper -> $(readlink /usr/local/bin/piper)"

echo "=== [5/6] Downloading piper voice: $PIPER_VOICE ==="
sudo mkdir -p "$PIPER_DATA"
sudo chown -R agentd:agentd "$PIPER_DATA"
VOICE_BASE="https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/lessac/medium"
for f in "${PIPER_VOICE}.onnx" "${PIPER_VOICE}.onnx.json"; do
  if [ ! -f "$PIPER_DATA/$f" ]; then
    sudo wget -q -P "$PIPER_DATA" "$VOICE_BASE/$f"
    sudo chown agentd:agentd "$PIPER_DATA/$f"
  fi
done

echo "=== [6/6] Adding WHISPER_MODEL + PIPER_MODEL to /etc/agentd/env ==="
ENV_FILE=/etc/agentd/env
for kv in \
  "WHISPER_MODEL=$WHISPER_DATA/$WHISPER_MODEL" \
  "WHISPER_BIN=/usr/local/bin/whisper-cpp" \
  "PIPER_MODEL=$PIPER_DATA/${PIPER_VOICE}.onnx"
do
  KEY="${kv%%=*}"
  if sudo grep -q "^$KEY=" "$ENV_FILE" 2>/dev/null; then
    echo "  $KEY already set, skipping"
  else
    echo "$kv" | sudo tee -a "$ENV_FILE" >/dev/null
    echo "  added $KEY"
  fi
done

echo ""
echo "=== Voice setup complete ==="
echo "Smoke tests:"
echo "  STT: echo '' | whisper-cpp -m $WHISPER_DATA/$WHISPER_MODEL --help"
echo "  TTS: echo 'hello from apex' | piper --model $PIPER_DATA/${PIPER_VOICE}.onnx --output-raw | aplay -q -r 22050 -f S16_LE -t raw -"
echo ""
echo "After deploying the new agentd binary, restart the service:"
echo "  sudo systemctl restart agentd"
