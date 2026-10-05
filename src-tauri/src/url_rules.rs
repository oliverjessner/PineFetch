//! Shared URL syntax and host boundaries; content import policies stay separate.
use std::fmt;

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
