# PineFetch

A local-first macOS desktop app that wraps **yt-dlp** with a clean UI: paste links, pick a preset, queue downloads, and optionally export audio. Transparent, minimal, and built for everyday workflows.

> PineFetch is designed for content you **own** or where you have **explicit permission** to download. Please respect platform Terms of Service and local laws.

![screenshot of the app](/src/images/download.png)

**Note:** Please only download content you have the rights or permission to access.

## Features

- Tauri app
- Minimal dark UI with split view
- Queue (FIFO), cancel, progress, ETA/speed
- `yt-dlp` spawned with argument array (no shell strings)
- Info fetch via `--dump-json` (title/uploader/duration/thumbnail)
- Persisted config (yt-dlp path + default output directory)

### Install

```bash
brew install yt-dlp ffmpeg
```

```bash
npm install
```

### Run (dev)

```bash
npm run dev
```

### Build

```bash
npm run publish
```

## yt-dlp location

- If `yt-dlp` is in your PATH, the app will find it automatically.
- Otherwise, set the full path in **Settings → yt-dlp Pfad**.
- PineFetch automatically tries to use `ffmpeg`/`ffprobe` from the same directory as `yt-dlp`.

## Legal/Use-Case Notes

- This app is for legitimate usage only: your own uploads, Creative Commons/Public Domain, or content with explicit permission to download.
- No DRM or paywall circumvention is supported or promoted.

## Features

- **Queue-based downloads** (multiple URLs, processed in order)
- **Presets** for common workflows (e.g. Best / Audio-only / Custom)
- **Optional logs** for transparency and troubleshooting
- **Playlist support** (where supported by yt-dlp)
- **Local-first**: no accounts, no cloud processing, files stay on your device

## Other Menus

The Settings screen lets you tune PineFetch for everyday use: default preset, download location, and whether logs are visible.
![screenshot of the app](/src/images/settings.png)

History shows completed and failed jobs at a glance, including status and timestamp.

![history](/src/images/history.png)

## Credits

yt-dlp: [https://github.com/yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp)
ffmpeg: [https://ffmpeg.org/](https://ffmpeg.org/)

## License

MIT
