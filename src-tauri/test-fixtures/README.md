# Synthetic schema fixtures

These files contain production schema SQL from the repository's Git history,
without any personal database or downloaded content. `integrity_tests.rs` adds
the synthetic values explicitly; the schema fixtures themselves contain only
the historical default settings/config rows.

| Fixture tag | Source commit                              |
| ----------- | ------------------------------------------ |
| v1.4.3      | `a567f1bd125f8e40180e496e8d533790989d7bf4` |
| v1.4.5      | `9d2ce8b021a18e77686bd9148cdbb701ce61eb73` |
| v1.5.0      | `02c55dc40ca4b38d64a3df9b8744252ebdfb34aa` |
| v1.6.0      | `f7e182aee9c84222a55f156b651d8e784e417821` |
| v1.7.2      | `cdfcbcdca54de1a081ffec2aed9b016cd88aebe4` |
| v1.8.0      | `e79959f1ce76b3eb462706607ea9862f1d273829` |
| v1.9.1      | `c26530127085af5c462202693538984f35719468` |
| v2.1.0      | `f5cbde00cb32ea68e9f499f448a35c9d57edde8a` |
| v2.2.0      | `a7bf14563d36e6ccd648023c7841a5abe53371bd` |

For each commit, the fixture is the first `r#"..."#` SQL batch inside production
`fn run_link_dump_migrations` in `src-tauri/src/main.rs`, excluding Rust tests.
Where that function also calls
`ensure_app_config_notifications_enabled_column`, the corresponding historical
`ALTER TABLE ... notifications_enabled ... DEFAULT 0` statement is appended.
Other historical column helpers are already represented in that release's fresh
table definitions. The v2.2.0 fixture also includes the two SHA-256 indexes
created by the later SQL batch in that same production migration function. Historical `DROP TABLE IF EXISTS link_dump_request_log`
statements are retained as part of fixture provenance and run only while making
a fresh synthetic database. The current adoption code does not drop that table.

Fixtures model fresh databases made by those release implementations. They do
not prove every real user's intermediate upgrade state. Existing partial-layout
regression tests separately cover earlier reduced history/config tables.
Version 1 of the new migration mechanism is constructed by the implemented
adoption step in a test, then isolated from version 2; it is not presented as an
earlier released schema.

See [data integrity](../../docs/DATA-INTEGRITY.md) for the migration rules,
restore procedure, tests and limits.
