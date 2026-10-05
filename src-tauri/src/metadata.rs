use crate::models::InfoResponse;
use crate::process::run_command_output;
use crate::yt_dlp::parse_info_json;
use std::process::Command;
use std::time::Duration;

pub(crate) const INFO_TIMEOUT: Duration = Duration::from_secs(90);

pub(crate) fn load_info_with_yt_dlp(
    yt_dlp: String,
    deno: Option<String>,
    url: String,
    state: &crate::process::ProcessState,
) -> Result<InfoResponse, String> {
    let mut command = Command::new(&yt_dlp);
    command.args(["--dump-json", "--no-playlist", "--no-warnings"]);
    if let Some(deno) = deno {
        command.arg("--js-runtimes");
        command.arg(format!("deno:{deno}"));
    }

    command.arg(&url);
    let output = run_command_output(command, None, Some(state), Some(INFO_TIMEOUT))
        .map_err(|e| format!("Failed to run yt-dlp: {e}"))?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("yt-dlp exited {code}: {stderr}"));
    }

    let raw = String::from_utf8_lossy(&output.stdout).to_string();
    parse_info_json(&raw)
}
