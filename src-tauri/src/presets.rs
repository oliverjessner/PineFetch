use serde::Serialize;

pub(super) const DEFAULT_DOWNLOAD_PRESET_KEY: &str = "best";

#[derive(Debug, Clone, Copy, Serialize)]
pub(super) struct DownloadPreset {
    pub(super) key: &'static str,
    pub(super) format: &'static str,
    pub(super) extract_audio: bool,
    pub(super) audio_format: Option<&'static str>,
    pub(super) transcribe_text: bool,
    pub(super) transcribe_timestamps: bool,
    pub(super) filename_suffix: Option<&'static str>,
}

pub(super) const DOWNLOAD_PRESETS: &[DownloadPreset] = &[
    DownloadPreset {
        key: "best",
        format: "bestvideo+bestaudio/best",
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        transcribe_timestamps: false,
        filename_suffix: Some("_best"),
    },
    DownloadPreset {
        key: "1080",
        format: "bv*[height<=1080]+ba/b[height<=1080]",
        extract_audio: false,
        audio_format: None,
        transcribe_text: false,
        transcribe_timestamps: false,
        filename_suffix: Some("__max"),
    },
    DownloadPreset {
        key: "audio_mp3",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("mp3"),
        transcribe_text: false,
        transcribe_timestamps: false,
        filename_suffix: None,
    },
    DownloadPreset {
        key: "audio_opus",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("opus"),
        transcribe_text: false,
        transcribe_timestamps: false,
        filename_suffix: None,
    },
    DownloadPreset {
        key: "text",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("mp3"),
        transcribe_text: true,
        transcribe_timestamps: false,
        filename_suffix: None,
    },
    DownloadPreset {
        key: "text_timestamps",
        format: "ba/b",
        extract_audio: true,
        audio_format: Some("mp3"),
        transcribe_text: true,
        transcribe_timestamps: true,
        filename_suffix: Some("_timestamps"),
    },
];

pub(super) fn download_preset_for_key(preset_key: Option<&str>) -> &'static DownloadPreset {
    let key = preset_key
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .unwrap_or(DEFAULT_DOWNLOAD_PRESET_KEY);

    DOWNLOAD_PRESETS
        .iter()
        .find(|preset| preset.key == key)
        .unwrap_or(&DOWNLOAD_PRESETS[0])
}

pub(super) fn normalize_download_preset_key(preset_key: Option<&str>) -> String {
    download_preset_for_key(preset_key).key.to_string()
}
