# URL download and caption compatibility

Here are our first class citizen listed:

TXT import, Magic import and Browser Import / Link Dump have their own narrower
platform rules. A manually accepted download link does not automatically qualify
for these import paths.

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
