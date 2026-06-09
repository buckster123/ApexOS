# ApexOS Banner Image Prompt

## Current banner (v2 — mesh colony)

Use with: Midjourney, DALL-E 3, Ideogram, Flux, or any high-quality image model.
Aspect ratio: **3:1 wide** (e.g. 1920×640 or 2400×800) for GitHub repo header.

---

### Prompt

```
Cinematic wide-format photograph of a cluster of five Raspberry Pi single-board computers 
arranged in a loose triangle formation on a dark anodized aluminum surface, shot from 
slightly above at a dramatic angle. The central Pi 5 board glows the brightest — its 
GPIO header and chip package emit a cool teal bioluminescent light. Four surrounding Pi 
boards glow progressively dimmer in amber and green, as if just waking up, being 
colonized by the central node.

Between the boards: thin luminous filaments of light — like fiber optic threads or 
mycelium strands — connect each board to the others in a mesh topology, the connections 
pulsing gently as data flows. The mesh forms a visible network graph in 3D space above 
the hardware, hovering as a ghostly holographic overlay.

Overlaid on the dark background in crisp green monospace terminal font, partially 
transparent, are fragments of real system output:

  [mesh] new peer discovered: apex-node2 @ 192.168.0.201
  [mesh] new peer discovered: apex-node3 @ 192.168.0.202
  plugin 'cerebro' up — 74 tools
  bootstrap_node: install.sh started (PID 4821)
  [agent] thinking...
  Council session #7: AZOTH / VAJRA / ELYSIAN / KETHER
  IAQ: 42  Temp: 21.4°C  Thermal: 31.2°C peak

In the upper-right corner, a subtle 32×24 pixel thermal grid (blue-to-red heat map) 
shows a room temperature scan, semi-transparent.

The overall aesthetic: a living organism propagating itself across silicon. Dark 
background (#0a0a0a). Accent colors: phosphor green (#39ff14), warm amber (#ffb300), 
cool teal (#00e5ff). Shallow depth of field — the central Pi is razor sharp, outer 
nodes slightly soft. Cinematic lens flare off the USB-C power connectors. 
Photorealistic but with subtle sci-fi overlay treatment. 16:9 or 3:1 crop.

Style: cyberpunk hardware photography, terminal aesthetic, bioluminescent circuit traces, 
hyperrealistic PCB detail, dramatic rim lighting from below, dark lab environment.
No humans. No text logos. Hardware only + holographic data overlays.
```

---

### Negative prompt (if model supports it)
```
white background, bright daylight, cartoon, illustration, blurry hardware, text logos, 
humans, hands, watermark, oversaturated, plastic toy aesthetic, single board only
```

---

### Notes for regeneration
- The **mesh filament connections** between boards is the key new element vs v1 (single Pi)
- The **terminal text fragments** should feel like real ApexOS output — not generic lorem ipsum
- **Five boards** = aspirational colony. Three is fine if composition is better.
- The **thermal grid** in the corner is a signature ApexOS element — keep it
- Board arrangement: one clear "origin" node (brightest) + satellites being colonized
- If the model supports it: animate the mesh filaments pulsing (for a video/gif version)

---

### v1 prompt (archived — single Pi)

```
Dramatic close-up of a Raspberry Pi 5 single-board computer on a dark surface. 
The board is backlit with cool blue and teal LED light. Overlaid on the dark background 
are green terminal text fragments showing sensor readings, agent output, and tool calls. 
A 32x24 thermal camera grid floats semi-transparently in the corner. Waveform 
visualization along the bottom edge. Cinematic, high-contrast, hardware photography 
aesthetic. Dark cyberpunk lab environment.
```
