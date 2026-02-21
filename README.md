# PineFetch

Local Tauri desktop UI for `yt-dlp` with a minimal terminal-inspired interface. Designed for legitimate use-cases only (your uploads, Creative Commons/Public Domain, or content you have explicit permission to download).

**Note:** Please only download content you have the rights or permission to access.

![screenshot of the app](/src/images/pinefetch.webp)

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
- `ffmpeg` + `ffprobe` (required for merging streams, audio extraction, and text preset)
- `deno` (recommended for reliable YouTube extraction in recent `yt-dlp` versions)
- Optional for local dev of text preset: Python 3 + `faster-whisper` (`pip install faster-whisper`)

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

`npm run build` now prepares and bundles a local `fast-whisper` runtime under
`src-tauri/resources/whisper-runtime` and a local `ffmpeg` runtime under
`src-tauri/resources/ffmpeg-runtime`, plus a local `deno` runtime under
`src-tauri/resources/deno-runtime`, before creating the app bundle.

## yt-dlp location

- If `yt-dlp` is in your PATH, the app will find it automatically.
- Otherwise, set the full path in **Settings → yt-dlp Pfad**.
- PineFetch automatically tries to use `ffmpeg`/`ffprobe` from the same directory as `yt-dlp`.

## Legal/Use-Case Notes

- This app is for legitimate usage only: your own uploads, Creative Commons/Public Domain, or content with explicit permission to download.
- No DRM or paywall circumvention is supported or promoted.
