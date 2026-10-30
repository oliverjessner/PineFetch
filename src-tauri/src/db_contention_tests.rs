//! Isolated reproducible lock measurements; no personal archive or media files.
use crate::database::{self, Database};
use crate::test_support::TempRoot;
use rusqlite::params;
use std::sync::Arc;
use std::time::Instant;

thread_local! {
    static ANALYSIS_WAIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static ANALYSIS_HOOK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
pub(crate) fn analysis_waiting() {
    let hook = ANALYSIS_WAIT.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
pub(crate) fn analysis_started() {
    let hook = ANALYSIS_HOOK.with(|slot| slot.borrow_mut().take());
    if let Some(hook) = hook {
        hook();
    }
}
fn before_analysis(hook: impl FnOnce() + 'static) {
    ANALYSIS_HOOK.with(|slot| {
        *slot.borrow_mut() = Some(Box::new(hook));
    });
}

fn keyword_archive() -> (TempRoot, Arc<Database>) {
    let root = TempRoot::new("keyword-locks");
    let conn = database::open(&root.path("archive.sqlite")).unwrap();
    conn.execute_batch("INSERT INTO history_entries(id,url,created_at) VALUES ('video','https://example.test/video',1);
        INSERT INTO history_ocr(history_entry_id,last_attempt_id,last_attempt_status,last_attempt_at,result_revision) VALUES ('video','fixture','completed',1,'original');").unwrap();
    let body = "Nintendo Pokémon SharedKeyword Alpha ";
    let text = body.repeat(65536);
    conn.execute(
        "INSERT INTO transcriptions(history_entry_id,text,type) VALUES ('video',?1,'text')",
        [&text],
    )
    .unwrap();
    conn.execute("INSERT INTO captions(history_entry_id,media_path,text,created_at) VALUES ('video','/fixture/video.mp4',?1,1)", [&text]).unwrap();
    let frame = body.repeat(512);
    let tx = conn.unchecked_transaction().unwrap();
    for index in 0..128 {
        tx.execute("INSERT INTO history_ocr_frames(id,history_entry_id,timestamp_ms,full_text,blocks_json) VALUES(?1,'video',?2,?3,'[]')", params![format!("frame-{index}"), index*1000, frame]).unwrap();
    }
    tx.commit().unwrap();
    (root, Arc::new(Database::new(conn)))
}
fn statistics_archive() -> (TempRoot, Arc<Database>) {
    let root = TempRoot::new("statistics-locks");
    let conn = database::open(&root.path("archive.sqlite")).unwrap();
    conn.execute_batch("WITH RECURSIVE n(i) AS (VALUES(1) UNION ALL SELECT i+1 FROM n WHERE i<200000)
        INSERT INTO history_entries(id,url,source,platform,created_at,duration_seconds,file_size_bytes)
        SELECT 'item-'||i,
            CASE i%4 WHEN 0 THEN 'https://www.youtube.com/watch?v='||i WHEN 1 THEN 'https://www.tiktok.com/@creator/video/'||i ELSE 'invalid-url-'||i END,
            CASE i%4 WHEN 0 THEN NULL WHEN 1 THEN '' WHEN 2 THEN char(8195) ELSE char(9)||char(10) END,
            CASE WHEN i%4=2 THEN 'Vimeo' ELSE NULL END, i,60,1024 FROM n;").unwrap();
    (root, Arc::new(Database::new(conn)))
}
fn report(label: &str, total: std::time::Duration) {
    let work = database::last_snapshot_work();
    eprintln!("[db-lock] {label}: total={:.3} ms; mutex wait={:.3} ms; mutex held={:.3} ms; outside={:.3} ms",
        total.as_secs_f64()*1000.,work.waiting.as_secs_f64()*1000.,work.held.as_secs_f64()*1000.,total.saturating_sub(work.waiting+work.held).as_secs_f64()*1000.);
}

#[test]
fn integration_database_analysis_lock_measurements() {
    let (_root, db) = keyword_archive();
    let start = Instant::now();
    let keywords = crate::keywords::mine(&db, "video").unwrap();
    report("keywords-7-MiB-130-sources", start.elapsed());
    assert_eq!(keywords.sources.ocr_frames, 128);
    assert_eq!(keywords.terms.len(), 4);
    assert!(keywords.terms.iter().all(|term| term.total == 196608));
    let (_root, db) = statistics_archive();
    let start = Instant::now();
    let stats = crate::history::get_history_stats_from_db(&db).unwrap();
    report("statistics-200k-legacy", start.elapsed());
    assert_eq!(stats.video_count, 200000);
    assert_eq!(
        stats.source_counts.iter().map(|row| row.count).sum::<u64>(),
        200000
    );
    db.lock()
        .unwrap()
        .execute("UPDATE history_entries SET source='youtube'", [])
        .unwrap();
    let start = Instant::now();
    let stats = crate::history::get_history_stats_from_db(&db).unwrap();
    report("statistics-200k-modern", start.elapsed());
    assert_eq!(stats.source_counts.len(), 1);
    assert_eq!(stats.source_counts[0].count, 200000);
}

fn during_analysis<T: Send + 'static>(
    db: Arc<Database>,
    analyze: impl FnOnce(&Database) -> Result<T, String> + Send + 'static,
    write: impl FnOnce(&Arc<Database>) -> Result<(), String> + Send + 'static,
) -> T {
    use std::sync::mpsc;
    use std::time::Duration;
    let (ready, waiting) = mpsc::channel();
    let (resume, continuation) = mpsc::channel();
    let reader_db = db.clone();
    let reader = std::thread::spawn(move || {
        before_analysis(move || {
            ready.send(()).unwrap();
            continuation
                .recv_timeout(crate::test_support::TEST_TIMEOUT)
                .unwrap();
        });
        analyze(&reader_db)
    });
    waiting
        .recv_timeout(crate::test_support::TEST_TIMEOUT)
        .unwrap();
    let (completed, result) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let started = Instant::now();
        let written = write(&db).map(|_| started.elapsed());
        let _ = completed.send(written);
    });
    // Always resume and join before asserting, including the old blocked case.
    let written_while_paused = result.recv_timeout(Duration::from_secs(2));
    resume.send(()).unwrap();
    let read = reader.join().unwrap();
    writer.join().unwrap();
    let duration = written_while_paused
        .expect("writer must finish while analysis is still paused")
        .unwrap();
    eprintln!(
        "[db-lock] config + completion write during paused analysis: {:.3} ms",
        duration.as_secs_f64() * 1000.
    );
    read.unwrap()
}

fn prepare_config(db: &Database) {
    crate::config::upsert_app_config_in_conn(
        &db.lock().unwrap(),
        &crate::models::AppConfig::default(),
    )
    .unwrap();
}
fn settings_and_download(db: &Arc<Database>) -> Result<(), String> {
    let config = crate::config::ConfigState::new(crate::models::AppConfig::default(), db.clone());
    crate::config::update_config(&config, |next| next.notifications_enabled = true)?;
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    let entry: crate::models::HistoryEntry = serde_json::from_value(serde_json::json!({
        "id":"new-download", "url":"https://www.youtube.com/watch?v=new-download", "created_at":300000,
        "duration_seconds":12, "file_size_bytes":100
    })).map_err(|e| e.to_string())?;
    crate::history::insert_history_entry_in_conn(&tx, &entry)?;
    tx.commit().map_err(|e| e.to_string())
}

#[test]
fn integration_keyword_analysis_releases_writers_and_keeps_every_source_from_one_snapshot() {
    let (_root, db) = keyword_archive();
    prepare_config(&db);
    let old = during_analysis(
        db.clone(),
        |db| crate::keywords::mine(db, "video"),
        |db| {
            settings_and_download(db)?;
            let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
            let tx = database::write_transaction(&conn)?;
            tx.execute("UPDATE transcriptions SET text='Replacement Replacement Replacement Replacement Replacement'", []).map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM history_ocr_frames", [])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM captions", [])
                .map_err(|e| e.to_string())?;
            tx.execute("UPDATE history_ocr SET result_revision='changed'", [])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())
        },
    );
    assert_eq!(old.sources.ocr_frames, 128);
    assert_eq!(old.sources.captions, 1);
    assert!(old.terms.iter().all(
        |term| (term.transcript, term.ocr, term.captions, term.total)
            == (65536, 65536, 65536, 196608)
    ));
    let fresh = crate::keywords::mine(&db, "video").unwrap();
    assert_eq!(fresh.terms.len(), 1);
    assert_eq!(fresh.terms[0].term, "Replacement");
    assert_eq!(fresh.sources.ocr_frames, 0);
    assert!(
        crate::config::load_config_from_db(&db.lock().unwrap())
            .unwrap()
            .notifications_enabled
    );
}

#[test]
fn integration_statistics_analysis_releases_writers_and_keeps_totals_and_sources_consistent() {
    let (_root, db) = statistics_archive();
    prepare_config(&db);
    let old = during_analysis(
        db.clone(),
        crate::history::get_history_stats_from_db,
        settings_and_download,
    );
    assert_eq!(old.video_count, 200000);
    assert_eq!(old.total_duration_seconds, 12000000);
    assert_eq!(old.total_file_size_bytes, 204800000);
    assert_eq!(
        old.source_counts.iter().map(|row| row.count).sum::<u64>(),
        old.video_count
    );
    assert!(old.source_counts.iter().all(|row| row.count == 50000));
    let fresh = crate::history::get_history_stats_from_db(&db).unwrap();
    assert_eq!(fresh.video_count, 200001);
    assert_eq!(fresh.total_duration_seconds, 12000012);
    assert_eq!(
        fresh
            .source_counts
            .iter()
            .find(|row| row.source == "youtube")
            .unwrap()
            .count,
        50001
    );
    assert!(
        crate::config::load_config_from_db(&db.lock().unwrap())
            .unwrap()
            .notifications_enabled
    );
}

#[test]
fn integration_read_snapshot_stays_consistent_across_wal_commits_and_rolls_back_on_errors() {
    let root = TempRoot::new("read-snapshot");
    let path = root.path("archive.sqlite");
    let conn = database::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute(
        "INSERT INTO history_entries(id,url,created_at) VALUES('first','https://example.test/1',1)",
        [],
    )
    .unwrap();
    let db = Database::new(conn);
    let counts = db.read_snapshot(|conn| {
        let before: u64 = conn.query_row("SELECT COUNT(*) FROM history_entries", [], |row|row.get(0)).map_err(|e|e.to_string())?;
        let other = rusqlite::Connection::open(&path).map_err(|e|e.to_string())?;
        other.execute("INSERT INTO history_entries(id,url,created_at) VALUES('second','https://example.test/2',2)", []).map_err(|e|e.to_string())?;
        let after: u64 = conn.query_row("SELECT COUNT(*) FROM history_entries", [], |row|row.get(0)).map_err(|e|e.to_string())?;
        Ok((before,after))
    }).unwrap();
    assert_eq!(counts, (1, 1));
    assert_eq!(
        db.lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history_entries", [], |row| row
                .get::<_, u64>(0))
            .unwrap(),
        2
    );
    let failed: Result<(), String> = db.read_snapshot(|conn| {
        conn.query_row("SELECT COUNT(*) FROM history_entries", [], |row| {
            row.get::<_, u64>(0)
        })
        .map_err(|e| e.to_string())?;
        Err("controlled snapshot failure".into())
    });
    assert_eq!(failed.unwrap_err(), "controlled snapshot failure");
    assert!(db.lock().unwrap().is_autocommit());
    db.lock().unwrap().execute("INSERT INTO history_entries(id,url,created_at) VALUES('after-error','https://example.test/3',3)", []).unwrap();
}

#[test]
fn integration_queued_heavy_analyses_never_reserve_the_connection_while_waiting() {
    use std::sync::mpsc;
    use std::time::Duration;
    let root = TempRoot::new("analysis-queue");
    let db = Arc::new(Database::new(
        database::open(&root.path("archive.sqlite")).unwrap(),
    ));
    prepare_config(&db);
    let (statistics, writer, completed) = db
        .analyze(|| {
            let reader_db = db.clone();
            let (ready, waiting) = mpsc::channel();
            let statistics = std::thread::spawn(move || {
                ready.send(()).unwrap();
                crate::history::get_history_stats_from_db(&reader_db)
            });
            waiting
                .recv_timeout(crate::test_support::TEST_TIMEOUT)
                .unwrap();
            let writer_db = db.clone();
            let (done, result) = mpsc::channel();
            let writer = std::thread::spawn(move || {
                let result = settings_and_download(&writer_db);
                let _ = done.send(result);
            });
            let completed = result.recv_timeout(Duration::from_secs(2));
            Ok((statistics, writer, completed))
        })
        .unwrap();
    writer.join().unwrap();
    statistics.join().unwrap().unwrap();
    completed
        .expect("a queued statistics request must not hold the shared connection")
        .unwrap();
}
