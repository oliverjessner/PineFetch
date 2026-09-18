# URL download and caption compatibility

Paste a video link into the **URL** field on PineFetch's Download screen. PineFetch passes it to yt-dlp. The platforms below have extractors in yt-dlp 2026.08.19; individual videos may still be unavailable. See yt-dlp's [full supported-sites list](https://github.com/yt-dlp/yt-dlp/blob/master/supportedsites.md).

| Source             | Example URL                                                       | Video download | Post caption saved                                                          |
| ------------------ | ----------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------- |
| YouTube            | `youtube.com/watch?...`, `youtube.com/shorts/...`, `youtu.be/...` | Via yt-dlp     | Yes, when **Save post captions** is enabled and the video has a description |
| TikTok             | `tiktok.com/@user/video/...`, `vm.tiktok.com/...`                 | Via yt-dlp     | Yes, when **Save post captions** is enabled and the video has a description |
| Instagram          | `instagram.com/p/...`, `instagram.com/reel/...`                   | Via yt-dlp     | Yes, when **Save post captions** is enabled and the video has a description |
| Facebook           | `facebook.com/...`, `fb.watch/...`                                | Via yt-dlp     | Yes, when **Save post captions** is enabled and the video has a description |
| Twitch             | `twitch.tv/...`                                                   | Via yt-dlp     | No                                                                          |
| X                  | `x.com/...`, `twitter.com/...`                                    | Via yt-dlp     | No                                                                          |
| Reddit             | `reddit.com/r/.../comments/...`                                   | Via yt-dlp     | No                                                                          |
| Snapchat Spotlight | `snapchat.com/spotlight/...`                                      | Via yt-dlp     | No                                                                          |
| Pinterest          | `pinterest.com/pin/...`                                           | Via yt-dlp     | No                                                                          |
| Bluesky            | `bsky.app/profile/.../post/...`                                   | Via yt-dlp     | No                                                                          |
| Tumblr             | `tumblr.com/...`                                                  | Via yt-dlp     | No                                                                          |
| LinkedIn           | `linkedin.com/posts/...`                                          | Via yt-dlp     | No                                                                          |
| VK                 | `vk.com/video...`                                                 | Via yt-dlp     | No                                                                          |
| 9GAG               | `9gag.com/gag/...`                                                | Via yt-dlp     | No                                                                          |
| Kick               | `kick.com/...`                                                    | Via yt-dlp     | No                                                                          |
| Rumble             | `rumble.com/...`                                                  | Via yt-dlp     | No                                                                          |
| Vimeo              | `vimeo.com/...`                                                   | Via yt-dlp     | No                                                                          |
