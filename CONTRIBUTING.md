# Contributing to kynxShare

Thanks for helping make multi-monitor sharing less painful.

## Dev setup

1. Install Rust (stable), Node 20+, Visual Studio C++ Build Tools, WebView2.
2. `cd apps/desktop && npm install`
3. `npm run tauri dev`

## Guidelines

- Keep Windows capture / Win32 code in the Rust crates; keep the UI thin.
- Prefer small, focused PRs with a short “why”.
- Run `cargo test` and `cargo check` before opening a PR.
- Do not commit secrets, personal config, or huge binaries.

## Code of conduct

Be respectful. Harassment or gatekeeping gets PRs closed.
