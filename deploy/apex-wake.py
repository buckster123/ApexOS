#!/usr/bin/env python3
"""
ApexOS wake word detector.
Continuously records 3s chunks from ALSA mic, transcribes with whisper-cpp,
triggers POST /api/wake when the wake phrase is heard.

Env vars (set in /etc/agentd/env or shell):
  WAKE_PHRASE          word/phrase to listen for (default: apex)
  WAKE_WHISPER_MODEL   path to whisper model (default: base.en)
  WAKE_WHISPER_BIN     whisper-cpp binary (default: /usr/local/bin/whisper-cpp)
  ALSA_CAPTURE_DEVICE  ALSA device (default: plughw:2,0)
  AGENTD_URL           agentd base URL (default: http://localhost:8787)
  WAKE_COOLDOWN        seconds to wait after trigger (default: 10)
  WAKE_CHUNK_SECS      recording chunk length (default: 3)
"""
import os
import subprocess
import tempfile
import time
import urllib.request

PHRASE   = os.environ.get("WAKE_PHRASE",        "apex").lower()
MODEL    = os.environ.get("WAKE_WHISPER_MODEL",  "/var/lib/agentd/whisper/ggml-base.en.bin")
BIN      = os.environ.get("WAKE_WHISPER_BIN",    "/usr/local/bin/whisper-cpp")
DEVICE   = os.environ.get("ALSA_CAPTURE_DEVICE", "plughw:2,0")
BASE_URL = os.environ.get("AGENTD_URL",          "http://localhost:8787")
COOLDOWN = int(os.environ.get("WAKE_COOLDOWN",   "10"))
CHUNK    = int(os.environ.get("WAKE_CHUNK_SECS", "3"))

print(f"[apex-wake] listening for '{PHRASE}' on {DEVICE} (model: {MODEL})", flush=True)

while True:
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as f:
        wav = f.name

    # Record a chunk
    rec = subprocess.run(
        ["arecord", "-D", DEVICE, "-f", "S16_LE", "-r", "16000", "-c", "1",
         "-d", str(CHUNK), wav],
        capture_output=True,
    )
    if rec.returncode != 0:
        print(f"[apex-wake] arecord error: {rec.stderr.decode()}", flush=True)
        time.sleep(1)
        os.unlink(wav)
        continue

    # Transcribe
    try:
        result = subprocess.run(
            [BIN, "-m", MODEL, "-f", wav, "-nt", "-l", "en", "--no-prints"],
            capture_output=True, text=True, timeout=15,
        )
        transcript = result.stdout.lower().strip()
    except subprocess.TimeoutExpired:
        transcript = ""
    finally:
        try:
            os.unlink(wav)
        except OSError:
            pass

    # Whisper hallucinates these on silence/music — ignore them
    HALLUCINATIONS = {"dramatic music", "upbeat music", "music playing", "applause",
                      "silence", "blank_audio", "thank you", "thanks for watching",
                      "you", "the", "[music]", "(music)"}
    if not transcript or any(h in transcript for h in HALLUCINATIONS):
        continue

    if PHRASE in transcript:
        print(f"[apex-wake] WAKE: '{transcript}'", flush=True)
        try:
            urllib.request.urlopen(
                urllib.request.Request(f"{BASE_URL}/api/wake", method="POST"),
                timeout=5,
            )
        except Exception as e:
            print(f"[apex-wake] POST /api/wake failed: {e}", flush=True)
        time.sleep(COOLDOWN)
    else:
        # Tiny log so you can see it's running without flooding
        if transcript and transcript != "[blank_audio]":
            print(f"[apex-wake] heard: {transcript[:60]}", flush=True)
