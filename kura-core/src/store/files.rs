//! The filesystem side. `Layout::image_path` is the only place an image path is
//! derived, and `stage`/`promote`/`remove_image_file` are the only code that
//! creates or removes image files.

use std::{
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    error::{Error, Result, ResultExt},
    model::{ImageHash, MediaType},
    source::Source,
};

const STAGING_DIR: &str = ".kura-staging";
/// Enough bytes to recognize every supported format (WebP needs 12).
const SNIFF_LEN: usize = 12;

#[derive(Debug, Clone)]
pub(crate) struct Layout {
    pictures: PathBuf,
    staging: PathBuf,
}

impl Layout {
    pub(crate) fn new(pictures: PathBuf) -> Self {
        let staging = pictures.join(STAGING_DIR);
        Self { pictures, staging }
    }

    pub(crate) async fn create_dirs(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.staging)
            .await
            .map_err(|e| Error::Setup(format!("can't create {}: {e}", self.staging.display())))
    }

    /// A file directly inside the picture folder (legacy files had arbitrary names).
    pub(crate) fn pictures_file(&self, file_name: &std::ffi::OsStr) -> PathBuf {
        self.pictures.join(file_name)
    }

    /// `<pictures>/<hash>.<extension>`: the on-disk contract.
    pub(crate) fn image_path(&self, hash: &ImageHash, extension: &str) -> PathBuf {
        self.pictures.join(format!("{hash}.{extension}"))
    }

    /// Stream a source into a new staging file while hashing, counting and
    /// sniffing it. Nothing is buffered in memory and no lock is held. The
    /// staged file is removed again if anything fails or it's never promoted.
    pub(crate) async fn stage(&self, source: Source, limit: u64) -> Result<Staged> {
        let Source { mut reader, origin, .. } = source;
        let temp = TempFile(self.staging.join(unique_name()));
        let mut file = tokio::fs::File::create(&temp.0).await.internal("creating staging file")?;

        let mut hasher = Sha256::new();
        let mut head = Vec::with_capacity(SNIFF_LEN);
        let mut media_type = None;
        let mut size: u64 = 0;
        let mut buf = vec![0; 64 * 1024];

        loop {
            let n = reader.read(&mut buf).await.map_err(|e| origin.read_error(e))?;
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];

            size += n as u64;
            if size > limit {
                return Err(Error::TooLarge { limit });
            }
            if media_type.is_none() {
                head.extend_from_slice(&chunk[..chunk.len().min(SNIFF_LEN - head.len())]);
                if head.len() == SNIFF_LEN {
                    media_type = Some(sniff(&head)?);
                }
            }

            hasher.update(chunk);
            file.write_all(chunk).await.internal("writing staging file")?;
        }

        let media_type = match media_type {
            Some(m) => m,
            None => sniff(&head)?,
        };
        file.sync_all().await.internal("syncing staging file")?;

        Ok(Staged { hash: ImageHash::from_digest(hasher), media_type, temp })
    }

    /// Move a staged file to its final name. The caller holds the write lock.
    /// If the target already exists it has identical bytes (content addressing),
    /// and rename simply replaces it.
    pub(crate) async fn promote(&self, staged: Staged) -> Result<()> {
        let target = self.image_path(&staged.hash, staged.media_type.extension());
        tokio::fs::rename(&staged.temp.0, &target).await.internal("storing image file")?;
        staged.temp.forget();
        self.sync_pictures_dir().await;
        Ok(())
    }

    /// Unlink an image file. The caller holds the write lock and has checked
    /// that no row references the hash.
    pub(crate) async fn remove_image_file(&self, hash: &ImageHash, extension: &str) -> Result<()> {
        match tokio::fs::remove_file(self.image_path(hash, extension)).await {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e).internal("removing image file"),
            _ => Ok(()),
        }
    }

    /// Remove staging files older than `max_age`: leftovers of a crashed add.
    pub(crate) async fn clear_stale_staging(&self, max_age: Duration) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.staging).await.internal("reading staging dir")?;
        while let Some(entry) = entries.next_entry().await.internal("reading staging dir")? {
            let age = entry
                .metadata()
                .await
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| SystemTime::now().duration_since(t).ok());
            if age.is_some_and(|age| age > max_age) {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
        Ok(())
    }

    /// Make a rename durable. Best effort: a failure here isn't worth failing the add.
    async fn sync_pictures_dir(&self) {
        #[cfg(unix)]
        if let Ok(dir) = tokio::fs::File::open(&self.pictures).await {
            let _ = dir.sync_all().await;
        }
    }
}

fn sniff(head: &[u8]) -> Result<MediaType> {
    MediaType::from_bytes(head).map_err(|_| Error::UnsupportedMedia)
}

fn unique_name() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    format!("{}-{nanos}-{}.part", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// A fully written, synced, not-yet-visible image file.
pub(crate) struct Staged {
    pub(crate) hash: ImageHash,
    pub(crate) media_type: MediaType,
    temp: TempFile,
}

/// Deletes the file on drop unless `forget` was called.
struct TempFile(PathBuf);

impl TempFile {
    /// The file was moved into place; don't delete anything on drop.
    fn forget(mut self) {
        self.0 = PathBuf::new();
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.0.as_os_str().is_empty() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// A shared advisory lock on `<data>/kura.lock`, held by every open `Library`.
/// `LibraryPaths::destroy` needs it exclusively, so deleting a library fails
/// while a server or TUI has it open.
pub(crate) struct LockGuard {
    _file: File,
}

impl LockGuard {
    pub(crate) fn shared(path: &Path) -> Result<Self> {
        let file = open_lock_file(path)?;
        match file.try_lock_shared() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::LibraryInUse),
            Err(TryLockError::Error(e)) => Err(e).internal("locking library"),
        }
    }

    pub(crate) fn exclusive(path: &Path) -> Result<Self> {
        let file = open_lock_file(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::LibraryInUse),
            Err(TryLockError::Error(e)) => Err(e).internal("locking library"),
        }
    }
}

fn open_lock_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .internal("opening lock file")
}
