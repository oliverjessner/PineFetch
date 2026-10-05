use crate::presets::DEFAULT_DOWNLOAD_PRESET_KEY;
use serde::Deserialize;
use serde::Serialize;

fn default_magic_import_enabled() -> bool {
    true
}

fn default_cut_at_timestamp_enabled() -> bool {
    true
}

fn default_faster_whisper_model() -> String {
    DEFAULT_FASTER_WHISPER_MODEL.to_string()
}

pub(crate) const DEFAULT_FASTER_WHISPER_MODEL: &str = "base";
pub(crate) const FASTER_WHISPER_MODELS: [&str; 4] = ["base", "small", "medium", "large-v3"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AppConfig {
    pub(crate) yt_dlp_path: Option<String>,
    pub(crate) default_output_dir: Option<String>,
    #[serde(default)]
    pub(crate) selected_preset_key: Option<String>,
    #[serde(default = "default_faster_whisper_model")]
    pub(crate) faster_whisper_model: String,
    #[serde(default)]
    pub(crate) download_video_with_transcript: bool,
    #[serde(default, alias = "save_instagram_captions")]
    pub(crate) save_captions: bool,
    #[serde(default)]
    pub(crate) save_thumbnails: bool,
    #[serde(default = "default_magic_import_enabled")]
    pub(crate) magic_import_enabled: bool,
    #[serde(default = "default_cut_at_timestamp_enabled")]
    pub(crate) cut_at_timestamp_enabled: bool,
    #[serde(default)]
    pub(crate) last_download_url: Option<String>,
    #[serde(default)]
    pub(crate) notifications_enabled: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: None,
            default_output_dir: None,
            selected_preset_key: Some(DEFAULT_DOWNLOAD_PRESET_KEY.to_string()),
            faster_whisper_model: default_faster_whisper_model(),
            download_video_with_transcript: false,
            save_captions: false,
            save_thumbnails: false,
            magic_import_enabled: default_magic_import_enabled(),
            cut_at_timestamp_enabled: default_cut_at_timestamp_enabled(),
            last_download_url: None,
            notifications_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownloadRequest {
    pub(crate) url: String,
    pub(crate) format: String,
    pub(crate) output_dir: Option<String>,
    pub(crate) extract_audio: bool,
    pub(crate) audio_format: Option<String>,
    pub(crate) transcribe_text: bool,
    #[serde(default)]
    pub(crate) transcribe_timestamps: bool,
    #[serde(default = "default_cut_at_timestamp_enabled")]
    pub(crate) cut_at_timestamp_enabled: bool,
    #[serde(default)]
    pub(crate) cut_start_time: Option<f64>,
    #[serde(default)]
    pub(crate) filename_suffix: Option<String>,
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) uploader: Option<String>,
    pub(crate) thumbnail: Option<String>,
    #[serde(default)]
    pub(crate) upload_date: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<i64>,
    #[serde(default)]
    pub(crate) duration_seconds: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownloadJob {
    pub(crate) id: String,
    pub(crate) url: String,
    pub(crate) format: String,
    pub(crate) output_dir: String,
    pub(crate) extract_audio: bool,
    pub(crate) audio_format: Option<String>,
    pub(crate) transcribe_text: bool,
    pub(crate) transcribe_timestamps: bool,
    #[serde(default = "default_faster_whisper_model")]
    pub(crate) faster_whisper_model: String,
    #[serde(default)]
    pub(crate) download_video_with_transcript: bool,
    #[serde(default, alias = "save_instagram_captions")]
    pub(crate) save_captions: bool,
    #[serde(default)]
    pub(crate) save_thumbnails: bool,
    pub(crate) title: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) thumbnail: Option<String>,
    pub(crate) upload_date: Option<String>,
    pub(crate) timestamp: Option<i64>,
    pub(crate) duration_seconds: Option<i64>,
    pub(crate) cut_start_time: Option<f64>,
    pub(crate) filename_suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownloadProgress {
    pub(crate) id: String,
    pub(crate) percent: Option<f32>,
    pub(crate) speed: Option<String>,
    pub(crate) eta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct DownloadStateEvent {
    pub(crate) id: String,
    pub(crate) state: DownloadState,
    pub(crate) exit_code: Option<i32>,
    pub(crate) error: Option<String>,
    pub(crate) output_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LogEvent {
    pub(crate) id: String,
    pub(crate) line: String,
    pub(crate) is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InfoFormat {
    pub(crate) format_id: Option<String>,
    pub(crate) ext: Option<String>,
    pub(crate) vcodec: Option<String>,
    pub(crate) acodec: Option<String>,
    pub(crate) height: Option<i64>,
    pub(crate) width: Option<i64>,
    pub(crate) fps: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InfoResponse {
    pub(crate) title: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) duration: Option<i64>,
    pub(crate) thumbnail: Option<String>,
    pub(crate) upload_date: Option<String>,
    pub(crate) timestamp: Option<i64>,
    pub(crate) formats: Option<Vec<InfoFormat>>,
    pub(crate) description: Option<String>,
    pub(crate) id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct HistoryEntry {
    pub(crate) id: String,
    pub(crate) url: String,
    #[serde(default)]
    pub(crate) title: Option<String>,
    #[serde(default)]
    pub(crate) uploader: Option<String>,
    #[serde(default)]
    pub(crate) filename: Option<String>,
    #[serde(default)]
    pub(crate) thumbnail: Option<String>,
    #[serde(default)]
    pub(crate) upload_date: Option<String>,
    #[serde(default)]
    pub(crate) timestamp: Option<i64>,
    #[serde(default)]
    pub(crate) duration_seconds: Option<i64>,
    #[serde(default)]
    pub(crate) file_size_bytes: Option<i64>,
    #[serde(default)]
    pub(crate) sha256: Option<String>,
    #[serde(default)]
    pub(crate) medium: Option<String>,
    #[serde(default)]
    pub(crate) source: Option<String>,
    #[serde(default)]
    pub(crate) platform: Option<String>,
    #[serde(default)]
    pub(crate) output_path: Option<String>,
    #[serde(default)]
    pub(crate) pinefetch_version: Option<String>,
    pub(crate) created_at: u64,
    #[serde(default)]
    pub(crate) completed_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HistoryPage {
    pub(crate) entries: Vec<HistoryEntry>,
    pub(crate) has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HistoryDetails {
    pub(crate) entry: HistoryEntry,
    pub(crate) output_file_available: bool,
    pub(crate) file_extension: Option<String>,
    pub(crate) transcript: Option<HistoryTranscriptSummary>,
    pub(crate) captions: Vec<HistoryCaptionSummary>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HistoryTranscriptSummary {
    pub(crate) transcription_type: String,
    pub(crate) language: Option<String>,
    pub(crate) file_available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HistoryCaptionSummary {
    pub(crate) media_path: String,
    pub(crate) caption_path: String,
    pub(crate) format: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) created_at: u64,
    pub(crate) file_available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HistoryTranscriptContent {
    pub(crate) text: String,
    pub(crate) transcription_type: String,
    pub(crate) language: Option<String>,
    pub(crate) file_available: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HistoryCaptionContent {
    pub(crate) media_path: String,
    pub(crate) caption_path: String,
    pub(crate) format: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) created_at: u64,
    pub(crate) file_available: bool,
    pub(crate) text: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HistoryStats {
    pub(crate) video_count: u64,
    pub(crate) total_duration_seconds: u64,
    pub(crate) total_file_size_bytes: u64,
    pub(crate) source_counts: Vec<HistorySourceCount>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct HistorySourceCount {
    pub(crate) source: String,
    pub(crate) count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InstalledYtDlpVersion {
    pub(crate) version: String,
    pub(crate) path: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct QueueStatus {
    pub(crate) auto_start: bool,
    pub(crate) worker_running: bool,
    pub(crate) paused: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct TxtImportFile {
    pub(crate) path: String,
    pub(crate) content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct LinkDumpSettings {
    pub(crate) server_enabled: bool,
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LinkDumpSecretView {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) created_at: String,
    pub(crate) last_used_at: Option<String>,
    pub(crate) revoked_at: Option<String>,
    pub(crate) deleted_at: Option<String>,
    pub(crate) status: String,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct GeneratedLinkDumpSecret {
    pub(crate) secret: String,
    pub(crate) connection: LinkDumpSecretView,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LinkDumpServerStatus {
    pub(crate) status: String,
    pub(crate) url: String,
    pub(crate) error_message: Option<String>,
}

impl Default for LinkDumpServerStatus {
    fn default() -> Self {
        Self {
            status: "stopped".to_string(),
            url: format!(
                "http://{}:{}",
                LINK_DUMP_DEFAULT_HOST, LINK_DUMP_DEFAULT_PORT
            ),
            error_message: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LinkDumpOverview {
    pub(crate) settings: LinkDumpSettings,
    pub(crate) secrets: Vec<LinkDumpSecretView>,
    pub(crate) server_status: LinkDumpServerStatus,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LinkDumpSettingsPatch {
    pub(crate) server_enabled: Option<bool>,
    pub(crate) host: Option<String>,
    pub(crate) port: Option<u16>,
}

#[derive(Debug, Clone)]
pub(crate) struct ValidSecretResult {
    pub(crate) id: String,
    #[allow(dead_code)]
    pub(crate) name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct NormalizedVideoUrl {
    pub(crate) url: String,
    pub(crate) key: String,
    pub(crate) thumbnail: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct LinkDumpQueueSummary {
    pub(crate) received: usize,
    pub(crate) added: usize,
    pub(crate) skipped: usize,
    pub(crate) invalid: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddVideoLinkRequestBody {
    pub(crate) url: Option<String>,
    pub(crate) secret: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct AddVideoLinksRequestBody {
    pub(crate) urls: Option<Vec<String>>,
    pub(crate) secret: Option<String>,
}

#[derive(Debug)]
pub(crate) struct HttpRequest {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) body: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(crate) struct DownloadRunResult {
    pub(crate) exit_code: i32,
    pub(crate) output_path: Option<String>,
    pub(crate) error: Option<String>,
    pub(crate) info: Option<InfoResponse>,
    pub(crate) captions: Vec<SavedCaption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TranscriptionRunResult {
    pub(crate) transcript_path: String,
    pub(crate) language: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedCaption {
    pub(crate) media_path: String,
    pub(crate) caption_path: String,
    pub(crate) text: String,
}

pub(crate) const LINK_DUMP_DEFAULT_HOST: &str = "127.0.0.1";
pub(crate) const LINK_DUMP_DEFAULT_PORT: u16 = 2255;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DownloadState {
    Queued,
    Downloading,
    Transcribing,
    Cancelling,
    Cancelled,
    Success,
    Error,
}
impl DownloadStateEvent {
    pub(crate) fn apply_result(&mut self, result: Result<(), String>) {
        if let Err(error) = result {
            self.state = DownloadState::Error;
            self.error = Some(error);
        }
    }
}
