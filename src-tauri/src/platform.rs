pub(super) const TIKTOK_FORMAT_SORT: &str = "vcodec:h264";

pub(super) fn detect_platform(url: &str) -> Option<String> {
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed
            .host_str()
            .unwrap_or("")
            .trim_end_matches('.')
            .to_ascii_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);

        for (domain, platform) in [
            ("youtube.com", "youtube"),
            ("facebook.com", "facebook"),
            ("twitch.tv", "twitch"),
            ("x.com", "x"),
            ("twitter.com", "x"),
            ("tiktok.com", "tiktok"),
            ("instagram.com", "instagram"),
            ("instagr.am", "instagram"),
        ] {
            if domain_matches(host, domain) {
                return Some(platform.to_string());
            }
        }
        if host == "youtu.be" {
            return Some("youtube".to_string());
        }
        if host == "fb.watch" {
            return Some("facebook".to_string());
        }
    }
    None
}

fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

pub(super) fn site_format_sort(url: &str) -> Option<&'static str> {
    // Some TikTok HEVC renditions are marked as AAC by the API even though the
    // downloaded container has no audio stream. Prefer the H.264 rendition,
    // which contains the muxed audio, while keeping yt-dlp's normal fallback.
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    domain_matches(&host, "tiktok.com").then_some(TIKTOK_FORMAT_SORT)
}
