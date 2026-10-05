# PineFetch architecture

PineFetch remains a Tauri desktop application with Vanilla JavaScript, a CLI and
a local Link Dump HTTP interface. The modules below separate existing rules from
their input/output boundaries; they do not introduce a service framework or a
second implementation of an application workflow.

## Backend responsibilities

All Rust modules live in `src-tauri/src`.

| Modules                                                                                            | Responsibility                                                                                                                                 |
| -------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `main`, `startup`, `state`                                                                         | Compose the app, resolve desktop data paths, wire legacy imports, register commands and install startup/shutdown hooks.                        |
| `models`                                                                                           | Shared serialized configuration, download, history and Link Dump models. `DownloadState` preserves the existing seven snake-case event values. |
| `config_rules`, `download_rules`, `history_rules`, `video_urls`, existing `platform` and `presets` | Pure normalization, job preparation, timestamps, output names, metadata fallback, URL identity and platform/preset decisions.                  |
| `queue`                                                                                            | FIFO storage, pause/status/next/removal, URL deduplication and final cancellation/success decisions, without Tauri or SQLite.                  |
| `worker`                                                                                           | Coordinate job execution, cancellation, completion, worker lifecycle and existing notifications/events.                                        |
| `download`, `metadata`, `runtime`                                                                  | Execute downloads/transcription/cuts, fetch metadata and locate/probe yt-dlp, FFmpeg/ffprobe, Python/Whisper and Deno.                         |
| `yt_dlp`                                                                                           | Pure download argument construction and progress, filepath, caption and metadata parsing.                                                      |
| `process`                                                                                          | Process groups, child registration, timeout/cancellation, output draining and structured process errors.                                       |
| `files`, `hashing`                                                                                 | Owned temporary files, no-clobber publication, flushing, local path validation, legacy JSON reads and hashing.                                 |
| `database`                                                                                         | Connection ownership, guarded writer transactions, schema compatibility, migrations and snapshots.                                             |
| `config`, `history`, `link_dump_store`, `completion_store`                                         | SQL for each data area; completion writes related history/transcript/caption rows atomically.                                                  |
| `completion`                                                                                       | Validate and flush produced files, prepare history metadata, then invoke the completion transaction.                                           |
| `commands`, `cli`, `browser_import`, `events`                                                      | Tauri commands, CLI transport, Link Dump HTTP/server integration and the desktop event adapter.                                                |

Imports name the owning module explicitly. Production modules do not import the
root namespace through `use super::*`. Shared models and rules do not depend on
Tauri, SQLite, global application state or running external programs.

`AppState` contains five components: `ConfigState`, one shared `Database`,
`QueueState`, `ProcessState` and Link Dump server state. Database operations take
the database; configuration operations take the configuration component; process
helpers take process state. They cannot implicitly access unrelated subsystems.
The configuration cache shares the same connection through `Arc`; no additional
connection is opened per operation. Queue/process components retain the existing
separate locks and lock order rather than replacing them with a single large lock.

## Dependency direction

Interfaces call application orchestration and narrowly scoped components.
Orchestration uses pure rules and concrete infrastructure modules. SQLite stores
use neutral models/rules and `database`; they do not know about commands, CLI,
events or workers. `events` depends on models and queue snapshots, so emitting a
download event does not require importing the worker. Pure rules never call back
into an interface.

Some concrete orchestration still uses Tauri: `worker` and `download` publish
desktop events, `browser_import` bridges its HTTP server to the running app, and
`runtime` resolves bundled resources through an `AppHandle`. Their decisions,
parsers and queue mutations are extracted where independent tests benefit. There
is no trait for each function or theoretical layer enforced through extra folders.

## A download from input to completion

1. A Tauri command, CLI request or Link Dump handler creates a `DownloadRequest`
   and enters the shared worker/application path. Interface-specific validation,
   HTTP authentication and CLI syntax remain at their existing boundaries.
2. Job construction validates the URL, resolves the output directory and copies
   only the four required configuration options while holding the config lock.
   `prepare_download_job` receives those options, a directory and an ID explicitly.
3. Queue insertion updates the FIFO and publishes the existing queue snapshot.
   Auto-start starts the existing worker; paused queues keep waiting jobs ordered.
4. The worker takes the next job. Runtime resolution locates concrete executables;
   `yt_dlp::build_download_args` produces separate arguments for `Command`, and the
   runner registers/reads the child. Parsers produce models before the event
   adapter publishes progress, logs or state.
5. Existing download, cut, caption and transcription processing produces files.
   Completion validates and flushes output and reads/hash-checks required content
   outside SQLite writer transactions. `completion_store` commits the receipt,
   history and related transcript/caption rows in the existing single transaction.
6. The queue's finalization function gives cancellation precedence and invokes an
   injected completion callback only for success. A persistence failure changes
   the event to `error` and retains its output path. The worker emits the final
   event and records a success only after required persistence succeeds.

Finalization retains the existing current-job/cancellation lock scope across the
completion callback so cancellation cannot race the success decision. The queue
function does not emit UI events or itself require a database.

## Queue and process lifecycle

`QueueState` owns pending jobs, auto-start/pause flags, worker lifecycle and the
active normalized video key. `ProcessState` owns the current job/child, utility
children, cancellation request and shutdown flag. Pause prevents taking the next
job; resume allows it. Removing a waiting job preserves the order of all others.
Active cancellation is coordinated by the worker and terminates the existing
process group. Shutdown pauses the queue and stops registered processes.

`ProcessError` distinguishes spawn, wait, state, timeout, output-drain and missing
output failures. Its display/conversion preserves the current interface messages.
Argument builders and output parsers require neither installed runtimes nor a
desktop. Process regression tests use local synthetic child processes.

## Persistence boundary

Repositories are concrete modules, not traits. They use one mutex-protected
connection and `database::write_transaction`. Config updates retain their
transactional re-read and update the cache after commit. Startup resolves legacy
paths and passes them to the import functions; stores do not need an `AppHandle`.
Database initialization and legacy imports finish before the native event loop
starts. `main` handles persistence errors with a diagnostic and exit code 1;
Tauri's setup hook only installs the initialized state and starts services.

Schema version, migration SQL, legacy JSON formats, completion receipts, backup
policy and file ownership rules are unchanged. See [DATA-INTEGRITY.md](DATA-INTEGRITY.md)
for those guarantees and their temporary-database/fault-injection regressions.

## Frontend boundary

`src/tauri-client.js` owns command/event names and the Tauri global bridge. Views
receive its named methods, and contract tests inject a fake bridge. Methods retain
argument objects, results, errors and event unsubscribe callbacks. ESLint prevents
direct `__TAURI__` property access in other owned frontend modules.

`src/app-state.js` owns the central state factory and pure job/queue status rules,
including terminal removal and suppression of late events. DOM references and
render scheduling remain in `src/main.js`. The existing settings, history,
history-details and browser-import views retain their UI behavior and now receive
the API client. `url-utils.js` remains independent of the DOM.

Views still manage presentation state and asynchronous UI requests. Settings save
coordination and the main download form are intentionally not rewritten into a
state framework. A future extraction can move another independently testable rule
or introduce event/resource callbacks if a second execution host needs them.

## Working rule and checks

New business logic should live outside `main.rs`, Tauri commands and DOM event
handlers when a separate function/component can be meaningfully tested without
their infrastructure. Prefer explicit parameters and a module or callback over a
new trait/container. Keep command names, serialized values and lock/transaction
semantics stable when moving code.

Run `npm run check`, `npm test` and `npm run build:check`; their details and build
limitations are in [DEVELOPMENT.md](DEVELOPMENT.md). Existing regression tests are
retained. `architecture_tests.rs` adds isolated job/argument/parser/queue/state/error
tests; Node tests cover the API contract and DOM-free state transitions. Tests use
synthetic inputs and temporary/in-memory stores, never personal databases or live
platform downloads. These gates compile/link the macOS application; they do not
replace a packaged-app, GUI or bundled-runtime smoke test.
