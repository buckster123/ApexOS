# GC9A01A Display Driver — Local Reference

Source: https://github.com/adafruit/Adafruit_GC9A01A (BSD license)
Pulled: 2026-06-09. Arduino/C++ library — NOT used directly on Pi Linux.
Purpose: Init sequence + protocol reference for `apex-face.py` (Python/spidev).

---

## Display facts

| Property | Value |
|----------|-------|
| Resolution | 240 × 240 pixels |
| Color depth | 16-bit RGB565 |
| Color order | BGR (MADCTL 0x48 = MX \| BGR) |
| Interface | SPI (4-wire: SCLK, MOSI, CS, DC) |
| SPI speed (Pi) | 80 MHz (`#define SPI_DEFAULT_FREQ 80000000` under `RASPI`) |
| Inversion | ON (INVON 0x21 is part of init) |
| Backlight | Active HIGH — `gpio_write(BL, 1)` to enable |

---

## Init sequence (translated from initcmd[] in Adafruit_GC9A01A.cpp)

Use this verbatim in `apex-face.py`. The 0x80 flag in the original means "delay 150ms after".

```python
import time

GC9A01A_INIT = [
    # (command, [data_bytes])
    (0xEF, []),
    (0xEB, [0x14]),
    (0xFE, []),              # Inter register enable 1
    (0xEF, []),              # Inter register enable 2
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
    (0x36, [0x48]),          # MADCTL: MX | BGR
    (0x3A, [0x05]),          # COLMOD: RGB565 (16-bit)
    (0x90, [0x08, 0x08, 0x08, 0x08]),
    (0xBD, [0x06]),
    (0xBC, [0x00]),
    (0xFF, [0x60, 0x01, 0x04]),
    (0xC3, [0x13]),          # Power Control 2
    (0xC4, [0x13]),          # Power Control 3
    (0xC9, [0x22]),          # Power Control 4
    (0xBE, [0x11]),
    (0xE1, [0x10, 0x0E]),
    (0xDF, [0x21, 0x0C, 0x02]),
    (0xF0, [0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),   # Gamma 1
    (0xF1, [0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),   # Gamma 2
    (0xF2, [0x45, 0x09, 0x08, 0x08, 0x26, 0x2A]),   # Gamma 3
    (0xF3, [0x43, 0x70, 0x72, 0x36, 0x37, 0x6F]),   # Gamma 4
    (0xED, [0x1B, 0x0B]),
    (0xAE, [0x77]),
    (0xCD, [0x63]),
    # NOTE: 0x70 line commented out in Adafruit source — "users reported issues"
    # (0x70, [0x07, 0x07, 0x04, 0x0E, 0x0F, 0x09, 0x07, 0x08, 0x03]),
    (0xE8, [0x34]),          # Frame rate control
    (0x62, [0x18, 0x0D, 0x71, 0xED, 0x70, 0x70,
            0x18, 0x0F, 0x71, 0xEF, 0x70, 0x70]),
    (0x63, [0x18, 0x11, 0x71, 0xF1, 0x70, 0x70,
            0x18, 0x13, 0x71, 0xF3, 0x70, 0x70]),
    (0x64, [0x28, 0x29, 0xF1, 0x01, 0xF1, 0x00, 0x07]),
    (0x66, [0x3C, 0x00, 0xCD, 0x67, 0x45, 0x45, 0x10, 0x00, 0x00, 0x00]),
    (0x67, [0x00, 0x3C, 0x00, 0x00, 0x00, 0x01, 0x54, 0x10, 0x32, 0x98]),
    (0x74, [0x10, 0x85, 0x80, 0x00, 0x00, 0x4E, 0x00]),
    (0x98, [0x3E, 0x07]),
    (0x35, []),              # Tearing effect ON
    (0x21, []),              # Inversion ON
]
# After init sequence, send these with delays:
# 0x11 (SLPOUT) + sleep 0.15s
# 0x29 (DISPON) + sleep 0.15s
```

---

## Sending a command via spidev + lgpio (Pi 5 safe)

```python
import spidev
import lgpio

# Pin config (BCM GPIO numbers)
DC_PIN  = 24
RST_PIN = 25
BL_PIN  = 18
CS_PIN  = 8   # CE0 — handled by spidev, no manual toggle needed

h = lgpio.gpiochip_open(4)          # gpiochip4 on Pi 5, gpiochip0 on Pi 3/4
lgpio.gpio_claim_output(h, DC_PIN)
lgpio.gpio_claim_output(h, RST_PIN)
lgpio.gpio_claim_output(h, BL_PIN)

spi = spidev.SpiDev()
spi.open(0, 0)                      # bus 0, device 0 (CE0)
spi.max_speed_hz = 80_000_000
spi.mode = 0

def cmd(c, data=None):
    lgpio.gpio_write(h, DC_PIN, 0)   # DC low = command
    spi.xfer2([c])
    if data:
        lgpio.gpio_write(h, DC_PIN, 1)  # DC high = data
        spi.xfer2(list(data))

def reset():
    lgpio.gpio_write(h, RST_PIN, 1); time.sleep(0.01)
    lgpio.gpio_write(h, RST_PIN, 0); time.sleep(0.01)
    lgpio.gpio_write(h, RST_PIN, 1); time.sleep(0.15)

def init_display():
    reset()
    for command, data in GC9A01A_INIT:
        cmd(command, data)
    cmd(0x11); time.sleep(0.15)     # Sleep out
    cmd(0x29); time.sleep(0.15)     # Display on
    lgpio.gpio_write(h, BL_PIN, 1)  # Backlight on
```

---

## Writing a full frame (Pillow → RGB565 → SPI)

```python
from PIL import Image
import struct

def send_frame(img: Image.Image):
    """Send a 240x240 PIL Image to the display."""
    img = img.convert('RGB').resize((240, 240))
    # Set full address window
    cmd(0x2A, [0x00, 0x00, 0x00, 0xEF])  # CASET: 0..239
    cmd(0x2B, [0x00, 0x00, 0x00, 0xEF])  # RASET: 0..239
    cmd(0x2C)                             # RAMWR
    # Convert RGB888 → RGB565 big-endian
    pixels = img.tobytes('raw', 'RGB')
    buf = bytearray(240 * 240 * 2)
    for i in range(240 * 240):
        r, g, b = pixels[i*3], pixels[i*3+1], pixels[i*3+2]
        rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3)
        buf[i*2]   = (rgb565 >> 8) & 0xFF
        buf[i*2+1] = rgb565 & 0xFF
    lgpio.gpio_write(h, DC_PIN, 1)
    # spidev max single transfer ~65535 bytes; split into chunks
    chunk = 4096
    for offset in range(0, len(buf), chunk):
        spi.xfer2(buf[offset:offset+chunk])
```

Performance note: full-frame at 80MHz takes ~115ms on Pi 5 (Python overhead).
For face animations, pre-render frames as RGB565 bytes and cache them.
Fast partial updates: use CASET/RASET to limit write window to changed region.

---

## Module pinout (SYS-SPI GC9A01A, 11-pin)

**Confirmed silkscreen** (left to right): `lite, sd_cs, DC, RST, TFTCS, > MOSI, < MISO, > SCK, Gnd, 3.3v, Vin`

`>` = signal into module (host drives), `<` = signal out of module (host reads).
FPC connector is separate — capacitive touch, not part of the 11-pin header.

| Silkscreen | Signal | Pi GPIO | Pi physical pin | Notes |
|------------|--------|---------|-----------------|-------|
| `Vin` | Power in (LDO) | — | Pin 1 (3.3V) | 3.3V–5V range, onboard regulator |
| `3.3v` | 3.3V bypass | — | **leave open** | Direct bypass; don't use both |
| `Gnd` | Ground | — | Pin 6 | |
| `SCK` | SPI clock | GPIO 11 | Pin 23 | |
| `MOSI` | SPI data in | GPIO 10 | Pin 19 | |
| `MISO` | SPI data out | GPIO 9 | Pin 21 | SD card reads only |
| `TFTCS` | Display CS | GPIO 8 (CE0) | Pin 24 | |
| `RST` | Reset | GPIO 25 | Pin 22 | |
| `DC` | Data/Command | GPIO 24 | Pin 18 | |
| `sd_cs` | SD card CS | GPIO 7 (CE1) | Pin 26 | |
| `lite` | Backlight | GPIO 18 (PWM) | Pin 12 | HIGH=on; PWM for dimming |

SD card shares MOSI/MISO/SCK with display — different CS pins keep them separate.
FPC connector = capacitive touch (GT911 or similar), ignore for step 38b, useful future feature.

---

## config.txt changes required

```
dtparam=spi=on
```
Reboot after adding. Confirms with `/dev/spidev0.0` appearing.

---

## Key constraints from source

- Init sequence has many undocumented registers (`?` comments in Adafruit source) — use as-is, don't trim
- Line `0x70` is commented out in Adafruit source: "users reported issues, seems to work OK without" — keep it out
- INVON (0x21) is part of init — display requires inversion ON for correct colors
- MADCTL 0x48 = MX|BGR — the BGR flag means red and blue are swapped vs RGB order
- No MISO needed for display-only use (SD card needs MISO for reads)
