# PineFetch 🍍

A local-first desktop app for macOS and Windows that wraps **yt-dlp** with a clean UI: paste links, pick a preset, queue downloads, export audio, or create transcripts. Transparent, minimal, and built for everyday workflows.

> PineFetch is designed for content you **own** or where you have **explicit permission** to download. Please respect platform Terms of Service and local laws.

![screenshot of the app](/src/images/download.png)

## Development and builds

Install the Node.js dependencies and start PineFetch in development mode:

```bash
npm install
npm run dev
```

Development requires Node.js, the Rust toolchain, `yt-dlp`, and the platform-specific Tauri system dependencies.

Create a macOS release build with:

```bash
npm run build
```

### Windows

Run this on Windows with the Rust MSVC toolchain, Node.js, ffmpeg/ffprobe, Deno, and Python 3.10+ installed:

```powershell
npm run build:windows
```

> Note: The Windows build is not currently tested by the maintainer.

## Version synchronization

`package.json` is the source of truth for the PineFetch version. Before each macOS or Windows build, `npm run sync:version` copies it to `src-tauri/tauri.conf.json` under `package.version`. The publish script performs the same sync before creating its release commit and Git tag.

## yt-dlp location

- If `yt-dlp` is in your PATH, the app will find it automatically.
- Otherwise, set the full path in **Settings → yt-dlp path**.
- Release builds bundle `ffmpeg`/`ffprobe` for post-processing.
- PineFetch also tries `ffmpeg`/`ffprobe` from the same directory as `yt-dlp`, Homebrew paths, and `PATH`.

## Transcription presets

PineFetch provides two `faster-whisper` text presets:

- **Text** writes a plain UTF-8 `.txt` transcript.
- **Text with timestamps** writes a separate `_timestamps.txt` transcript with start and end times for every detected segment:

```text
[00:00:03 → 00:00:08] Welcome to this video.
[00:00:09 → 00:00:14] Today we are looking at PineFetch.
```

The transcription quality can be selected in **Settings → Options → Transcription**:

- **Fast** uses the `base` model (default).
- **Balanced** uses the `small` model.
- **High** uses the `medium` model.
- **Best** uses the `large-v3` model.

Larger models can improve transcription accuracy, but require more download time, memory, and processing time. The selected model is stored when a download enters the queue, so queued jobs keep their original quality setting.

Enable **Settings → Options → Transcription → Download video with transcript** to keep the video file alongside the generated TXT transcript. PineFetch prepares a temporary 16 kHz mono audio file for faster transcription and removes it afterward. The option is stored per queued job.

Successful transcripts are also stored in SQLite table `transcriptions`. Each row contains its own primary key, the complete transcript text, a foreign key to `history_entries`, and a checked type of either `text` or `text with timestamps`. Deleting the related history entry also deletes its stored transcript.

The timestamped preset uses segment-level timestamps and keeps the regular text preset unchanged.

## Import YouTube and TikTok links from TXT

Use **Import TXT** on the Download screen to add multiple YouTube and TikTok videos to the queue at once:

1. Select the preset you want to use.
2. Click **Import TXT** and choose a `.txt` file.
3. PineFetch validates the file and queues every new supported link using the selected preset and output folder.

The file expects one YouTube or TikTok URL per line. Empty lines and lines beginning with `#` are ignored, so comments and groups can be added freely:

```text
# Tutorials
https://www.youtube.com/watch?v=abc123
https://youtu.be/def456

# Start at 1 minute 30 seconds
https://www.youtube.com/watch?v=ghi789&t=1m30s

# TikTok
https://www.tiktok.com/@creator/video/7412345678901234567
https://vm.tiktok.com/ZMexample/
```

Supported URLs include regular YouTube links as well as `youtu.be`, Shorts, Live, and Embed variants. TikTok video, photo, and short share links from `vm.tiktok.com`, `vt.tiktok.com`, or `/t/...` are also supported. Invalid lines are skipped. Duplicate URLs in the same file or already present in the current queue are not added again. The same YouTube video with a different start timestamp is treated as a separate queue item. When **Cut at timestamp** is enabled, timestamps in imported YouTube URLs are handled like manually queued links.

After the import, PineFetch reports how many links were added, invalid, duplicated, or failed to queue. Imported items follow the current queue auto-start setting.

## Legal/Use-Case Notes

- This app is for legitimate usage only: your own uploads, Creative Commons/Public Domain, or content with explicit permission to download.
- No DRM or paywall circumvention is supported or promoted.

## Local Link Dump API

PineFetch starts a local loopback server for browser extensions at:

```text
http://127.0.0.1:2255
```

Open **Link Dump** and create a connection secret under **Connections**. Copy it immediately; PineFetch stores only a hash and will not show the secret again.

Links sent through Link Dump are queued with the currently selected PineFetch preset.

Single link:

```bash
curl -X POST http://127.0.0.1:2255/addYoutubeLinkToQueue/ \
  -H "Content-Type: application/json" \
  -d '{"url":"https://www.youtube.com/watch?v=abc123","secret":"pfld_REPLACE_ME"}'
```

Multiple links:

```bash
curl -X POST http://127.0.0.1:2255/addYoutubeLinksToQueue/ \
  -H "Content-Type: application/json" \
  -d '{"urls":["https://www.youtube.com/watch?v=abc123","https://youtu.be/def456"],"secret":"pfld_REPLACE_ME"}'
```

Preflight:

```bash
curl -X OPTIONS http://127.0.0.1:2255/addYoutubeLinksToQueue/ -i
```

## Features

- **Queue-based downloads** (multiple URLs, processed in order)
- **Presets** for Best, Max 1080p, MP3, Opus, Text, and Text with timestamps
- **Terminal logs** for transparency and troubleshooting, limited to the latest 500 lines
- **Persistent history statistics** for downloaded videos, storage usage, and runtime
- **Local-first**: no accounts, no cloud processing, files stay on your device

## Queue and keyboard controls

Queue items start automatically by default. Disable **Auto-start** to collect multiple items and start them together with **Start queue**. Right-click a queue item to copy its link, download it again with another preset, cancel an active job, or remove a completed job. Clicking a completed item opens its output location.

When the URL field is focused:

- `Enter` adds the current URL to the queue.
- `Cmd + I` loads information for the current URL.
- `Esc` clears the URL and its loaded information.

## Other menus

The Settings screen contains the output folder, `yt-dlp` path, installed and latest `yt-dlp` versions, and the terminal log. Versions are checked once per app run. The adjacent Options panel contains **Magic import**, **Cut at timestamp**, and a separate **Transcription** group for quality and video-retention settings.

**Magic import** reads a supported URL from the clipboard when the PineFetch logo is clicked while the URL field is empty. **Cut at timestamp** starts supported downloads at timestamps embedded in their URLs.

![screenshot of the app](/src/images/settings.png)

History keeps successful downloads in the local SQLite database. Each entry includes the source URL, source service, uploader, medium (`video`, `audio`, or `transcript`), title, filename, thumbnail, platform, output path, upload date, completion time, duration in seconds, and final file size in bytes when available. The source service is derived locally from the URL hostname by removing the protocol, subdomain, and TLD; known short domains such as `youtu.be` are normalized to their canonical service name. Sources are also backfilled from the URLs of existing History entries during migration.

The History view also provides an overview of:

- **Downloaded videos** — the number of entries currently stored in History
- **Total data** — the combined size of downloaded files
- **Total runtime** — the combined duration of downloaded media

Removing an entry or clearing History immediately updates these totals. Existing entries created before duration and file-size tracking was introduced remain available, but missing metadata is not included in the totals.

History loads 20 entries per page. Use **Load more** to append the next page; previously loaded entries remain cached while switching between views.

![history](/src/images/history.png)

## Credits

- yt-dlp: [https://github.com/yt-dlp/yt-dlp](https://github.com/yt-dlp/yt-dlp)
- FFmpeg: [https://ffmpeg.org/](https://ffmpeg.org/)

## License

MIT
