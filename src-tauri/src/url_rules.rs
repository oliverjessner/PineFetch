//! Shared URL syntax and host boundaries; content import policies stay separate.
use std::fmt;

// Full URLs retain the existing validation boundary, including control-character
// rejection. Only opt-in IDs are converted into fixed HTTPS platform URLs.
pub(crate) fn normalize_video_input(input: &str, id_import_enabled: bool) -> String {
    if !id_import_enabled || parse_http_url(input).is_some() {
        return input.to_string();
    }
    let value = input.trim();
    let is_code = |code: &str| {
        code.bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    };
    if value.len() == 11 && is_code(value) {
        return format!("https://www.youtube.com/watch?v={value}");
    }
    if let Some((platform, id)) = value.split_once(':') {
        match platform.to_ascii_lowercase().as_str() {
            "youtube" if id.len() == 11 && is_code(id) => {
                return format!("https://www.youtube.com/watch?v={id}");
            }
            "tiktok" if (6..=25).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) => {
                return format!("https://www.tiktok.com/@_/video/{id}");
            }
            "instagram" if (3..=128).contains(&id.len()) && is_code(id) => {
                return format!("https://www.instagram.com/p/{id}/");
            }
            _ => {}
        }
    }
    input.to_string()
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

pub(crate) fn domain_matches(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct InvalidDownloadUrl;

impl fmt::Display for InvalidDownloadUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("URL must start with http:// or https://")
    }
}
impl std::error::Error for InvalidDownloadUrl {}

pub(crate) fn validate_download_url(input: &str) -> Result<(), InvalidDownloadUrl> {
    match parse_http_url(input) {
        Some(url) if url.host_str().is_some() && !input.chars().any(char::is_control) => Ok(()),
        _ => Err(InvalidDownloadUrl),
    }
}
