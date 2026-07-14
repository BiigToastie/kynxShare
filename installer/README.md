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

Artifacts land in `target/release/bundle/` at the **repo root** (Cargo workspace), e.g. `target/release/bundle/nsis/kynxShare_*_x64-setup.exe`.

## Optional companion

Für Discord **Bildschirm**-Tab mit Performance-Optionen brauchst du den
**Parsec Virtual Display Driver** (einmalig, Admin):

https://builds.parsec.app/vdd/parsec-vdd-0.41.0.0.exe

Danach in kynxShare „Virtueller Bildschirm“ an → Stream starten → Discord listet
den neuen Monitor. Details: `docs/discord.md`.

Alternativ (ohne kynxShare-Steuerung):  
`winget install --id=VirtualDrivers.Virtual-Display-Driver -e`
