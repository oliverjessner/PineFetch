//! Local transcription of an existing History video, independent of the queue.
use crate::database::{self, Database};
use crate::files::OwnedTemporaryFile;
use crate::media_tools::{
    build_transcription_args, build_transcription_audio_args, parse_transcription_line,
    TranscriptionLine,
};
use crate::ocr::{fingerprint, Fingerprint};
use crate::process::{self, ProcessState};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const NO_VIDEO: &str =
    "The saved video is missing or unreadable. Restore it to transcribe this video.";
const EXISTING_TRANSCRIPT: &str = "This video already has a saved transcript.";
const BUSY: &str = "Another video is being transcribed. Wait for it to finish or cancel it.";
const CANCELLED: &str = "Transcription was cancelled.";
const SOURCE_CHANGED: &str =
    "The saved video changed during transcription. Reopen the video and try again.";
const TRANSCRIPTION_FAILED: &str =
    "Transcription failed. See Settings → Terminal Log for details and try again.";
const SAVE_FAILED: &str =
    "The transcript could not be saved. Your saved video is unchanged. Try again.";
const MAX_TRANSCRIPT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct TranscriptionProgress {
    pub(crate) history_id: String,
    pub(crate) attempt_id: String,
    pub(crate) attempt_at: u64,
    pub(crate) revision: u64,
    pub(crate) status: String,
    pub(crate) phase: String,
    pub(crate) model: String,
    pub(crate) message: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct TranscriptionStatus {
    pub(crate) history_id: String,
    pub(crate) transcript_available: bool,
    pub(crate) can_transcribe: bool,
    pub(crate) unavailable_reason: Option<String>,
    pub(crate) progress: Option<TranscriptionProgress>,
}

struct ActiveTranscription {
    progress: TranscriptionProgress,
    processes: Arc<ProcessState>,
}

#[derive(Default)]
struct Registry {
    active: Option<ActiveTranscription>,
    // Terminal states survive tab/video changes. SQLite is the durable source
    // of truth for completed transcripts; transient errors need no migration.
    finished: HashMap<String, TranscriptionProgress>,
}

#[derive(Default)]
pub(crate) struct HistoryTranscriptionState {
    registry: Mutex<Registry>,
    worker: Mutex<Option<JoinHandle<()>>>,
    stopping: AtomicBool,
}

impl HistoryTranscriptionState {
    pub(crate) fn cancel(
        &self,
        id: &str,
        attempt_id: &str,
    ) -> Result<TranscriptionProgress, String> {
        let mut registry = self.registry.lock().map_err(|_| TRANSCRIPTION_FAILED)?;
        let active = registry
            .active
            .as_mut()
            .filter(|active| {
                active.progress.history_id == id && active.progress.attempt_id == attempt_id
            })
            .ok_or("This transcription is no longer running.")?;
        // The same lock protects publication, so accepted cancellation cannot
        // race a transcript INSERT or produce a subsequent completed event.
        active.progress.status = "cancelled".into();
        active.progress.message = Some(CANCELLED.into());
        active.progress.revision += 1;
        stop_processes(&active.processes);
        Ok(active.progress.clone())
    }

    pub(crate) fn stop(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Ok(registry) = self.registry.lock() {
            if let Some(active) = registry.active.as_ref() {
                stop_processes(&active.processes);
            }
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                let _ = worker.join();
            }
        }
    }
}

fn stop_processes(processes: &ProcessState) {
    processes.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(child) = processes.current_child.lock() {
        if let Some(child) = child.as_ref() {
            if let Ok(mut child) = child.lock() {
                let _ = process::terminate_child_process_tree(&mut child);
            }
        }
    }
}

fn transcript_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM history_entries WHERE id=?1)",
            [id],
            |row| row.get(0),
        )
        .map_err(|_| SAVE_FAILED)?;
    if !exists {
        return Err("This history entry no longer exists.".into());
    }
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM transcriptions WHERE history_entry_id=?1)",
        [id],
        |row| row.get(0),
    )
    .map_err(|_| SAVE_FAILED.into())
}

fn source(db: &Database, id: &str) -> Result<PathBuf, String> {
    crate::history::local_history_media_path(db, id).map_err(|error| {
        if error.starts_with("Several downloaded videos") {
            "Several local videos match this entry. Open the original video's History entry to transcribe it."
        } else if error.contains("supported video extension") {
            "This History entry has no saved video file that can be transcribed."
        } else if error.contains("unambiguous absolute path") {
            "The saved video path is invalid. Restore the original video location and try again."
        } else {
            NO_VIDEO
        }
        .into()
    })
}

pub(crate) fn status(
    db: &Database,
    state: &HistoryTranscriptionState,
    id: &str,
) -> Result<TranscriptionStatus, String> {
    // Match the publication lock order so progress and transcript availability
    // describe the same completion boundary even during a navigation refresh.
    let registry = state.registry.lock().map_err(|_| TRANSCRIPTION_FAILED)?;
    let transcript_available = {
        let conn = db.lock().map_err(|_| SAVE_FAILED)?;
        transcript_exists(&conn, id)?
    };
    let unavailable_reason = if transcript_available {
        Some(EXISTING_TRANSCRIPT.to_owned())
    } else {
        source(db, id).err()
    };
    let progress = registry
        .active
        .as_ref()
        .filter(|active| active.progress.history_id == id)
        .map(|active| active.progress.clone())
        .or_else(|| registry.finished.get(id).cloned());
    let unavailable_reason = unavailable_reason.or_else(|| {
        registry
            .active
            .as_ref()
            .filter(|active| active.progress.history_id != id)
            .map(|_| BUSY.to_owned())
    });
    Ok(TranscriptionStatus {
        history_id: id.to_owned(),
        transcript_available,
        can_transcribe: unavailable_reason.is_none(),
        unavailable_reason,
        progress,
    })
}

pub(crate) struct TranscriptionTools {
    pub(crate) python: PathBuf,
    pub(crate) ffmpeg: PathBuf,
}

pub(crate) fn start(
    db: Arc<Database>,
    state: Arc<HistoryTranscriptionState>,
    id: String,
    tools: TranscriptionTools,
    model: String,
    emit: impl Fn(TranscriptionProgress) + Send + Sync + 'static,
    log: impl Fn(String, bool) + Send + Sync + 'static,
) -> Result<TranscriptionProgress, String> {
    let mut registry = state.registry.lock().map_err(|_| TRANSCRIPTION_FAILED)?;
    if state.stopping.load(Ordering::SeqCst) {
        return Err("Application is shutting down.".into());
    }
    if let Some(active) = registry.active.as_ref() {
        if active.progress.history_id == id && active.progress.status == "running" {
            return Ok(active.progress.clone());
        }
        return Err(BUSY.into());
    }
    {
        let conn = db.lock().map_err(|_| SAVE_FAILED)?;
        if transcript_exists(&conn, &id)? {
            return Err(EXISTING_TRANSCRIPT.into());
        }
    }
    let path = source(&db, &id)?;
    let identity = fingerprint(&path).map_err(|_| NO_VIDEO)?;
    let progress = TranscriptionProgress {
        history_id: id,
        attempt_id: uuid::Uuid::new_v4().to_string(),
        attempt_at: crate::files::current_timestamp_millis(),
        revision: 0,
        status: "running".into(),
        phase: "preparing".into(),
        model: crate::config_rules::normalize_faster_whisper_model(&model),
        message: None,
    };
    let processes = Arc::new(ProcessState::default());
    registry.active = Some(ActiveTranscription {
        progress: progress.clone(),
        processes: processes.clone(),
    });
    let worker_state = state.clone();
    let worker_progress = progress.clone();
    let worker = thread::Builder::new()
        .name("pinefetch-history-transcription".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                emit(worker_progress.clone());
                transcribe(
                    &worker_state,
                    &worker_progress,
                    &processes,
                    &path,
                    &tools,
                    &emit,
                    &log,
                )
            }))
            .unwrap_or_else(|_| {
                stop_processes(&processes);
                Err(TRANSCRIPTION_FAILED.into())
            });
            let final_event = finish(
                &db,
                &worker_state,
                &worker_progress.attempt_id,
                &path,
                &identity,
                result,
            );
            if let Some(event) = final_event {
                emit(event);
            }
        });
    match worker {
        Ok(worker) => {
            *state.worker.lock().map_err(|_| TRANSCRIPTION_FAILED)? = Some(worker);
        }
        Err(_) => {
            registry.active = None;
            return Err("Local transcription could not start. Try again.".into());
        }
    }
    Ok(progress)
}

fn update_phase(
    state: &HistoryTranscriptionState,
    attempt_id: &str,
    phase: &str,
    emit: &impl Fn(TranscriptionProgress),
) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|_| TRANSCRIPTION_FAILED)?;
    let active = registry
        .active
        .as_mut()
        .filter(|active| {
            active.progress.attempt_id == attempt_id && active.progress.status == "running"
        })
        .ok_or(CANCELLED)?;
    if active.processes.shutting_down.load(Ordering::SeqCst) {
        return Err(CANCELLED.into());
    }
    active.progress.phase = phase.into();
    active.progress.revision += 1;
    let progress = active.progress.clone();
    drop(registry);
    emit(progress);
    Ok(())
}

struct Transcript {
    text: String,
    language: String,
}

struct TranscriptionTemporaryDirectory(PathBuf);

impl TranscriptionTemporaryDirectory {
    fn create(attempt: &str) -> Result<Self, String> {
        let path = std::env::temp_dir().join(format!("pinefetch-transcription-{attempt}"));
        #[cfg(unix)]
        let mut builder = fs::DirBuilder::new();
        #[cfg(not(unix))]
        let builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|_| "Temporary storage is unavailable. Free some disk space and try again.")?;
        Ok(Self(path))
    }
}

impl Drop for TranscriptionTemporaryDirectory {
    fn drop(&mut self) {
        // This private job directory also cleans up on Windows, where an owned
        // temporary file cannot be removed until its open handle is dropped.
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TranscriptionPanicGuard<'a>(&'a ProcessState);

impl Drop for TranscriptionPanicGuard<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            // Reap the child before temporary handles/directories unwind, also
            // releasing Windows file locks if a logging callback panics.
            stop_processes(self.0);
            if let Ok(mut slot) = self.0.current_child.lock() {
                if let Some(child) = slot.take() {
                    if let Ok(mut child) = child.lock() {
                        let _ = child.wait();
                    }
                }
            }
        }
    }
}

fn transcribe(
    state: &HistoryTranscriptionState,
    progress: &TranscriptionProgress,
    processes: &ProcessState,
    path: &Path,
    tools: &TranscriptionTools,
    emit: &impl Fn(TranscriptionProgress),
    log: &impl Fn(String, bool),
) -> Result<Transcript, String> {
    crate::runtime::ensure_whisper_platform()?;
    update_phase(state, &progress.attempt_id, "preparing", emit)?;
    // Drop order closes both file handles before removing the job directory.
    let temporary = TranscriptionTemporaryDirectory::create(&progress.attempt_id)?;
    let audio = OwnedTemporaryFile::create_at(temporary.0.join("audio.wav"))
        .map_err(|_| "Temporary storage is unavailable. Free some disk space and try again.")?;
    let output = OwnedTemporaryFile::create_at(temporary.0.join("transcript.txt"))
        .map_err(|_| "Temporary storage is unavailable. Free some disk space and try again.")?;
    let _panic_guard = TranscriptionPanicGuard(processes);
    let input = path.to_str().ok_or(NO_VIDEO)?;
    let audio_path = audio.path.to_str().ok_or(NO_VIDEO)?;
    let output_path = output.path.to_str().ok_or(NO_VIDEO)?;
    let mut command = Command::new(&tools.ffmpeg);
    command.arg("-nostdin");
    command.args(build_transcription_audio_args(input, audio_path));
    let extracted = process::run_command_output(
        command,
        Some(processes),
        None,
        Some(Duration::from_secs(30 * 60)),
    )
    .map_err(|error| {
        log(format!("[transcription] FFmpeg: {error}"), true);
        "The saved video could not be decoded for transcription. Check the Terminal Log.".to_owned()
    })?;
    if !extracted.status.success() {
        log(
            format!(
                "[transcription] FFmpeg: {}",
                String::from_utf8_lossy(&extracted.stderr)
                    .chars()
                    .take(4096)
                    .collect::<String>()
            ),
            true,
        );
        return Err(
            "The saved video could not be decoded for transcription. Check the Terminal Log."
                .into(),
        );
    }
    update_phase(state, &progress.attempt_id, "transcribing", emit)?;
    log(
        format!("[transcription] faster-whisper model: {}", progress.model),
        false,
    );
    let mut command = Command::new(&tools.python);
    command.args(build_transcription_args(
        &audio.path,
        output_path,
        &progress.model,
        true,
    ));
    let mut language = None;
    let status = process::run_command_streaming(
        command,
        processes,
        |line| {
            if let TranscriptionLine::Language(detected) = parse_transcription_line(line) {
                if language.is_none() {
                    language = detected;
                }
            }
        },
        |line| {
            log(
                format!(
                    "[transcription] faster-whisper: {}",
                    line.chars().take(4096).collect::<String>()
                ),
                true,
            );
        },
    )
    .map_err(|error| {
        log(format!("[transcription] faster-whisper: {error}"), true);
        TRANSCRIPTION_FAILED.to_owned()
    })?;
    if !status.success() {
        return Err(TRANSCRIPTION_FAILED.into());
    }
    update_phase(state, &progress.attempt_id, "saving", emit)?;
    if fs::metadata(&output.path)
        .map_err(|_| TRANSCRIPTION_FAILED)?
        .len()
        > MAX_TRANSCRIPT_BYTES
    {
        return Err("The generated transcript exceeds the 16 MiB limit. It was not saved.".into());
    }
    let text = fs::read_to_string(&output.path).map_err(|_| TRANSCRIPTION_FAILED)?;
    if text.trim().is_empty() {
        return Err("No speech was detected. No transcript was saved.".into());
    }
    let language = language.ok_or(TRANSCRIPTION_FAILED)?;
    Ok(Transcript { text, language })
}

fn finish(
    db: &Database,
    state: &HistoryTranscriptionState,
    attempt_id: &str,
    source: &Path,
    identity: &Fingerprint,
    result: Result<Transcript, String>,
) -> Option<TranscriptionProgress> {
    let mut registry = state.registry.lock().ok()?;
    let active = registry
        .active
        .as_mut()
        .filter(|active| active.progress.attempt_id == attempt_id)?;
    let result = if active.processes.shutting_down.load(Ordering::SeqCst) {
        Err(
            if active.progress.status == "cancelled" || state.stopping.load(Ordering::SeqCst) {
                CANCELLED.into()
            } else {
                result.err().unwrap_or_else(|| TRANSCRIPTION_FAILED.into())
            },
        )
    } else {
        result.and_then(|transcript| {
            save(
                db,
                &active.progress.history_id,
                source,
                identity,
                &transcript.text,
                &transcript.language,
            )
        })
    };
    match result {
        Ok(()) => {
            active.progress.status = "completed".into();
            active.progress.phase = "completed".into();
            active.progress.message = None;
        }
        Err(error) => {
            if active.progress.status != "cancelled" {
                active.progress.status = if error == CANCELLED {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
            }
            active.progress.message = Some(error);
        }
    }
    active.progress.revision += 1;
    let progress = active.progress.clone();
    registry.active = None;
    // Bound only transient UI state, never stored transcripts or AI records.
    if registry.finished.len() >= 64 && !registry.finished.contains_key(&progress.history_id) {
        if let Some(oldest) = registry
            .finished
            .values()
            .min_by_key(|progress| progress.attempt_at)
            .map(|progress| progress.history_id.clone())
        {
            registry.finished.remove(&oldest);
        }
    }
    registry
        .finished
        .insert(progress.history_id.clone(), progress.clone());
    Some(progress)
}

pub(crate) fn save(
    db: &Database,
    id: &str,
    source: &Path,
    identity: &Fingerprint,
    text: &str,
    language: &str,
) -> Result<(), String> {
    if text.trim().is_empty() || text.len() as u64 > MAX_TRANSCRIPT_BYTES {
        return Err("No usable transcript was generated. No transcript was saved.".into());
    }
    let conn = db.lock().map_err(|_| SAVE_FAILED)?;
    let tx = database::write_transaction(&conn).map_err(|_| SAVE_FAILED)?;
    if transcript_exists(&tx, id)? {
        return Err(EXISTING_TRANSCRIPT.into());
    }
    let current = crate::ocr_store::saved_path_in_conn(&tx, id)
        .and_then(|path| fs::canonicalize(path).map_err(|_| SOURCE_CHANGED.to_owned()))
        .map_err(|_| SOURCE_CHANGED)?;
    if current != source || fingerprint(source).map_err(|_| SOURCE_CHANGED)? != *identity {
        return Err(SOURCE_CHANGED.into());
    }
    let source = source.to_str().ok_or(SOURCE_CHANGED)?;
    crate::history::insert_transcription_in_conn(
        &tx,
        id,
        text,
        "text with timestamps",
        language,
        Some(source),
    )
    .map_err(|_| SAVE_FAILED)?;
    tx.commit().map_err(|_| SAVE_FAILED.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use rusqlite::params;

    fn fixture() -> (TempRoot, Arc<Database>, PathBuf) {
        let root = TempRoot::new("history-transcription");
        let video = root.path("Grüße #1.mp4");
        fs::write(&video, b"original saved video bytes").unwrap();
        let video = fs::canonicalize(video).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        database::initialize(&conn).unwrap();
        conn.execute(
            "INSERT INTO history_entries(id,url,title,uploader,filename,medium,output_path,sha256,file_size_bytes,duration_seconds,created_at)
             VALUES('video','https://example.com/video','Saved title','Creator','Grüße #1.mp4','video',?1,'original hash',26,123,1)",
            [video.to_str().unwrap()],
        ).unwrap();
        (root, Arc::new(Database::new(conn)), video)
    }

    fn progress() -> TranscriptionProgress {
        TranscriptionProgress {
            history_id: "video".into(),
            attempt_id: "attempt-one".into(),
            attempt_at: 1,
            revision: 0,
            status: "running".into(),
            phase: "transcribing".into(),
            model: "small".into(),
            message: None,
        }
    }

    fn active_state() -> Arc<HistoryTranscriptionState> {
        let state = Arc::new(HistoryTranscriptionState::default());
        state.registry.lock().unwrap().active = Some(ActiveTranscription {
            progress: progress(),
            processes: Arc::new(ProcessState::default()),
        });
        state
    }

    #[test]
    fn eligibility_checks_the_actual_local_video_and_existing_transcript() {
        let (_root, db, video) = fixture();
        let state = HistoryTranscriptionState::default();
        let available = status(&db, &state, "video").unwrap();
        assert!(available.can_transcribe);
        assert!(!available.transcript_available);
        fs::remove_file(&video).unwrap();
        let missing = status(&db, &state, "video").unwrap();
        assert!(!missing.can_transcribe);
        assert_eq!(missing.unavailable_reason.as_deref(), Some(NO_VIDEO));
        fs::write(&video, b"restored video").unwrap();
        assert!(status(&db, &state, "video").unwrap().can_transcribe);
        crate::history::insert_transcription_in_db(&db, "video", "", "text", "en").unwrap();
        let saved = status(&db, &state, "video").unwrap();
        assert!(!saved.can_transcribe);
        assert!(saved.transcript_available);
        assert_eq!(
            saved.unavailable_reason.as_deref(),
            Some(EXISTING_TRANSCRIPT)
        );
        assert!(status(&db, &state, "missing").is_err());
    }

    #[test]
    fn saving_adds_transcript_to_the_same_entry_and_preserves_media_metadata_and_ai() {
        let (_root, db, video) = fixture();
        db.lock().unwrap().execute_batch(
            "INSERT INTO history_ai_summaries(history_entry_id,transcript_hash,generation_id,quick_summary,key_takeaways_json,metadata_json,created_at,updated_at)
             VALUES('video','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','old-summary','Saved summary','[\"Saved takeaway\"]','{}',1,1);
             INSERT INTO history_ai_chat(id,history_entry_id,transcript_hash,sequence,question,answer,metadata_json,created_at)
             VALUES('old-chat','video','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,'Saved question','Saved answer','{}',1);",
        ).unwrap();
        let identity = fingerprint(&video).unwrap();
        save(
            &db,
            "video",
            &video,
            &identity,
            "[00:00:01 → 00:00:03] Hallo 🌲\n",
            " DE ",
        )
        .unwrap();
        let conn = db.lock().unwrap();
        let row: (String, String, String, String, String, i64, i64) = conn.query_row(
            "SELECT title,uploader,medium,output_path,sha256,file_size_bytes,duration_seconds FROM history_entries WHERE id='video'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?,row.get(6)?)),
        ).unwrap();
        assert_eq!(
            row,
            (
                "Saved title".into(),
                "Creator".into(),
                "video".into(),
                video.to_str().unwrap().into(),
                "original hash".into(),
                26,
                123
            )
        );
        let transcript: (String, String, String, String, bool) = conn.query_row(
            "SELECT text,type,language,media_path,media_path_known FROM transcriptions WHERE history_entry_id='video'",
            [],
            |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?)),
        ).unwrap();
        assert_eq!(
            transcript,
            (
                "[00:00:01 → 00:00:03] Hallo 🌲\n".into(),
                "text with timestamps".into(),
                "de".into(),
                video.to_str().unwrap().into(),
                true
            )
        );
        assert_eq!(
            conn.query_row(
                "SELECT answer FROM history_ai_chat WHERE id='old-chat'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "Saved answer"
        );
        assert_eq!(
            conn.query_row(
                "SELECT quick_summary FROM history_ai_summaries WHERE generation_id='old-summary'",
                [],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
            "Saved summary"
        );
        drop(conn);
        assert_eq!(
            crate::history::local_history_media_path(&db, "video").unwrap(),
            video
        );
        assert_eq!(fs::read(&video).unwrap(), b"original saved video bytes");
    }

    #[test]
    fn publication_rejects_changed_media_changed_source_and_deleted_history() {
        let (root, db, video) = fixture();
        let identity = fingerprint(&video).unwrap();
        fs::write(&video, b"changed video content with a different size").unwrap();
        assert_eq!(
            save(&db, "video", &video, &identity, "Transcript", "en").unwrap_err(),
            SOURCE_CHANGED
        );
        let identity = fingerprint(&video).unwrap();
        let other = root.path("other.mp4");
        fs::write(&other, b"other video").unwrap();
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_entries SET output_path=?1 WHERE id='video'",
                [other.to_str().unwrap()],
            )
            .unwrap();
        assert_eq!(
            save(&db, "video", &video, &identity, "Transcript", "en").unwrap_err(),
            SOURCE_CHANGED
        );
        db.lock()
            .unwrap()
            .execute("DELETE FROM history_entries WHERE id='video'", [])
            .unwrap();
        assert!(save(&db, "video", &video, &identity, "Transcript", "en")
            .unwrap_err()
            .contains("no longer exists"));
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn publication_never_overwrites_a_transcript_saved_by_another_writer() {
        let (_root, db, video) = fixture();
        let identity = fingerprint(&video).unwrap();
        crate::history::insert_transcription_in_db(&db, "video", "Already saved", "text", "fr")
            .unwrap();
        assert_eq!(
            save(&db, "video", &video, &identity, "New text", "en").unwrap_err(),
            EXISTING_TRANSCRIPT
        );
        let saved = crate::history::get_history_transcript_from_db(&db, "video")
            .unwrap()
            .unwrap();
        assert_eq!(saved.text, "Already saved");
        assert_eq!(saved.language.as_deref(), Some("fr"));
        assert!(save(&db, "video", &video, &identity, "  \n", "en").is_err());
    }

    #[test]
    fn accepted_cancellation_prevents_save_and_survives_view_changes() {
        let (_root, db, video) = fixture();
        let state = active_state();
        let identity = fingerprint(&video).unwrap();
        assert!(state.cancel("video", "different-attempt").is_err());
        assert_eq!(
            status(&db, &state, "video")
                .unwrap()
                .progress
                .unwrap()
                .status,
            "running"
        );
        let cancelled = state.cancel("video", "attempt-one").unwrap();
        assert_eq!(cancelled.status, "cancelled");
        assert_eq!(cancelled.revision, 1);
        let event = finish(
            &db,
            &state,
            "attempt-one",
            &video,
            &identity,
            Ok(Transcript {
                text: "Recognized speech".into(),
                language: "en".into(),
            }),
        )
        .unwrap();
        assert_eq!(event.status, "cancelled");
        assert!(event.revision > cancelled.revision);
        let read = status(&db, &state, "video").unwrap();
        assert!(!read.transcript_available);
        assert_eq!(read.progress, Some(event));
        assert!(read.can_transcribe);
    }

    #[test]
    fn duplicate_start_returns_the_same_attempt_without_starting_another_process() {
        let (_root, db, _video) = fixture();
        let state = active_state();
        let tools = || TranscriptionTools {
            python: PathBuf::from("must-not-start-python"),
            ffmpeg: PathBuf::from("must-not-start-ffmpeg"),
        };
        let existing = start(
            db.clone(),
            state.clone(),
            "video".into(),
            tools(),
            "large-v3".into(),
            |_| panic!("duplicate must not emit or start"),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(existing, progress());
        db.lock().unwrap().execute("INSERT INTO history_entries(id,url,created_at) VALUES('other','https://example.com/other',2)", []).unwrap();
        assert_eq!(
            start(
                db,
                state,
                "other".into(),
                tools(),
                "base".into(),
                |_| {},
                |_, _| {}
            )
            .unwrap_err(),
            BUSY
        );
    }

    #[test]
    fn ambiguous_and_non_video_sources_have_actionable_statuses() {
        let (root, db, video) = fixture();
        let txt = video.with_extension("txt");
        let other = video.with_extension("mkv");
        fs::write(&other, b"another matching video").unwrap();
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_entries SET output_path=?1,medium='transcript' WHERE id='video'",
                [txt.to_str().unwrap()],
            )
            .unwrap();
        let state = HistoryTranscriptionState::default();
        let ambiguous = status(&db, &state, "video").unwrap();
        assert!(!ambiguous.can_transcribe);
        assert!(ambiguous
            .unavailable_reason
            .unwrap()
            .starts_with("Several local videos"));
        let audio = root.path("audio.mp3");
        fs::write(&audio, b"audio").unwrap();
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_entries SET output_path=?1,medium='audio' WHERE id='video'",
                [audio.to_str().unwrap()],
            )
            .unwrap();
        assert_eq!(
            status(&db, &state, "video")
                .unwrap()
                .unavailable_reason
                .as_deref(),
            Some("This History entry has no saved video file that can be transcribed.")
        );
    }

    #[test]
    fn failed_attempts_remain_visible_and_can_be_retried_without_a_transcript() {
        let (_root, db, video) = fixture();
        let state = active_state();
        let event = finish(
            &db,
            &state,
            "attempt-one",
            &video,
            &fingerprint(&video).unwrap(),
            Err(TRANSCRIPTION_FAILED.into()),
        )
        .unwrap();
        assert_eq!(event.status, "failed");
        assert_eq!(event.message.as_deref(), Some(TRANSCRIPTION_FAILED));
        let read = status(&db, &state, "video").unwrap();
        assert!(read.can_transcribe);
        assert!(!read.transcript_available);
        assert_eq!(read.progress, Some(event));
    }

    #[test]
    fn save_rolls_back_when_language_is_missing() {
        let (_root, db, video) = fixture();
        assert_eq!(
            save(
                &db,
                "video",
                &video,
                &fingerprint(&video).unwrap(),
                "Recognized speech",
                " "
            )
            .unwrap_err(),
            SAVE_FAILED
        );
        let count = db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM transcriptions WHERE history_entry_id=?1",
                params!["video"],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn temporary_audio_and_text_are_cleaned_when_the_job_scope_exits() {
        let directory =
            TranscriptionTemporaryDirectory::create(&uuid::Uuid::new_v4().to_string()).unwrap();
        let path = directory.0.clone();
        {
            let mut audio = OwnedTemporaryFile::create_at(path.join("audio.wav")).unwrap();
            let mut text = OwnedTemporaryFile::create_at(path.join("transcript.txt")).unwrap();
            audio.write_all(b"private temporary audio").unwrap();
            text.write_all(b"private temporary transcript").unwrap();
        }
        drop(directory);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    fn fake_tool(root: &TempRoot, name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = root.path(name);
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    #[cfg(unix)]
    #[test]
    fn integration_local_pipeline_uses_configured_model_and_saves_detected_language() {
        if crate::runtime::ensure_whisper_platform().is_err() {
            return;
        }
        let (root, db, video) = fixture();
        let tools = TranscriptionTools {
            ffmpeg: fake_tool(&root, "ffmpeg", "for argument do output=\"$argument\"; done\nprintf 'decoded audio' > \"$output\""),
            python: fake_tool(&root, "python", "test \"$7\" = 'small'\ntest \"$8\" = '1'\nprintf 'pinefetch_language:de\\n'\nprintf '[00:00:01 → 00:00:03] Erkannter Text\\n' > \"$6\""),
        };
        let state = Arc::new(HistoryTranscriptionState::default());
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = start(
            db.clone(),
            state.clone(),
            "video".into(),
            tools,
            " small ".into(),
            move |event| {
                let _ = sender.send(event);
            },
            |_, _| {},
        )
        .unwrap();
        let mut phases = Vec::new();
        let terminal = loop {
            let event = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            phases.push(event.phase.clone());
            if event.status != "running" {
                break event;
            }
        };
        state.stop();
        assert_eq!(terminal.status, "completed", "{:?}", terminal.message);
        assert_eq!(terminal.attempt_id, started.attempt_id);
        assert!(phases.contains(&"transcribing".to_owned()));
        assert!(phases.contains(&"saving".to_owned()));
        let saved = crate::history::get_history_transcript_from_db(&db, "video")
            .unwrap()
            .unwrap();
        assert_eq!(saved.language.as_deref(), Some("de"));
        assert_eq!(saved.transcription_type, "text with timestamps");
        assert!(saved.text.contains("Erkannter Text"));
        assert_eq!(fs::read(&video).unwrap(), b"original saved video bytes");
        assert!(!std::env::temp_dir()
            .join(format!("pinefetch-transcription-{}", started.attempt_id))
            .exists());
    }

    #[cfg(unix)]
    #[test]
    fn integration_cancelled_local_pipeline_kills_its_child_and_cleans_temporary_files() {
        if crate::runtime::ensure_whisper_platform().is_err() {
            return;
        }
        let (root, db, video) = fixture();
        let tools = TranscriptionTools {
            ffmpeg: fake_tool(
                &root,
                "ffmpeg",
                "for argument do output=\"$argument\"; done\nprintf 'decoded audio' > \"$output\"",
            ),
            python: fake_tool(
                &root,
                "python",
                "printf 'pinefetch_language:en\\n'\nsleep 30\nprintf 'must not be saved' > \"$6\"",
            ),
        };
        let state = Arc::new(HistoryTranscriptionState::default());
        let (sender, receiver) = std::sync::mpsc::channel();
        let started = start(
            db.clone(),
            state.clone(),
            "video".into(),
            tools,
            "base".into(),
            move |event| {
                let _ = sender.send(event);
            },
            |_, _| {},
        )
        .unwrap();
        loop {
            let event = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(event.status, "running");
            if event.phase == "transcribing" {
                break;
            }
        }
        let processes = state
            .registry
            .lock()
            .unwrap()
            .active
            .as_ref()
            .unwrap()
            .processes
            .clone();
        state.cancel("video", &started.attempt_id).unwrap();
        let terminal = loop {
            let event = receiver.recv_timeout(Duration::from_secs(10)).unwrap();
            if event.status != "running" {
                break event;
            }
        };
        state.stop();
        assert_eq!(terminal.status, "cancelled");
        assert!(processes.current_child.lock().unwrap().is_none());
        assert!(crate::history::get_history_transcript_from_db(&db, "video")
            .unwrap()
            .is_none());
        assert_eq!(fs::read(&video).unwrap(), b"original saved video bytes");
        assert!(!std::env::temp_dir()
            .join(format!("pinefetch-transcription-{}", started.attempt_id))
            .exists());
    }
}
