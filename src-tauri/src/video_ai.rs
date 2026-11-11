//! Independent, cancellable transcript AI operations. No summary owns a conversation.
use crate::process::{self, ProcessState};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) enum Lane {
    Summary,
    Chat,
}

#[derive(Default)]
pub(crate) struct VideoAiState {
    requests: Mutex<HashMap<(String, Lane), ActiveRequest>>,
}

struct ActiveRequest {
    id: String,
    processes: Arc<ProcessState>,
}

pub(crate) struct Request {
    owner: Arc<VideoAiState>,
    history_id: String,
    id: String,
    lane: Lane,
    transcript_hash: String,
    pub(crate) processes: Arc<ProcessState>,
}

impl VideoAiState {
    pub(crate) fn begin(
        self: &Arc<Self>,
        history_id: &str,
        id: &str,
        lane: Lane,
        transcript_hash: &str,
    ) -> Result<Request, String> {
        if history_id.is_empty() || history_id.len() > 256 {
            return Err("A valid History entry is required.".into());
        }
        if transcript_hash.len() != 64
            || !transcript_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(
                "Reload AI Video Summary to select the current transcript revision.".into(),
            );
        }
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err("A valid AI request ID is required.".into());
        }
        let mut requests = self
            .requests
            .lock()
            .map_err(|_| "AI request lock poisoned")?;
        let key = (history_id.to_owned(), lane);
        if requests.contains_key(&key) {
            return Err(match lane {
                Lane::Summary => "Summary generation is already running for this video.",
                Lane::Chat => "An answer is already being generated for this video.",
            }
            .into());
        }
        if requests.values().any(|request| request.id == id) {
            return Err("This AI request ID is already in use.".into());
        }
        if requests.len() >= 8 {
            return Err(
                "Too many AI requests are running. Wait for an answer or cancel a request.".into(),
            );
        }
        let processes = Arc::new(ProcessState::default());
        requests.insert(
            key,
            ActiveRequest {
                id: id.into(),
                processes: Arc::clone(&processes),
            },
        );
        Ok(Request {
            owner: Arc::clone(self),
            history_id: history_id.into(),
            id: id.into(),
            lane,
            transcript_hash: transcript_hash.into(),
            processes,
        })
    }

    pub(crate) fn cancel(&self, history_id: &str, id: &str) -> Result<bool, String> {
        let requests = self
            .requests
            .lock()
            .map_err(|_| "AI request lock poisoned")?;
        let Some(request) = requests.iter().find_map(|((history, _), request)| {
            (history == history_id && request.id == id).then_some(request)
        }) else {
            return Ok(false);
        };
        stop(&request.processes);
        Ok(true)
    }

    pub(crate) fn stop(&self) {
        if let Ok(requests) = self.requests.lock() {
            for request in requests.values() {
                stop(&request.processes);
            }
        }
    }
}

fn stop(processes: &ProcessState) {
    processes.shutting_down.store(true, Ordering::SeqCst);
    if let Ok(child) = processes.current_child.lock() {
        if let Some(child) = child.as_ref() {
            if let Ok(mut child) = child.lock() {
                let _ = process::terminate_child_process_tree(&mut child);
            }
        }
    }
}

impl Request {
    fn verify_transcript(&self, hash: &str) -> Result<(), String> {
        if self.transcript_hash != hash {
            return Err(
                "The saved transcript changed. Reload AI Video Summary before asking or analyzing."
                    .into(),
            );
        }
        Ok(())
    }

    pub(crate) fn check_cancelled(&self) -> Result<(), String> {
        if self.processes.shutting_down.load(Ordering::SeqCst) {
            Err("AI request cancelled.".into())
        } else {
            Ok(())
        }
    }

    // The registry lock orders cancellation against the final database commit.
    // Transcript and chat revision checks remain in the guarded store transaction.
    pub(crate) fn commit<T>(&self, save: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let _requests = self
            .owner
            .requests
            .lock()
            .map_err(|_| "AI request lock poisoned")?;
        self.check_cancelled()?;
        save()
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.owner.requests.lock() {
            let key = (self.history_id.clone(), self.lane);
            if requests
                .get(&key)
                .is_some_and(|request| request.id == self.id)
            {
                requests.remove(&key);
            }
        }
    }
}

pub(crate) fn analyze(
    db: &crate::database::Database,
    processes: &ProcessState,
    request: &Request,
) -> Result<crate::video_ai_store::Summary, String> {
    request.check_cancelled()?;
    let transcript = crate::video_ai_store::transcript_for_summary(db, &request.history_id)?;
    request.verify_transcript(&transcript.transcript_hash)?;
    let generated = crate::codex::generate_summary(&transcript.text, processes, &request.processes)
        .map_err(|error| error.to_string())?;
    let content = crate::video_ai_store::SummaryContent {
        quick_summary: generated.result.quick_summary,
        key_takeaways: generated.result.key_takeaways,
    };
    let metadata = crate::video_ai_store::GenerationMetadata {
        provider: generated.provider,
        model: generated.model,
        cli_version: generated.cli_version,
        context_strategy: generated.context_strategy,
    };
    request.commit(|| {
        if processes.shutting_down.load(Ordering::SeqCst) {
            return Err("Application is shutting down.".into());
        }
        crate::video_ai_store::save_summary(db, &transcript, &content, &metadata)
    })
}

#[derive(serde::Serialize)]
pub(crate) struct ChatReply {
    messages: Vec<crate::video_ai_store::ChatMessage>,
}

pub(crate) fn ask(
    db: &crate::database::Database,
    processes: &ProcessState,
    request: &Request,
    question: &str,
) -> Result<ChatReply, String> {
    request.check_cancelled()?;
    let question = question.trim();
    if question.is_empty() || question.chars().count() > 4000 {
        return Err("Enter a question between 1 and 4,000 characters.".into());
    }
    let transcript = crate::video_ai_store::transcript(db, &request.history_id)?;
    request.verify_transcript(&transcript.transcript_hash)?;
    // Conversation is an input for references and follow-up questions, never evidence.
    // A summary is neither read nor required by this path.
    let conversation: Vec<_> = transcript
        .messages
        .chunks_exact(2)
        .map(|pair| crate::codex::ChatTurn {
            question: pair[0].content.clone(),
            answer: pair[1].content.clone(),
        })
        .collect();
    let generated = crate::codex::answer_question(
        &transcript.text,
        question,
        &conversation,
        processes,
        &request.processes,
    )
    .map_err(|error| error.to_string())?;
    let metadata = crate::video_ai_store::GenerationMetadata {
        provider: generated.provider,
        model: generated.model,
        cli_version: generated.cli_version,
        context_strategy: generated.context_strategy,
    };
    request.commit(|| {
        if processes.shutting_down.load(Ordering::SeqCst) {
            return Err("Application is shutting down.".into());
        }
        crate::video_ai_store::append_exchange(
            db,
            &transcript,
            question,
            &generated.result.answer,
            &metadata,
        )
        .map(|messages| ChatReply { messages })
    })
}

#[cfg(test)]
mod tests {
    use super::{Lane, VideoAiState};
    use std::sync::Arc;
    const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn chat_and_summary_have_independent_lifetimes_and_cancellation() {
        let state = Arc::new(VideoAiState::default());
        let chat = state.begin("one", "chat", Lane::Chat, HASH).unwrap();
        let summary = state.begin("one", "summary", Lane::Summary, HASH).unwrap();
        assert!(state
            .begin("one", "another-chat", Lane::Chat, HASH)
            .is_err());
        assert!(state
            .begin("one", "another-summary", Lane::Summary, HASH)
            .is_err());
        assert!(!state.cancel("two", "chat").unwrap());
        state.cancel("one", "summary").unwrap();
        assert!(summary.commit(|| Ok(())).is_err());
        assert!(chat.commit(|| Ok(())).is_ok());
        drop(summary);
        assert!(state
            .begin("one", "regenerate", Lane::Summary, HASH)
            .is_ok());
        drop(chat);
        assert!(state
            .begin("one", "next-question", Lane::Chat, HASH)
            .is_ok());
    }

    #[test]
    fn switching_videos_never_shares_an_operation_and_shutdown_prevents_commit() {
        let state = Arc::new(VideoAiState::default());
        let first = state.begin("one", "first", Lane::Chat, HASH).unwrap();
        let second = state.begin("two", "second", Lane::Chat, HASH).unwrap();
        state.cancel("one", "first").unwrap();
        assert!(first.check_cancelled().is_err());
        assert!(second.check_cancelled().is_ok());
        state.stop();
        assert!(second.commit(|| Ok(())).is_err());
        assert!(state.begin("three", "../unsafe", Lane::Chat, HASH).is_err());
    }

    #[test]
    fn changed_transcript_is_rejected_before_generation_can_start() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        crate::database::initialize(&connection).unwrap();
        connection.execute_batch(
            "INSERT INTO history_entries(id,url,created_at) VALUES('one','https://example.com/video',1);
             INSERT INTO transcriptions(id,history_entry_id,text,\"type\") VALUES('t','one','The saved source has changed.','text');",
        ).unwrap();
        let db = crate::database::Database::new(connection);
        let processes = crate::process::ProcessState::default();
        // Even if the revision guard regresses, this test cannot launch Codex.
        processes
            .shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let state = Arc::new(VideoAiState::default());
        let chat = state.begin("one", "chat", Lane::Chat, HASH).unwrap();
        let summary = state.begin("one", "summary", Lane::Summary, HASH).unwrap();
        assert!(super::analyze(&db, &processes, &summary)
            .unwrap_err()
            .contains("transcript changed"));
        assert!(super::ask(&db, &processes, &chat, "What changed?")
            .err()
            .unwrap()
            .contains("transcript changed"));
        let saved = crate::video_ai_store::snapshot(&db, "one").unwrap();
        assert!(saved.summary.is_none() && saved.messages.is_empty());
        assert!(processes.utility_children.lock().unwrap().is_empty());
        assert!(state
            .begin("two", "bad-hash", Lane::Summary, "old")
            .is_err());
    }
}
