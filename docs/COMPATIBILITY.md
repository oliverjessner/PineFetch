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

## Creator subtitles

Enable **Save creator subtitles** in Settings → Download to save available creator-provided subtitle tracks beside video or audio downloads. All available languages are saved in the best original format, with their original timing. Automatic captions and live-chat exports are excluded. When no creator subtitles are available, the media download continues without them. This option is separate from post descriptions and local Whisper transcripts. Timestamp cuts leave the subtitle tracks unchanged, so their timing still refers to the full video.

Open the downloaded item in **History → Video Details → Subtitles** to select a language and view or copy its original timed text. Subtitle text is saved with the History entry and remains available if the sidecar file is later removed. Existing downloads are not retroactively fetched.
