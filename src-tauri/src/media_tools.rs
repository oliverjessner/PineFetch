//! Pure FFmpeg/Whisper arguments and transcription stdout interpretation.
use std::ffi::OsStr;
use std::path::Path;
pub(crate) fn build_cut_args<'a>(
    input: &'a str,
    output: &'a str,
    timestamp: &'a str,
) -> [&'a str; 13] {
    [
        "-hide_banner",
        "-y",
        "-ss",
        timestamp,
        "-i",
        input,
        "-map",
        "0",
        "-c",
        "copy",
        "-avoid_negative_ts",
        "make_zero",
        output,
    ]
}

pub(crate) fn build_transcription_audio_args<'a>(input: &'a str, output: &'a str) -> [&'a str; 14] {
    [
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-i",
        input,
        "-vn",
        "-ac",
        "1",
        "-ar",
        "16000",
        "-c:a",
        "pcm_s16le",
        output,
    ]
}

pub(crate) fn build_transcription_args<'a>(
    input: &'a Path,
    output: &'a str,
    model: &'a str,
    timestamps: bool,
) -> [&'a OsStr; 6] {
    [
        OsStr::new("-c"),
        OsStr::new(FASTER_WHISPER_TRANSCRIBE_SNIPPET),
        input.as_os_str(),
        OsStr::new(output),
        OsStr::new(model),
        OsStr::new(if timestamps { "1" } else { "0" }),
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TranscriptionLine<'a> {
    Language(Option<String>),
    Log(&'a str),
}

pub(crate) fn parse_transcription_line(line: &str) -> TranscriptionLine<'_> {
    match line.strip_prefix("pinefetch_language:") {
        Some(raw) => {
            let language = raw.trim().to_ascii_lowercase();
            TranscriptionLine::Language((!language.is_empty()).then_some(language))
        }
        None => TranscriptionLine::Log(line),
    }
}

const FASTER_WHISPER_TRANSCRIBE_SNIPPET: &str = r#"
import sys
from pathlib import Path

try:
    from faster_whisper import WhisperModel
except Exception as exc:
    print(f"Failed to import faster_whisper: {exc}", file=sys.stderr)
    raise

audio_path = sys.argv[1]
output_path = Path(sys.argv[2])
model_name = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else "base"
include_timestamps = len(sys.argv) > 4 and sys.argv[4] == "1"

def format_timestamp(seconds):
    total_seconds = max(0, int(round(seconds)))
    hours, remainder = divmod(total_seconds, 3600)
    minutes, seconds = divmod(remainder, 60)
    return f"{hours:02d}:{minutes:02d}:{seconds:02d}"

model = WhisperModel(model_name, compute_type="int8")
segments, info = model.transcribe(audio_path, beam_size=5)
print(f"pinefetch_language:{info.language}", flush=True)
lines = []
for segment in segments:
    text = segment.text.strip()
    if text:
        if include_timestamps:
            start = format_timestamp(segment.start)
            end = format_timestamp(segment.end)
            lines.append(f"[{start} → {end}] {text}")
        else:
            lines.append(text)

content = "\n".join(lines).strip()
if content:
    content += "\n"
output_path.write_text(content, encoding="utf-8")
print(str(output_path))
"#;
