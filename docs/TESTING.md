# Testing PineFetch behavior

**Happy paths alone are not sufficient tests for critical PineFetch components.**
For a central workflow, test a relevant failure and its resulting state as well
as normal operation. A process returning zero is not sufficient proof of a
successful download: the required output and postprocessing must be available,
required persistence must commit, and the final job state must agree.

## Commands and test levels

Install the pinned toolchains and dependencies as described in
[DEVELOPMENT.md](DEVELOPMENT.md), then run the complete local gate:

```sh
npm run check
npm run build:check
```

| Command                         | Scope                                                                                           |
| ------------------------------- | ----------------------------------------------------------------------------------------------- |
| `npm run check`                 | Version, lint and formatting checks, all JavaScript and all Cargo test targets                  |
| `npm test`                      | All JavaScript and Rust tests                                                                   |
| `npm run test:js`               | Native Node test runner; each test file has a 30-second limit                                   |
| `npm run test:rust`             | All registered Rust tests, including the built CLI target                                       |
| `npm run check:fast`            | Version, lint and formatting checks plus the fast tests                                         |
| `npm run test:fast`             | JavaScript, pure Rust rules and inexpensive application/component checks                        |
| `npm run test:rust:fast`        | Only the fast Rust partition                                                                    |
| `npm run test:integration`      | Real temporary files, file-backed SQLite, fake child processes, loopback HTTP/IPC and built CLI |
| `npm run test:rust:integration` | Same Rust integration partition                                                                 |
| `npm run build:check`           | Release compilation/linking with embedded frontend assets; not a bundle or desktop smoke        |

The pyramid has five levels:

1. **Unit:** URL/domain/timestamp parsing, presets, process arguments, metadata
   and HTTP parsing, formatters and status serialization. Feed strings or values
   directly into production functions; do not start a process to test a parser.
2. **Component/application:** job preparation, queue state, cancellation
   precedence, config/history interpretation, completion error propagation and
   injected Tauri invoke/listen. Fast component tests may use in-memory SQLite;
   this is separate from the pure unit level.
3. **Local integration:** real file-backed SQLite transactions and reopening,
   temporary filesystem operations, actual controlled child processes, TCP over
   loopback and the compiled CLI. These run without media runtimes or a desktop.
4. **Artifact/runtime:** the compile/link gate catches native linking and embedded
   asset problems. Installed bundles and prepared runtime contents require
   separate release validation.
5. **Application smoke:** one offline request-to-completion scenario combines
   real process execution, parsed metadata, a synthetic output file, SQLite,
   actual queue finalization and persistence failure. It does not exercise the
   desktop WebView or a real platform download.

`scripts/run-rust-tests.mjs` discovers executable test artifacts using Cargo
`--locked --all-targets --no-run --message-format=json`, then lists each harness.
It partitions every discovered test exactly once and batches the selected names
using libtest's multiple `--exact` filters. It rejects unknown artifact/listing
types, empty selections, nonzero exits, missing success summaries, mismatched
passed counts and ignored selected tests. It runs selected ignored tests rather
than silently counting them as passed; no current application tests use ignore.

All `integrity_tests` belong to integration. New tests crossing process, TCP or
temporary-file boundaries use an `integration_` function prefix. Existing names
remain intact; the runner has an explicit compatibility list for their real
filesystem/process/CLI-transport cases. In-memory component checks stay fast.
When adding a new Cargo integration test target, all its tests automatically
belong to integration. This is test selection, not a coverage target.

## Inventory and error catalog

The previous quality work already established strong coverage of pure rules,
interface adapters and database integrity. Those tests remain. In particular,
file-backed migrations, backups, rollback, locking, disk-full simulation,
concurrent writers and abrupt process termination were already tested; a new
suite should not recreate those cases with weaker mocks.

| Behavior or boundary                 | Main evidence and failure cases                                                                                                                                                                                                                                                                                               |
| ------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Input and shared interface rules     | `reuse_tests`, `cli::tests`, Node URL contracts: invalid/empty/control-character URLs, spoofed hosts, unsupported presets, defaults, timestamp/normalization rules, typed status values and GUI/CLI/Link Dump job equivalence                                                                                                 |
| External bytes and parsing           | `http_failure_tests`, `process_failure_tests`, existing parser tests: malformed JSON, missing fields, invalid UTF-8, unrecognized process output and deterministic arbitrary-byte corpora produce controlled errors or absent parsed results                                                                                  |
| Runtime resolution and prerequisites | `runtime_failure_tests`: missing/stale yt-dlp paths use explicit isolated search paths; operations requiring FFmpeg reject its absence; missing output settings return controlled errors                                                                                                                                      |
| Queue state                          | `queue_failure_tests`, `architecture_tests`: queued/running/completed/failed/cancelled transitions, next FIFO job after failure/success, pause/resume, auto/manual start, unknown or repeated cancel, simultaneous worker claims and concurrent enqueue                                                                       |
| Process lifecycle                    | `process_failure_tests`, existing process regressions: missing executable, start failure, nonzero exit, partial/large/slow output, hang timeout, inherited pipes, cancellation before registration, repeated cancellation, shutdown and Unix descendant termination                                                           |
| Required versus optional outputs     | `persistence_failure_tests`: no required output, directory output, disappearing output, unreadable transcript, changed/missing/invalid-UTF-8 produced caption and missing caption media prevent completion; absent optional caption/thumbnail metadata does not invalidate valid media                                        |
| File publication and ownership       | `process_failure_tests`, `integrity_tests`: nonexistent parent, file instead of directory, existing file/directory target, vanished temporary file, Unicode/spaces, long names and Unix symlinks; existing outputs and foreign replacements remain intact                                                                     |
| Database integrity                   | `integrity_tests`: locked/read-only/full databases, failed migrations/import markers, refused future/corrupt schemas, errors between related writes, WAL snapshots, concurrent migration/completion, process-killed transactions and retry after reopen                                                                       |
| Config and saved content             | `persistence_failure_tests`, existing config/history tests: failed config writes retain cached and persisted settings; corrupt current config/caption/transcript values return errors without replacing stored data; repair permits controlled retry                                                                          |
| Link Dump HTTP                       | Pure framing tests plus real TCP tests: short body, duplicate/conflicting Content-Length, unsupported Transfer-Encoding, invalid request line/headers, header/body limits, invalid JSON, missing/wrong/revoked/deleted secret, wrong method, unknown endpoint, stalled client, connection limit and authentication DB failure |
| CLI process boundary                 | `src-tauri/tests/cli_process.rs`: actual binary help/version, invalid commands/arguments, exit 0/1/2, stdout/stderr separation, queue/history/stats IPC, backend errors and malformed/incomplete/oversized replies                                                                                                            |
| Frontend events and API              | `tauri-client.test.mjs`, `app-state.test.mjs`, `event-failures.test.mjs`: exact command/parameter/event mapping, rejected invoke, out-of-order responses, unknown/removed jobs, duplicate completion, malformed events and late progress/state after a terminal state                                                         |

For a download completion failure, assertions should cover **both** the returned
error/final status and durable state: no partially committed related rows, a
usable recovery receipt, retained produced files, and retry behavior. Optional
metadata that was never provided differs from a produced caption that has
changed or become unreadable. A requested transcript is required output.
Thumbnail saving has no separate required completion receipt.

The HTTP parser consumes a declared body and the server closes the connection
after one request; pipelining is not an additional supported feature. Tests
preserve that boundary instead of introducing a full HTTP stack.

## Fake process and fixtures

`test/fixtures/fake-process.rs` is a small Rust program compiled into a temporary
directory by `test_support::FakeProcess` with the repository's Rust toolchain.
It has no dependency on installed yt-dlp, FFmpeg, Python, Whisper or Deno and no
external network calls. Tests exercise the real production process runner;
parser tests continue to call parsers directly.

The Rust format commands check/format this standalone fixture explicitly as well
as Cargo sources; Cargo's module traversal alone does not include the fixture.

| Scenario           | Controlled behavior                                                                                                |
| ------------------ | ------------------------------------------------------------------------------------------------------------------ |
| `success`          | Exit zero, yt-dlp-like progress/metadata/caption lines, optional small synthetic output file and stderr diagnostic |
| `exit-error`       | Stderr error and exit code 23                                                                                      |
| `malformed-output` | Malformed metadata and invalid UTF-8 output                                                                        |
| `slow-output`      | First progress line, readiness handshake, remaining output after explicit release                                  |
| `huge-output`      | One MiB each of stdout/stderr, exercising simultaneous draining                                                    |
| `hang`             | Remain blocked until release or cancellation/timeout                                                               |
| `spawn-child`      | Confirm a descendant is ready before testing process-tree cancellation                                             |
| `partial-output`   | Unterminated stdout/stderr and exit code 7                                                                         |

`child-hang` is an internal descendant mode. Readiness and release use a bounded
loopback connection on port zero, not a fixed sleep to guess whether a process
started. Channels, barriers and process registration also establish concurrency
checkpoints; deadline polling is only a wait mechanism and safety limit.

Fixtures are small and synthetic:

- `test/fixtures/url-contracts.json` is read by Rust and Node; it keeps explicit
  backend/frontend expectations where TXT and Link Dump import semantics differ.
- `src-tauri/test-fixtures/schema/*.sql` reconstruct historical release schemas,
  with no copies of a personal database.
- Fake process output generates tiny text files as needed. No downloaded media,
  binary media corpus, secrets or personal identifiers are committed.

Do not add large HTML/CLI/JSON snapshots when a few assertions can identify the
behavior. Exact text belongs in a test only when it is a documented contract;
otherwise assert error category, status/exit code and relevant semantic content.

## Isolation, local integration and deadlines

`test_support::TempRoot` allocates a unique root under the system temporary
directory and removes it with RAII. New file-backed SQLite and filesystem tests
keep databases, outputs and fixtures inside that root. Existing synthetic
integrity tests have their own equivalent cleanup fixture. Never supply the
production app-data directory, personal Downloads or a user database.

The built CLI's environment explicitly redirects HOME, XDG data/config/cache and
temporary locations into its own root and removes media-runtime override
variables. Its read commands connect to an isolated controlled IPC endpoint;
they do not launch a desktop or open a user's SQLite database. Application tests
separately prove SQLite-backed history interpretation.

Link Dump TCP tests bind only `127.0.0.1` with port `0`. They use the production
HTTP reader and request handler, real authentication storage and queue state,
with only desktop event/worker-start boundaries controlled. Limits and permit
release are checked against state, not inferred from emitted events.

No normal application test requires external network access or real platform
downloads. Installing locked npm/Cargo dependencies may require network; that
is build preparation, not a test exercising an external service. Release-gate
Node tests use mocked publication/network commands and never publish.

Process, TCP and lock tests have internal deadlines. The Rust tier runner also
limits discovery to 30 seconds and each executable test suite to 120 seconds;
Cargo compilation has a separate 15-minute limit. On outer timeout it terminates
the harness process tree, including confirmed Unix descendants and their process
groups (Windows uses `taskkill /T`). A failure is never converted to success.

Unix process-group/descendant, permission and symlink cases are gated with
`cfg(unix)` because those OS contracts differ. Permission-denial tests require
an unprivileged account. Built CLI loopback profile tests
currently run on macOS/Linux; help/version and invalid-argument tests have no
such restriction. The maintained CI runner label is `macos-15`;
tests not compiled for another platform are not proof that platform passed.

## CI and runtime boundaries

The existing Quality workflow has three mandatory jobs:

- `Quality checks (macOS)`: the fast quality gate, including lint/format/version
  checks and fast tests.
- `Local integration tests (macOS)`: local integration tests after the fast gate.
- `Build (macOS)`: release compile/link validation.

There is no `continue-on-error`, skip-to-green gate or coverage percentage.
Require all three status checks when configuring branch protection; changing
workflow files does not itself update GitHub repository settings.

`build:check` first prepares the pinned design-system's local CSS, ESM, fonts,
icons and license notices, then embeds them through Tauri's static frontend.
`npm ci`, development, screenshots and release packaging also prepare these
assets. The screenshot script renders the same local assets and waits for font
loading before capturing each view; see [DEVELOPMENT.md](DEVELOPMENT.md).

`npm run test:ui` runs the design-system integration in Chromium and WebKit with
an isolated Tauri fixture bridge and offline requests. It covers local fonts,
icons and ESM loading, plus integrated keyboard navigation and dialogs. Prepare
the browser engines with `npm exec playwright install chromium webkit` first.
This explicit browser check is separate from the normal Node/Rust gates; it does
not replace native Tauri or installed-bundle smoke tests.

`build:check` does not prepare or validate bundled Whisper/FFmpeg/Deno runtimes,
signatures, notarization, DMG installation or the desktop event loop. There is no
existing Tauri desktop automation setup. The explicit browser integration check
uses a fixture bridge and does not exercise the native desktop event loop.
Installed-app smoke and real-runtime checks remain explicit release
validation, described in [COMPATIBILITY.md](COMPATIBILITY.md) and
[DEVELOPMENT.md](DEVELOPMENT.md); they are not silently skipped PR tests.

## Adding a regression

This test pass reproduced and corrected three groups of existing failures:

- HTTP accepted premature body EOF, duplicate lengths, unsupported transfer
  encoding and malformed request lines; authentication database failures also
  closed the socket instead of returning controlled JSON 500 responses.
- Accepted HTTP/CLI sockets could retain nonblocking mode on macOS, preventing
  their configured read deadlines from protecting fragmented requests.
- Late frontend running/progress events could overwrite terminal job state;
  malformed state/progress payloads could throw instead of being ignored.

Each correction has a regression that failed before the product fix.

1. Identify the failing boundary and the expected durable/resulting state.
2. Add the smallest deterministic test using the production function. Use a
   fake process only for a process boundary, real temporary SQLite for a storage
   boundary, and direct input bytes for a parser.
3. Run it before the fix and retain the failing output. A newly discovered bug
   needs a demonstrated red test, not an assumed reproduction.
4. Make the smallest product fix, rerun the regression, then run its tier and
   the complete quality gate. Preserve existing interface contracts.
5. For new local integration tests, use the `integration_` prefix. Add deadlines
   and cleanup for processes/threads/sockets. Never use ignore/skip to hide a
   failure or mutate process-global environment in parallel unit tests.

For example:

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked incomplete_http_body_is_rejected
node --test --test-timeout=30000 --test-name-pattern='late running states' test/event-failures.test.mjs
npm run test:integration
npm run check
```

Also prove the harness can fail: temporarily alter one meaningful expectation or
fake response, verify that the relevant command exits nonzero, restore the exact
change and rerun. Such a negative self-check is a local validation step, not a
permanently failing test.

Mutation testing is a possible diagnostic for small URL, timestamp, HTTP parser
and queue-state rules. No mutation framework or coverage threshold is added:
the current priority is reproducible failure/state contracts. Full desktop
interaction, actual media-runtime execution and live platform compatibility
remain deliberate gaps outside normal offline PR tests.
