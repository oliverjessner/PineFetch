use crate::models::LinkDumpSettings;
use crate::models::NormalizedVideoUrl;
use crate::models::LINK_DUMP_DEFAULT_HOST;

pub(crate) fn normalize_video_url(input: &str) -> Option<NormalizedVideoUrl> {
    normalize_youtube_url(input)
        .or_else(|| normalize_tiktok_url(input))
        .or_else(|| normalize_instagram_url(input))
        .or_else(|| normalize_facebook_url(input))
        .or_else(|| normalize_x_url(input))
        .or_else(|| normalize_reddit_url(input))
}

pub(crate) fn normalize_youtube_url(input: &str) -> Option<NormalizedVideoUrl> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return None;
    }

    let host = parsed
        .host_str()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let host_without_www = host.strip_prefix("www.").unwrap_or(&host);
    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.collect::<Vec<_>>())
        .unwrap_or_default();

    let video_id = if host_without_www == "youtu.be" {
        path_parts.first().map(|part| (*part).to_string())
    } else if matches!(
        host_without_www,
        "youtube.com" | "m.youtube.com" | "music.youtube.com"
    ) {
        match path_parts
            .first()
            .map(|part| part.to_ascii_lowercase())
            .as_deref()
        {
            Some("watch") => parsed.query_pairs().find_map(|(name, value)| {
                if name == "v" {
                    Some(value.into_owned())
                } else {
                    None
                }
            }),
            Some("shorts") | Some("live") | Some("embed") | Some("v") => {
                path_parts.get(1).map(|part| (*part).to_string())
            }
            _ => None,
        }
    } else {
        None
    }?;

    if !is_plausible_youtube_video_id(&video_id) {
        return None;
    }

    Some(NormalizedVideoUrl {
        url: format!("https://www.youtube.com/watch?v={video_id}"),
        key: format!("youtube:{video_id}"),
        thumbnail: Some(format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg")),
    })
}

pub(crate) fn is_plausible_youtube_video_id(video_id: &str) -> bool {
    let len = video_id.len();
    (6..=64).contains(&len)
        && video_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

pub(crate) fn normalize_tiktok_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    if host != "tiktok.com" && !host.ends_with(".tiktok.com") {
        return None;
    }

    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();

    if path_parts.len() >= 3
        && path_parts[0].starts_with('@')
        && path_parts[1].eq_ignore_ascii_case("video")
    {
        let handle = path_parts[0].strip_prefix('@')?;
        let video_id = path_parts[2];
        if is_plausible_tiktok_handle(handle) && is_plausible_numeric_id(video_id) {
            return Some(NormalizedVideoUrl {
                url: format!("https://www.tiktok.com/@{handle}/video/{video_id}"),
                key: format!("tiktok:{video_id}"),
                thumbnail: None,
            });
        }
    }

    let short_code = if matches!(host.as_str(), "vm.tiktok.com" | "vt.tiktok.com") {
        path_parts.first().copied()
    } else if path_parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("t"))
    {
        path_parts.get(1).copied()
    } else {
        None
    }?;

    if !is_plausible_content_code(short_code) {
        return None;
    }

    let url = if matches!(host.as_str(), "vm.tiktok.com" | "vt.tiktok.com") {
        format!("https://{host}/{short_code}/")
    } else {
        format!("https://www.tiktok.com/t/{short_code}/")
    };
    Some(NormalizedVideoUrl {
        url,
        key: format!("tiktok-short:{short_code}"),
        thumbnail: None,
    })
}

pub(crate) fn normalize_instagram_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let is_instagram_host = host == "instagram.com"
        || host.ends_with(".instagram.com")
        || host == "instagr.am"
        || host.ends_with(".instagr.am");
    if !is_instagram_host {
        return None;
    }

    let path_parts = parsed
        .path_segments()
        .map(|segments| segments.filter(|part| !part.is_empty()).collect::<Vec<_>>())
        .unwrap_or_default();
    let (route, content_code) = match path_parts.as_slice() {
        [route, content_code, ..] if is_instagram_content_route(route) => {
            ((*route).to_ascii_lowercase(), *content_code)
        }
        [_, route, content_code, ..] if is_instagram_content_route(route) => {
            ((*route).to_ascii_lowercase(), *content_code)
        }
        _ => return None,
    };
    if !is_plausible_content_code(content_code) {
        return None;
    }

    Some(NormalizedVideoUrl {
        url: format!("https://www.instagram.com/{route}/{content_code}/"),
        key: format!("instagram:{content_code}"),
        thumbnail: None,
    })
}

pub(crate) fn normalize_facebook_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    if host == "fb.watch" {
        let code = *parts.first()?;
        if !is_plausible_content_code(code) {
            return None;
        }
        return Some(NormalizedVideoUrl {
            url: format!("https://fb.watch/{code}/"),
            key: format!("facebook-short:{code}"),
            thumbnail: None,
        });
    }
    if host != "facebook.com" && !host.ends_with(".facebook.com") {
        return None;
    }

    let video_id = if parts
        .first()
        .is_some_and(|part| matches!(part.to_ascii_lowercase().as_str(), "watch" | "video.php"))
    {
        parsed
            .query_pairs()
            .find_map(|(name, value)| (name == "v").then(|| value.into_owned()))
    } else if parts
        .first()
        .is_some_and(|part| part.eq_ignore_ascii_case("reel"))
    {
        parts.get(1).map(|part| (*part).to_string())
    } else {
        parts
            .windows(2)
            .find(|pair| pair[0].eq_ignore_ascii_case("videos"))
            .map(|pair| pair[1].to_string())
    };
    if let Some(video_id) = video_id.filter(|id| is_plausible_numeric_id(id)) {
        return Some(NormalizedVideoUrl {
            url: format!("https://www.facebook.com/watch/?v={video_id}"),
            key: format!("facebook:{video_id}"),
            thumbnail: None,
        });
    }

    let post = match parts.as_slice() {
        [account, "posts", id, ..] if !account.is_empty() => Some(format!("{account}/posts/{id}")),
        ["groups", group, "posts", id, ..] if !group.is_empty() => {
            Some(format!("groups/{group}/posts/{id}"))
        }
        _ => None,
    };
    if let Some(post) = post {
        let post_id = post.rsplit('/').next()?;
        if is_plausible_numeric_id(post_id)
            || (post_id.starts_with("pfbid") && is_plausible_content_code(post_id))
        {
            return Some(NormalizedVideoUrl {
                url: format!("https://www.facebook.com/{post}/"),
                key: format!("facebook-post:{post_id}"),
                thumbnail: None,
            });
        }
    }

    let (route, code) = match parts.as_slice() {
        ["share", route @ ("v" | "r"), code, ..] => (*route, *code),
        _ => return None,
    };
    if !is_plausible_content_code(code) {
        return None;
    }
    Some(NormalizedVideoUrl {
        url: format!("https://www.facebook.com/share/{route}/{code}/"),
        key: format!("facebook-share:{route}:{code}"),
        thumbnail: None,
    })
}

pub(crate) fn normalize_x_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    if !matches!(
        host.as_str(),
        "x.com"
            | "www.x.com"
            | "mobile.x.com"
            | "twitter.com"
            | "www.twitter.com"
            | "mobile.twitter.com"
    ) {
        return None;
    }
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let (handle, route, status_id) = match parts.as_slice() {
        ["i", "web", route, status_id, ..] => ("i", *route, *status_id),
        [handle, route, status_id, ..] => (*handle, *route, *status_id),
        _ => return None,
    };
    if !route.eq_ignore_ascii_case("status")
        || !is_plausible_numeric_id(status_id)
        || (handle != "i"
            && (handle.is_empty()
                || handle.len() > 15
                || !handle
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')))
    {
        return None;
    }
    Some(NormalizedVideoUrl {
        url: format!("https://x.com/{handle}/status/{status_id}"),
        key: format!("x:{status_id}"),
        thumbnail: None,
    })
}

pub(crate) fn normalize_reddit_url(input: &str) -> Option<NormalizedVideoUrl> {
    let parsed = parse_http_url(input)?;
    let host = normalized_url_host(&parsed)?;
    let parts = parsed
        .path_segments()?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let post_id = if host == "redd.it" {
        *parts.first()?
    } else if matches!(host.as_str(), "reddit.com" | "redditmedia.com")
        || host.ends_with(".reddit.com")
        || host.ends_with(".redditmedia.com")
    {
        match parts.as_slice() {
            ["comments", id, ..]
            | ["r", _, "comments", id, ..]
            | ["user", _, "comments", id, ..] => *id,
            _ => return None,
        }
    } else {
        return None;
    };
    if !(5..=16).contains(&post_id.len())
        || !post_id.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return None;
    }
    let post_id = post_id.to_ascii_lowercase();
    Some(NormalizedVideoUrl {
        url: format!("https://www.reddit.com/comments/{post_id}/"),
        key: format!("reddit:{post_id}"),
        thumbnail: None,
    })
}

pub(crate) fn parse_http_url(input: &str) -> Option<url::Url> {
    let parsed = url::Url::parse(input.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

pub(crate) fn normalized_url_host(parsed: &url::Url) -> Option<String> {
    Some(
        parsed
            .host_str()?
            .trim_end_matches('.')
            .to_ascii_lowercase(),
    )
}

pub(crate) fn is_plausible_tiktok_handle(handle: &str) -> bool {
    !handle.is_empty()
        && handle.len() <= 64
        && handle
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.'))
}

pub(crate) fn is_plausible_numeric_id(value: &str) -> bool {
    (6..=32).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

pub(crate) fn is_plausible_content_code(value: &str) -> bool {
    (3..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(crate) fn is_instagram_content_route(route: &str) -> bool {
    matches!(route.to_ascii_lowercase().as_str(), "p" | "reel" | "tv")
}
pub(crate) fn normalize_link_dump_host(host: &str) -> String {
    if host.trim() == "127.0.1" {
        return LINK_DUMP_DEFAULT_HOST.to_string();
    }
    host.trim().to_string()
}

pub(crate) fn is_allowed_link_dump_host(host: &str) -> bool {
    let normalized = normalize_link_dump_host(host);
    if normalized.eq_ignore_ascii_case("localhost") {
        return true;
    }
    normalized
        .parse::<std::net::IpAddr>()
        .map(|addr| addr.is_loopback())
        .unwrap_or(false)
}

pub(crate) fn link_dump_server_url(settings: &LinkDumpSettings) -> String {
    format!("http://{}:{}", settings.host, settings.port)
}
