# History and Account pagination in 2.7.0

Measured on 2026-10-28 with an isolated, synthetic 200,000-row SQLite archive.
The fixture has 100,000 rows per platform, 20,000 downloads for one frequent
creator, mixed completion/creation timestamps and legacy NULL completions.
No personal profile, real media, external requests or installed app was used.

## Local measurements

These are individual database-function timings from the same unoptimized Rust
test profile on the same development machine. They are diagnostic observations,
not portable latency targets or desktop first-paint measurements. Each run
creates its own file-backed archive. Statistics are a separate call and can
still contribute to total History loading time.

| Database operation, 20 rows per page          | Before D1 (ms) | After D1 (ms) |
| --------------------------------------------- | -------------: | ------------: |
| First History page, including initial count   |          92.50 |          1.35 |
| History offset 180,000, compatibility request |         555.01 |         12.15 |
| First source-filtered page, including count   |          94.89 |         10.50 |
| First exact Account page, including count     |          87.71 |          3.12 |
| Account offset 18,000, compatibility request  |         133.27 |          5.33 |
| Full archive statistics                       |         250.04 |         54.83 |

The production views now send the returned cursor for Load more. The History
continuation after offset 180,000 took 0.73 ms, with 1,605 SQLite VM steps and no
count query. The corresponding Account continuation took 0.83 ms and 1,592 VM
steps. Continuations on earlier pages used comparable work. A separate 20,000-row
case with identical timestamps guards against sorting or scanning a whole tie
group. VM-step checks, rather than wall-clock thresholds, enforce this behavior.

The baseline was measured at parent commit `22b7500` with the same archive fixture
and existing public database functions before query/index changes. The new
continuation measurements use the production cursor reader. Absolute timings
vary with caches, hardware and other work; the compatibility offset comparison
and continuation observations deliberately describe different requests.

## Reproduce

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked \
  history_archive_tests::integration_large_archive_query_measurements \
  -- --nocapture --test-threads=1

cargo test --manifest-path src-tauri/Cargo.toml --locked history_archive_tests
npm run test:ui
```

`npm run check` includes all native archive regressions. The browser suite uses
controlled command responses to exercise the real views in Chromium/WebKit;
SQLite migration, counts, ordering and seek costs are verified by Rust tests.
A manual desktop performance test on a real user archive remains separate.

## Query and migration contract

Schema 13 backs up the old database and transactionally replaces two older
indexes with three indexes matching global order, source order and exact
creator/platform order. It preserves History, texts, insights and completion
receipts. Reopening checks expression SQL, index columns, collation and direction;
a failed migration rolls back. Old instances refuse writes after the upgrade.

Searches build the predicates actually selected and bind every user value.
Title/user/description/transcript contains searches retain escaped, literal LIKE
behavior. They can still scan candidate ranges; this change does not add FTS or
change Unicode matching. Account membership still uses the existing exact name
and platform rule; stable creator identities remain the separate D2 task.

Cursors retain the raw signed ordering tuple (completion-or-creation, creation,
ID), a normalized filter signature and the first-page count. Their opaque string
form prevents JavaScript rounding of 64-bit keys. An initial read transaction
keeps count and rows consistent. Each of three disjoint continuation ranges is
limited before merging, and metadata/OCR is joined only for the resulting page.
Lookahead determines whether more entries exist without recounting the archive.
Newer inserts or a deleted boundary do not shift remaining positions. This is a
live ordered traversal: later edits to ordering/search values are not frozen.
A fresh view recounts; offset requests remain available for existing callers.

Platform totals group existing source labels in SQLite. URL/platform fallback is
read only for legacy blank-source groups, including Unicode whitespace. This
preserves earlier totals without transferring every URL to Rust on each refresh.
The three desktop read commands use the blocking executor to keep database work
off the UI thread.

SQLite documents the cost of offset navigation and the use of ordering keys in
[scrolling window queries](https://sqlite.org/rowvalue.html#scrolling_window_queries),
and requires matching expressions for
[expression indexes](https://sqlite.org/expridx.html). The bounded range queries
are verified against the bundled SQLite, including timestamp collisions.

Current Unicode search rules are documented in [UNICODE_SEARCH.md](UNICODE_SEARCH.md).
Later keyword/statistics lock measurements and read-snapshot behavior are documented
in [DATABASE_CONCURRENCY.md](DATABASE_CONCURRENCY.md); the table above records the
original schema-13 comparison.
