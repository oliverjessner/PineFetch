# Data integrity

PineFetch stores local data in `pinefetch.sqlite` under Tauri's
`app_data_dir()` for `com.pinefetch.app`. The database contains configuration,
Link Dump settings and secret **hashes**, history metadata, transcript text,
caption text and their file relationships. Media, transcript and caption files
remain in the selected output directory. Removing or clearing history deletes
database records and their existing caption/transcript dependents; it does not
delete downloaded files.

Legacy inputs are `config.json` under `app_config_dir()` and `history.json`
under `app_data_dir()`. Their original files remain untouched after import.
The CLI's endpoint descriptor and startup lock are transient IPC files, not
databases or legacy imports. CLI history/statistics and Link Dump video read
endpoints keep their existing read-only contracts. The desktop opens SQLite;
the CLI sends commands to that desktop process. Multiple desktop versions or
processes can nevertheless reach the same database. An in-process mutex alone
does not protect against that.

## Initialization and compatibility

`src-tauri/src/database.rs` owns opening, compatibility checks, connection
settings, migration validation and migration snapshots. Existing files are
probed through a read-only connection before opening for application writes.
Missing files can be initialized; unreadable files, invalid SQLite contents and
unexpected schemas produce an error. They are never removed or replaced with
an empty database.

A crashed transaction can have spilled uncommitted pages into the main file.
SQLite then requires native journal recovery even to read the committed schema
version. Only the explicit `SQLITE_READONLY_ROLLBACK`/`SQLITE_READONLY_RECOVERY`
probe errors permit reopening for that recovery, with a diagnostic. SQLite
restores its committed state under its own locks, then PineFetch checks the
recovered version before any application settings/schema/data write. This is
not an application downgrade, repair or backup restoration. Recovery can change
physical database/journal files; preserving an unrecovered journal byte-for-byte
is not promised. Other read errors are propagated. See
[SQLite hot-journal recovery](https://www.sqlite.org/lockingv3.html#dealing_with_hot_journals).

`PRAGMA user_version` is the single authoritative **schema** version, currently
**2**. It is independent of the package/release version. Import markers describe
imports only; they are not additional migration-version authorities.

Versions outside `0..=2` are refused with the present and supported versions.
There is no downgrade or repair. Compatibility is checked again after acquiring
the migration writer lock, so a competing newer process cannot turn an old
preflight result into an unsafe upgrade. Every application write also rechecks
the version under its writer reservation: a running older instance refuses
writes after another process upgrades the database. Unknown tables, columns, constraints,
indexes, relationships, views or triggers are rejected and preserved rather
than guessed at. An explicitly versioned but incomplete schema is also refused.

Every application connection enables foreign keys **before** transactions and
sets a three-second SQLite busy timeout. Foreign keys cannot be enabled inside
an already active transaction; see the
[SQLite foreign key pragma](https://www.sqlite.org/pragma.html#pragma_foreign_keys).
Lock failures surface as errors. Journal mode and synchronization settings are
not changed by normal initialization or migrations. Tests explicitly select WAL
for WAL scenarios only.

Ordinary starts validate schema metadata without acquiring a migration writer
lock, updating defaults, backfilling history, scanning all user rows or creating
snapshots. `quick_check`, `foreign_key_check`, null-identity checks and the
historical source backfill run only during an actual upgrade.

Database/config/history initialization runs before Tauri's native event loop.
Initialization failures print `PineFetch could not start: ...` and exit with code
1, rather than returning an error from the setup callback (which Tauri turns into
a panic and macOS can terminate with `SIGABRT`). CLI help/version still return
before database initialization; ordinary CLI requests retain their desktop IPC path.

## Ordered migrations and legacy adoption

The entire required upgrade section uses one `BEGIN IMMEDIATE` transaction:

1. **Version 1:** adopt a recognized unversioned layout, add missing known
   settings/nullable metadata, retain historical defaults, rename
   `save_instagram_captions` to `save_captions`, copy a successful old
   `app_meta` config-import marker, and apply the existing source backfill.
2. **Version 2:** add `legacy_imports` and `job_completions`. These are import
   and completion receipts; they do not persist the queue or enable resume.

Each step writes its literal version inside the same transaction. The final
version is checked before commit. A failure in either step rolls back **all**
steps in that upgrade attempt, including schema, data changes and versions.
After the error is resolved, reopening repeats the upgrade from its previous
committed version. There are no nested transactions. External downloads,
Whisper, hashing and normal file publication do not run in these transactions.

Version zero is not treated as an empty database. Adoption inspects actual
tables, column types/nullability/defaults/primary keys, unique and named indexes,
foreign keys and the historical singleton/transcript constraints. Missing
objects are added only after the remaining layout is recognized. The existing
history-only and config-only regression fixtures remain supported, including
recognizable interrupted historical subsets. The minimum history layout needs
`id`, `url`, `created_at` and `completed_at`; other known nullable history fields
can be added. Caption/transcript tables without a history parent are refused.

The validator also recognizes the narrowly reproduced upgraded layout reported
in a startup crash: both `save_instagram_captions` and `save_captions` with their
original integer/default constraints, and `pinefetch_version TEXT NOT NULL
DEFAULT '2.1.0'`. Both caption columns and their values are retained; the existing
`save_captions` setting remains authoritative. The version constraint/default is
retained on upgrade and reopen. History entries without a version use that
database's `2.1.0` default only when this exact constraint exists; nullable layouts
continue to store NULL. Other column types/defaults, unknown objects and future
versions still fail validation. Migration SQL and schema version 2 are unchanged.
Tests reproduce these schema variants with synthetic rows, including rollback,
snapshot preservation, retry and reopening; they do not claim Git-release
provenance for this additional upgrade combination.

No user table is rebuilt or dropped. Old `app_meta` is retained. Unexpected old
objects such as an obsolete request-log table are preserved and cause a clear
compatibility error, rather than the former unconditional request-log deletion.
Orphans, null identities and conflicting schema definitions block adoption.
There is no row deduplication or silent conflict correction. Existing IDs,
caption/transcript relationships, secret hashes and nullable hashes/languages
remain intact. `created_at`/`completed_at` keep milliseconds; platform
`timestamp` metadata keeps its existing seconds semantics. The pre-existing
source backfill only derives a recognized source for missing/blank source fields.

File-backed fixtures were reconstructed from the production migration SQL in
these Git tags:

| Historical app | Tested unversioned layout                                    |
| -------------- | ------------------------------------------------------------ |
| v1.4.3         | SQLite history, Link Dump settings/secrets; JSON config      |
| v1.4.5         | SQLite config and `app_meta` import marker                   |
| v1.5.0         | Platform timestamp metadata                                  |
| v1.6.0         | Duration and byte-size metadata                              |
| v1.7.2, v1.8.0 | Transcription and richer history/config fields               |
| v1.9.1         | Notifications and `save_instagram_captions` alias            |
| v2.1.0         | General captions, thumbnail setting, config import column    |
| v2.2.0         | SHA-256 fields, PineFetch version and transcription language |

See [fixture provenance](../src-tauri/test-fixtures/README.md) for exact commits
and extraction details. These are synthetic reconstructions, not copies of real
users' databases. Tests assert specific Unicode text, IDs, timestamps, NULLs,
choices, hashes and joined relationships. Version 1 is tested as the newly
defined internal baseline, not claimed as an earlier released app schema.
Arbitrary modified layouts, every possible interrupted legacy combination and
other release tags are not individually proven. Pre-SQLite JSON data is covered
by JSON fixtures, not a claimed older SQLite schema.

Once released, migration 1, migration 2 and their helper behavior are immutable.
For a change: add the next explicit ordered step, update the maximum supported
version and metadata validator for each still-supported version, and add real
file fixtures for successful upgrade, SQL failure, unchanged committed version,
retry, competing startup and newer-version refusal. Never just raise
`SCHEMA_VERSION` or edit a previous step; the final-version guard deliberately
rejects an upgrade without its matching step.

## Pre-upgrade SQLite snapshots

Before the first schema/data mutation on an existing database with tables, a
required snapshot is published alongside it:

`pinefetch.sqlite.pre-schema-<old-version>-<uuid>.sqlite3`

The UUID prevents overwriting prior snapshots. On the supported Unix/macOS path,
the reserved file uses mode `0600`. A failed or interrupted snapshot remains
`.partial` and is not treated as a completed backup. No snapshot is created on a
fresh empty database or a current-schema start. Failed-upgrade retries may create
additional snapshots; all completed snapshots are retained until the owner
explicitly reviews and removes them. There is no automatic retention purge.

Snapshots use the [SQLite backup API](https://www.sqlite.org/backup.html), not
raw copying of the main database file. The migration connection holds its
`BEGIN IMMEDIATE` writer reservation and has not written any changes yet. A
dedicated read-only source connection sees that committed database, including
WAL pages. Other writers cannot change it while the snapshot is made. The
writing connection itself is not used as a backup source: SQLite can return
`SQLITE_LOCKED` for that case. This extra connection is only a snapshot reader,
not a workaround for nested transactions.

Backup pages are copied in bounded chunks with a 30-second total deadline;
busy/locked states fail rather than retry forever. This upgrade-only writer
reservation covers snapshot validation and the subsequent short schema steps.
It can temporarily block another process; a large database may require a retry
after other writers have stopped. Snapshot failure blocks the upgrade. The
snapshot is checked, closed, flushed and published using a no-clobber hard link;
unsupported publication/filesystem behavior fails the upgrade. Normal downloads
and large media operations are never performed while holding this reservation.

The WAL regression restores a snapshot through the SQLite restore API into an
isolated new file, checks its old version, exact joined history/transcript/caption
contents and secret hash, runs `quick_check`, and successfully upgrades that
restored copy. Existence alone is not the restoration test.

### Manual restoration

No automatic restore writes over an active database. To restore manually:

1. Stop every PineFetch desktop instance, other versions and related CLI activity.
   Check that no process still owns the database or is writing outputs.
2. Keep the current database together with any `-wal`/`-shm` files in a separate
   preservation directory. Keep downloaded files and legacy JSON originals too.
   A schema snapshot does not back up those files.
3. Select a **completed** `.sqlite3` snapshot for the correct database and starting
   version. Restore it with SQLite's backup/restore API into a **new isolated
   destination**, never by copying just the main file of a possibly active DB.
4. In that isolated destination run `PRAGMA quick_check` and
   `PRAGMA foreign_key_check`; inspect known history IDs, config choices, secret
   hash presence and caption/transcript relationships. Confirm `user_version`
   and use an app that supports it. Do not expose private text/hashes in logs.
5. Close all validation connections, including the restored destination, so WAL
   commits are checkpointed. Only then install the verified standalone restored
   file at the original database path with restrictive permissions. Preserve
   the quarantined original DB and sidecars; never pair its old WAL with the
   restored file.
6. Start one supported PineFetch instance. Any necessary upgrade makes its own
   pre-upgrade snapshot. Keep both the old database and snapshot until the
   restored contents and file relationships have been checked.

That stop/quarantine/install procedure on a real desktop has not been automated
or executed against personal data. The isolated SQLite restoration is proven.

## Legacy JSON import boundaries

Missing files are a valid absence; unreadable files, invalid JSON and unexpected
structures are errors. Parse errors report category/position, not private JSON
values. Config imports retain the `save_instagram_captions` alias and existing
older field defaults. A config document must contain recognized configuration
fields. History requires its stable IDs, URL and original creation timestamp;
duplicate/missing IDs or an ID linked to a different existing URL block import.

Config fields plus `legacy_config_json_migrated` commit together. History entries
plus the `legacy_imports('history_json')` receipt commit together. Both recheck
their marker after obtaining the writer lock. A missing file records a completed
absence; adding a legacy file later does not automatically overwrite live data.
Read/parse failure never writes that marker. A failed/aborted import rolls back
all new entries and remains retryable. An existing matching history ID keeps its
possibly newer SQLite metadata. New entries pass through the existing history
normalization before insertion: blank optional metadata and invalid numeric/hash
values are cleared, and missing filename/title/source/platform fields are derived
where possible. This stores the values needed by SQL search and filtering without
changing stable IDs, URLs or creation timestamps. Repeated URLs with
**different IDs** remain separate history entries. Import does not depend on
whether history is empty, and clearing history cannot silently resurrect the old
JSON file.

Normal config patches re-read the current persisted config under a writer lock,
apply only the requested changes, commit, then update the process cache. A stale
full cache cannot overwrite another process's newer fields. Link Dump settings
use the same read/patch/commit boundary, and generated secret insertion plus its
returned record commit together. The single-field revoke/delete/use updates
remain single update statements inside these guarded transactions.

## Files and completion boundaries

The worker retains existing public states such as `downloading`, `transcribing`,
`success`, `error` and `cancelled`. Internally each existing job ID has a receipt:

`processing` → `output_ready` → `complete`

`processing` is committed before launching external processing. Once the media
file is available, `output_ready` records its path. A transcript can become the
job's final output path. Output/file-directory flushing, hashing, file validation and required transcript reading
happen outside the SQLite writer transaction. Then history (using the **job ID**
as its new history identity), required transcription, all produced caption
metadata and the `complete` receipt commit in one transaction. Only that commit
permits the `success` event and success notification count. A platform returning
no optional caption contributes an empty caption set and can succeed normally.

A missing required output/language, changed/unreadable produced caption, hash
read error, lock, full database or failed metadata insert leaves completion
unfinished. A finished output is retained, and the public error identifies the
failed persistence step and whether the output still exists. No database error
triggers deletion of media, transcript or caption outputs.

Repeating the same committed job ID/path returns without inserting history or
metadata again. A new job ID can download the same URL and reuse the same valid
existing media file. There is no URL uniqueness constraint or `INSERT OR REPLACE`.
Identity/path conflicts fail without overwriting existing history. Intentional
history deletion clears the completion receipt's history link with `SET NULL`;
the receipt prevents a later repeated completion from implicitly recreating it.
The existing caption/transcript delete cascades are unchanged.

PineFetch-owned cut, transcript, audio and caption temporaries are exclusively
reserved with `create_new`; cut/transcript/audio names include a job ID and UUID
where available. On Unix cleanup verifies the reserved inode before unlinking.
Foreign replacements, pre-existing files and ambiguous paths are retained.
Non-Unix cleanup conservatively retains temporary files because the same inode
identity check is not implemented there.

Caption/transcript/cut publication first attempts a no-clobber hard link to a
unique final name. Existing targets get a numbered sibling, not truncation.
Where hard links are unavailable, publication falls back to exclusive creation
and a flushed copy. That fallback is **not** an atomic rename: a process kill
can leave an incomplete new candidate, which is retained with an unfinished job
receipt. File and parent-directory flush errors surface. The full cut input is
always retained because yt-dlp may have reused a pre-existing user file.

SQLite and the filesystem do not share one atomic commit. A kill after file
publication but before DB completion leaves `processing`/`output_ready`, not a
completed history record. A kill after the SQLite commit but before the GUI
event leaves a committed receipt/history that a repeat cannot duplicate. There
is no automatic queue recovery, cross-process cancellation or deletion of old
pending receipts/temporary paths. In a stopped, isolated copy, inspect unfinished
receipts with:

```sql
SELECT job_id, state, output_path
FROM job_completions
WHERE state != 'complete';
```

An output path can be unknown if the kill preceded its receipt. Preserve
ambiguous files; inspect them manually. These records distinguish unfinished
work without marking another live process's work as abandoned at startup.

## Regression checks and limits

The tests live in the existing Rust suite and use synthetic SQLite files and
temporary directories only. They do not launch Tauri's desktop, download media,
load models, use browser credentials or inspect personal app directories.

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked --all-targets integrity_tests
npm run check
npm run build:check
```

The existing quality gate/CI already executes these Rust tests. Coverage includes
fresh/current/version-1/known unversioned schemas; actual failing SQL, rollback
and retry; future versions including a version race; corrupt/unreadable inputs;
missing/valid/invalid JSON, marker failures, stable identities and concurrent
imports; WAL snapshot restoration; two-connection migration/completion races;
bounded writer-lock failures; readonly/permission failures; deliberate SQLite
`SQLITE_FULL` using `max_page_count` without filling the runner disk; metadata
write failures; absent optional captions, required file/language failures;
pre-existing targets; foreign temporary replacements; spaces/non-ASCII paths;
configuration concurrency; and actual child-process **SIGKILL** during migration,
history import and completion before commit, including forced pager spill and
real application reopening after hot-journal recovery. A future-schema hot-journal
case verifies that recovery preserves its exact prior committed database bytes,
version and records before refusing all application writes. Crash tests wait for pipe handshakes
at controlled points; they do not rely on sleeps to hit a race window.

Existing tests remain intact. Tests that previously initialized only a config
cache now seed the matching synthetic DB, and the temporary-audio test reserves
its file explicitly before testing cleanup. Their original assertions remain.

The verified target is macOS arm64 and the existing macOS CI configuration.
Linux/Windows native execution, network/removable filesystems, the hard-link
fallback under abrupt kill, arbitrary OS/device I/O faults and a real-desktop
manual restoration are not proven here. SQLite full-page injection is not an
actual full disk. SIGKILL proves recovery with the running OS and SQLite, not
power-loss, hardware, filesystem/controller or concurrent file-replacement
durability. External yt-dlp/Whisper/FFmpeg runtime behavior and user-supplied
runtime configurations remain outside these offline regression tests.
