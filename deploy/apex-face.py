#!/usr/bin/env python3
"""
apex-face.py — GC9A01A round TFT face daemon for ApexOS
Listens on /tmp/apex-face.sock for JSON commands from display_face MCP tool.
Renders face states as animated PIL frames to the 240x240 SPI display.

Wiring (SYS-SPI GC9A01A 11-pin module):
  Vin  → Pi Pin 1  (3.3V)
  Gnd  → Pi Pin 6  (GND)
  SCK  → GPIO 11   (Pin 23)
  MOSI → GPIO 10   (Pin 19)
  MISO → GPIO 9    (Pin 21) — SD card only, display doesn't need MISO
  TFTCS→ GPIO 8    (Pin 24, CE0)
  RST  → GPIO 25   (Pin 22)
  DC   → GPIO 24   (Pin 18)
  lite → GPIO 18   (Pin 12, PWM for dimming or plain HIGH)
  sd_cs→ GPIO 7    (Pin 26, CE1) — tie HIGH if SD unused

Run: systemctl start apex-face
     OR: python3 /home/apexos/ApexOS/deploy/apex-face.py

Requires: pip install lgpio spidev pillow
          dtparam=spi=on in /boot/firmware/config.txt (then reboot)
"""

import os
import sys
import json
import time
import math
import socket
import struct
import threading
import signal

try:
    import lgpio
    import spidev
    from PIL import Image, ImageDraw, ImageFont
except ImportError as e:
    print(f"Missing dependency: {e}")
    print("Run: pip install lgpio spidev pillow")
    sys.exit(1)

# ── Pin config (BCM GPIO numbers) ────────────────────────────────────────────
DC_PIN  = 24
RST_PIN = 25
BL_PIN  = 18
SPI_BUS = 0
SPI_DEV = 0   # CE0
SPI_HZ  = 80_000_000

SOCK_PATH = "/tmp/apex-face.sock"
WIDTH  = 240
HEIGHT = 240

# ── Pi 5 vs Pi 3/4: gpiochip ─────────────────────────────────────────────────
def detect_gpio_chip():
    try:
        model = open("/proc/device-tree/model").read()
        return 4 if "Raspberry Pi 5" in model else 0
    except Exception:
        return 0

# ── GC9A01A init sequence (from Adafruit_GC9A01A.cpp, initcmd[]) ─────────────
GC9A01A_INIT = [
    (0xEF, []),
    (0xEB, [0x14]),
    (0xFE, []),
    (0xEF, []),
    (0xEB, [0x14]),
    (0x84, [0x40]),
    (0x85, [0xFF]),
    (0x86, [0xFF]),
    (0x87, [0xFF]),
    (0x88, [0x0A]),
    (0x89, [0x21]),
    (0x8A, [0x00]),
    (0x8B, [0x80]),
    (0x8C, [0x01]),
    (0x8D, [0x01]),
    (0x8E, [0xFF]),
    (0x8F, [0xFF]),
    (0xB6, [0x00, 0x00]),
    (0x36, [0x48]),           # MADCTL: MX | BGR
    (0x3A, [0x05]),           # COLMOD: RGB565
    (0x90, [0x08, 0x08, 0x08, 0x08]),
    (0xBD, [0x06]),
    (0xBC, [0x00]),
    (0xFF, [0x60, 0x01, 0x04]),
    (0xC3, [0x13]),
    (0xC4, [0x13]),
    (0xC9, [0x22]),
    (0xBE, [0x11]),
    (0xE1, [0x10, 0x0E]),
    (0xDF, [0x21, 0x0C, 0x02]),
    (0xF0, [0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),
    (0xF1, [0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),
    (0xF2, [0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),
    (0xF3, [0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),
    (0xED, [0x1B, 0x0B]),
    (0xAE, [0x77]),
    (0xCD, [0x63]),
    # 0x70 line omitted — Adafruit source: "users reported issues"
    (0xE8, [0x34]),
    (0x62, [0x18, 0x0D, 0x71, 0xED, 0x70, 0x70,
            0x18, 0x0F, 0x71, 0xEF, 0x70, 0x70]),
    (0x63, [0x18, 0x11, 0x71, 0xF1, 0x70, 0x70,
            0x18, 0x13, 0x71, 0xF3, 0x70, 0x70]),
    (0x64, [0x28, 0x29, 0xF1, 0x01, 0xF1, 0x00, 0x07]),
    (0x66, [0x3C, 0x00, 0xCD, 0x67, 0x45, 0x45, 0x10, 0x00, 0x00, 0x00]),
    (0x67, [0x00, 0x3C, 0x00, 0x00, 0x00, 0x01, 0x54, 0x10, 0x32, 0x98]),
    (0x74, [0x10, 0x85, 0x80, 0x00, 0x00, 0x4E, 0x00]),
    (0x98, [0x3E, 0x07]),
    (0x35, []),               # Tearing effect ON
    (0x21, []),               # Inversion ON (required for correct colors)
]

# ── Display driver ────────────────────────────────────────────────────────────
class GC9A01A:
    def __init__(self):
        chip = detect_gpio_chip()
        self.h = lgpio.gpiochip_open(chip)
        lgpio.gpio_claim_output(self.h, DC_PIN)
        lgpio.gpio_claim_output(self.h, RST_PIN)
        lgpio.gpio_claim_output(self.h, BL_PIN)

        self.spi = spidev.SpiDev()
        self.spi.open(SPI_BUS, SPI_DEV)
        self.spi.max_speed_hz = SPI_HZ
        self.spi.mode = 0

    def _cmd(self, c, data=None):
        lgpio.gpio_write(self.h, DC_PIN, 0)
        self.spi.xfer2([c])
        if data:
            lgpio.gpio_write(self.h, DC_PIN, 1)
            self.spi.xfer2(list(data))

    def reset(self):
        lgpio.gpio_write(self.h, RST_PIN, 1); time.sleep(0.01)
        lgpio.gpio_write(self.h, RST_PIN, 0); time.sleep(0.01)
        lgpio.gpio_write(self.h, RST_PIN, 1); time.sleep(0.15)

    def init(self):
        self.reset()
        for cmd, data in GC9A01A_INIT:
            self._cmd(cmd, data or None)
        self._cmd(0x11); time.sleep(0.15)   # Sleep out
        self._cmd(0x29); time.sleep(0.15)   # Display on
        lgpio.gpio_write(self.h, BL_PIN, 1) # Backlight on

    def show(self, img: Image.Image):
        img = img.convert('RGB').resize((WIDTH, HEIGHT))
        self._cmd(0x2A, [0x00, 0x00, 0x00, 0xEF])  # CASET 0..239
        self._cmd(0x2B, [0x00, 0x00, 0x00, 0xEF])  # RASET 0..239
        self._cmd(0x2C)                              # RAMWR
        pixels = img.tobytes('raw', 'RGB')
        buf = bytearray(WIDTH * HEIGHT * 2)
        for i in range(WIDTH * HEIGHT):
            r, g, b = pixels[i*3], pixels[i*3+1], pixels[i*3+2]
            rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
            buf[i*2]   = (rgb565 >> 8) & 0xFF
            buf[i*2+1] = rgb565 & 0xFF
        lgpio.gpio_write(self.h, DC_PIN, 1)
        chunk = 4096
        for off in range(0, len(buf), chunk):
            self.spi.xfer2(buf[off:off+chunk])

    def close(self):
        lgpio.gpio_write(self.h, BL_PIN, 0)
        self.spi.close()
        lgpio.gpiochip_close(self.h)

# ── Face renderer ─────────────────────────────────────────────────────────────
# Palette
BG       = (8,   8,  18)    # deep dark blue-black
EYE      = (57, 255, 20)    # neon green
PUPIL    = (0,   0,   0)
MOUTH    = (57, 255, 20)
DIM      = (20,  60,  20)
ALERT    = (255, 60,  30)
BLUE     = (30, 160, 255)
GOLD     = (255, 200,  40)
WHITE    = (230, 230, 230)

def _circle_mask(draw, cx, cy, r, color):
    draw.ellipse([cx-r, cy-r, cx+r, cy+r], fill=color)

def _eye(draw, cx, cy, open_ratio=1.0, color=EYE):
    r_outer = 28
    r_inner = int(18 * open_ratio)
    r_pupil = 10
    _circle_mask(draw, cx, cy, r_outer, color)
    _circle_mask(draw, cx, cy, r_inner, PUPIL)
    if open_ratio > 0.3:
        _circle_mask(draw, cx, cy, r_pupil, PUPIL)
    # highlight
    if open_ratio > 0.5:
        _circle_mask(draw, cx-8, cy-8, 5, WHITE)

def _mouth_smile(draw, cx, cy, color=MOUTH):
    # Arc approximated with line segments
    points = []
    for i in range(13):
        t = math.pi * i / 12
        x = cx - 30 + int(60 * i / 12)
        y = cy + int(16 * math.sin(t))
        points.append((x, y))
    draw.line(points, fill=color, width=4)

def _mouth_neutral(draw, cx, cy, color=MOUTH):
    draw.line([(cx-25, cy), (cx+25, cy)], fill=color, width=4)

def _mouth_open(draw, cx, cy, open_h=12, color=MOUTH):
    draw.ellipse([cx-20, cy-open_h//2, cx+20, cy+open_h//2], fill=color)

def render_face(state: str, text: str = "", tick: int = 0) -> Image.Image:
    img  = Image.new('RGB', (WIDTH, HEIGHT), BG)
    draw = ImageDraw.Draw(img)

    # Circular face boundary
    draw.ellipse([2, 2, WIDTH-3, HEIGHT-3], outline=(30, 30, 50), width=2)

    cx, cy = 120, 110  # face centre

    if state == 'idle':
        _eye(draw, cx-45, cy-20)
        _eye(draw, cx+45, cy-20)
        _mouth_smile(draw, cx, cy+35)

    elif state == 'thinking':
        # One eye slightly squinted, looking up-right
        blink = 0.7 + 0.3 * math.sin(tick * 0.15)
        _eye(draw, cx-45, cy-20, open_ratio=blink)
        _eye(draw, cx+45, cy-25, open_ratio=blink * 0.8)
        _mouth_neutral(draw, cx, cy+35)
        # Thinking dots
        for i in range(3):
            alpha = 0.3 + 0.7 * ((tick + i * 4) % 12 < 6)
            col = tuple(int(c * alpha) for c in EYE)
            draw.ellipse([cx-12+i*14, cy+55, cx-4+i*14, cy+63], fill=col)

    elif state == 'speaking':
        open_h = 6 + 10 * abs(math.sin(tick * 0.4))
        _eye(draw, cx-45, cy-20)
        _eye(draw, cx+45, cy-20)
        _mouth_open(draw, cx, cy+38, open_h=int(open_h))

    elif state == 'alert':
        # Red eyes, open wide
        _eye(draw, cx-45, cy-20, color=ALERT)
        _eye(draw, cx+45, cy-20, color=ALERT)
        _mouth_open(draw, cx, cy+35, open_h=16, color=ALERT)
        # Alert ring pulse
        pulse_r = 115 - (tick % 10) * 3
        if pulse_r > 50:
            draw.ellipse([cx-pulse_r, cy-pulse_r, cx+pulse_r, cy+pulse_r],
                         outline=ALERT, width=2)

    elif state == 'listening':
        _eye(draw, cx-45, cy-20, color=BLUE)
        _eye(draw, cx+45, cy-20, color=BLUE)
        _mouth_neutral(draw, cx, cy+35, color=BLUE)
        # Sound wave arcs on the sides
        for i in range(1, 4):
            r = 15 + i * 12
            alpha = 0.3 + 0.7 * ((tick + i * 3) % 9 < 5)
            col = tuple(int(c * alpha) for c in BLUE)
            draw.arc([cx-60-r, cy-r, cx-60+r, cy+r], -60, 60, fill=col, width=2)
            draw.arc([cx+60-r, cy-r, cx+60+r, cy+r], 120, 240, fill=col, width=2)

    elif state == 'sleeping':
        # Half-closed eyes (squint)
        _eye(draw, cx-45, cy-20, open_ratio=0.15, color=DIM)
        _eye(draw, cx+45, cy-20, open_ratio=0.15, color=DIM)
        _mouth_neutral(draw, cx, cy+35, color=DIM)
        # Zzz
        draw.text((cx+50, cy-50), "z", fill=DIM)
        draw.text((cx+60, cy-65), "Z", fill=DIM)
        draw.text((cx+72, cy-82), "Z", fill=DIM)

    elif state == 'happy':
        _eye(draw, cx-45, cy-20, color=GOLD)
        _eye(draw, cx+45, cy-20, color=GOLD)
        _mouth_smile(draw, cx, cy+35, color=GOLD)
        # Star sparkles
        for i in range(4):
            angle = (tick * 3 + i * 90) * math.pi / 180
            sx = cx + int(85 * math.cos(angle))
            sy = cy + int(85 * math.sin(angle))
            draw.text((sx-4, sy-4), "✦", fill=GOLD)

    # Optional text line at bottom
    if text:
        short = text[:22]
        draw.text((WIDTH//2 - len(short)*4, HEIGHT - 28), short, fill=WHITE)

    return img

# ── Animation loop ────────────────────────────────────────────────────────────
class FaceDaemon:
    def __init__(self):
        self.state = 'idle'
        self.text  = ''
        self.tick  = 0
        self.lock  = threading.Lock()
        self.display = None
        self.running = True

    def set_state(self, state: str, text: str = ''):
        with self.lock:
            self.state = state
            self.text  = text

    def animate(self):
        try:
            self.display = GC9A01A()
            self.display.init()
            print("[apex-face] display initialised", flush=True)
        except Exception as e:
            print(f"[apex-face] display init failed: {e}", flush=True)
            self.display = None

        while self.running:
            with self.lock:
                state = self.state
                text  = self.text
            try:
                frame = render_face(state, text, self.tick)
                if self.display:
                    self.display.show(frame)
            except Exception as e:
                print(f"[apex-face] render error: {e}", flush=True)
            self.tick += 1
            time.sleep(0.12)  # ~8 fps — smooth enough, Pi 5 handles it

    def stop(self):
        self.running = False
        if self.display:
            self.display.close()

# ── Unix socket server ────────────────────────────────────────────────────────
def serve(daemon: FaceDaemon):
    if os.path.exists(SOCK_PATH):
        os.unlink(SOCK_PATH)
    srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    srv.bind(SOCK_PATH)
    os.chmod(SOCK_PATH, 0o666)
    srv.listen(5)
    srv.settimeout(1.0)
    print(f"[apex-face] listening on {SOCK_PATH}", flush=True)

    while daemon.running:
        try:
            conn, _ = srv.accept()
        except socket.timeout:
            continue
        try:
            data = conn.recv(256).decode().strip()
            msg  = json.loads(data)
            state = msg.get('state', 'idle')
            text  = msg.get('text', '')
            daemon.set_state(state, text)
        except Exception:
            pass
        finally:
            conn.close()

    srv.close()
    if os.path.exists(SOCK_PATH):
        os.unlink(SOCK_PATH)

# ── Main ──────────────────────────────────────────────────────────────────────
def main():
    daemon = FaceDaemon()

    def _shutdown(sig, frame):
        print("[apex-face] shutting down", flush=True)
        daemon.stop()

    signal.signal(signal.SIGTERM, _shutdown)
    signal.signal(signal.SIGINT,  _shutdown)

    anim_thread = threading.Thread(target=daemon.animate, daemon=True)
    anim_thread.start()

    serve(daemon)
    daemon.stop()
    anim_thread.join(timeout=2)

if __name__ == '__main__':
    main()
