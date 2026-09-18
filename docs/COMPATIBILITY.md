# Video and caption compatibility

PineFetch passes downloads to yt-dlp. This list describes the link types PineFetch accepts and the caption features implemented in the app. An accepted link does not guarantee a successful download: availability, access restrictions, and changes to a site or yt-dlp can affect the result.

| Source | Video links accepted by TXT and Browser Import | Video download | Post caption saved |
| --- | --- | --- | --- |
| YouTube | Video URLs (`watch`), Shorts, `youtu.be` links, and video links using `/live`, `/embed`, or `/v` | Attempted through yt-dlp | No |
| TikTok | `/@user/video/...` links and `vm.tiktok.com`, `vt.tiktok.com`, or `/t/...` share links | Attempted through yt-dlp | No |
| Instagram | Posts (`/p/...`), Reels (`/reel/...`), and `/tv/...` links | Attempted through yt-dlp when the post contains downloadable video | Yes, if **Save Instagram captions** is enabled and the post has a nonempty description |

For a single download, PineFetch accepts any `http://` or `https://` URL and passes it to yt-dlp. Downloads from other sites depend on yt-dlp; TXT and Browser Import accept only the YouTube, TikTok, and Instagram link types above. Playlist downloads are disabled.

**Post captions** are the text accompanying a post. For supported Instagram downloads, PineFetch writes them to a `.caption.txt` file beside the downloaded media and stores them in its local SQLite database. The setting is off by default. A post without a description produces no caption file.

The **Text** and **Text with timestamps** presets create speech transcripts locally with Whisper from downloaded audio. They are separate from post captions and do not download a site's subtitles.
