# ApexOS Audio Editor — Step 37 Architecture

Desktop audio editor + agent toolkit for post-processing audio files.
Primarily useful for Sonus/Suno tracks but works on any audio file.
Separate from Sonus — no Suno API key required.

Load this when working on audio tools in apexos-tools, audio gateway routes,
or the desktop Audio Editor window.

---

## Vision

Suno's model, when pushed hard with prompt engineering, produces extraordinary
results — but 3/10 tracks have fixable artifacts: silence at the end, a peak
spike, a loudness imbalance. These are mechanical problems with mechanical
solutions. The agent should fix them automatically before the human ever hears
the track. The human sees the result in the desktop editor, can fine-tune, and
exports.

**The split:**

| Problem | Detection | Fix | Who |
|---------|-----------|-----|-----|
| Silence at end/start | waveform amplitude | ffmpeg trim | Agent (auto) |
| Peak clipping / hot level | ffprobe peak scan | ffmpeg loudnorm | Agent (auto) |
| Loudness too quiet | ffprobe LUFS | ffmpeg loudnorm | Agent (auto) |
| DC offset | ffprobe stats | ffmpeg highpass | Agent (auto) |
| Weird artifact / hallucination | requires hearing | — | Human (editor) |
| Creative EQ / compression | requires hearing | — | Human (editor) |
| Trim in/out points | waveform + ears | ffmpeg cut | Human (editor) |

---

## Architecture

```
apexos-tools MCP server (already on Pi)
  new tools: audio_analyze, audio_clean, audio_trim_silence,
             audio_normalize, audio_peak_limit

gateway (new routes)
  POST /api/audio/analyze     { path } → stats JSON
  POST /api/audio/waveform    { path } → amplitude array (for canvas)
  POST /api/audio/process     { path, ops[] } → processed file path
  GET  /api/audio/files       → list audio files in workspace dirs

desktop UI
  🎛️ Audio Editor WinBox
  → waveform canvas, trim handles, playback (reuses /api/sonus/stream)
  → auto-fix panel, manual gain/trim controls, export
```

---

## New apexos-tools MCP Tools

All implemented in `tools/crates/apexos-tools/src/main.rs` alongside existing
tools. All shell out to `ffmpeg`/`ffprobe` (already installed).

### `audio_analyze`

```json
{ "name": "audio_analyze", "input": { "path": "/var/lib/agentd/workspace/sonus/track.mp3" } }
```

Returns:
```json
{
  "duration_s": 187.4,
  "sample_rate": 44100,
  "channels": 2,
  "format": "mp3",
  "bit_rate": 320000,
  "peak_db": -1.2,
  "rms_db": -14.8,
  "lufs_integrated": -13.4,
  "silence_start_s": 0.0,
  "silence_end_s": 3.2,
  "has_clipping": false,
  "dc_offset": 0.001
}
```

Implementation:
```bash
# loudness + stats:
ffprobe -v quiet -print_format json -show_streams -show_format <path>
ffmpeg -i <path> -af "loudnorm=print_format=json" -f null - 2>&1
# silence detection:
ffmpeg -i <path> -af "silencedetect=noise=-50dB:d=0.5" -f null - 2>&1
```

Parse combined output into the JSON struct above.

---

### `audio_clean`

One-shot composite fix. Analyzes then applies all applicable automatic fixes.

```json
{
  "name": "audio_clean",
  "input": {
    "path": "/var/lib/agentd/workspace/sonus/track.mp3",
    "target_lufs": -14,
    "silence_threshold_db": -50,
    "output_path": "/var/lib/agentd/workspace/sonus/track_clean.mp3"
  }
}
```

Sequence:
1. Run `audio_analyze` on input
2. If `silence_end_s > 0.3`: trim trailing silence
3. If `lufs_integrated` outside `[target_lufs-2, target_lufs+1]`: normalize
4. If `peak_db > -1.0`: apply true-peak limiter
5. If `dc_offset > 0.01`: apply highpass filter at 5Hz

Returns: `{ "output_path", "ops_applied": ["trim_silence", "normalize", "peak_limit"], "stats_before": {...}, "stats_after": {...} }`

The agent calls this as a post-processing step after `download_track`. Can be
wired into sonus download flow: `download_track` → `audio_clean` → done.

---

### `audio_trim_silence`

```json
{
  "name": "audio_trim_silence",
  "input": {
    "path": "...",
    "start": true,
    "end": true,
    "threshold_db": -50,
    "min_silence_ms": 500,
    "output_path": "..."
  }
}
```

```bash
ffmpeg -i <in> -af "silenceremove=start_periods=1:start_threshold=-50dB:stop_periods=-1:stop_threshold=-50dB:stop_duration=0.5" <out>
```

---

### `audio_normalize`

```json
{
  "name": "audio_normalize",
  "input": {
    "path": "...",
    "target_lufs": -14,
    "true_peak": -2.0,
    "output_path": "..."
  }
}
```

```bash
# Two-pass loudnorm for accurate integrated LUFS:
ffmpeg -i <in> -af "loudnorm=I=-14:TP=-2:LRA=11:print_format=json" -f null - 2>&1
# parse measured values, then:
ffmpeg -i <in> -af "loudnorm=I=-14:TP=-2:LRA=11:measured_I=<I>:measured_TP=<TP>:measured_LRA=<LRA>:measured_thresh=<thresh>:offset=<offset>:linear=true" <out>
```

---

### `audio_trim`

Manual trim (agent can call with explicit timestamps):

```json
{
  "name": "audio_trim",
  "input": {
    "path": "...",
    "start_s": 0.0,
    "end_s": 183.0,
    "output_path": "..."
  }
}
```

```bash
ffmpeg -i <in> -ss <start> -to <end> -c copy <out>
```

---

## Gateway Routes

All in `agentd/crates/gateway/src/lib.rs`.

```
GET  /api/audio/files               → list all .mp3/.wav/.flac in workspace dirs
POST /api/audio/analyze             → body: { path } → run audio_analyze, return JSON
POST /api/audio/waveform            → body: { path, samples: 1200 } → amplitude array
POST /api/audio/process             → body: { path, ops: [{type, params}] } → output path
```

`/api/audio/waveform` extracts N amplitude samples for canvas rendering:

```bash
# Downmix to mono, resample to N points, extract PCM, output as float array
ffmpeg -i <in> -ac 1 -ar <N> -f f32le - 2>/dev/null | head -c <N*4>
```

Returns: `{ "samples": [0.02, 0.14, 0.31, ...], "duration_s": 187.4 }`

Frontend renders this as a waveform on `<canvas>`.

`/api/audio/process` accepts an ops array so the UI can batch multiple operations
into one ffmpeg call:

```json
{
  "path": "...",
  "ops": [
    { "type": "trim", "start_s": 0, "end_s": 183 },
    { "type": "normalize", "target_lufs": -14, "true_peak": -2 },
    { "type": "fade_out", "duration_s": 2 }
  ],
  "output_path": "..."
}
```

Ops are chained as ffmpeg `-af` filters where possible, or multi-pass where
not. The gateway builds the filter chain from the op list.

Playback: **reuse existing `/api/sonus/stream`** — it already handles HTTP 206
range requests and works with any path. The editor's `<audio>` element points at it.

---

## Desktop UI — Audio Editor Window

`launchApp('audio')` → WinBox. Title: `🎛️ Audio Editor`.

```
┌──────────────────────────────────────────────────────────────────────┐
│  🎛️ AUDIO EDITOR                                          [_][□][X]  │
├──────────────────────────────────────────────────────────────────────┤
│  File: [track_clean.mp3 ▾]   [📂 Browse]   [💾 Export]              │
├──────────────────────────────────────────────────────────────────────┤
│                                                                        │
│  ┌── Waveform ─────────────────────────────────────────────────────┐  │
│  │▓▓▓░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░  │  │
│  │  ↑ trim in                                       trim out ↑      │  │
│  └─────────────────────────────────────────────────────────────────┘  │
│  [◀◀] [▶ Play] [⏹] [⏺ Loop]     00:42 / 03:07     Zoom: [──●──]     │
│                                                                        │
├─────────────────────────┬────────────────────────────────────────────┤
│  AUTO-FIX               │  MANUAL CONTROLS                            │
│                         │                                              │
│  Analysis:              │  Trim In:  [  0.00s  ]  [Set to playhead]  │
│  Peak:    -1.2 dB  ⚠    │  Trim Out: [183.00s  ]  [Set to playhead]  │
│  LUFS:    -18.4    ⚠    │                                              │
│  Silence: 3.2s tail ⚠   │  Gain:    [──●────── ] -3.0 dB             │
│                         │  Fade in:  [  0.5s   ]                      │
│  [🪄 Auto-fix All]      │  Fade out: [  2.0s   ]                      │
│  [✂ Trim Silence]       │                                              │
│  [📊 Normalize -14]     │  [Preview]  [Apply]                         │
│  [🔊 Peak Limit]        │                                              │
└─────────────────────────┴────────────────────────────────────────────┘
```

**Auto-fix All** calls `POST /api/audio/analyze` → shows results → calls
`POST /api/audio/process` with applicable ops → reloads waveform. One click,
done.

**Waveform rendering**: Web Audio API + `<canvas>`. Fetch `/api/audio/waveform`
on file load → draw amplitude bars. Trim handles are draggable divs overlaid on
canvas. Playhead is a vertical line updated via `audio.currentTime`.

**File picker**: `GET /api/audio/files` returns files from
`/var/lib/agentd/workspace/sonus/` and `/var/lib/agentd/workspace/` — dropdown.
File is directly available to ffmpeg on the Pi since it lives in the workspace.

**Export**: `POST /api/audio/process` with all current settings → returns output
path → downloads via `/api/sonus/stream?path=<output>` (or browser `<a>` with
the stream URL).

**Open from Player**: The Sonus player window gets an "Edit" button per track
that calls `launchApp('audio')` with the track path pre-loaded.

---

## Separation from Sonus

| Sonus | Audio Editor |
|-------|-------------|
| Suno API key required | Not required |
| Generate / extend / download tracks | Edit any audio file |
| `hermes-sonus` MCP server | `apexos-tools` MCP server (existing) |
| `GET /api/sonus/files` + stream | `GET /api/audio/files` + reuses `/api/sonus/stream` |
| 🎵 Player window | 🎛️ Audio Editor window |

The only coupling: the Sonus Player gets an "Edit" button that opens the
Audio Editor with the track pre-loaded. Everything else is independent.

---

## Dependencies

**None new.** All tooling already present on Pi:

| Tool | Already installed? | Used for |
|------|------------------|----------|
| `ffmpeg` | ✓ (voice I/O, step 25) | Audio processing, filter chains |
| `ffprobe` | ✓ (bundled with ffmpeg) | Analysis, loudness measurement |
| Web Audio API | ✓ (browser API) | Waveform decode + canvas render |

No new system packages. No new Cargo deps (we shell out like apexos-tools always has).

---

## Build Phases

| Phase | What | Status |
|-------|------|--------|
| 37a | New apexos-tools tools: `audio_analyze`, `audio_trim_silence`, `audio_normalize`, `audio_peak_limit`, `audio_trim`, `audio_clean` (composite) | ☐ |
| 37b | Gateway routes: `/api/audio/files`, `/api/audio/analyze`, `/api/audio/waveform`, `/api/audio/process` (op-chain builder) | ☐ |
| 37c | Desktop Audio Editor window: file picker, waveform canvas, playback, trim handles, auto-fix panel, manual controls, export | ☐ |
| 37d | Sonus Player → Editor integration: "Edit" button per track; `audio_clean` called automatically after `download_track` if `AUDIO_AUTO_CLEAN=true` | ☐ |

**37a can be implemented before 37b/c** — the tools work independently and are
immediately callable by agents. Agent can call `audio_clean` on any Sonus track
right after download without any UI.

---

## Key Constraints

| Constraint | Detail |
|-----------|--------|
| ffmpeg loudnorm is two-pass | First pass measures, second applies. Both needed for accurate LUFS. Cache first-pass result in a temp file. |
| Large audio files | Waveform extraction for a 3-min MP3 takes ~1s on Pi 5. Return 1200 samples max. |
| In-place editing | Never overwrite originals. Always write to `<name>_clean.mp3` or user-specified output path. |
| `/api/sonus/stream` reuse | Playback in the editor uses the existing range-request endpoint — no new streaming code. |
| Agent can't hear | `audio_clean` only applies fixes detectable from waveform data. Never applies creative EQ or compression. |
| AUDIO_AUTO_CLEAN | Env var gate on automatic post-processing. Off by default. |
