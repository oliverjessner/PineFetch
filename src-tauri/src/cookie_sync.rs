use crate::process::{run_command_output, ProcessError, ProcessState};
use serde::Serialize;
use std::process::Command;
use std::time::Duration;

// This public metadata request cannot establish a YouTube sign-in.
const CHECK_ID: &str = "jNQXAC9IVRw";
const CHECK_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct BrowserAccessReport {
    public_access: &'static str,
    authentication: &'static str,
}

// Only these fixed codes cross IPC. Never serialize stderr, cookie values,
// account metadata, profile paths, or a process/OS error message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum BrowserAccessError {
    #[serde(rename = "YT_DLP_UNAVAILABLE")]
    YtDlpUnavailable,
    InvalidBrowser,
    ProfileUnavailable,
    PermissionDenied,
    BrowserLocked,
    CookieDecryption,
    CookiesUnavailable,
    NetworkDns,
    NetworkTls,
    NetworkTimeout,
    NetworkConnection,
    RateLimited,
    SignInRequired,
    AccessRejected,
    CheckTimeout,
    CheckFailed,
}

fn diagnose(stderr: &str) -> Option<BrowserAccessError> {
    use BrowserAccessError::*;
    let text = stderr.to_ascii_lowercase();
    let has = |patterns: &[&str]| patterns.iter().any(|pattern| text.contains(pattern));
    // Cookie extraction failures take precedence over a public fallback or
    // subsequent network errors. Missing databases can also mean denied access.
    if has(&[
        "permission denied",
        "operation not permitted",
        "access is denied",
        "errno 13",
        "winerror 5",
    ]) {
        Some(PermissionDenied)
    } else if has(&[
        "database is locked",
        "database is busy",
        "could not copy chrome cookie database",
        "used by another process",
    ]) {
        Some(BrowserLocked)
    } else if has(&["could not find"]) && has(&["cookies database", "local state file"]) {
        Some(ProfileUnavailable)
    } else if has(&[
        "failed to decrypt",
        "cannot decrypt",
        "could not be decrypted",
        "failed to get cookie encryption key",
        "failed to read from keychain",
        "find-generic-password failed",
        "exception running find-generic-password",
        "failed to read from keyring",
        "no encrypted key in local state",
        "secretstorage not available",
        "kwallet-query command not found",
        "unknown cookie version",
    ]) {
        Some(CookieDecryption)
    } else if has(&["extracted 0 cookies"]) {
        Some(CookiesUnavailable)
    } else if has(&["http error 429", "too many requests", "rate limit"]) {
        Some(RateLimited)
    } else if has(&[
        "sign in to confirm",
        "login required",
        "sign in to access",
        "private video",
        "only available for registered users",
        "cookies are no longer valid",
    ]) {
        Some(SignInRequired)
    } else if has(&[
        "certificate_verify_failed",
        "certificate verify failed",
        "certificate has expired",
        "ssl: certificate",
        "tls handshake",
    ]) {
        Some(NetworkTls)
    } else if has(&[
        "name or service not known",
        "temporary failure in name resolution",
        "nodename nor servname",
        "getaddrinfo failed",
        "failed to resolve",
        "name resolution",
    ]) {
        Some(NetworkDns)
    } else if has(&["timed out", "timeout error"]) {
        Some(NetworkTimeout)
    } else if has(&[
        "connection refused",
        "connection reset",
        "network is unreachable",
        "network unreachable",
        "no route to host",
        "remote end closed connection",
        "unable to connect",
        "proxyerror",
    ]) {
        Some(NetworkConnection)
    } else if has(&["http error 403", "http error 401"]) {
        Some(AccessRejected)
    } else {
        None
    }
}

pub(crate) fn check_access(
    yt_dlp: &str,
    deno: Option<&str>,
    browser: &str,
    processes: &ProcessState,
) -> Result<BrowserAccessReport, BrowserAccessError> {
    check_access_with_timeout(yt_dlp, deno, browser, processes, Duration::from_secs(90))
}

fn check_access_with_timeout(
    yt_dlp: &str,
    deno: Option<&str>,
    browser: &str,
    processes: &ProcessState,
    timeout: Duration,
) -> Result<BrowserAccessReport, BrowserAccessError> {
    if !matches!(
        browser,
        "brave" | "chrome" | "chromium" | "edge" | "firefox" | "opera" | "safari" | "vivaldi"
    ) {
        return Err(BrowserAccessError::InvalidBrowser);
    }
    let mut command = Command::new(yt_dlp);
    command.args([
        "--ignore-config",
        "--no-cache-dir",
        "--simulate",
        "--skip-download",
        "--no-playlist",
        "--no-color",
        "--print",
        "id",
        "--no-quiet",
        "--socket-timeout",
        "15",
        "--retries",
        "1",
        "--extractor-retries",
        "1",
    ]);
    crate::yt_dlp::apply_browser_cookies(&mut command, CHECK_URL, Some(browser));
    if let Some(deno) = deno {
        command.args(["--js-runtimes", &format!("deno:{deno}")]);
    }
    command.arg(CHECK_URL);
    let output =
        run_command_output(command, None, Some(processes), Some(timeout)).map_err(|error| {
            match error {
                ProcessError::Spawn(_) => BrowserAccessError::YtDlpUnavailable,
                ProcessError::TimedOut => BrowserAccessError::CheckTimeout,
                _ => BrowserAccessError::CheckFailed,
            }
        })?;
    let diagnosis = diagnose(&format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    ));
    let browser_problem = matches!(
        diagnosis,
        Some(
            BrowserAccessError::PermissionDenied
                | BrowserAccessError::BrowserLocked
                | BrowserAccessError::ProfileUnavailable
                | BrowserAccessError::CookieDecryption
                | BrowserAccessError::CookiesUnavailable
                | BrowserAccessError::SignInRequired
        )
    );
    if browser_problem || !output.status.success() {
        return Err(diagnosis.unwrap_or(BrowserAccessError::CheckFailed));
    }
    // A successful exit alone (including empty output) is not proof of a fetch.
    if !String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim() == CHECK_ID)
    {
        return Err(BrowserAccessError::CheckFailed);
    }
    Ok(BrowserAccessReport {
        public_access: "checked",
        authentication: "not_verified",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_categories_are_safe_and_missing_profiles_are_not_claimed_absent() {
        use BrowserAccessError::*;
        let cases = [
            (
                "ERROR: [Errno 13] Permission denied: /private/profile/Cookies",
                PermissionDenied,
            ),
            ("Operation not permitted", PermissionDenied),
            (
                "could not find chrome cookies database in /private/profile",
                ProfileUnavailable,
            ),
            ("Could not copy Chrome cookie database", BrowserLocked),
            ("database is locked", BrowserLocked),
            (
                "WARNING: cannot decrypt v10 cookies: no key found",
                CookieDecryption,
            ),
            ("WARNING: Failed to decrypt with DPAPI", CookieDecryption),
            ("WARNING: find-generic-password failed", CookieDecryption),
            ("ERROR: failed to read from keyring", CookieDecryption),
            ("WARNING: 23 could not be decrypted", CookieDecryption),
            ("Extracted 0 cookies from firefox", CookiesUnavailable),
            ("HTTP Error 429: Too Many Requests", RateLimited),
            ("Sign in to confirm you're not a bot", SignInRequired),
            (
                "The provided YouTube account cookies are no longer valid",
                SignInRequired,
            ),
            ("CERTIFICATE_VERIFY_FAILED", NetworkTls),
            ("[Errno -2] Name or service not known", NetworkDns),
            ("getaddrinfo failed", NetworkDns),
            ("The read operation timed out", NetworkTimeout),
            ("Connection refused", NetworkConnection),
            ("HTTP Error 403: Forbidden", AccessRejected),
        ];
        for (message, expected) in cases {
            let actual = diagnose(message).unwrap();
            assert_eq!(actual, expected, "{message}");
            let serialized = serde_json::to_string(&actual).unwrap();
            assert!(serialized
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || matches!(byte, b'_' | b'"')));
        }
        assert_eq!(diagnose("synthetic private cookie secret"), None);
        assert_eq!(
            diagnose("cannot decrypt cookie; HTTP Error 429"),
            Some(CookieDecryption)
        );
    }

    #[cfg(unix)]
    #[test]
    fn integration_cookie_check_requires_public_result_and_never_claims_authentication() {
        use crate::test_support::TempRoot;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("cookie-access-check");
        let executable = root.path("fake-yt-dlp");
        fs::write(
            &executable,
            r#"#!/bin/sh
set -eu
[ "$1" = '--ignore-config' ]
[ "$2" = '--no-cache-dir' ]
[ "$3" = '--simulate' ]
[ "$4" = '--skip-download' ]
case " $* " in *" --cookies-from-browser brave "*) ;; *) exit 1 ;; esac
case " $* " in *" --print id "*) ;; *) exit 1 ;; esac
case " $* " in *" --dump-json "*|*" --no-warnings "*|*" --cookies "*|*" --verbose "*) exit 1 ;; esac
for arg; do last="$arg"; done
[ "$last" = 'https://www.youtube.com/watch?v=jNQXAC9IVRw' ]
printf 'jNQXAC9IVRw\n'
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let processes = ProcessState::default();
        let path = executable.to_str().unwrap();
        let report = check_access(path, None, "brave", &processes).unwrap();
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::json!({"public_access":"checked", "authentication":"not_verified"})
        );
        for (body, expected) in [
            ("echo 'private cookie secret' >&2; exit 1", BrowserAccessError::CheckFailed),
            ("exit 0", BrowserAccessError::CheckFailed),
            ("echo jNQXAC9IVRw; echo 'cannot decrypt v10 cookies: no key found; private secret' >&2", BrowserAccessError::CookieDecryption),
            ("echo jNQXAC9IVRw; echo 'Extracted 0 cookies from brave'", BrowserAccessError::CookiesUnavailable),
            ("echo jNQXAC9IVRw; echo 'The provided YouTube account cookies are no longer valid' >&2", BrowserAccessError::SignInRequired),
            ("echo 'could not find chrome cookies database in /private/profile' >&2; exit 1", BrowserAccessError::ProfileUnavailable),
            ("echo 'HTTP Error 429; private secret' >&2; exit 1", BrowserAccessError::RateLimited),
        ] {
            fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
            assert_eq!(check_access(path, None, "brave", &processes).unwrap_err(), expected);
            assert!(processes.utility_children.lock().unwrap().is_empty());
        }
        assert_eq!(
            check_access(path, None, "chrome:arbitrary-path", &processes).unwrap_err(),
            BrowserAccessError::InvalidBrowser
        );
        assert_eq!(
            check_access(
                root.path("missing").to_str().unwrap(),
                None,
                "chrome",
                &processes
            )
            .unwrap_err(),
            BrowserAccessError::YtDlpUnavailable
        );
        fs::write(&executable, "#!/bin/sh\nsleep 30\n").unwrap();
        assert_eq!(
            check_access_with_timeout(path, None, "brave", &processes, Duration::from_millis(150))
                .unwrap_err(),
            BrowserAccessError::CheckTimeout
        );
        assert!(processes.utility_children.lock().unwrap().is_empty());
    }
}
