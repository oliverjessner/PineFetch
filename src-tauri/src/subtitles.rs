//! Creator subtitle snapshots. Only yt-dlp-reported tracks beside the media
//! are read; the UI can retrieve stored text only through a history identity.
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::path::Path;

pub(crate) const SCHEMA_SQL: &str = "CREATE TABLE history_subtitles (
    history_entry_id TEXT NOT NULL REFERENCES history_entries(id) ON DELETE CASCADE,
    path TEXT NOT NULL,
    language TEXT NOT NULL,
    format TEXT NOT NULL,
    text TEXT NOT NULL,
    source_start_seconds REAL NOT NULL DEFAULT 0,
    PRIMARY KEY(history_entry_id, path)
);";
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct SavedSubtitle {
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) format: String,
    pub(crate) text: String,
    pub(crate) source_start_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SubtitleSummary {
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) format: String,
    pub(crate) file_available: bool,
    pub(crate) source_start_seconds: f64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SubtitleContent {
    #[serde(flatten)]
    pub(crate) summary: SubtitleSummary,
    pub(crate) text: String,
}

pub(crate) fn read_text(path: &Path) -> Result<String, String> {
    if !fs::symlink_metadata(path)
        .map_err(|e| e.to_string())?
        .file_type()
        .is_file()
    {
        return Err("Subtitle is not a regular file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|e| e.to_string())?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err("Subtitle file is unsupported or too large".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Subtitle file is too large".into());
    }
    String::from_utf8(bytes).map_err(|_| "Subtitle is not UTF-8 text".into())
}

pub(crate) fn collect(
    line: &str,
    media: &Path,
    source_start_seconds: f64,
) -> Result<Vec<SavedSubtitle>, String> {
    let value: Value = serde_json::from_str(
        line.strip_prefix("pinefetch_subtitles:")
            .ok_or("Invalid subtitle metadata")?,
    )
    .map_err(|_| "Invalid subtitle metadata")?;
    if value["filepath"].as_str() != media.to_str() {
        return Ok(Vec::new());
    }
    let Some(tracks) = value["requested_subtitles"].as_object() else {
        return Ok(Vec::new());
    };
    if tracks.len() > 256 {
        return Err("Too many subtitle tracks".into());
    }
    let mut result = Vec::new();
    let mut total_bytes = 0;
    for (language, track) in tracks {
        if language == "live_chat"
            || language.is_empty()
            || language.len() > 64
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            continue;
        }
        let Some(format) = track["ext"].as_str().filter(|ext| {
            matches!(
                *ext,
                "vtt"
                    | "srt"
                    | "ass"
                    | "ssa"
                    | "ttml"
                    | "dfxp"
                    | "srv1"
                    | "srv2"
                    | "srv3"
                    | "json3"
                    | "lrc"
                    | "txt"
                    | "smi"
                    | "sami"
                    | "scc"
            )
        }) else {
            continue;
        };
        // Derive the expected sibling name rather than trusting a path from
        // the remote extractor. Audio conversion retains the same stem.
        let path = media.with_extension(format!("{language}.{format}"));
        if !path.try_exists().map_err(|e| e.to_string())? {
            continue;
        }
        let text = read_text(&path)?;
        total_bytes += text.len();
        if total_bytes > MAX_TOTAL_BYTES {
            return Err("Subtitle tracks exceed the storage limit".into());
        }
        result.push(SavedSubtitle {
            path: path.to_string_lossy().into_owned(),
            language: language.clone(),
            format: format.into(),
            text,
            source_start_seconds,
        });
    }
    Ok(result)
}

pub(crate) fn insert(conn: &Connection, id: &str, tracks: &[SavedSubtitle]) -> Result<(), String> {
    for track in tracks {
        conn.execute(
            "INSERT INTO history_subtitles(history_entry_id,path,language,format,text,source_start_seconds)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![id, track.path, track.language, track.format, track.text, track.source_start_seconds],
        ).map_err(|e| format!("Subtitle persistence failed: {e}"))?;
    }
    Ok(())
}

fn map_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<SubtitleSummary> {
    let path: String = row.get(0)?;
    Ok(SubtitleSummary {
        file_available: Path::new(&path).is_file(),
        path,
        language: row.get(1)?,
        format: row.get(2)?,
        source_start_seconds: row.get(3)?,
    })
}

pub(crate) fn summaries(conn: &Connection, id: &str) -> Result<Vec<SubtitleSummary>, String> {
    let mut query = conn.prepare(
        "SELECT path,language,format,source_start_seconds FROM history_subtitles WHERE history_entry_id=?1 ORDER BY language,path",
    ).map_err(|e| e.to_string())?;
    let rows = query
        .query_map([id], map_summary)
        .map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|e| e.to_string())
}

pub(crate) fn content(
    conn: &Connection,
    id: &str,
    path: &str,
) -> Result<Option<SubtitleContent>, String> {
    conn.query_row(
        "SELECT path,language,format,source_start_seconds,text FROM history_subtitles WHERE history_entry_id=?1 AND path=?2",
        params![id, path],
        |row| Ok(SubtitleContent { summary: map_summary(row)?, text: row.get(4)? }),
    ).optional().map_err(|e| format!("Subtitle read failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::completion::{complete_files_with_hook, CompletionFiles, CompletionPoint};
    use crate::download_rules::{prepare_download_job, request_from_preset, DownloadOptions};
    use crate::models::AppConfig;
    use crate::presets::download_preset_for_key;
    use crate::test_support::TempRoot;
    use serde_json::json;

    fn metadata(media: &Path) -> String {
        format!(
            "pinefetch_subtitles:{}",
            json!({"filepath": media, "requested_subtitles": {
                "en": {"ext":"vtt", "filepath":"/untrusted/ignored"},
                "de": {"ext":"srt"}, "live_chat":{"ext":"json3"}, "../../escape":{"ext":"txt"}
            }})
        )
    }

    #[test]
    fn integration_creator_tracks_are_bounded_siblings_and_preserve_original_text() {
        let root = TempRoot::new("subtitle-collection");
        let media = root.path("clip.opus");
        fs::write(&media, b"media").unwrap();
        fs::write(
            root.path("clip.en.vtt"),
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\nCreator <b>term</b> 🌲\n",
        )
        .unwrap();
        fs::write(
            root.path("clip.de.srt"),
            "1\n00:00:01,000 --> 00:00:02,000\nGrüße\n",
        )
        .unwrap();
        fs::write(root.path("clip.live_chat.json3"), "not subtitles").unwrap();
        let tracks = collect(&metadata(&media), &media, 61.0).unwrap();
        assert_eq!(tracks.len(), 2);
        assert!(tracks
            .iter()
            .any(|track| track.text.contains("<b>term</b> 🌲")));
        assert!(tracks
            .iter()
            .all(|track| track.source_start_seconds == 61.0));
        assert!(collect(&metadata(&root.path("another.opus")), &media, 0.0)
            .unwrap()
            .is_empty());
        assert!(collect("pinefetch_subtitles:{bad", &media, 0.0).is_err());
        assert!(collect(
            &format!(
                "pinefetch_subtitles:{}",
                json!({"filepath": media,"requested_subtitles":null})
            ),
            &media,
            0.0
        )
        .unwrap()
        .is_empty());
        #[cfg(unix)]
        {
            fs::remove_file(root.path("clip.en.vtt")).unwrap();
            std::os::unix::fs::symlink(root.path("clip.de.srt"), root.path("clip.en.vtt")).unwrap();
            assert!(collect(&metadata(&media), &media, 0.0).is_err());
        }
        assert!(read_text(root.root()).is_err());
        let oversized = root.path("oversized.vtt");
        fs::File::create(&oversized)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(read_text(&oversized).is_err());
    }

    #[test]
    fn integration_subtitle_completion_is_atomic_persistent_identity_scoped_and_cascades() {
        let root = TempRoot::new("subtitle-completion");
        let data = root.path("data");
        let config_dir = root.path("config");
        let state = crate::startup::load_from_dirs(&data, &config_dir).unwrap();
        let media = root.path("clip.mp4");
        fs::write(&media, b"synthetic media").unwrap();
        fs::write(
            root.path("clip.en.vtt"),
            "WEBVTT\n\n00:00:00.000 --> 00:00:01.000\nOriginal\n",
        )
        .unwrap();
        let tracks = collect(&metadata(&media), &media, 0.0).unwrap();
        let job = prepare_download_job(
            request_from_preset(
                download_preset_for_key(Some("best")),
                "https://youtube.com/watch?v=synthetic".into(),
                false,
            ),
            DownloadOptions::from(&AppConfig::default()),
            root.root().to_string_lossy().into_owned(),
            "subtitles".into(),
        );
        crate::completion::begin(&state.db, &job).unwrap();
        let files = || CompletionFiles {
            path: media.to_str(),
            info: None,
            captions: &[],
            subtitles: &tracks,
            language: None,
        };
        assert!(complete_files_with_hook(&state.db, &job, files(), |point| {
            if point == CompletionPoint::Subtitles {
                Err("synthetic rollback".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert!(summaries(&state.db.lock().unwrap(), &job.id)
            .unwrap()
            .is_empty());
        assert!(
            crate::history::get_history_details_from_db(&state.db, &job.id)
                .unwrap()
                .is_none()
        );
        complete_files_with_hook(&state.db, &job, files(), |_| Ok(())).unwrap();
        complete_files_with_hook(&state.db, &job, files(), |_| Ok(())).unwrap();
        fs::remove_file(&tracks[0].path).unwrap();
        drop(state);
        let reopened = crate::startup::load_from_dirs(&data, &config_dir).unwrap();
        let details = crate::history::get_history_details_from_db(&reopened.db, &job.id)
            .unwrap()
            .unwrap();
        assert_eq!(details.subtitles.len(), 1);
        assert!(!details.subtitles[0].file_available);
        let conn = reopened.db.lock().unwrap();
        assert!(content(&conn, "other-entry", &tracks[0].path)
            .unwrap()
            .is_none());
        assert_eq!(
            content(&conn, &job.id, &tracks[0].path)
                .unwrap()
                .unwrap()
                .text,
            tracks[0].text
        );
        conn.execute("DELETE FROM history_entries WHERE id=?1", [&job.id])
            .unwrap();
        assert!(summaries(&conn, &job.id).unwrap().is_empty());
    }

    #[test]
    fn integration_schema_seven_subtitles_upgrade_rolls_back_preserves_cookie_settings_and_retries()
    {
        let conn = Connection::open_in_memory().unwrap();
        crate::database::initialize(&conn).unwrap();
        conn.execute_batch("DROP TABLE history_subtitles; ALTER TABLE app_config DROP COLUMN save_creator_subtitles; PRAGMA user_version=7; UPDATE app_config SET youtube_cookie_browser='chrome',save_captions=1;").unwrap();
        assert!(crate::database::initialize_with_hook(&conn, |point| {
            if point == crate::database::MigrationPoint::CreatorSubtitles {
                Err("synthetic failure".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            7
        );
        crate::database::initialize(&conn).unwrap();
        let config = crate::config::load_config_from_db(&conn).unwrap();
        assert_eq!(config.youtube_cookie_browser.as_deref(), Some("chrome"));
        assert!(config.save_captions);
        assert!(!config.save_creator_subtitles);
        crate::database::initialize(&conn).unwrap();
    }
}
