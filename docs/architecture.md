# Architecture

kynxShare is a Windows desktop app that captures multiple monitors, composites them into one framebuffer, and exposes that result to Discord/OBS via a share window, an optional virtual-camera shared-memory bridge, and optional Virtual Display Driver guidance.

## Crates

| Crate | Role |
|---|---|
| `kynx-capture` | DXGI Desktop Duplication, one thread per monitor |
| `kynx-compositor` | Layout model, CPU blit compose, mouse-follow viewport |
| `kynx-output` | Share window (Win32), virtual-cam mapping, VDD detection |
| `kynx-core` | Config (`%AppData%/kynxShare`), engine loop |
| `apps/desktop` | Tauri 2 + React UI + tray |

## Frame path

```
Monitors → DXGI capture threads → latest frame map
       → compose_frame(layout / mouse-follow)
       → ShareWindow.present + VirtualCamera.push_frame
       → JPEG preview → UI
```

## Output channels

1. **Share window** titled `kynxShare Output` — pick as Discord application/window source.
2. **Virtual camera bridge** — named mapping `Local\KynxShareVirtualCam` with a fixed header + BGRA payload (see [virtual-camera.md](virtual-camera.md)).
3. **Virtual Display Driver** — optional companion; Discord can share the virtual screen (see [discord.md](discord.md)).

## Config

Persisted at `%AppData%\kynxShare\kynxShare\config.json` (via the `directories` crate).
