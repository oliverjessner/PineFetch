//! Account navigation reads local History; network actions are always explicit.
use crate::database::{self, Database};
use crate::history::search_history_page_from_db;
use crate::models::HistoryPage;
use crate::process::ProcessState;
use crate::social_insights::{self, SocialInsightsTools};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

const PAGE_SIZE: u32 = 50;
#[derive(Default)]
pub(crate) struct AccountState {
    active: Mutex<HashSet<(String, String, String)>>,
}
struct Guard<'a> {
    state: &'a AccountState,
    key: (String, String, String),
}
impl Drop for Guard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.state.active.lock() {
            active.remove(&self.key);
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
struct Context {
    id: String,
    url: String,
    source: String,
    uploader: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AccountSnapshot {
    pub(crate) source: String,
    pub(crate) uploader: String,
    pub(crate) profile_url: String,
    pub(crate) fetched_at: u64,
    pub(crate) fetchary_version: String,
    pub(crate) data: Value,
}
#[derive(Serialize)]
pub(crate) struct AccountPage {
    source: String,
    uploader: String,
    saved: HistoryPage,
    insights: Option<AccountSnapshot>,
}
#[derive(Debug, Serialize)]
pub(crate) struct ProfileVideo {
    url: String,
    title: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct ProfileVideos {
    profile_url: String,
    entries: Vec<ProfileVideo>,
    next_offset: u32,
    has_more: bool,
}
fn context(conn: &rusqlite::Connection, id: &str) -> Result<Context, String> {
    let (url, source, uploader): (String, Option<String>, Option<String>) = conn
        .query_row(
            "SELECT url,source,uploader FROM history_entries WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| e.to_string())?
        .ok_or("This History entry no longer exists.")?;
    let source = source
        .filter(|s| !s.trim().is_empty())
        .or_else(|| crate::history_rules::source_from_url(&url))
        .ok_or("The platform is unknown.")?
        .trim()
        .to_ascii_lowercase();
    let uploader = uploader
        .filter(|s| !s.trim().is_empty())
        .ok_or("No account is saved for this video.")?;
    Ok(Context {
        id: id.into(),
        url,
        source,
        uploader,
    })
}
fn current(db: &Database, id: &str) -> Result<Context, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    context(&conn, id)
}
fn reserve<'a>(state: &'a AccountState, ctx: &Context, action: &str) -> Result<Guard<'a>, String> {
    let key = (ctx.source.clone(), ctx.uploader.clone(), action.to_string());
    if !state
        .active
        .lock()
        .map_err(|_| "Account lock poisoned")?
        .insert(key.clone())
    {
        return Err("This account is already loading. Please wait.".into());
    }
    Ok(Guard { state, key })
}
fn snapshot(db: &Database, ctx: &Context) -> Result<Option<AccountSnapshot>, String> {
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let row: Option<(String,u64,String,String)> = conn.query_row(
        "SELECT profile_url,fetched_at,fetchary_version,payload_json FROM history_account_insights WHERE source=?1 AND uploader=?2",
        params![ctx.source,ctx.uploader], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|e| e.to_string())?;
    row.map(|(profile_url, fetched_at, fetchary_version, json)| {
        let data =
            serde_json::from_str(&json).map_err(|_| "Saved account insights could not be read.")?;
        social_insights::validate_data(&fetchary_version, &data)?;
        validate_profile(&profile_url, &ctx.source)?;
        Ok(AccountSnapshot {
            source: ctx.source.clone(),
            uploader: ctx.uploader.clone(),
            profile_url,
            fetched_at,
            fetchary_version,
            data,
        })
    })
    .transpose()
}
pub(crate) fn get(db: &Database, id: &str, offset: u32) -> Result<AccountPage, String> {
    let ctx = current(db, id)?;
    let saved = search_history_page_from_db(
        db,
        20,
        offset,
        Some(&ctx.uploader),
        Some("user"),
        Some(&ctx.source),
        true,
    )?;
    let insights = snapshot(db, &ctx)?;
    Ok(AccountPage {
        source: ctx.source,
        uploader: ctx.uploader,
        saved,
        insights,
    })
}
fn validate_profile(raw: &str, source: &str) -> Result<String, String> {
    let url = url::Url::parse(raw).map_err(|_| "No valid account URL was exposed.")?;
    let host = url
        .host_str()
        .unwrap_or("")
        .trim_start_matches("www.")
        .trim_start_matches("m.");
    let domains: &[&str] = match source {
        "youtube" => &["youtube.com"],
        "tiktok" => &["tiktok.com"],
        "instagram" => &["instagram.com"],
        "x" | "twitter" => &["x.com", "twitter.com"],
        "facebook" => &["facebook.com"],
        "vimeo" => &["vimeo.com"],
        "twitch" => &["twitch.tv"],
        "reddit" => &["reddit.com"],
        "linkedin" => &["linkedin.com"],
        _ => return Err("Profile browsing is unavailable for this platform.".into()),
    };
    let parts: Vec<_> = url.path().split('/').filter(|p| !p.is_empty()).collect();
    let valid_path = match source {
        "youtube" => {
            matches!(parts.as_slice(), [handle] if handle.starts_with('@'))
                || matches!(parts.as_slice(), ["channel" | "user" | "c", _])
        }
        "tiktok" => matches!(parts.as_slice(), [handle] if handle.starts_with('@')),
        "reddit" => matches!(parts.as_slice(), ["user" | "u", _]),
        "linkedin" => matches!(parts.as_slice(), ["in" | "company", _]),
        _ => {
            parts.len() == 1
                && ![
                    "watch", "reel", "reels", "shorts", "video", "videos", "p", "status", "home",
                    "explore",
                ]
                .contains(&parts[0])
        }
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !domains.contains(&host)
        || !valid_path
    {
        return Err("No verified account URL was exposed for this video.".into());
    }
    let mut clean = url.clone();
    clean.set_fragment(None);
    clean.set_query(None);
    if source == "facebook" && url.path().trim_end_matches('/') == "/profile.php" {
        let id = url
            .query_pairs()
            .find(|(key, _)| key == "id")
            .map(|(_, value)| value.into_owned())
            .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()))
            .ok_or("No verified Facebook account ID was exposed.")?;
        clean.query_pairs_mut().append_pair("id", &id);
    }
    Ok(clean.to_string().trim_end_matches('/').to_string())
}
fn resolve_profile(
    db: &Database,
    ctx: &Context,
    processes: &ProcessState,
    yt_dlp: &str,
    deno: Option<&str>,
) -> Result<String, String> {
    if let Some(saved) = snapshot(db, ctx)? {
        if matches!(saved.data["status"].as_str(), Some("available" | "partial")) {
            return Ok(saved.profile_url);
        }
    }
    crate::url_rules::validate_download_url(&ctx.url).map_err(|e| e.to_string())?;
    let config =
        crate::config::load_config_from_db(&*db.lock().map_err(|_| "SQLite lock poisoned")?)?;
    let raw = crate::metadata::load_raw_info(
        yt_dlp,
        deno,
        &ctx.url,
        processes,
        config.youtube_cookie_browser.as_deref(),
    )?;
    let data: Value = serde_json::from_str(&raw).map_err(|_| "Invalid account metadata.")?;
    // TikTok's channel_url contains the internal secUid, while uploader_url
    // contains the public handle that Fetchary can verify against the page.
    let fields = if ctx.source == "tiktok" {
        ["uploader_url", "channel_url"]
    } else {
        ["channel_url", "uploader_url"]
    };
    for field in fields {
        if let Some(raw) = data[field].as_str() {
            if let Ok(url) = validate_profile(raw, &ctx.source) {
                return Ok(url);
            }
        }
    }
    Err("The platform did not expose a verified account URL for this video.".into())
}
fn save(db: &Database, ctx: &Context, saved: AccountSnapshot) -> Result<AccountSnapshot, String> {
    social_insights::validate_data(&saved.fetchary_version, &saved.data)?;
    validate_profile(&saved.profile_url, &ctx.source)?;
    if saved.data["unavailable"]
        .as_object()
        .is_some_and(|reasons| reasons.values().any(|reason| reason == "identity_mismatch"))
    {
        return Err(
            "The account identity could not be verified. Previous insights were preserved.".into(),
        );
    }
    if !matches!(saved.data["kind"].as_str(), Some("profile" | "channel")) {
        return Err(
            "The platform did not return account insights. Previous data was preserved.".into(),
        );
    }
    if saved.source != ctx.source
        || saved.uploader != ctx.uploader
        || saved.data["url"].as_str() != Some(saved.profile_url.as_str())
    {
        return Err("The account identity changed. Previous insights were preserved.".into());
    }
    let json = serde_json::to_string(&saved.data).map_err(|e| e.to_string())?;
    if json.len() > 262144 {
        return Err("Account insights exceed the storage limit.".into());
    }
    let conn = db.lock().map_err(|_| "SQLite lock poisoned")?;
    let tx = database::write_transaction(&conn)?;
    if context(&tx, &ctx.id)? != *ctx {
        return Err("The History entry changed. Account insights were not saved.".into());
    }
    tx.execute("INSERT INTO history_account_insights(source,uploader,profile_url,fetched_at,fetchary_version,payload_json) VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(source,uploader) DO UPDATE SET profile_url=excluded.profile_url,fetched_at=excluded.fetched_at,fetchary_version=excluded.fetchary_version,payload_json=excluded.payload_json",
        params![saved.source,saved.uploader,saved.profile_url,saved.fetched_at,saved.fetchary_version,json]).map_err(|e| format!("Account insights save failed: {e}"))?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(saved)
}
pub(crate) fn fetch_insights(
    db: &Database,
    state: &AccountState,
    processes: &ProcessState,
    id: &str,
    tools: &SocialInsightsTools,
    yt_dlp: &str,
    deno: Option<&str>,
) -> Result<AccountSnapshot, String> {
    let ctx = current(db, id)?;
    let _guard = reserve(state, &ctx, "insights")?;
    let profile_url = resolve_profile(db, &ctx, processes, yt_dlp, deno)?;
    let (fetchary_version, data) = social_insights::fetch_url(processes, &profile_url, tools)?;
    save(
        db,
        &ctx,
        AccountSnapshot {
            source: ctx.source.clone(),
            uploader: ctx.uploader.clone(),
            profile_url,
            fetched_at: crate::files::current_timestamp_millis(),
            fetchary_version,
            data,
        },
    )
}
pub(crate) fn fetch_videos(
    db: &Database,
    state: &AccountState,
    processes: &ProcessState,
    id: &str,
    offset: u32,
    yt_dlp: &str,
    deno: Option<&str>,
) -> Result<ProfileVideos, String> {
    if offset > 100_000 {
        return Err("Profile paging limit reached.".into());
    }
    let ctx = current(db, id)?;
    let _guard = reserve(state, &ctx, "videos")?;
    let profile_url = resolve_profile(db, &ctx, processes, yt_dlp, deno)?;
    let target = if ctx.source == "youtube" {
        format!("{profile_url}/videos")
    } else {
        profile_url.clone()
    };
    let mut args = crate::youtube_playlist::playlist_args(&target, deno);
    let target = args.pop().ok_or("Profile URL missing")?;
    args.extend([
        "--playlist-start".into(),
        (offset + 1).to_string(),
        "--playlist-end".into(),
        (offset + PAGE_SIZE).to_string(),
        target,
    ]);
    let mut command = Command::new(yt_dlp);
    let config =
        crate::config::load_config_from_db(&*db.lock().map_err(|_| "SQLite lock poisoned")?)?;
    crate::yt_dlp::apply_browser_cookies(
        &mut command,
        &profile_url,
        config.youtube_cookie_browser.as_deref(),
    );
    command.args(args);
    let output = crate::process::run_command_output(
        command,
        None,
        Some(processes),
        Some(Duration::from_secs(90)),
    )
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("Profile videos could not be loaded. The profile may require sign-in or be unavailable.".into());
    }
    if output.stdout.len() > 8 * 1024 * 1024 {
        return Err("Profile response is too large.".into());
    }
    if current(db, id)? != ctx {
        return Err("The History entry changed while loading the profile.".into());
    }
    parse_videos(&output.stdout, &ctx.source, profile_url, offset)
}
fn parse_videos(
    raw: &[u8],
    source: &str,
    profile_url: String,
    offset: u32,
) -> Result<ProfileVideos, String> {
    let data: Value = serde_json::from_slice(raw).map_err(|_| "Invalid profile video response.")?;
    let rows = data["entries"]
        .as_array()
        .ok_or("This platform does not expose a browsable video list.")?;
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for row in rows.iter().take(PAGE_SIZE as usize) {
        let raw = row["webpage_url"].as_str().or_else(|| row["url"].as_str());
        let candidate = raw
            .filter(|s| s.starts_with("https://"))
            .map(str::to_string)
            .or_else(|| {
                let id = row["id"].as_str()?;
                (source == "youtube"
                    && id.len() == 11
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
                .then(|| format!("https://www.youtube.com/watch?v={id}"))
            });
        let Some(url) = candidate else { continue };
        let Some(video) = crate::video_urls::normalize_video_url(&url) else {
            continue;
        };
        let url = video.url;
        if crate::history_rules::source_from_url(&url).as_deref()
            != Some(if source == "twitter" { "x" } else { source })
            || !seen.insert(url.clone())
        {
            continue;
        }
        entries.push(ProfileVideo {
            title: row["title"].as_str().unwrap_or(&url).into(),
            url,
        });
    }
    Ok(ProfileVideos {
        profile_url,
        entries,
        next_offset: offset.saturating_add(rows.len().min(PAGE_SIZE as usize) as u32),
        has_more: rows.len() >= PAGE_SIZE as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use rusqlite::Connection;
    use std::fs;
    fn db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        database::initialize(&conn).unwrap();
        conn.execute_batch("INSERT INTO history_entries(id,url,source,uploader,created_at) VALUES ('one','https://www.youtube.com/watch?v=abcdefghijk','youtube','Creator',1),('two','https://www.youtube.com/watch?v=lmnopqrstuv','youtube','Creator',2),('other','https://www.tiktok.com/@creator/video/1234567890123456789','tiktok','Creator',3),('similar','https://www.youtube.com/watch?v=abcdefghijk','youtube','Creator Extra',4);").unwrap();
        Database::new(conn)
    }
    fn insight(count: u64) -> AccountSnapshot {
        let metrics: serde_json::Map<String, Value> = social_insights::METRICS
            .iter()
            .map(|k| (k.to_string(), Value::Null))
            .chain([
                ("subscribers".into(), count.into()),
                ("posts".into(), 0.into()),
            ])
            .collect();
        AccountSnapshot {
            source: "youtube".into(),
            uploader: "Creator".into(),
            profile_url: "https://www.youtube.com/@creator".into(),
            fetched_at: count,
            fetchary_version: "2.0.4".into(),
            data: serde_json::json!({"platform":"youtube","kind":"profile","url":"https://www.youtube.com/@creator","status":"partial","metrics":metrics,"sources":{},"unavailable":{}}),
        }
    }
    #[test]
    fn local_account_pages_use_exact_names_platforms_and_all_pages() {
        let db = db();
        for i in 0..25 {
            db.lock().unwrap().execute("INSERT INTO history_entries(id,url,source,uploader,created_at) VALUES (?1,'https://www.youtube.com/watch?v=abcdefghijk','youtube','Creator',?2)",params![format!("extra-{i}"),10+i]).unwrap();
        }
        let first = get(&db, "one", 0).unwrap();
        assert_eq!(first.saved.total_count, 27);
        assert_eq!(first.saved.entries.len(), 20);
        assert!(first.saved.has_more);
        assert!(first.insights.is_none());
        let last = get(&db, "one", 20).unwrap();
        assert_eq!(last.saved.entries.len(), 7);
        assert!(!last.saved.has_more);
        assert_eq!(get(&db, "other", 0).unwrap().saved.total_count, 1);
        assert!(get(&db, "missing", 0).is_err());
    }
    #[test]
    fn snapshots_are_shared_per_account_atomic_and_removed_with_last_download() {
        let db = db();
        let ctx = current(&db, "one").unwrap();
        save(&db, &ctx, insight(123)).unwrap();
        assert_eq!(
            get(&db, "two", 0).unwrap().insights.unwrap().data["metrics"]["posts"],
            0
        );
        assert!(get(&db, "other", 0).unwrap().insights.is_none());
        db.lock().unwrap().execute_batch("CREATE TRIGGER reject_account BEFORE UPDATE ON history_account_insights BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
        assert!(save(&db, &ctx, insight(456)).is_err());
        assert_eq!(
            get(&db, "one", 0).unwrap().insights.unwrap().fetched_at,
            123
        );
        db.lock()
            .unwrap()
            .execute_batch("DROP TRIGGER reject_account")
            .unwrap();
        crate::history::delete_history_entry_from_db(&db, "one").unwrap();
        assert!(get(&db, "two", 0).unwrap().insights.is_some());
        assert!(save(&db, &ctx, insight(789)).is_err());
        crate::history::delete_history_entry_from_db(&db, "two").unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM history_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }
    #[test]
    fn changed_identity_and_invalid_snapshots_preserve_previous_data() {
        let db = db();
        let ctx = current(&db, "one").unwrap();
        save(&db, &ctx, insight(123)).unwrap();
        let mut bad = insight(456);
        bad.data["url"] = "https://www.youtube.com/@someoneelse".into();
        assert!(save(&db, &ctx, bad).is_err());
        let mut bad = insight(456);
        bad.data["kind"] = "video".into();
        assert!(save(&db, &ctx, bad).is_err());
        let mut bad = insight(456);
        bad.data["metrics"]["posts"] = (-1).into();
        assert!(save(&db, &ctx, bad).is_err());
        let mut bad = insight(456);
        bad.data["status"] = "unavailable".into();
        bad.data["unavailable"]["subscribers"] = "identity_mismatch".into();
        assert!(save(&db, &ctx, bad)
            .unwrap_err()
            .contains("account identity could not be verified"));
        assert_eq!(
            get(&db, "one", 0).unwrap().insights.unwrap().fetched_at,
            123
        );
        db.lock()
            .unwrap()
            .execute(
                "UPDATE history_entries SET uploader='New creator' WHERE id='one'",
                [],
            )
            .unwrap();
        assert!(save(&db, &ctx, insight(456)).is_err());
        assert_eq!(
            get(&db, "two", 0).unwrap().insights.unwrap().fetched_at,
            123
        );
        crate::history::clear_history_entries_in_db(&db).unwrap();
        assert_eq!(
            db.lock()
                .unwrap()
                .query_row("SELECT COUNT(*) FROM history_account_insights", [], |r| r
                    .get::<_, i64>(
                    0
                ))
                .unwrap(),
            0
        );
    }
    #[test]
    fn profile_addresses_require_real_platform_profiles() {
        for raw in [
            "https://youtube.com.evil.test/@creator",
            "https://user:secret@youtube.com/@creator",
            "http://youtube.com/@creator",
            "https://youtube.com/watch?v=abcdefghijk",
            "https://youtube.com/@creator/videos",
            "https://youtube.com:444/@creator",
            "https://tiktok.com/@creator",
        ] {
            assert!(validate_profile(raw, "youtube").is_err(), "{raw}");
        }
        assert_eq!(
            validate_profile(
                "https://www.youtube.com/channel/UCabc?tracking=1#tab",
                "youtube"
            )
            .unwrap(),
            "https://www.youtube.com/channel/UCabc"
        );
        assert!(validate_profile("https://www.tiktok.com/@creator", "tiktok").is_ok());
        assert_eq!(
            validate_profile(
                "https://www.facebook.com/profile.php?id=123&tracking=1",
                "facebook"
            )
            .unwrap(),
            "https://www.facebook.com/profile.php?id=123"
        );
        assert!(validate_profile("https://www.facebook.com/profile.php", "facebook").is_err());
    }
    #[test]
    fn same_account_requests_are_serialized_and_guard_releases_on_failure() {
        let db = db();
        let state = AccountState::default();
        let one = current(&db, "one").unwrap();
        let two = current(&db, "two").unwrap();
        let guard = reserve(&state, &one, "insights").unwrap();
        assert!(reserve(&state, &two, "insights").is_err());
        assert!(reserve(&state, &two, "videos").is_ok());
        drop(guard);
        assert!(reserve(&state, &two, "insights").is_ok());
    }
    #[test]
    fn remote_pages_normalize_deduplicate_and_exclude_nonvideo_or_other_platform_links() {
        let page=parse_videos(br#"{"entries":[{"id":"abcdefghijk","title":"First"},{"url":"https://youtu.be/abcdefghijk"},{"url":"https://www.youtube.com/@creator"},{"url":"https://www.tiktok.com/@someone/video/1234567890123456789"},null,{"id":"lmnopqrstuv","title":"Second"}]}"#,"youtube","https://www.youtube.com/@creator".into(),50).unwrap();
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].title, "First");
        assert_eq!(page.next_offset, 56);
        assert!(!page.has_more);
        assert!(parse_videos(b"{}", "youtube", "profile".into(), 0).is_err());
        let rows = serde_json::json!({"entries":vec![Value::Null;50]});
        let page =
            parse_videos(rows.to_string().as_bytes(), "youtube", "profile".into(), 0).unwrap();
        assert!(page.has_more);
        assert_eq!(page.next_offset, 50);
    }
    #[test]
    fn integration_schema_five_upgrade_has_backup_rollback_and_offline_persistence() {
        let root = TempRoot::new("account-migration");
        let path = root.path("history.sqlite");
        let conn = database::open(&path).unwrap();
        conn.execute_batch("DROP TABLE history_account_insights; DROP INDEX idx_history_account_lookup; ALTER TABLE app_config DROP COLUMN youtube_cookie_browser; PRAGMA user_version=5; INSERT INTO history_entries(id,url,source,uploader,title,created_at) VALUES ('one','https://www.youtube.com/watch?v=abcdefghijk','youtube','Creator','Original',1)").unwrap();
        drop(conn);
        let before = fs::read(&path).unwrap();
        assert!(database::open_with_hook(&path, |point| {
            if point == database::MigrationPoint::AccountInsights {
                Err("synthetic migration failure".into())
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(before, fs::read(&path).unwrap());
        let db = Database::new(database::open(&path).unwrap());
        let ctx = current(&db, "one").unwrap();
        save(&db, &ctx, insight(123)).unwrap();
        drop(db);
        let db = Database::new(database::open(&path).unwrap());
        let page = get(&db, "one", 0).unwrap();
        assert_eq!(page.saved.entries[0].title.as_deref(), Some("Original"));
        assert_eq!(page.insights.unwrap().fetched_at, 123);
        assert!(fs::read_dir(root.root())
            .unwrap()
            .flatten()
            .any(|e| e.file_name().to_string_lossy().contains("pre-schema-5")));
    }
    #[cfg(unix)]
    #[test]
    fn integration_tiktok_refresh_resolves_public_handle_and_recovers_legacy_snapshot() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("tiktok-account-refresh");
        let yt = root.path("yt-dlp");
        let node = root.path("node");
        let public_url = "https://www.tiktok.com/@creator";
        let internal_url = "https://www.tiktok.com/@MS4wLjABfixture";
        let db = db();
        let mut legacy = insight(123);
        legacy.source = "tiktok".into();
        legacy.profile_url = internal_url.into();
        legacy.data["platform"] = "tiktok".into();
        legacy.data["url"] = internal_url.into();
        legacy.data["status"] = "unavailable".into();
        for name in social_insights::METRICS {
            legacy.data["metrics"][*name] = Value::Null;
            legacy.data["unavailable"][*name] = "identity_mismatch".into();
        }
        // Earlier versions saved failed secUid results. Seed one without using
        // save(), which now rejects identity mismatches.
        db.lock().unwrap().execute(
            "INSERT INTO history_account_insights(source,uploader,profile_url,fetched_at,fetchary_version,payload_json) VALUES (?1,?2,?3,?4,?5,?6)",
            params![legacy.source,legacy.uploader,legacy.profile_url,legacy.fetched_at,legacy.fetchary_version,legacy.data.to_string()],
        ).unwrap();
        let state = AccountState::default();
        let processes = ProcessState::default();
        let tools = SocialInsightsTools {
            node: node.clone(),
            script: root.path("adapter.cjs"),
        };
        fs::write(&yt, "#!/bin/sh\nexit 1\n").unwrap();
        fs::set_permissions(&yt, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(fetch_insights(
            &db,
            &state,
            &processes,
            "other",
            &tools,
            yt.to_str().unwrap(),
            None
        )
        .is_err());
        assert_eq!(
            get(&db, "other", 0).unwrap().insights.unwrap().profile_url,
            internal_url
        );

        let metadata = serde_json::json!({"channel_url":internal_url,"uploader_url":public_url});
        fs::write(&yt, format!("#!/bin/sh\nprintf '%s' '{metadata}'\n")).unwrap();
        let mut data = legacy.data.clone();
        data["url"] = public_url.into();
        data["status"] = "available".into();
        data["metrics"]["followers"] = 15683.into();
        data["metrics"]["following"] = 30.into();
        data["metrics"]["posts"] = 858.into();
        data["unavailable"] = serde_json::json!({"subscribers":"unsupported"});
        let reply = serde_json::json!({"fetchary_version":"2.0.6","data":data});
        fs::write(
            &node,
            format!("#!/bin/sh\n[ \"$2\" = '{public_url}' ] || exit 2\nprintf '%s' '{reply}'\n"),
        )
        .unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o700)).unwrap();
        let saved = fetch_insights(
            &db,
            &state,
            &processes,
            "other",
            &tools,
            yt.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(saved.profile_url, public_url);
        assert_eq!(saved.data["metrics"]["followers"], 15683);
        assert_eq!(saved.data["metrics"]["following"], 30);
        assert_eq!(saved.data["metrics"]["posts"], 858);
        assert_eq!(
            get(&db, "other", 0).unwrap().insights.unwrap().profile_url,
            public_url
        );
        // A successful snapshot still supplies its verified URL without
        // repeating the metadata request on every subsequent action.
        fs::write(&yt, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(fetch_insights(
            &db,
            &state,
            &processes,
            "other",
            &tools,
            yt.to_str().unwrap(),
            None
        )
        .is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn integration_network_actions_use_verified_profile_and_preserve_snapshots_after_failure() {
        use std::os::unix::fs::PermissionsExt;
        let root = TempRoot::new("account-process");
        let yt = root.path("yt-dlp");
        let node = root.path("node");
        fs::write(&yt,"#!/bin/sh\ncase \"$2\" in --flat-playlist) printf '%s' '{\"entries\":[{\"id\":\"abcdefghijk\",\"title\":\"Public video\"}]}' ;; *) printf '%s' '{\"channel_url\":\"https://www.youtube.com/@creator\",\"uploader_url\":\"https://www.youtube.com/@other\"}' ;; esac\n").unwrap();
        fs::set_permissions(&yt, fs::Permissions::from_mode(0o700)).unwrap();
        let reply = serde_json::json!({"fetchary_version":"2.0.4","data":insight(123).data});
        fs::write(&node,format!("#!/bin/sh\n[ \"$2\" = 'https://www.youtube.com/@creator' ] || exit 2\nprintf '%s' '{}'\n",reply)).unwrap();
        fs::set_permissions(&node, fs::Permissions::from_mode(0o700)).unwrap();
        let tools = SocialInsightsTools {
            node: node.clone(),
            script: root.path("adapter.cjs"),
        };
        let db = db();
        let state = AccountState::default();
        let processes = ProcessState::default();
        let saved = fetch_insights(
            &db,
            &state,
            &processes,
            "one",
            &tools,
            yt.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(saved.data["metrics"]["subscribers"], 123);
        let videos = fetch_videos(
            &db,
            &state,
            &processes,
            "one",
            0,
            yt.to_str().unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(videos.entries.len(), 1);
        fs::write(&node, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(fetch_insights(
            &db,
            &state,
            &processes,
            "two",
            &tools,
            yt.to_str().unwrap(),
            None
        )
        .is_err());
        assert_eq!(
            get(&db, "one", 0).unwrap().insights.unwrap().data["metrics"]["subscribers"],
            123
        );
    }
}
