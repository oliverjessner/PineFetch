# URL download and caption compatibility

Paste a video link into the **URL** field on PineFetch's Download screen. PineFetch passes it to yt-dlp. The platforms below have extractors in yt-dlp 2026.08.19; individual videos may still be unavailable. See yt-dlp's [full supported-sites list](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md).

Here are our first class citizen listed:

| Source             | Example URL                     | captions\* |
| ------------------ | ------------------------------- | ---------- |
| YouTube            | `youtube.com/watch?...`,        | Yes        |
| TikTok             | `tiktok.com/@user/video/...`    | Yes        |
| Instagram          | `instagram.com/p/...`           | Yes        |
| Facebook           | `facebook.com/...`              | Yes        |
| X                  | `x.com/...`,                    | Yes        |
| Reddit             | `reddit.com/r/.../comments/...` | Yes        |
| Twitch             | `twitch.tv/...`                 | No         |
| Snapchat Spotlight | `snapchat.com/spotlight/...`    | No         |
| Pinterest          | `pinterest.com/pin/...`         | No         |
| LinkedIn           | `linkedin.com/posts/...`        | No         |

- \* Available captions and video descriptions are always stored in History. **Save caption TXT files** additionally exports `.caption.txt` files beside downloads. Reddit uses the title and optional self-text.

check [PineFetch-Link-Dump](https://github.com/oliverjessner/PineFetch-Link-Dump) to see which platforms are supported by import text and sending urls via browser

## ID import

**ID import** in Settings → Download is off by default, including after an upgrade. Enable it to use video IDs in the Download URL field, Magic import (when separately enabled), and TXT imports. Full HTTP(S) links continue to work with the option off.

| Input                        | Resolution when enabled                                                                                                                                                  |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `dQw4w9WgXcQ`                | An 11-character YouTube ID is recognized automatically.                                                                                                                  |
| `youtube:dQw4w9WgXcQ`        | Explicit YouTube video ID.                                                                                                                                               |
| `tiktok:7421234567890123456` | TikTok video URL with an unknown creator (`@_`), following [yt-dlp's ID-only URL construction](https://github.com/yt-dlp/yt-dlp/blob/master/yt_dlp/extractor/tiktok.py). |
| `instagram:ABC123xyz`        | Instagram post URL (`https://www.instagram.com/p/ABC123xyz/`).                                                                                                           |
| Other bare IDs               | Add a platform prefix; PineFetch does not guess between platforms.                                                                                                       |

YouTube IDs must contain exactly 11 ASCII letters, digits, underscores or hyphens. Explicit TikTok IDs accept 6–25 digits; Instagram shortcodes accept 3–128 ASCII letters, digits, underscores or hyphens. Prefixes are case-insensitive; IDs preserve their case. IDs and matching URLs are deduplicated together in TXT imports.

Resolution constructs a platform URL, then uses the existing metadata and download pipeline. It does not guarantee that a post exists or is available: TikTok and Instagram restrictions, unavailable content and authentication requirements still apply. Browser Import / Link Dump and the CLI `--link` option continue to accept full URLs.

## Creator subtitles

Enable **Save creator subtitles** in Settings → Download to save available creator-provided subtitle tracks beside video or audio downloads. All available languages are saved in the best original format, with their original timing. Automatic captions and live-chat exports are excluded. When no creator subtitles are available, the media download continues without them. This option is separate from post descriptions and local Whisper transcripts. Timestamp cuts leave the subtitle tracks unchanged, so their timing still refers to the full video.

Open the downloaded item in **History → Video Details → Subtitles** to select a language and view or copy its original timed text. Subtitle text is saved with the History entry and remains available if the sidecar file is later removed. Existing downloads are not retroactively fetched.
