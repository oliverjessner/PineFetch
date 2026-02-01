# PineFetch

Local Tauri desktop UI for `yt-dlp` with a minimal terminal-inspired interface. Designed for legitimate use-cases only (your uploads, Creative Commons/Public Domain, or content you have explicit permission to download).

**Note:** Please only download content you have the rights or permission to access.

## Features

- Tauri app
- Minimal dark UI with split view
- Queue (FIFO), cancel, progress, ETA/speed
- `yt-dlp` spawned with argument array (no shell strings)
- Info fetch via `--dump-json` (title/uploader/duration/thumbnail)
- Persisted config (yt-dlp path + default output directory)

## Setup

### Prerequisites

- Rust toolchain (stable)
- Node.js
- Tauri CLI
- `yt-dlp` installed and available in PATH, or set a custom path in the app

### Install

```bash
npm install
```

### Run (dev)

```bash
npm run dev
```

### Build

```bash
npm run build
```

## yt-dlp location

- If `yt-dlp` is in your PATH, the app will find it automatically.
- Otherwise, set the full path in **Settings → yt-dlp Pfad**.

## Legal/Use-Case Notes

- This app is for legitimate usage only: your own uploads, Creative Commons/Public Domain, or content with explicit permission to download.
- No DRM or paywall circumvention is supported or promoted.
