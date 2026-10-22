use crate::process::{run_command_output, ProcessState};
use std::process::Command;
use std::time::Duration;

// A public video provides a real cookie-access check without downloading media.
const CHECK_URL: &str = "https://www.youtube.com/watch?v=jNQXAC9IVRw";

pub(crate) fn check_access(
    yt_dlp: &str,
    deno: Option<&str>,
    browser: &str,
    processes: &ProcessState,
) -> Result<(), String> {
    if !matches!(
        browser,
        "brave" | "chrome" | "chromium" | "edge" | "firefox" | "opera" | "safari" | "vivaldi"
    ) {
        return Err("COOKIE_ACCESS_FAILED".into());
    }
    let mut command = Command::new(yt_dlp);
    command.args([
        "--ignore-config",
        "--no-cache-dir",
        "--simulate",
        "--skip-download",
        "--ignore-no-formats-error",
    ]);
    crate::yt_dlp::apply_browser_cookies(&mut command, CHECK_URL, Some(browser));
    command.args(crate::yt_dlp::build_info_args(CHECK_URL, deno));
    let output = run_command_output(
        command,
        None,
        Some(processes),
        Some(Duration::from_secs(90)),
    )
    .map_err(|_| "COOKIE_ACCESS_FAILED".to_string())?;
    // Do not forward process output: it may contain private account metadata.
    if output.status.success() {
        Ok(())
    } else {
        Err("COOKIE_ACCESS_FAILED".into())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn integration_cookie_check_runs_simulation_and_suppresses_process_errors() {
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
[ "$6" = '--cookies-from-browser' ]
[ "$7" = 'brave' ]
"#,
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let processes = ProcessState::default();
        assert!(check_access(executable.to_str().unwrap(), None, "brave", &processes).is_ok());
        fs::write(
            &executable,
            "#!/bin/sh\necho 'synthetic private diagnostic' >&2\nexit 1\n",
        )
        .unwrap();
        assert_eq!(
            check_access(executable.to_str().unwrap(), None, "brave", &processes).unwrap_err(),
            "COOKIE_ACCESS_FAILED"
        );
        assert_eq!(
            check_access(
                "missing-executable",
                None,
                "chrome:arbitrary-path",
                &processes
            )
            .unwrap_err(),
            "COOKIE_ACCESS_FAILED"
        );
        assert!(processes.utility_children.lock().unwrap().is_empty());
    }
}
