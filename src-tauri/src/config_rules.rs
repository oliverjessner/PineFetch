use crate::models::AppConfig;
use crate::models::DEFAULT_FASTER_WHISPER_MODEL;
use crate::models::FASTER_WHISPER_MODELS;
use crate::presets::normalize_download_preset_key;
use serde::Deserialize;

pub(crate) fn normalize_app_config(mut config: AppConfig) -> AppConfig {
    config.selected_preset_key = Some(normalize_download_preset_key(
        config.selected_preset_key.as_deref(),
    ));
    config.faster_whisper_model = normalize_faster_whisper_model(&config.faster_whisper_model);
    config
}

pub(crate) fn normalize_faster_whisper_model(model: &str) -> String {
    let model = model.trim();
    FASTER_WHISPER_MODELS
        .iter()
        .find(|candidate| **candidate == model)
        .copied()
        .unwrap_or(DEFAULT_FASTER_WHISPER_MODEL)
        .to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct ConfigPatch {
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub(crate) yt_dlp_path: Option<Option<String>>,
    #[serde(default, deserialize_with = "deserialize_nullable_patch")]
    pub(crate) default_output_dir: Option<Option<String>>,
    pub(crate) selected_preset_key: Option<String>,
    pub(crate) faster_whisper_model: Option<String>,
    pub(crate) download_video_with_transcript: Option<bool>,
    #[serde(alias = "save_instagram_captions")]
    pub(crate) save_captions: Option<bool>,
    pub(crate) save_thumbnails: Option<bool>,
    pub(crate) magic_import_enabled: Option<bool>,
    pub(crate) cut_at_timestamp_enabled: Option<bool>,
    pub(crate) notifications_enabled: Option<bool>,
}

pub(crate) fn deserialize_nullable_patch<'de, D>(
    deserializer: D,
) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

pub(crate) fn apply_config_patch(config: &mut AppConfig, changes: ConfigPatch) {
    if let Some(value) = changes.yt_dlp_path {
        config.yt_dlp_path = value;
    }
    if let Some(value) = changes.default_output_dir {
        config.default_output_dir = value;
    }
    if let Some(value) = changes.selected_preset_key {
        config.selected_preset_key = Some(value);
    }
    if let Some(value) = changes.faster_whisper_model {
        config.faster_whisper_model = value;
    }
    if let Some(value) = changes.download_video_with_transcript {
        config.download_video_with_transcript = value;
    }
    if let Some(value) = changes.save_captions {
        config.save_captions = value;
    }
    if let Some(value) = changes.save_thumbnails {
        config.save_thumbnails = value;
    }
    if let Some(value) = changes.magic_import_enabled {
        config.magic_import_enabled = value;
    }
    if let Some(value) = changes.cut_at_timestamp_enabled {
        config.cut_at_timestamp_enabled = value;
    }
    if let Some(value) = changes.notifications_enabled {
        config.notifications_enabled = value;
    }
}
