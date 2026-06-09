# ApexOS Step 38 — GPIO Tools + Display Face

Physical world actuator layer. GPIO read/write/PWM as MCP tools, plus an
optional display "face" driven by bus events. Foundation for the ClaudeBot arc.

Load this when working on GPIO tools in apexos-tools, the display daemon,
or the face animation event bridge.

---

## Hardware reality check (Pi 5 specific)

### Confirmed live state (2026-06-09)
```
Model:     Raspberry Pi 5 Model B Rev 1.0
gpiochip4: RP1 — main 40-pin header GPIO (BCM GPIO 0–27)
gpiochip0: RP1 internal/other (NOT the 40-pin header)
I2C:       enabled — dtparam=i2c_arm=on at 400kHz (fast mode)
SPI:       DISABLED — dtparam=spi=on is commented out in /boot/firmware/config.txt
sysfs base: GPIO 512 (so BCM GPIO N = sysfs 512+N on Pi 5)
```

### Pi 5 vs Pi 3/4 — the RP1 trap

| | Pi 3 / Pi 4 | Pi 5 |
|--|-------------|------|
| GPIO chip | BCM2835/BCM2711 | RP1 southbridge |
| 40-pin header chip | `/dev/gpiochip0` | **`/dev/gpiochip4`** |
| sysfs GPIO base | 0 (GPIO N = sysfs N) | 512 (GPIO N = sysfs 512+N) |
| `rppal` crate | Works | Works ≥0.19 (verify at build) |
| `libgpiod` / `gpiod` crate | Works | Works — preferred |
| RPi.GPIO (Python) | Works | **Broken** — RP1 not supported |
| `raspi-gpio` CLI | Works | **Broken** on Pi 5 |
| Hardware PWM | GPIO 12/13/18/19 | Same pins, different kernel path |

**Detection pattern:**
```rust
fn gpio_chip() -> &'static str {
    let model = std::fs::read_to_string("/proc/device-tree/model").unwrap_or_default();
    if model.contains("Raspberry Pi 5") { "/dev/gpiochip4" } else { "/dev/gpiochip0" }
}
```

---

## Pin audit — with sensor head installed

The sensor head HAT (BME688 + MLX90640) uses I2C and passes through all 40 pins.

### Reserved pins
| GPIO | Physical | Function | Reserved by |
|------|----------|----------|-------------|
| 2 | 3 | SDA1 (I2C) | sensor-head (BME688 + MLX90640) |
| 3 | 5 | SCL1 (I2C) | sensor-head |
| 4 | 7 | optional 1-wire | sensor-head (check your HAT) |
| 27 | 13 | HAT ID EEPROM SD | HAT standard |
| 28 | 27 | HAT ID EEPROM SC | HAT standard |

### Free and usable
SPI0 pins are completely free (sensor head is I2C only):

| GPIO | Physical | Use |
|------|----------|-----|
| 8 | 24 | SPI0 CE0 — display chip select |
| 9 | 21 | SPI0 MISO — (unused for display) |
| 10 | 19 | SPI0 MOSI — display data |
| 11 | 23 | SPI0 SCLK — display clock |
| 18 | 12 | PWM0 — display backlight brightness |
| 24 | 18 | free — display DC (data/command) |
| 25 | 22 | free — display RST (reset) |
| 12–17, 19–26 | various | free for general GPIO |

**Conclusion: SPI TFT display + full GPIO tooling is possible with sensor head installed.**
The only requirement is enabling SPI in config.txt.

---

## Step 38a — GPIO core tools

### New apexos-tools MCP tools (6 tools)

All in `tools/crates/apexos-tools/src/tools.rs`, same shell-out pattern as audio tools.

#### `gpio_info`
```json
{ "name": "gpio_info", "input": {} }
```
Returns detected Pi model, chip path, and reserved pin list.
```json
{
  "model": "Raspberry Pi 5 Model B Rev 1.0",
  "chip": "/dev/gpiochip4",
  "reserved_gpio": [2, 3, 4, 27, 28],
  "reserved_reason": "I2C (sensor-head), HAT EEPROM"
}
```

#### `gpio_read`
```json
{ "name": "gpio_read", "input": { "gpio": 17 } }
```
```bash
gpioget /dev/gpiochip4 17     # Pi 5
gpioget /dev/gpiochip0 17     # Pi 3/4
```
Returns: `{ "gpio": 17, "value": 1 }`

Refuses if gpio is in reserved list. Returns error if pin not accessible.

#### `gpio_write`
```json
{ "name": "gpio_write", "input": { "gpio": 17, "value": 1 } }
```
```bash
gpioset --mode=time --usec=100000 /dev/gpiochip4 17=1   # Pi 5
```
Note: `gpioset` in libgpiod 2.x releases the line after exit unless `--hold` or mode is set.
Use `--hold-period` or the persistent-set mode for sustained output.
Returns: `{ "gpio": 17, "value": 1, "ok": true }`

**Safety note baked into tool description:**
> Pi GPIO is 3.3V logic, max ~16mA per pin. Never connect 5V signals directly.
> Never exceed 50mA total from all GPIO. Do not drive inductive loads without a flyback diode.

#### `gpio_pwm`
```json
{ "name": "gpio_pwm", "input": { "gpio": 18, "duty_pct": 75, "freq_hz": 1000 } }
```
Hardware PWM on Pi 5: GPIO 12, 13, 18, 19 — mapped to `/sys/class/pwm/pwmchip*/`.

```bash
# Enable PWM channel (one-time setup per boot, or via dtoverlay)
# Write duty_cycle + period to sysfs
echo 1000000 > /sys/class/pwm/pwmchip2/pwm0/period       # 1ms = 1kHz
echo 750000  > /sys/class/pwm/pwmchip2/pwm0/duty_cycle    # 75%
echo 1       > /sys/class/pwm/pwmchip2/pwm0/enable
```

PWM chip mapping on Pi 5 (RP1):
- GPIO 12 → pwmchip2/pwm0
- GPIO 13 → pwmchip2/pwm1
- GPIO 18 → pwmchip2/pwm2 (or pwmchip0 depending on kernel — detect at runtime)
- GPIO 19 → pwmchip2/pwm3

Requires `dtoverlay=pwm-2chan` in config.txt, or handle via sysfs export directly.
Tool should detect the correct pwmchip by reading `/sys/class/pwm/` at startup.

#### `gpio_pulse`
```json
{ "name": "gpio_pulse", "input": { "gpio": 17, "duration_ms": 500 } }
```
High for N ms, then low. Useful for buzzers, relay triggers, LED blinks.
Simple: `gpioset --mode=time --usec=500000 /dev/gpiochip4 17=1`

#### `gpio_servo`
```json
{ "name": "gpio_servo", "input": { "gpio": 18, "angle_deg": 90 } }
```
Standard servo: 50Hz PWM, 1ms–2ms pulse width = 0°–180°.
- angle 0°   → duty 5%  (1ms at 50Hz)
- angle 90°  → duty 7.5% (1.5ms at 50Hz)
- angle 180° → duty 10% (2ms at 50Hz)

Maps angle to sysfs PWM duty_cycle. Only works on PWM-capable pins (12, 13, 18, 19).

**Note:** Servos often need 5V supply (not from Pi GPIO — use a separate 5V rail).
Signal line is 3.3V-compatible on most servos.

---

## Step 38b — SPI enable + display driver

### Enable SPI (one-time, requires reboot)
In `/boot/firmware/config.txt`:
```
dtparam=spi=on
```
After reboot: `/dev/spidev0.0` and `/dev/spidev0.1` appear.
Add to `install.sh` alongside I2C enable.

### Display options

| Display | Driver | Interface | Res | Color | Status w/ sensorhead |
|---------|--------|-----------|-----|-------|----------------------|
| Round TFT 1.28" (Waveshare/GC9A01) | GC9A01 | SPI | 240×240 | 16-bit | ✓ free pins |
| Round OLED 1.5" (SSD1327) | SSD1327 | SPI or I2C | 128×128 | 16-level grey | ✓ (I2C addr 0x3c) |
| Square OLED 1.3" (SH1106) | SH1106 | I2C | 128×64 | mono | ✓ (I2C addr 0x3c) |
| Round OLED 1.28" (ER-OLEDM1.28) | SSD1351 | SPI | 128×128 | 16-bit | ✓ free pins |

**Best face candidate: GC9A01 round TFT 240×240** — color, circular mask, high-res enough
for an animated face. The 1.28" Waveshare module is widely available and well documented
for Pi. Needs SPI enabled.

### GC9A01 wiring (with sensor head installed)
| Display pin | Pi GPIO | Physical pin |
|-------------|---------|--------------|
| VCC | — | Pin 1 (3.3V) |
| GND | — | Pin 6 (GND) |
| DIN (MOSI) | GPIO 10 | Pin 19 |
| CLK (SCLK) | GPIO 11 | Pin 23 |
| CS | GPIO 8 | Pin 24 |
| DC | GPIO 24 | Pin 18 |
| RST | GPIO 25 | Pin 22 |
| BL | GPIO 18 | Pin 12 (PWM) |

Zero conflict with sensor head. Backlight brightness dimmable via gpio_pwm on GPIO 18.

### Display daemon

A small Python daemon (`apex-face.py`) using Pillow + spidev:
- Runs as a systemd service (`apex-face.service`)
- Listens on a Unix socket `/tmp/apex-face.sock`
- Accepts JSON commands: `{ "cmd": "show_face", "state": "thinking" }`
- Face states: `idle`, `thinking`, `speaking`, `alert`, `happy`, `sleeping`

Alternative Rust approach: `embedded-graphics` crate + `display-interface-spi`.
Python is easier for iterating on face art; switch to Rust once designs are stable.

#### New apexos-tools MCP tool: `display_face`
```json
{
  "name": "display_face",
  "input": { "state": "thinking", "text": "hmm..." }
}
```
Sends command to apex-face.sock. Returns `{ "ok": true }`.

APEX calls this in soul.md naturally: when starting a complex task, `display_face("thinking")`.

---

## Step 38c — Event-driven face animation

Wire bus events → display state without APEX needing to call `display_face` explicitly.
A lightweight task in `main.rs` subscribes to the bus:

```rust
// In main.rs spawn task
AgentTurnStarted   → display_face("thinking")
AssistantMessage   → display_face("speaking")  // while streaming
AgentTurnComplete  → display_face("idle")
SensorReading(IAQ > 150) → display_face("alert")
WakeTriggered      → display_face("listening")
// idle timeout (30s no activity) → display_face("sleeping")
```

This makes the face animate automatically without APEX needing to manage it consciously.
APEX can still override via `display_face` tool for intentional expression.

---

## Step 38d — Desktop GPIO panel UI

New WinBox: `🔌 GPIO Panel`

```
┌─────────────────────────────────────────┐
│  🔌 GPIO PANEL                  [_][□][X]│
├─────────────────────────────────────────┤
│  Pi 5 — chip: gpiochip4                 │
│                                         │
│  PIN MAP  [show all / show free only]   │
│  ┌────────────────────────────────────┐ │
│  │ GPIO 17 [IN ▾] ●HIGH  [Read] [▼]   │ │
│  │ GPIO 18 [PWM] ████░░ 75%  [Set]    │ │
│  │ GPIO 24 [OUT▾] ●LOW   [High][Low]  │ │
│  │ GPIO 2  [---]  reserved: I2C       │ │
│  └────────────────────────────────────┘ │
│                                         │
│  SERVO CONTROL                          │
│  GPIO 18  [────●────]  90°  [Set]       │
│                                         │
│  FACE  idle ▾  [Set]  [Auto: ON]        │
└─────────────────────────────────────────┘
```

Frontend-only. Calls `/api/gpio/*` gateway routes that proxy to apexos-tools.

---

## Build phases

| Phase | What | Deps |
|-------|------|------|
| 38a | GPIO tools: gpio_info/read/write/pulse/pwm/servo; libgpiod install; Pi 5 auto-detect | `libgpiod2` + `gpiod` pkg on Pi |
| 38b | SPI enable; display daemon `apex-face.py`; `display_face` MCP tool; `apex-face.service` | SPI hardware + display wired |
| 38c | Event-to-face bridge in main.rs; face state machine; idle timeout | 38b complete |
| 38d | Desktop GPIO panel UI (pin map, servo slider, face override) | 38a + optional 38b |

38a is hardware-independent — works without any display. Implement and test first.
38b/38c require André to confirm display hardware and wire it up.

---

## Dependencies to add

**Pi packages** (add to `install.sh`):
```bash
apt-get install -y gpiod libgpiod2 libgpiod-dev   # libgpiod + CLI tools
# dtparam=spi=on in /boot/firmware/config.txt     # if using SPI display
pip3 install pillow spidev RPi-lgpio               # display daemon (Python)
```

**Note:** `RPi-lgpio` is the Pi 5-compatible RPi.GPIO drop-in (uses lgpio underneath).
Do NOT install `RPi.GPIO` — it breaks on Pi 5/RP1.

---

## Safety rules (baked into tool descriptions)

1. **3.3V logic only** — Pi GPIO is not 5V tolerant. Connecting 5V signals will damage the Pi.
2. **16mA per pin, 50mA total** — Never exceed. Drive LEDs through resistors (330Ω for 3.3V).
3. **Reserved pins are refused** — gpio_read/write/pwm refuse GPIO 2/3/27/28 by default.
4. **Servo power** — Servos need 5V supply from an external rail, not from Pi GPIO pins.
5. **Inductive loads** — Motors/relays need a flyback diode (1N4007 or similar) across the coil.
6. **Never call gpio_write on an input pin** — configure direction first or use gpio_pulse.

---

## Key constraints

| Constraint | Detail |
|-----------|--------|
| SPI disabled by default | Add `dtparam=spi=on` to config.txt + reboot before SPI display works |
| gpiochip4 for Pi 5 | Not gpiochip0 — auto-detect from /proc/device-tree/model |
| libgpiod v2 API | `gpioget`/`gpioset` syntax changed between v1 and v2 — detect version in gpio_info |
| PWM needs dtoverlay | `dtoverlay=pwm-2chan` for GPIO 12+13 or `dtoverlay=pwm` for GPIO 18 only |
| apex-face.sock | display_face tool fails gracefully (ok: false, reason: "display daemon not running") |
| Sensor head I2C | Always reserved — do not allow gpio_write to GPIO 2/3 even if user asks |
