# Building & installing kynxShare

## Prerequisites (Windows)

- Windows 10 1903+ / Windows 11
- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- Visual Studio Build Tools (C++ workload) + WebView2

## Dev

```bash
cd apps/desktop
npm install
npm run tauri dev
```

## One-click build & install (Windows)

Double-click **`build.bat`** in the project root. It runs `npm install`, builds the Tauri release installer, then launches it.

## Release installer (manual)

```bash
cd apps/desktop
npm install
npm run tauri build
```

Artifacts land in `apps/desktop/src-tauri/target/release/bundle/` (NSIS + MSI when configured).

## Optional companion

Virtual Display Driver (for Discord “Screen” source):

```bash
winget install --id=VirtualDrivers.Virtual-Display-Driver -e
```

See `docs/discord.md`.
