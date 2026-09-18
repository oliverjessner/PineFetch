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

- \* Captions are saved when **Save post captions** is enabled and post text is available. Reddit uses the title and optional self-text.

check [PineFetch-Link-Dump](https://github.com/oliverjessner/PineFetch-Link-Dump) to see which platforms are supported by import text and sending urls via browser
