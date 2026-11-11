# AI Video Summary and Ask this Video

Open a saved History item and select **AI Video Summary** in Video Details.
**Ask this Video** is available immediately when the item has a nonempty saved
transcript and Codex is installed, compatible and authenticated. A summary is
never required. Chat reads the saved transcript and that video's conversation;
it does not read summaries, OCR, descriptions or websites.

**Analyze Video** is optional. It generates Quick Summary and Key Takeaways.
Chat and analysis can run concurrently. Generating or regenerating a summary
does not remove messages or replace a chat error/loading state. Each operation
has its own cancellation and error handling. Leaving the video cancels its
pending requests; stale replies cannot populate a different video.

## Setup

Settings → Diagnostics → **Codex CLI** checks executable discovery,
`codex --version`, supported CLI options and `codex login status`. Use
**Check Again** after installing/updating Codex or signing in. PineFetch never
asks for an API key. The existing ChatGPT CLI login works; an existing API-key
login is identified separately. An unknown authentication method is not treated
as a verified login merely because the status command exits successfully.

Install Codex using the instructions in the
[official Codex documentation](https://learn.chatgpt.com/docs/cli), then run
`codex login` in a terminal. PineFetch checks common desktop installation paths
as well as PATH. Diagnostics show setup instructions for missing, incompatible
or unauthenticated installations. Authentication contents are never returned to
the frontend or logged.

AI requests send the transcript and question to the service used by Codex. This
is an optional online feature; saved summaries and conversations remain in the
local PineFetch database. Opening the tab only reads saved content and performs
diagnostics. Generation starts only from Analyze Video or sending a question.

## Storage and revisions

Schema **17** adds `history_ai_summaries` and `history_ai_chat`, each referencing
History directly with a cascading foreign key. Chat has no summary foreign key.
The existing transactional migration, layout checks and mandatory backup apply.
No transcript is duplicated in the new tables.

Both tables store the SHA-256 revision of the complete saved transcript, provider
and CLI/context metadata, and timestamps. Summary regeneration updates only the
summary row for that transcript. Each successful chat exchange is appended
atomically. Failed or cancelled requests do not save incomplete exchanges.

Reads return only records matching the current transcript. Previous revisions
are retained but hidden with an explanation. Writes check the transcript again
inside the writer transaction. Chat also checks its captured conversation
revision; concurrent stale answers cannot overwrite or silently reorder newer
conversation state. Removing History removes its summaries and messages.

## Execution and grounding boundaries

The Rust integration uses the existing process runner and utility registry,
passing literal arguments without a shell and the prompt through stdin. A
private temporary directory replaces the repository/home working directory.
Requests use read-only execution, ephemeral sessions, structured output, explicit
timeouts and bounded output capture. User configuration and instruction files
are excluded; supported tool-disabling options are checked before generation.
Unexpected tool activity rejects the result. No real account is needed by tests.

Before generation, the integration inventories configured MCP servers, applies
per-server disabling overrides and checks again that every listed server is
disabled. It passes the same overrides to execution. This avoids relying on
post-execution tool-event rejection to prevent configured MCP side effects.

Transcripts and conversation text are untrusted data, encoded separately from
the instructions. Responses must provide exact evidence from the supplied
transcript; unsupported questions return an explicit insufficient-information
answer. Size limits fail with an explanation instead of silently clipping text.
Transcripts up to 768 KiB use 32 KiB UTF-8-safe chunks covering the whole source,
with synthesis of evidence from all chunks for the final result. Larger sources
fail explicitly. Each CLI generation has a two-minute timeout and the generation
phase has a ten-minute aggregate limit. Each prerequisite diagnostic command
has a separate ten-second timeout. Prior conversation is limited to the latest
12 turns / 16 KiB for follow-up references; every message remains saved. Prior
messages are not supporting evidence.

These controls reduce tool and prompt-injection exposure. They do not prove
semantic entailment of every model statement. Codex itself retains access to its
authentication and service network, and managed CLI configuration may apply.
A read-only sandbox by itself allows file reads; it is not a guarantee that the
process can access only the transcript. PineFetch does not claim an offline or
absolute transcript-only operating-system sandbox. Administrative configuration
could change between MCP preflight and execution; post-execution rejection cannot
undo a tool side effect. PineFetch never retries a generation automatically, but
Codex can retry its own network transport internally. See the
[official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)
and [structured execution guidance](https://learn.chatgpt.com/docs/non-interactive-mode).

## Manual acceptance

1. In Diagnostics, check missing/install, signed-out, ChatGPT and API-key states;
   confirm Check Again refreshes the state without exposing credentials.
2. Open a video with a saved transcript and no summary. Ask a transcript question
   immediately. Close and reopen the video: its messages remain, with Analyze
   Video still available.
3. Start analysis while asking another question. Confirm independent progress,
   completion and errors. Regenerate and confirm all messages remain.
4. Ask about something absent from the transcript. Expect an explicit statement
   that the transcript does not provide enough information.
5. Cancel each operation and switch videos during generation. Confirm the other
   video's conversation stays isolated and reopening shows only committed data.
6. Open an item without a transcript. Both generation actions explain the missing
   saved transcript. Disconnect/sign out of Codex and confirm actionable setup.
7. In an isolated test profile, change a transcript and reopen the tab. Old
   summary/messages are hidden; new questions work without generating a summary.
8. Check keyboard tab navigation, labelled input, focus and narrow-window layout.

Automated tests use synthetic transcript data, temporary/in-memory databases and
mock CLI processes. They do not spend account usage or modify a personal profile.
Native app/account acceptance remains a separate manual check.

Run `npm run check` and `npm run build:check` for the standard gates. Run
`npm run test:ai-ui`, `npm run test:ui` and `npm run test:settings-ui` sequentially
for Chromium/WebKit interaction coverage.

## Changed files

- `src-tauri/src/codex.rs`: discovery, diagnostics, safe CLI execution, prompts,
  chunking, JSON/evidence validation and mock-process tests.
- `src-tauri/src/video_ai.rs`: independent chat/summary execution, cancellation,
  transcript selection checks and operation tests.
- `src-tauri/src/video_ai_store.rs`, `database.rs`, `history.rs`: schema 17,
  independent persistence, shared transcript reads and migration/storage tests.
- `src-tauri/src/commands.rs`, `main.rs`, `state.rs`: asynchronous IPC and lifecycle
  registration.
- `src/history-ai-view.js`, `codex-diagnostics.js`, `history-details-view.js`,
  `settings-view.js`, `tauri-client.js`, `main.js`, `index.html`, `styles.css`:
  the AI workspace, setup UI and existing design-system integration.
- `test/history-ai.test.mjs`, `history-ai-ui.mjs`, `tauri-client.test.mjs`:
  state, API, accessibility, responsive and navigation regressions.
- `package.json`: the AI browser test command and the existing version bump;
  `package-lock.json`, `src-tauri/Cargo.toml`, `Cargo.lock`, `tauri.conf.json`:
  synchronization of the existing 3.1.0 version.
- `docs/ARCHITECTURE.md`, `changelog.md`, `AI_VIDEO_SUMMARY.md`: architecture,
  release notes, constraints and acceptance steps.
