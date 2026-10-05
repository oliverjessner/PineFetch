use crate::url_rules::domain_matches;
use crate::url_rules::normalized_url_host;

pub(super) const TIKTOK_FORMAT_SORT: &str = "vcodec:h264";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    YouTube,
    Facebook,
    Twitch,
    X,
    Reddit,
    TikTok,
    Instagram,
}

impl Platform {
    pub(crate) fn from_url(input: &str) -> Option<Self> {
        let parsed = url::Url::parse(input).ok()?;
        Self::from_host(&normalized_url_host(&parsed)?)
    }

    pub(crate) fn from_host(host: &str) -> Option<Self> {
        let host = host.strip_prefix("www.").unwrap_or(host);
        for (domain, platform) in [
            ("youtube.com", Self::YouTube),
            ("facebook.com", Self::Facebook),
            ("twitch.tv", Self::Twitch),
            ("x.com", Self::X),
            ("twitter.com", Self::X),
            ("reddit.com", Self::Reddit),
            ("redditmedia.com", Self::Reddit),
            ("tiktok.com", Self::TikTok),
            ("instagram.com", Self::Instagram),
            ("instagr.am", Self::Instagram),
        ] {
            if domain_matches(host, domain) {
                return Some(platform);
            }
        }
        match host {
            "youtu.be" => Some(Self::YouTube),
            "fb.watch" => Some(Self::Facebook),
            "redd.it" => Some(Self::Reddit),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::YouTube => "youtube",
            Self::Facebook => "facebook",
            Self::Twitch => "twitch",
            Self::X => "x",
            Self::Reddit => "reddit",
            Self::TikTok => "tiktok",
            Self::Instagram => "instagram",
        }
    }

    pub(crate) fn supports_captions(self) -> bool {
        !matches!(self, Self::Twitch)
    }

    pub(crate) fn format_sort(self) -> Option<&'static str> {
        // Some TikTok HEVC renditions are marked as AAC by the API even though the
        // downloaded container has no audio stream. Prefer the H.264 rendition,
        // which contains the muxed audio, while keeping yt-dlp's normal fallback.
        matches!(self, Self::TikTok).then_some(TIKTOK_FORMAT_SORT)
    }
}

pub(super) fn detect_platform(url: &str) -> Option<String> {
    Platform::from_url(url).map(|platform| platform.as_str().to_string())
}

pub(super) fn caption_platform(url: &str, enabled: bool) -> Option<String> {
    if !enabled {
        return None;
    }
    Platform::from_url(url)
        .filter(|platform| platform.supports_captions())
        .map(|platform| platform.as_str().to_string())
}

#[cfg(test)]
pub(super) fn site_format_sort(url: &str) -> Option<&'static str> {
    Platform::from_url(url).and_then(Platform::format_sort)
}
