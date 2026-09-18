# URL download and caption compatibility

Paste a video link into the **URL** field on PineFetch's Download screen. PineFetch accepts `http://` and `https://` links and passes downloads to yt-dlp. Whether a video can be downloaded depends on the site, access to the video, and yt-dlp support.

| Source | Example URL forms for the Download screen | Video download | Post caption saved |
| --- | --- | --- | --- |
| YouTube | `youtube.com/watch?...`, `youtube.com/shorts/...`, `youtu.be/...` | Via yt-dlp | No |
| TikTok | `tiktok.com/@user/video/...`, `vm.tiktok.com/...`, `vt.tiktok.com/...` | Via yt-dlp | No |
| Instagram | `instagram.com/p/...`, `instagram.com/reel/...`, `instagram.com/tv/...` | Via yt-dlp if the post contains downloadable video | Yes, if **Save Instagram captions** is enabled and the post has a nonempty description |
| Facebook | `facebook.com/...`, `fb.watch/...` | Via yt-dlp | No |
| Twitch | `twitch.tv/...` | Via yt-dlp | No |
| X | `x.com/...`, `twitter.com/...` | Via yt-dlp | No |
| Other sites | Any `http://` or `https://` video URL | Via yt-dlp if that site is supported | No |

PineFetch disables playlist downloads, including when a pasted link points to a playlist or channel.

**Post captions** are the text accompanying a post. For supported Instagram downloads, PineFetch writes them to a `.caption.txt` file beside the downloaded media and stores them in its local SQLite database. The setting is off by default. A post without a description produces no caption file.

The **Text** and **Text with timestamps** presets create speech transcripts locally with Whisper from downloaded audio. They are separate from post captions and do not download a site's subtitles.
