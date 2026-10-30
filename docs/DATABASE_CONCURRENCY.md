# Keyword and statistics database contention

Measured on 2026-10-30 using isolated, file-backed SQLite fixtures and the debug
Rust test profile. These single-run observations diagnose shared connection
contention; they are not portable desktop latency guarantees. No personal
archive, source media, network service or installed app was used.

## Measurements

The baseline used the original counting/classification paths inside the measured
shared critical section. Both versions used the same read-transaction boundary
and workload. Test-only instrumentation records acquisition wait and held time;
there is no production timing or query-content logging.

| Workload                              | Shared mutex held before | Shared mutex held after |   Request before / after |
| ------------------------------------- | -----------------------: | ----------------------: | -----------------------: |
| Keywords, 7.125 MiB / 130 source rows |             1,382.224 ms |                6.323 ms | 1,382.245 / 1,329.192 ms |
| Statistics, 200,000 legacy entries    |             1,068.572 ms |              238.837 ms | 1,068.611 / 1,038.498 ms |
| Statistics, 200,000 modern entries    |                79.774 ms |               72.450 ms |       79.777 / 72.484 ms |

Keywords include a transcript, 128 OCR frames and one caption with four repeated
terms. Legacy statistics include NULL, empty and Unicode-whitespace sources,
YouTube/TikTok URLs, invalid URLs, a platform fallback and unknown sources.
The modern statistics fixture has a saved source on every row.

Separate deterministic tests pause the real analysis phase and perform a
configuration patch plus a representative download-completion History write
through production transaction helpers. Those writes completed in 6.899 ms during
keyword analysis and 1.673 ms during statistics analysis. The tests gate the
analysis phase instead of guessing its duration with sleeps. They also verify
that queued heavy work cannot hold the shared connection while waiting.

## Execution and consistency

`Database::read_snapshot` holds the connection only for one SQLite read transaction
and materializes owned projections. The transaction commits and the connection
mutex is released before tokenization, term sorting, URL classification and
source-count sorting. Multiple SELECTs observe one committed database state,
including when another connection commits in WAL mode.

Keyword source order and counts include every saved frame and caption. The
existing 128 MiB text-byte budget is checked before copying, and the existing
50,000-term limit still applies during mining. No source files are opened.
Modern statistics retain SQL aggregation and the source-index path; only blank
legacy groups copy URL/platform pairs for later classification. Temporary memory
includes those projections and row metadata, so legacy statistics memory scales
with the amount of blank-source data. It is released as the analysis completes.

A separate per-profile analysis gate admits one heavy request at a time, before
it can take the connection lock or build a snapshot. This bounds concurrent
snapshot/CPU work without putting settings, queue persistence, History reads or
completion writers behind the analysis gate. SQL scanning/copying still holds
the shared connection; end-to-end computation can take about as long as before.
There is no cache, schema migration, new connection pool or journal-mode change.

An in-flight response represents its read snapshot. Later writes cannot mix new
text with old source counts or new platform totals with old archive totals. A
fresh request sees subsequent commits. Failures roll back the read transaction
and leave the connection usable.

## Reproduce

```sh
cargo test --manifest-path src-tauri/Cargo.toml --locked db_contention_tests -- --nocapture --test-threads=1
```

The same module covers settings/completion liveness during analysis, queued work,
coherent multi-source results, WAL snapshot consistency and rollback. Keyword
unit tests cover byte and term limits after the split.
