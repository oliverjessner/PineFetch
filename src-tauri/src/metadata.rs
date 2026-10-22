use crate::models::InfoResponse;
use crate::process::run_command_output;
use crate::yt_dlp::build_info_args;
use crate::yt_dlp::parse_info_json;
use std::process::Command;
use std::time::Duration;

pub(crate) const INFO_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) fn load_info_with_yt_dlp(
    yt_dlp: String,
    deno: Option<String>,
    url: String,
    state: &crate::process::ProcessState,
    browser: Option<&str>,
) -> Result<InfoResponse, String> {
    parse_info_json(&load_raw_info(
        &yt_dlp,
        deno.as_deref(),
        &url,
        state,
        browser,
    )?)
}

pub(crate) fn load_raw_info(
    yt_dlp: &str,
    deno: Option<&str>,
    url: &str,
    state: &crate::process::ProcessState,
    browser: Option<&str>,
) -> Result<String, String> {
    let mut command = Command::new(yt_dlp);
    crate::yt_dlp::apply_browser_cookies(&mut command, url, browser);
    command.args(build_info_args(url, deno));
    let output = run_command_output(command, None, Some(state), Some(INFO_TIMEOUT))
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("yt-dlp exited {code}: {stderr}"));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(raw)
}
