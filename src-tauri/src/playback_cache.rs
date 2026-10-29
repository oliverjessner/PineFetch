//! Retention applies only to generated files in the private playback cache.
use crate::database::Database;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tauri::Manager;

pub(crate) const MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub(crate) const MAX_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

#[derive(Serialize)]
pub(crate) struct PlaybackSource {
    pub(crate) path: String,
    pub(crate) lease_id: Option<String>,
}

struct Lease {
    path: PathBuf,
    generation: u64,
    // Shared OS locks also protect a preview opened by another process.
    file: File,
}

impl Drop for Lease {
    fn drop(&mut self) {
        // Explicit unlock also releases inherited copies of this file handle
        // in helpers that forked while a player was open.
        let _ = self.file.set_modified(SystemTime::now());
        let _ = self.file.unlock();
    }
}

#[derive(Default)]
pub(crate) struct PlaybackCache {
    leases: HashMap<String, Lease>,
    temporary: HashSet<PathBuf>,
}

impl PlaybackCache {
    pub(crate) fn acquire(&mut self, path: &Path, generation: u64) -> Result<String, String> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .or_else(|_| File::open(path))
            .map_err(|_| "The playback preview is unavailable.")?;
        file.try_lock_shared()
            .map_err(|_| "The playback preview is busy. Reopen this tab to retry.")?;
        let _ = file.set_modified(SystemTime::now());
        let id = uuid::Uuid::new_v4().to_string();
        self.leases.insert(
            id.clone(),
            Lease {
                generation,
                path: path.to_path_buf(),
                file,
            },
        );
        Ok(id)
    }

    pub(crate) fn release(&mut self, id: &str) {
        self.leases.remove(id);
    }

    pub(crate) fn release_before(&mut self, generation: u64) {
        self.leases
            .retain(|_, lease| lease.generation >= generation);
    }

    pub(crate) fn begin_temporary(&mut self, path: PathBuf) {
        self.temporary.insert(path);
    }
    pub(crate) fn end_temporary(&mut self, path: &Path) {
        self.temporary.remove(path);
    }

    fn maintain(
        &mut self,
        db: &Database,
        directory: &Path,
        max_bytes: u64,
        max_age: Duration,
        now: SystemTime,
    ) -> Result<(), String> {
        if contains_originals(db, directory)? {
            return Ok(());
        }
        self.prune(directory, max_bytes, max_age, now)
    }

    pub(crate) fn prune(
        &mut self,
        directory: &Path,
        max_bytes: u64,
        max_age: Duration,
        now: SystemTime,
    ) -> Result<(), String> {
        let metadata = match fs::symlink_metadata(directory) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("Playback cache could not be inspected.".into()),
        };
        if !metadata.is_dir() || metadata.is_symlink() {
            return Err("Playback cache is not a private directory.".into());
        }
        let mut files = Vec::new();
        let mut total = 0u64;
        for entry in fs::read_dir(directory).map_err(|_| "Playback cache could not be read.")? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Some(kind) = managed_file(&path) else {
                continue;
            };
            let Ok(metadata) = fs::symlink_metadata(&path) else {
                continue;
            };
            if !metadata.is_file() || metadata.is_symlink() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() != 1 {
                    continue;
                }
            }
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            total = total.saturating_add(metadata.len());
            files.push((path, metadata, modified, kind));
        }
        // Oldest unused previews go first. A stable path tie-breaker makes
        // eviction deterministic when several files have equal timestamps.
        files.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
        for (path, before, modified, temporary) in files {
            let expired = now.duration_since(modified).unwrap_or_default() >= max_age;
            if !temporary && !expired && total <= max_bytes {
                continue;
            }
            if self.temporary.contains(&path)
                || self.leases.values().any(|lease| lease.path == path)
            {
                continue;
            }
            let Ok(file) = OpenOptions::new().read(true).write(true).open(&path) else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            // Recheck after acquiring the exclusive lock. A newly used or
            // replaced file must survive this sweep; never follow a symlink.
            let (Ok(opened), Ok(current)) = (file.metadata(), fs::symlink_metadata(&path)) else {
                continue;
            };
            if !same_file(&before, &opened) || !same_file(&opened, &current) {
                continue;
            }
            if fs::remove_file(&path).is_ok() {
                total = total.saturating_sub(before.len());
            }
        }
        Ok(())
    }
}

fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    if !b.is_file()
        || b.is_symlink()
        || a.len() != b.len()
        || a.modified().ok() != b.modified().ok()
    {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if a.dev() != b.dev() || a.ino() != b.ino() || b.nlink() != 1 {
            return false;
        }
    }
    true
}

// Existing 64-hex previews stay eligible; no migration or broad directory delete.
// Registered temporary files belong to active conversions; other UUID files are orphans.
fn managed_file(path: &Path) -> Option<bool> {
    let name = path.file_name()?.to_str()?;
    if let Some(stem) = name.strip_suffix(".tmp.mp4") {
        return (uuid::Uuid::parse_str(stem).ok()?.to_string() == stem).then_some(true);
    }
    let stem = name.strip_suffix(".mp4")?;
    (stem.len() == 64
        && stem
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then_some(false)
}

// If a user deliberately chose the private cache as an output folder, preserve
// it in full. Cleanup must never remove a History original or retained source.
fn contains_originals(db: &Database, directory: &Path) -> Result<bool, String> {
    let canonical = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let prefix = |path: &Path| {
        format!(
            "{}{sep}",
            path.to_string_lossy()
                .trim_end_matches(std::path::MAIN_SEPARATOR),
            sep = std::path::MAIN_SEPARATOR
        )
    };
    db.lock().map_err(|_| "SQLite lock poisoned")?.query_row(
        "SELECT EXISTS(SELECT 1 FROM history_entries WHERE instr(output_path, ?1)=1 OR instr(output_path, ?2)=1
         UNION ALL SELECT 1 FROM transcriptions WHERE instr(media_path, ?1)=1 OR instr(media_path, ?2)=1)",
        rusqlite::params![prefix(directory), prefix(&canonical)], |row| row.get(0)
    ).map_err(|_| "Playback originals could not be checked.".into())
}

fn is_cache_destination(directory: &Path, output: &str) -> bool {
    let directory = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let output = fs::canonicalize(output).unwrap_or_else(|_| PathBuf::from(output));
    output.starts_with(directory)
}

// Protect downloads that have not yet produced a History row, including jobs
// queued before the user changed the default output folder.
fn has_download_destination(state: &crate::AppState, directory: &Path) -> Result<bool, String> {
    let mut outputs = HashSet::new();
    if let Some(output) = state
        .config
        .lock()
        .map_err(|_| "Config lock poisoned")?
        .default_output_dir
        .clone()
    {
        outputs.insert(output);
    }
    {
        let queue = state.queue.data.lock().map_err(|_| "Queue lock poisoned")?;
        for job in queue
            .pending
            .iter()
            .chain(queue.active.iter())
            .chain(queue.interrupted.iter().map(|job| &job.job))
        {
            outputs.insert(job.output_dir.clone());
        }
    }
    Ok(outputs
        .iter()
        .any(|output| is_cache_destination(directory, output)))
}

pub(crate) fn maintain(app: &tauri::AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|_| "Playback cache unavailable.")?
        .join("history-media");
    if !directory.exists() {
        return Ok(());
    }
    let state = app.state::<crate::AppState>();
    if has_download_destination(&state, &directory)? {
        return Ok(());
    }
    let mut cache = state
        .history_media
        .cache
        .lock()
        .map_err(|_| "Playback lock poisoned")?;
    cache.maintain(&state.db, &directory, MAX_BYTES, MAX_AGE, SystemTime::now())
}

pub(crate) fn start_maintenance(app: tauri::AppHandle) -> std::io::Result<()> {
    std::thread::Builder::new()
        .name("playback-cache".into())
        .spawn(move || {
            // Startup also clears old previews and orphaned interrupted conversions.
            loop {
                if app
                    .state::<crate::AppState>()
                    .processes
                    .shutting_down
                    .load(std::sync::atomic::Ordering::SeqCst)
                {
                    break;
                }
                // Cleanup errors never prevent playback. A later sweep retries.
                let _ = maintain(&app);
                for _ in 0..60 {
                    std::thread::sleep(Duration::from_secs(1));
                    if app
                        .state::<crate::AppState>()
                        .processes
                        .shutting_down
                        .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        return;
                    }
                }
            }
        })
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;
    use std::fs::FileTimes;

    fn preview(root: &TempRoot, character: char, bytes: u64, modified: SystemTime) -> PathBuf {
        let path = root.path(format!("{}.mp4", character.to_string().repeat(64)));
        let file = File::create(&path).unwrap();
        file.set_len(bytes).unwrap();
        file.set_times(FileTimes::new().set_modified(modified))
            .unwrap();
        path
    }

    #[test]
    fn integration_retention_expires_unused_previews_and_evicts_oldest_until_within_budget() {
        let root = TempRoot::new("playback-retention");
        let now = SystemTime::now();
        let day = Duration::from_secs(24 * 60 * 60);
        let old = preview(&root, 'a', 10, now - day * 31);
        let oldest = preview(&root, 'b', 20, now - day * 3);
        let recent = preview(&root, 'c', 20, now - day);
        let newest = preview(&root, 'd', 20, now);
        let future = preview(&root, 'e', 10, now + day);
        let mut cache = PlaybackCache::default();
        cache.prune(&root.path(""), 50, MAX_AGE, now).unwrap();
        assert!(!old.exists());
        assert!(!oldest.exists());
        assert!(recent.exists() && newest.exists() && future.exists());
        // Restart uses the persisted file timestamp; no in-memory LRU required.
        PlaybackCache::default()
            .prune(&root.path(""), 50, MAX_AGE, now + day * 40)
            .unwrap();
        assert!(!recent.exists() && !newest.exists() && !future.exists());
    }

    #[test]
    fn integration_shared_leases_preserve_active_oversized_previews_until_every_player_releases() {
        let root = TempRoot::new("playback-leases");
        let now = SystemTime::now();
        let path = preview(&root, 'a', 100, now - MAX_AGE * 2);
        let mut first = PlaybackCache::default();
        let mut other = PlaybackCache::default();
        let one = first.acquire(&path, 1).unwrap();
        let two = first.acquire(&path, 1).unwrap();
        other.prune(&root.path(""), 0, Duration::ZERO, now).unwrap();
        assert!(path.exists(), "OS lock protects another state/process too");
        first.release(&one);
        first.release("unknown-lease");
        first.prune(&root.path(""), 0, Duration::ZERO, now).unwrap();
        assert!(path.exists());
        first.release(&two);
        assert!(fs::metadata(&path).unwrap().modified().unwrap() >= now - Duration::from_secs(2));
        other
            .prune(&root.path(""), 0, MAX_AGE, SystemTime::now())
            .unwrap();
        assert!(
            !path.exists(),
            "unreferenced oversize preview becomes evictable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn integration_inherited_helper_handle_cannot_keep_a_released_player_pinned() {
        use std::os::fd::AsRawFd;
        use std::os::unix::process::CommandExt;
        let root = TempRoot::new("playback-inherited-lock");
        let path = preview(&root, 'a', 100, SystemTime::now());
        let control = preview(&root, 'b', 100, SystemTime::now());
        let control_file = File::open(&control).unwrap();
        control_file.try_lock_shared().unwrap();
        let mut cache = PlaybackCache::default();
        let lease = cache.acquire(&path, 1).unwrap();
        let descriptors = [
            cache.leases[&lease].file.as_raw_fd(),
            control_file.as_raw_fd(),
        ];
        let mut flags = Vec::new();
        for fd in descriptors {
            let original = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            assert!(original >= 0);
            assert_eq!(
                unsafe { libc::fcntl(fd, libc::F_SETFD, original & !libc::FD_CLOEXEC) },
                0
            );
            flags.push(original);
        }
        let mut command = std::process::Command::new("sleep");
        command.arg("30");
        // Force fork/exec, then retain both fixture descriptors across exec.
        // The child callback performs no operations that are unsafe after fork.
        unsafe {
            command.pre_exec(|| Ok(()));
        }
        let child = command.spawn();
        let mut restored = true;
        for (fd, flags) in descriptors.into_iter().zip(flags) {
            restored &= unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } == 0;
        }
        let mut child = child.unwrap();
        drop(control_file); // ordinary close leaves the child's copy locked
        cache.release(&lease); // explicit unlock revokes inherited copies too
        let result = cache.prune(&root.path(""), 0, Duration::ZERO, SystemTime::now());
        let evicted = !path.exists();
        let control_still_locked = control.exists();
        let _ = child.kill();
        child.wait().unwrap();
        assert!(restored);
        result.unwrap();
        assert!(
            control_still_locked,
            "the helper really inherited a live shared lock"
        );
        assert!(
            evicted,
            "a released player cannot be kept pinned by an inherited descriptor"
        );
    }

    #[test]
    fn integration_reuse_refreshes_retention_and_reload_releases_only_the_old_page() {
        let root = TempRoot::new("playback-reload");
        let now = SystemTime::now();
        let old = preview(&root, 'a', 10, now - MAX_AGE * 2);
        let current = preview(&root, 'b', 10, now - MAX_AGE * 2);
        let mut cache = PlaybackCache::default();
        let one = cache.acquire(&old, 1).unwrap();
        let two = cache.acquire(&current, 2).unwrap();
        cache.release_before(2);
        assert!(!cache.leases.contains_key(&one));
        assert!(cache.leases.contains_key(&two));
        cache
            .prune(&root.path(""), 100, MAX_AGE, SystemTime::now())
            .unwrap();
        assert!(
            old.exists(),
            "last use survives a restart through its timestamp"
        );
        cache
            .prune(&root.path(""), 0, Duration::ZERO, SystemTime::now())
            .unwrap();
        assert!(!old.exists());
        assert!(
            current.exists(),
            "new page references survive late invalidation"
        );
        cache.release_before(2);
        assert!(cache.leases.contains_key(&two));
        cache.release_before(3);
        cache
            .prune(&root.path(""), 0, Duration::ZERO, SystemTime::now())
            .unwrap();
        assert!(!current.exists());
    }

    #[test]
    fn integration_cleanup_preserves_foreign_files_originals_subdirectories_and_links() {
        let root = TempRoot::new("playback-safe-cleanup");
        let directory = root.path("cache");
        fs::create_dir(&directory).unwrap();
        let original = root.path("original.mp4");
        fs::write(&original, b"unchanged original video").unwrap();
        fs::write(directory.join("user-video.mp4"), b"foreign file").unwrap();
        fs::create_dir(directory.join(format!("{}.mp4", "a".repeat(64)))).unwrap();
        let temporary = directory.join(format!("{}.tmp.mp4", uuid::Uuid::new_v4()));
        fs::write(&temporary, b"interrupted conversion").unwrap();
        let unknown = directory.join("not-a-uuid.tmp.mp4");
        fs::write(&unknown, b"foreign file").unwrap();
        #[cfg(unix)]
        {
            let linked = directory.join(format!("{}.mp4", "b".repeat(64)));
            std::os::unix::fs::symlink(&original, &linked).unwrap();
            fs::hard_link(&original, directory.join(format!("{}.mp4", "c".repeat(64)))).unwrap();
            let alias = root.path("cache-link");
            std::os::unix::fs::symlink(&directory, &alias).unwrap();
            assert!(PlaybackCache::default()
                .prune(&alias, 0, Duration::ZERO, SystemTime::now())
                .is_err());
        }
        PlaybackCache::default()
            .prune(&directory, 0, Duration::ZERO, SystemTime::now())
            .unwrap();
        assert!(!temporary.exists());
        assert!(unknown.exists());
        assert!(directory.join("user-video.mp4").exists());
        assert!(directory.join(format!("{}.mp4", "a".repeat(64))).is_dir());
        assert_eq!(fs::read(&original).unwrap(), b"unchanged original video");
        #[cfg(unix)]
        {
            assert!(
                fs::symlink_metadata(directory.join(format!("{}.mp4", "b".repeat(64))))
                    .unwrap()
                    .is_symlink()
            );
            assert!(directory.join(format!("{}.mp4", "c".repeat(64))).exists());
        }
        let missing = root.path("missing");
        PlaybackCache::default()
            .prune(&missing, 0, Duration::ZERO, SystemTime::now())
            .unwrap();
        assert!(!missing.exists());
    }

    #[test]
    fn integration_history_originals_in_the_cache_directory_disable_deletion_conservatively() {
        let root = TempRoot::new("playback-original-guard");
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE history_entries(output_path TEXT); CREATE TABLE transcriptions(media_path TEXT);").unwrap();
        let db = Database::new(conn);
        assert!(is_cache_destination(
            &root.path(""),
            root.path("").to_str().unwrap()
        ));
        assert!(!is_cache_destination(
            &root.path(""),
            root.path("").parent().unwrap().to_str().unwrap()
        ));
        #[cfg(unix)]
        {
            let alias = root.path("alias");
            std::os::unix::fs::symlink(root.path(""), &alias).unwrap();
            assert!(is_cache_destination(
                &root.path(""),
                alias.to_str().unwrap()
            ));
        }
        assert!(!contains_originals(&db, &root.path("")).unwrap());
        let original = preview(&root, 'a', 10, SystemTime::now());
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO history_entries VALUES (?1)",
                [original.to_str().unwrap()],
            )
            .unwrap();
        assert!(contains_originals(&db, &root.path("")).unwrap());
        db.lock()
            .unwrap()
            .execute("DELETE FROM history_entries", [])
            .unwrap();
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO transcriptions VALUES (?1)",
                [original.to_str().unwrap()],
            )
            .unwrap();
        assert!(contains_originals(&db, &root.path("")).unwrap());
        assert!(!contains_originals(&db, &root.path("nested")).unwrap());
        let mut cache = PlaybackCache::default();
        cache
            .maintain(&db, &root.path(""), 0, Duration::ZERO, SystemTime::now())
            .unwrap();
        assert!(
            original.exists(),
            "retained originals disable deletion, even with no budget"
        );
        db.lock()
            .unwrap()
            .execute("DELETE FROM transcriptions", [])
            .unwrap();
        db.lock()
            .unwrap()
            .execute("DROP TABLE history_entries", [])
            .unwrap();
        assert!(cache
            .maintain(&db, &root.path(""), 0, Duration::ZERO, SystemTime::now())
            .is_err());
        assert!(original.exists(), "database errors must fail closed");
    }
}
