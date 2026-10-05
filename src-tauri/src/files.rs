use std::fs;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug)]
pub(super) struct OwnedTemporaryFile {
    pub(super) path: PathBuf,
    file: fs::File,
}

impl OwnedTemporaryFile {
    pub(crate) fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.file.write_all(bytes)
    }
    pub(super) fn create_at(path: PathBuf) -> Result<Self, String> {
        let mut options = fs::OpenOptions::new();
        options.write(true).read(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|e| format!("Could not reserve job temporary file: {e}"))?;
        Ok(Self { path, file })
    }

    pub(crate) fn sync(&self) -> Result<(), String> {
        self.file
            .sync_all()
            .map_err(|e| format!("Could not flush produced temporary file: {e}"))
    }
}

impl Drop for OwnedTemporaryFile {
    fn drop(&mut self) {
        // A path can have been replaced by another process. Delete only the
        // inode reserved by this job; preserve ambiguous paths elsewhere.
        #[cfg(unix)]
        {
            if let (Ok(owned), Ok(current)) =
                (self.file.metadata(), fs::symlink_metadata(&self.path))
            {
                if owned.dev() == current.dev() && owned.ino() == current.ino() {
                    let _ = fs::remove_file(&self.path);
                }
            }
        }
    }
}

pub(super) fn publish_unique_output(temp_path: &Path, base: &Path) -> Result<PathBuf, String> {
    for index in 1..=1000 {
        let candidate = if index == 1 {
            base.to_path_buf()
        } else {
            let stem = base
                .file_stem()
                .and_then(|value| value.to_str())
                .ok_or_else(|| "Cut file has no usable file name".to_string())?;
            let extension = base.extension().and_then(|value| value.to_str());
            let mut name = format!("{stem}__{index}");
            if let Some(extension) = extension {
                name.push('.');
                name.push_str(extension);
            }
            base.with_file_name(name)
        };

        match fs::hard_link(temp_path, &candidate) {
            Ok(()) => {
                sync_output(&candidate)?;
                return Ok(candidate);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => {
                // Some removable and network filesystems do not support hard links.
                let mut source = fs::File::open(temp_path)
                    .map_err(|err| format!("Could not read cut file: {err}"))?;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    options.mode(0o600);
                }
                let mut destination = match options.open(&candidate) {
                    Ok(file) => file,
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(err) => {
                        return Err(format!(
                            "Could not save output file: {err}; previous files preserved"
                        ))
                    }
                };
                if let Err(err) = std::io::copy(&mut source, &mut destination)
                    .and_then(|_| destination.sync_all())
                {
                    // The partially copied candidate is ambiguous after an I/O
                    // failure. Preserve it and the source; never remove a path
                    // another process may now own.
                    return Err(format!("Could not copy cut file: {err}"));
                }
                sync_output(&candidate)?;
                return Ok(candidate);
            }
        }
    }
    Err("Could not find a free output file name; previous files preserved".to_string())
}

pub(super) fn sync_output(path: &Path) -> Result<(), String> {
    fs::File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Output file flush failed: {e}; file preserved"))?;
    #[cfg(unix)]
    fs::File::open(path.parent().ok_or("Output parent unavailable")?)
        .and_then(|f| f.sync_all())
        .map_err(|e| format!("Output directory flush failed: {e}; file preserved"))?;
    Ok(())
}

pub(crate) fn canonical_existing_local_path(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.contains('\0')
        || trimmed.contains("://")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
    {
        return Err("Only local filesystem paths can be opened".to_string());
    }

    match fs::canonicalize(trimmed) {
        Ok(path) => Ok(Some(path.to_string_lossy().to_string())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Path could not be opened: {err}")),
    }
}

pub(crate) fn file_size_bytes_from_path(path: Option<&str>) -> Option<i64> {
    let size = fs::metadata(path?).ok()?.len();
    i64::try_from(size).ok()
}

pub(crate) fn current_timestamp_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(crate) type TemporaryTranscriptionAudio = OwnedTemporaryFile;

pub(super) fn read_legacy_json<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<Option<T>, String> {
    let raw = match fs::read(path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(format!(
                "Legacy JSON read failed for {}: {e}; original preserved",
                path.display()
            ))
        }
    };
    serde_json::from_slice(&raw).map(Some).map_err(|e| {
        format!(
            "Legacy JSON is invalid for {} ({:?}, line {}, column {}); original preserved",
            path.display(),
            e.classify(),
            e.line(),
            e.column()
        )
    })
}
