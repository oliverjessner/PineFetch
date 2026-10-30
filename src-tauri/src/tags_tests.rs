use crate::database::{self, Database};
use crate::history_query::{self, HistoryFilter};
use crate::tags::{self, TagInput};
use crate::test_support::TempRoot;
use rusqlite::{params, Connection};
use std::time::Instant;
fn input(name: &str, color: &str) -> TagInput {
    TagInput {
        name: name.into(),
        color: color.into(),
        description: " Research ".into(),
    }
}
fn db() -> Database {
    let conn = Connection::open_in_memory().unwrap();
    database::initialize(&conn).unwrap();
    seed(&conn);
    Database::new(conn)
}
fn seed(conn: &Connection) {
    conn.execute_batch("INSERT INTO history_entries(id,url,title,source,uploader,created_at,creator_source,creator_id) VALUES
 ('one','https://youtube.com/watch?v=abcdefghijk','Österreich','youtube','Creator',1,'youtube','channel:UCaaaaaaaaaaaaaaaaaaaaaa'),
 ('two','https://youtube.com/watch?v=lmnopqrstuv','Second','YouTube','Renamed',2,'youtube','channel:UCaaaaaaaaaaaaaaaaaaaaaa'),
 ('other','https://tiktok.com/@creator/video/12345678','Other','tiktok','Creator',3,'tiktok','user:123'),
 ('legacy','https://youtube.com/watch?v=abcdefghijk','Legacy','youtube','Creator',4,NULL,NULL),
 ('legacy-two','https://youtube.com/watch?v=lmnopqrstuv','Legacy two','youtube','Creator',5,NULL,NULL),
 ('missing','https://example.test','No creator',NULL,NULL,6,NULL,NULL);").unwrap();
}
#[test]
fn definitions_validate_unicode_names_lengths_colors_and_atomic_renames() {
    let db = db();
    let first = tags::create(&db, input("  Straße  ", "#abc")).unwrap();
    assert_eq!(first.name, "Straße");
    assert_eq!(first.description, "Research");
    assert_eq!(first.color, "#AABBCC");
    assert!(tags::create(&db, input("STRASSE", "#abcdef"))
        .unwrap_err()
        .contains("already exists"));
    for name in ["", " \u{2003}", "bad\nname", &"x".repeat(81)] {
        assert!(tags::create(&db, input(name, "#123456")).is_err());
    }
    for color in ["", "red", "##123", "#12345g", "#12345678", "<img>"] {
        assert!(tags::create(&db, input("Valid", color)).is_err(), "{color}");
    }
    let mut long = input("Description", "#abcdef");
    long.description = "🌲".repeat(501);
    assert!(tags::create(&db, long).is_err());
    let accent = tags::create(&db, input("Österreich", "123456")).unwrap();
    assert!(tags::create(&db, input("O\u{308}STERREICH", "#ffffff")).is_err());
    assert!(tags::update(&db, &accent.id, input("strasse", "#111111")).is_err());
    assert_eq!(tags::get(&db, &accent.id).unwrap().tag.name, "Österreich");
    let updated = tags::update(&db, &first.id, input("Investigation", "#0f0")).unwrap();
    assert_eq!(updated.id, first.id);
    assert_eq!(updated.created_at, first.created_at);
    assert!(updated.updated_at >= first.updated_at);
    assert!(tags::get(&db, "' OR 1=1").is_err());
}
#[test]
fn assignments_are_many_to_many_idempotent_batched_and_never_delete_content() {
    let db = db();
    let a = tags::create(&db, input("A", "#ff0000")).unwrap();
    let b = tags::create(&db, input("B", "#00ff00")).unwrap();
    for (entry, tag) in [("one", &a), ("one", &b), ("two", &a)] {
        tags::assign(&db, entry, &tag.id, true).unwrap();
        tags::assign(&db, entry, &tag.id, true).unwrap();
    }
    let batch = tags::batch(&db, &["one".into(), "two".into(), "missing".into()]).unwrap();
    assert_eq!(
        batch
            .iter()
            .find(|row| row.history_entry_id == "one")
            .unwrap()
            .tags
            .len(),
        2
    );
    let summary = tags::get(&db, &a.id).unwrap();
    assert_eq!((summary.content_count, summary.account_count), (2, 1));
    tags::update(&db, &a.id, input("Renamed", "#aabbcc")).unwrap();
    assert_eq!(tags::get(&db, &a.id).unwrap().content_count, 2);
    tags::assign(&db, "two", &a.id, false).unwrap();
    tags::assign(&db, "two", &a.id, false).unwrap();
    tags::delete(&db, &a.id).unwrap();
    assert!(crate::accounts::get(&db, "one", 0).is_ok());
    assert_eq!(
        tags::batch(&db, &["one".into()]).unwrap()[0].tags[0].id,
        b.id
    );
    assert!(tags::assign(&db, "one", &a.id, true).is_err());
    assert!(tags::assign(&db, "gone", &b.id, true).is_err());
    crate::history::delete_history_entry_from_db(&db, "one").unwrap();
    tags::assign(&db, "one", &b.id, false).unwrap();
    assert_eq!(tags::get(&db, &b.id).unwrap().content_count, 0);
    tags::assign(&db, "two", &b.id, true).unwrap();
    crate::history::clear_history_entries_in_db(&db).unwrap();
    assert_eq!(tags::list(&db).unwrap().len(), 1);
    assert_eq!(tags::get(&db, &b.id).unwrap().content_count, 0);
}
#[test]
fn counts_derive_current_creator_ids_keep_unverified_separate_and_exclude_missing_identity() {
    let db = db();
    let tag = tags::create(&db, input("Research", "#123456")).unwrap();
    for id in ["one", "two", "other", "legacy", "legacy-two", "missing"] {
        tags::assign(&db, id, &tag.id, true).unwrap();
    }
    assert_eq!(
        (
            tags::get(&db, &tag.id).unwrap().content_count,
            tags::get(&db, &tag.id).unwrap().account_count
        ),
        (6, 4)
    );
    let mut cursor = None;
    let mut entries = Vec::new();
    loop {
        let page = tags::accounts(&db, &tag.id, "", 1, cursor.as_deref()).unwrap();
        assert_eq!(page.total_count, 4);
        entries.extend(page.entries);
        if !page.has_more {
            break;
        }
        cursor = page.next_cursor;
    }
    assert_eq!(entries.len(), 4);
    assert_eq!(
        entries.iter().filter(|row| !row.identity_verified).count(),
        2
    );
    assert_eq!(
        entries
            .iter()
            .find(|row| row.identity_verified && row.source == "youtube")
            .unwrap()
            .content_count,
        2
    );
    for account in entries {
        crate::accounts::get(&db, &account.history_entry_id, 0).unwrap();
    }
    let first = tags::accounts(&db, &tag.id, "", 1, None).unwrap();
    let second = tags::create(&db, input("Other", "#123456")).unwrap();
    assert!(tags::accounts(&db, &second.id, "", 1, first.next_cursor.as_deref()).is_err());
    assert!(tags::accounts(&db, &tag.id, "Changed", 1, first.next_cursor.as_deref()).is_err());
}
#[test]
fn tagged_content_combines_unicode_search_source_filters_and_tag_bound_cursors() {
    let db = db();
    let a = tags::create(&db, input("A", "#123456")).unwrap();
    let b = tags::create(&db, input("B", "#123456")).unwrap();
    for id in ["one", "two", "other", "missing"] {
        tags::assign(&db, id, &a.id, true).unwrap();
    }
    let filter = || {
        HistoryFilter::new(None, None, None, false)
            .unwrap()
            .with_tag(Some(&a.id))
            .unwrap()
    };
    let first = history_query::read_page(&db, 2, 0, filter(), None).unwrap();
    assert_eq!(first.total_count, 4);
    assert!(first.has_more);
    let second =
        history_query::read_page(&db, 2, 0, filter(), first.next_cursor.as_deref()).unwrap();
    assert_eq!(second.entries.len(), 2);
    assert!(!second.has_more);
    assert!(second
        .entries
        .iter()
        .all(|entry| !first.entries.iter().any(|old| old.id == entry.id)));
    assert!(history_query::read_page(
        &db,
        2,
        0,
        HistoryFilter::new(None, None, None, false)
            .unwrap()
            .with_tag(Some(&b.id))
            .unwrap(),
        first.next_cursor.as_deref()
    )
    .is_err());
    let result = history_query::read_page(
        &db,
        20,
        0,
        HistoryFilter::new(
            Some("o\u{308}STERREICH"),
            Some("title"),
            Some("youtube"),
            false,
        )
        .unwrap()
        .with_tag(Some(&a.id))
        .unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(result.total_count, 1);
    assert_eq!(result.entries[0].id, "one");
}
#[test]
fn failed_transactions_leave_definitions_and_membership_unchanged() {
    let db = db();
    let tag = tags::create(&db, input("Original", "#123456")).unwrap();
    tags::assign(&db, "one", &tag.id, true).unwrap();
    db.lock().unwrap().execute_batch("CREATE TRIGGER fail_tag_update BEFORE UPDATE ON tags BEGIN SELECT RAISE(ABORT,'fixture'); END; CREATE TRIGGER fail_tag_delete BEFORE DELETE ON tags BEGIN SELECT RAISE(ABORT,'fixture'); END; CREATE TRIGGER fail_assignment BEFORE INSERT ON history_entry_tags BEGIN SELECT RAISE(ABORT,'fixture'); END;").unwrap();
    assert!(tags::update(&db, &tag.id, input("Changed", "#654321")).is_err());
    assert!(tags::delete(&db, &tag.id).is_err());
    assert!(tags::assign(&db, "two", &tag.id, true).is_err());
    assert_eq!(tags::get(&db, &tag.id).unwrap().tag.name, "Original");
    assert_eq!(tags::get(&db, &tag.id).unwrap().content_count, 1);
}
#[test]
fn integration_schema15_backs_up_rolls_back_preserves_history_and_recovers_tag_data() {
    let root = TempRoot::new("tags-migration");
    let path = root.path("archive.sqlite");
    let conn = database::open(&path).unwrap();
    database::restore_schema14_tags_fixture(&conn);
    seed(&conn);
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    assert!(
        database::open_with_hook(&path, |point| if point == database::MigrationPoint::Tags {
            Err("fixture migration failure".into())
        } else {
            Ok(())
        })
        .is_err()
    );
    assert_eq!(before, std::fs::read(&path).unwrap());
    let db = Database::new(database::open(&path).unwrap());
    let tag = tags::create(&db, input("Persistent 🌲", "#123456")).unwrap();
    tags::assign(&db, "one", &tag.id, true).unwrap();
    drop(db);
    let db = Database::new(database::open(&path).unwrap());
    assert_eq!(tags::get(&db, &tag.id).unwrap().content_count, 1);
    assert_eq!(
        tags::batch(&db, &["one".into()]).unwrap()[0].tags[0].name,
        "Persistent 🌲"
    );
    assert!(std::fs::read_dir(root.root())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains("pre-schema-14")));
}
#[test]
fn integration_large_tag_archives_measure_counts_content_accounts_and_batch_indexes() {
    let root = TempRoot::new("tags-large");
    let conn = database::open(&root.path("archive.sqlite")).unwrap();
    conn.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<200000) INSERT INTO history_entries(id,url,title,uploader,source,creator_source,creator_id,created_at) SELECT printf('row-%06d',i),'https://example.test/'||i,'Video '||i,'Creator '||(i%1000),'youtube','youtube','user:'||(i%1000),i FROM n;").unwrap();
    let db = Database::new(conn);
    let dense = tags::create(&db, input("Dense", "#123456")).unwrap();
    let sparse = tags::create(&db, input("Sparse", "#123456")).unwrap();
    db.lock().unwrap().execute("INSERT INTO history_entry_tags SELECT id,?1,1 FROM history_entries WHERE created_at%2=0",[&dense.id]).unwrap();
    db.lock().unwrap().execute("INSERT INTO history_entry_tags SELECT id,?1,1 FROM history_entries WHERE created_at%19999=0",[&sparse.id]).unwrap();
    for tag in [&dense, &sparse] {
        let filter = || {
            HistoryFilter::new(None, None, None, false)
                .unwrap()
                .with_tag(Some(&tag.id))
                .unwrap()
        };
        let start = Instant::now();
        let first = history_query::read_page(&db, 20, 0, filter(), None).unwrap();
        let work = history_query::last_query_work();
        assert!(
            work.page_steps < 5000,
            "tag page must not scan the archive or sort all assignments"
        );
        eprintln!(
            "[tags] {} first: {:.2} ms; count steps {}; page steps {}",
            tag.name,
            start.elapsed().as_secs_f64() * 1000.,
            work.count_steps,
            work.page_steps
        );
        if let Some(cursor) = first.next_cursor {
            let start = Instant::now();
            history_query::read_page(&db, 20, 0, filter(), Some(&cursor)).unwrap();
            eprintln!(
                "[tags] {} continuation: {:.2} ms; steps {}",
                tag.name,
                start.elapsed().as_secs_f64() * 1000.,
                history_query::last_query_work().page_steps
            );
        }
    }
    for (label, result) in [
        ("overview", tags::list(&db).map(|rows| rows.len())),
        (
            "accounts",
            tags::accounts(&db, &dense.id, "", 20, None).map(|page| page.entries.len()),
        ),
    ] {
        assert!(result.unwrap() > 0, "{label}");
    }
    let conn = db.lock().unwrap();
    let plan = conn
        .prepare(
            "EXPLAIN QUERY PLAN SELECT history_entry_id FROM history_entry_tags WHERE tag_id=?1",
        )
        .unwrap()
        .query_map([&dense.id], |r| r.get::<_, String>(3))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join(" ");
    assert!(plan.contains("idx_history_entry_tags_tag"));
    let count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM history_entry_tags WHERE tag_id=?1",
            params![dense.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 100000);
    drop(conn);
    let start = Instant::now();
    let rows = tags::list(&db).unwrap();
    assert_eq!(rows.len(), 2);
    eprintln!(
        "[tags] overview counts: {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    let start = Instant::now();
    let accounts = tags::accounts(&db, &dense.id, "", 20, None).unwrap();
    assert_eq!(accounts.total_count, 500);
    eprintln!(
        "[tags] derived accounts: {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    let renamed = tags::update(&db, &dense.id, input("Dense renamed", "#abcdef")).unwrap();
    assert_eq!(renamed.id, dense.id);
    assert_eq!(tags::get(&db, &dense.id).unwrap().content_count, 100000);
    let start = Instant::now();
    tags::delete(&db, &dense.id).unwrap();
    eprintln!(
        "[tags] delete 100k assignments: {:.2} ms",
        start.elapsed().as_secs_f64() * 1000.
    );
    assert_eq!(
        db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history_entries", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        200000
    );
    assert_eq!(tags::choices(&db).unwrap().len(), 1);
}

#[test]
fn integration_tags_schema_refuses_changed_membership_index_without_mutating_the_archive() {
    let root = TempRoot::new("tags-index-integrity");
    let path = root.path("archive.sqlite");
    let conn = database::open(&path).unwrap();
    conn.execute_batch("DROP INDEX idx_history_entry_tags_tag; CREATE INDEX idx_history_entry_tags_tag ON history_entry_tags(tag_id DESC,history_entry_id DESC);").unwrap();
    drop(conn);
    let before = std::fs::read(&path).unwrap();
    assert!(database::open(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
