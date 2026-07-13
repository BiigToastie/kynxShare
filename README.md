# kynxShare

**Combine multiple monitors into one streamable output** for Discord, OBS, and friends.

kynxShare runs in the background, lets you pick and arrange displays in a live preview, then exposes a single composite as a window you can share — at native summed resolution, or as a smooth mouse-follow viewport.

![Windows](https://img.shields.io/badge/platform-Windows%2010%2F11-0c0e12)
![License](https://img.shields.io/badge/license-MIT-3ecf8e)
![Built with](https://img.shields.io/badge/stack-Tauri%20%2B%20Rust%20%2B%20React-1a1f2a)

## Why

Discord and many apps only let you share **one** screen or window. With two, three, or four monitors you usually lose context. kynxShare stitches your chosen displays into **one** output so viewers see your full setup — side by side, stacked, or auto-tracked to your cursor.

## Features

- **Multi-monitor capture** via DXGI Desktop Duplication
- **Live layout editor** — drag tiles, snap to grid, toggle displays
- **Native canvas size** by default (sum of enabled monitors), with optional max width/height for Discord-friendly downscale
- **Static layout** or **mouse follow** (smooth viewport across monitors)
- **Share window** (`kynxShare Output`) for Discord / OBS window capture
- **Virtual camera bridge** (shared memory producer; filter companion documented)
- **Optional Virtual Display Driver** detection & guidance for a true “Screen” source
- **System tray** — hide to tray, toggle output / mode, quit
- First-run **onboarding** with Discord steps

## Quick start (users)

1. Download the latest installer from [Releases](../../releases).
2. Run **kynxShare**, finish the short welcome flow, click **Start**.
3. In Discord: **Share screen → Window → kynxShare Output**.

More detail: [docs/discord.md](docs/discord.md)

## Quick start (developers)

```bash
# prerequisites: Rust, Node 20+, VS C++ Build Tools, WebView2
cd apps/desktop
npm install
npm run tauri dev
```

Workspace layout and design notes: [docs/architecture.md](docs/architecture.md)

## Discord in 30 seconds

| Goal | How |
|---|---|
| Show all monitors | Mode **Static layout**, share **kynxShare Output** |
| Follow cursor | Mode **Mouse follow**, set Follow W/H (e.g. 1920×1080) |
| Appear as a real screen | Install [Virtual Display Driver](https://github.com/VirtualDrivers/Virtual-Display-Driver), maximize output on that display, share **Screen** |

## Project structure

```
crates/kynx-capture      DXGI multi-monitor capture
crates/kynx-compositor   Layout + compose + mouse-follow
crates/kynx-output       Share window, virtual cam, VDD
crates/kynx-core         Config + engine loop
apps/desktop             Tauri 2 + React UI
docs/                    Architecture & Discord / VCam guides
```

## Roadmap

- Signed companion virtual-camera filter
- Tighter VDD resolution sync
- System audio mix
- Presets & global hotkeys polish
- HDR path

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Issues and PRs welcome.

## License

MIT — see [LICENSE](LICENSE).
