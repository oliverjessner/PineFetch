//! Large synthetic archives; no personal profile, media tools or network.
use crate::database::{self, Database};
use crate::history::{get_history_stats_from_db, search_history_page_from_db};
use crate::test_support::TempRoot;
use std::time::Instant;

fn archive() -> (TempRoot, Database) {
    let root = TempRoot::new("history-archive");
    let conn = database::open(&root.path("archive.sqlite")).unwrap();
    conn.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<200000)
        INSERT INTO history_entries(id,url,title,uploader,source,created_at,completed_at,duration_seconds,file_size_bytes)
        SELECT printf('item-%06d',i),'https://www.youtube.com/watch?v='||i,'Video '||i,
        CASE WHEN i%10=0 THEN 'Frequent Creator' ELSE 'Creator '||(i%1000) END,
        CASE WHEN i%2=0 THEN 'youtube' ELSE 'tiktok' END, i,
        CASE WHEN i%7=0 THEN NULL ELSE i/3 END, 60,1024 FROM n;").unwrap();
    (root, Database::new(conn))
}

#[test]
fn integration_large_archive_query_measurements() {
    let (_root, db) = archive();
    for (label, offset, query, source) in [
        ("history-first", 0, None, None),
        ("history-deep-offset", 180_000, None, None),
        ("source-first", 0, None, Some("youtube")),
        (
            "account-first",
            0,
            Some("Frequent Creator"),
            Some("youtube"),
        ),
        (
            "account-deep-offset",
            18_000,
            Some("Frequent Creator"),
            Some("youtube"),
        ),
    ] {
        let start = Instant::now();
        let page = search_history_page_from_db(&db, 20, offset, query, Some("user"), source, true)
            .unwrap();
        eprintln!(
            "[archive] {label}: {:.3} ms; total={}",
            start.elapsed().as_secs_f64() * 1000.,
            page.total_count
        );
        assert_eq!(page.entries.len(), 20);
        let work = crate::history_query::last_query_work();
        assert!(
            work.sorts <= 1,
            "{label} may only sort the bounded output page"
        );
        let start = Instant::now();
        let next = crate::history_query::read_page(
            &db,
            20,
            0,
            crate::history_query::HistoryFilter::new(query, Some("user"), source, true).unwrap(),
            page.next_cursor.as_deref(),
        )
        .unwrap();
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let work = crate::history_query::last_query_work();
        eprintln!(
            "[archive] {label} cursor: {elapsed:.3} ms; page VM steps={}; count VM steps={}; sort operations={}",
            work.page_steps, work.count_steps,work.sorts
        );
        assert_eq!(next.entries.len(), 20);
        assert_eq!(
            work.count_steps, 0,
            "continuations must not recount the archive"
        );
        assert!(work.sorts <= 4);
        assert!(
            work.page_steps < 5000,
            "cursor page must seek instead of scanning earlier rows: {}",
            work.page_steps
        );
    }
    let start = Instant::now();
    assert_eq!(get_history_stats_from_db(&db).unwrap().video_count, 200_000);
    eprintln!(
        "[archive] statistics: {:.3} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
}

fn all_filter() -> crate::history_query::HistoryFilter {
    crate::history_query::HistoryFilter::new(None, None, None, false).unwrap()
}
fn small_archive() -> Database {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    database::initialize(&conn).unwrap();
    conn.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<65)
        INSERT INTO history_entries(id,url,title,uploader,source,created_at,completed_at)
        SELECT printf('row-%03d',i),'https://example.com/'||i,'Video '||i,'Creator',
        CASE WHEN i%2=0 THEN 'YouTube' ELSE 'youtube' END, i/2,CASE WHEN i%3=0 THEN NULL ELSE i/5 END FROM n;
        INSERT INTO history_entries(id,url,source,uploader,created_at,completed_at) VALUES
        ('negative','https://example.com/negative','youtube','Creator',-10,-5),
        ('max-a','https://example.com/a','youtube','Creator',9223372036854775806,9223372036854775807),
        ('max-b','https://example.com/b','youtube','Creator',9223372036854775807,NULL);").unwrap();
    Database::new(conn)
}
fn ordered_ids(db: &Database) -> Vec<String> {
    db.lock().unwrap().prepare("SELECT id FROM history_entries ORDER BY COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC").unwrap()
        .query_map([],|row|row.get(0)).unwrap().collect::<Result<_,_>>().unwrap()
}

#[test]
fn cursor_pages_keep_sqlite_order_with_ties_nulls_and_signed_64_bit_keys() {
    use crate::history_query::read_page;
    let db = small_archive();
    let expected = ordered_ids(&db);
    for limit in [1, 2, 7, 20, 100] {
        let mut cursor = None;
        let mut actual = Vec::new();
        loop {
            let page = read_page(&db, limit, 0, all_filter(), cursor.as_deref()).unwrap();
            assert_eq!(page.total_count, expected.len() as u64);
            actual.extend(page.entries.into_iter().map(|entry| entry.id));
            assert_eq!(page.has_more, page.next_cursor.is_some());
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(actual, expected);
    }
}

#[test]
fn cursor_navigation_does_not_repeat_or_skip_rows_after_inserts_and_deletions() {
    use crate::history_query::read_page;
    let db = small_archive();
    let original = ordered_ids(&db);
    let first = read_page(&db, 20, 0, all_filter(), None).unwrap();
    let deleted = first.entries.last().unwrap().id.clone();
    {
        let conn = db.lock().unwrap();
        // Remove the cursor's boundary and another already loaded row; add a
        // newer row before the current position. Offset pagination shifts here.
        conn.execute(
            "DELETE FROM history_entries WHERE id=?1 OR id=?2",
            rusqlite::params![deleted, first.entries[0].id],
        )
        .unwrap();
        conn.execute("INSERT INTO history_entries(id,url,created_at,completed_at) VALUES ('new','https://example.com/new',9223372036854775807,9223372036854775807)",[]).unwrap();
    }
    let mut actual: Vec<_> = first.entries.into_iter().map(|entry| entry.id).collect();
    let mut cursor = first.next_cursor;
    while let Some(after) = cursor {
        let page = read_page(&db, 7, 0, all_filter(), Some(&after)).unwrap();
        assert_eq!(page.total_count, original.len() as u64); // first-page count
        actual.extend(page.entries.into_iter().map(|entry| entry.id));
        cursor = page.next_cursor;
    }
    assert_eq!(actual, original);
    assert_eq!(
        read_page(&db, 20, 0, all_filter(), None)
            .unwrap()
            .total_count,
        67
    );
}

#[test]
fn cursor_is_bound_to_filters_and_refuses_malformed_or_conflicting_requests() {
    use crate::history_query::{read_page, HistoryFilter};
    let db = small_archive();
    let cursor = read_page(&db, 3, 0, all_filter(), None)
        .unwrap()
        .next_cursor
        .unwrap();
    for filter in [
        HistoryFilter::new(Some("Video"), None, None, false).unwrap(),
        HistoryFilter::new(None, Some("description"), None, false).unwrap(),
        HistoryFilter::new(None, None, Some("tiktok"), false).unwrap(),
        HistoryFilter::new(Some("Creator"), Some("user"), Some("youtube"), true).unwrap(),
    ] {
        assert!(read_page(&db, 3, 0, filter, Some(&cursor)).is_err());
    }
    for bad in ["not json".to_owned(), "null".into(), "x".repeat(4097)] {
        assert!(read_page(&db, 3, 0, all_filter(), Some(&bad)).is_err());
    }
    let mut unsupported: serde_json::Value = serde_json::from_str(&cursor).unwrap();
    unsupported["version"] = 2.into();
    assert!(read_page(&db, 3, 0, all_filter(), Some(&unsupported.to_string())).is_err());
    assert!(read_page(&db, 3, 1, all_filter(), Some(&cursor)).is_err());
    assert!(
        HistoryFilter::new(None, Some("title; DROP TABLE history_entries"), None, false).is_err()
    );
    assert_eq!(ordered_ids(&db).len(), 68);
}

#[test]
fn account_cursor_keeps_exact_creator_case_platform_and_complete_results() {
    let db = small_archive();
    db.lock()
        .unwrap()
        .execute_batch(
            "INSERT INTO history_entries(id,url,uploader,source,created_at) VALUES
        ('other-case','https://example.com/a','creator','youtube',90),
        ('other-platform','https://example.com/b','Creator','tiktok',91),
        ('similar-name','https://example.com/c','Creator Extra','youtube',92);",
        )
        .unwrap();
    let first = crate::accounts::get_page(&db, "row-001", 0, None).unwrap();
    let mut page = serde_json::to_value(first).unwrap();
    let mut ids = Vec::new();
    loop {
        for entry in page["saved"]["entries"].as_array().unwrap() {
            assert_eq!(entry["uploader"], "Creator");
            assert_eq!(entry["source"], "youtube");
            ids.push(entry["id"].as_str().unwrap().to_owned());
        }
        let Some(cursor) = page["saved"]["next_cursor"].as_str() else {
            break;
        };
        page = serde_json::to_value(
            crate::accounts::get_page(&db, "row-001", 0, Some(cursor)).unwrap(),
        )
        .unwrap();
        assert_eq!(crate::history_query::last_query_work().count_steps, 0);
    }
    assert_eq!(ids, ordered_ids(&small_archive()));
}

#[test]
fn integration_schema13_indexes_migrate_with_backup_rollback_and_definition_validation() {
    use crate::database::MigrationPoint;
    let root = TempRoot::new("history-index-migration");
    let path = root.path("archive.sqlite");
    let conn = database::open(&path).unwrap();
    database::restore_schema12_history_indexes_fixture(&conn);
    conn.execute_batch("PRAGMA user_version=12; INSERT INTO history_entries(id,url,title,created_at) VALUES('keep','https://example.com/keep','Keep me',42);").unwrap();
    assert!(database::initialize_with_hook(&conn, |step| {
        if step == MigrationPoint::HistoryIndexes {
            Err("synthetic index migration failure".into())
        } else {
            Ok(())
        }
    })
    .is_err());
    assert_eq!(
        conn.pragma_query_value::<i64, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        12
    );
    assert_eq!(
        conn.query_row::<i64, _, _>(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='idx_history_order'",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        0
    );
    database::initialize(&conn).unwrap();
    assert_eq!(
        conn.query_row::<String, _, _>(
            "SELECT title FROM history_entries WHERE id='keep'",
            [],
            |row| row.get(0)
        )
        .unwrap(),
        "Keep me"
    );
    assert!(std::fs::read_dir(root.root()).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .contains("pre-schema-12")));
    drop(conn);
    let conn = database::open(&path).unwrap();
    conn.execute_batch("DROP INDEX idx_history_order; CREATE INDEX idx_history_order ON history_entries(COALESCE(created_at,completed_at) DESC,created_at DESC,id DESC);").unwrap();
    drop(conn);
    assert!(database::open(&path)
        .err()
        .unwrap()
        .contains("idx_history_order"));
}

#[test]
fn integration_cursor_seeks_with_twenty_thousand_identical_timestamps() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    database::initialize(&conn).unwrap();
    conn.execute_batch(
        "WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<20000)
        INSERT INTO history_entries(id,url,source,uploader,created_at,completed_at)
        SELECT printf('tie-%06d',i),'https://example.com/'||i,'youtube','Creator',1,1 FROM n;",
    )
    .unwrap();
    let db = Database::new(conn);
    let boundary = search_history_page_from_db(&db, 20, 18000, None, None, None, false).unwrap();
    let page =
        crate::history_query::read_page(&db, 20, 0, all_filter(), boundary.next_cursor.as_deref())
            .unwrap();
    assert_eq!(page.entries[0].id, "tie-001980");
    let work = crate::history_query::last_query_work();
    assert_eq!(work.count_steps, 0);
    assert!(
        work.page_steps < 5000,
        "equal timestamps must still seek directly: {}",
        work.page_steps
    );
}

#[test]
fn statistics_preserve_legacy_unicode_blank_sources_and_url_platform_fallbacks() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    database::initialize(&conn).unwrap();
    for (id, url, source, platform) in [
        ("tabs", "https://youtu.be/abc", Some("\t\n"), None),
        ("unicode", "invalid URL", Some("\u{2003}"), Some("Vimeo")),
        (
            "missing",
            "https://www.tiktok.com/@creator/video/1",
            None,
            None,
        ),
        ("space", "https://example.com/1", Some(" YouTube "), None),
        ("case", "https://example.com/2", Some("YOUTUBE"), None),
    ] {
        conn.execute(
            "INSERT INTO history_entries(id,url,source,platform,created_at) VALUES(?1,?2,?3,?4,1)",
            rusqlite::params![id, url, source, platform],
        )
        .unwrap();
    }
    let stats = get_history_stats_from_db(&Database::new(conn)).unwrap();
    assert_eq!(stats.video_count, 5);
    assert_eq!(
        stats
            .source_counts
            .into_iter()
            .map(|row| (row.source, row.count))
            .collect::<Vec<_>>(),
        vec![
            ("youtube".into(), 3),
            ("tiktok".into(), 1),
            ("vimeo".into(), 1)
        ]
    );
}

#[test]
fn integration_unicode_search_keeps_large_archive_cursor_order_and_bounded_sorting() {
    let (_root, db) = archive();
    let original = "O\u{308}sterreich Straße";
    let expected = {
        let conn = db.lock().unwrap();
        conn.execute(
            "UPDATE history_entries SET title=?1 WHERE CAST(substr(id,6) AS INTEGER)%10000=0",
            [original],
        )
        .unwrap();
        let mut statement = conn.prepare("SELECT id FROM history_entries WHERE title=?1 ORDER BY COALESCE(completed_at,created_at) DESC,created_at DESC,id DESC").unwrap();
        let rows = statement
            .query_map([original], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        rows
    };
    let filter = |query| {
        crate::history_query::HistoryFilter::new(Some(query), Some("title"), Some("youtube"), false)
            .unwrap()
    };
    let start = Instant::now();
    let first =
        crate::history_query::read_page(&db, 7, 0, filter("ÖSTERREICH STRASSE"), None).unwrap();
    eprintln!(
        "[archive] unicode-title-first: {:.3} ms; total={}",
        start.elapsed().as_secs_f64() * 1000.,
        first.total_count
    );
    assert_eq!(first.total_count, 20);
    assert!(crate::history_query::last_query_work().sorts <= 1);
    let second = crate::history_query::read_page(
        &db,
        7,
        0,
        filter("österreich straße"),
        first.next_cursor.as_deref(),
    )
    .unwrap();
    let last = crate::history_query::read_page(
        &db,
        7,
        0,
        filter("O\u{308}STERREICH STRASSE"),
        second.next_cursor.as_deref(),
    )
    .unwrap();
    assert!(!last.has_more && last.next_cursor.is_none());
    let found = first
        .entries
        .into_iter()
        .chain(second.entries)
        .chain(last.entries)
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    assert_eq!(found, expected);
}
