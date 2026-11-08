//! `Library`: one handle, every operation, all invariants behind it.
//!
//! How the database and the files stay consistent:
//!
//! 1. **A file appears only together with its row.** `add` streams bytes into a
//!    staging file with no lock held, then in one short write transaction
//!    (`BEGIN IMMEDIATE`) inserts the rows, renames the staged file to
//!    `<hash>.<ext>` and commits.
//! 2. **A file is removed only while holding the write lock**, after checking
//!    that no row references its hash (`collect`). Since (1) holds the same lock
//!    while creating files, creation and removal serialize across every process.
//!
//! So every row has a file at every commit. A crash can leave an unreferenced
//! file or a staging file behind, never a row without a file.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use directories::{ProjectDirs, UserDirs};

use crate::{
    error::{Error, Result, ResultExt},
    model::{Added, Edited, Image, ImageFilter, ImageHash, ImageName, ImageRef, MediaType, NewName, Tag, TagCount},
    source::Source,
    store::{
        ChangeKind, Query, Store,
        files::{Layout, LockGuard},
        upgrade,
    },
    thumbs::{ThumbSize, Thumbnail, Thumbs},
};

/// Folder name for the library's data. Not `CARGO_PKG_NAME`: this crate is
/// `kura-core`, and renaming the folders would orphan existing libraries.
const APP_NAME: &str = "kura";

/// Where a library lives. Plain input: nothing trusts these after `open`.
#[derive(Debug, Clone)]
pub struct LibraryPaths {
    /// Image files, `<hash>.<ext>`, plus a `.kura-staging/` folder.
    pub pictures: PathBuf,
    /// `kura.db` and `kura.lock`.
    pub data: PathBuf,
    /// Thumbnails. Always safe to delete.
    pub cache: PathBuf,
}

impl LibraryPaths {
    /// `~/Pictures/kura`, plus the platform's app-data and cache folders.
    pub fn for_current_user() -> Result<Self> {
        let user_dirs = UserDirs::new().ok_or_else(|| Error::Setup("can't find your home folder".into()))?;
        let pictures = user_dirs
            .picture_dir()
            .ok_or_else(|| Error::Setup("can't find your Pictures folder".into()))?
            .join(APP_NAME);
        let project = ProjectDirs::from("com", "", APP_NAME)
            .ok_or_else(|| Error::Setup("can't find the app data folder".into()))?;
        Ok(Self {
            pictures,
            data: project.data_local_dir().to_path_buf(),
            cache: project.cache_dir().to_path_buf(),
        })
    }

    /// Everything under one folder: `root/{pictures,data,cache}`.
    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self { pictures: root.join("pictures"), data: root.join("data"), cache: root.join("cache") }
    }

    /// What every binary's `--library` flag means: a folder, or the user's default library.
    pub fn resolve(library: Option<&Path>) -> Result<Self> {
        library.map_or_else(Self::for_current_user, |root| Ok(Self::under(root)))
    }

    /// Delete the whole library (images, database, thumbnails) without opening it.
    /// Fails with `LibraryInUse` while any process has it open.
    pub fn destroy(self) -> Result<()> {
        let _lock = if self.data.exists() {
            Some(LockGuard::exclusive(&self.data.join("kura.lock"))?)
        } else {
            None
        };
        for dir in [&self.pictures, &self.cache, &self.data] {
            if dir.exists() {
                std::fs::remove_dir_all(dir).internal("deleting library")?;
            }
        }
        Ok(())
    }
}

/// Tunables with one source of truth; the server derives its limits from these.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Largest image `add` accepts, from any source. Default 50 MiB.
    pub max_image_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self { max_image_bytes: 50 * 1024 * 1024 }
    }
}

/// An open library. Cloning is cheap; share it between tasks and handlers.
///
/// A `Library` exists only after `open` has created the folders, taken the
/// shared process lock, configured SQLite and upgraded the library.
#[derive(Clone)]
pub struct Library {
    inner: Arc<Inner>,
}

struct Inner {
    store: Store,
    layout: Layout,
    thumbs: Thumbs,
    limits: Limits,
    _lock: LockGuard,
}

/// An image file opened for reading, ready to stream.
pub struct ImageFile {
    file: tokio::fs::File,
    len: u64,
    media_type: Option<MediaType>,
    hash: ImageHash,
}

impl ImageFile {
    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn media_type(&self) -> Option<MediaType> {
        self.media_type
    }

    /// The content hash: a strong ETag, since the bytes at a hash never change.
    pub fn hash(&self) -> &ImageHash {
        &self.hash
    }

    pub fn into_file(self) -> tokio::fs::File {
        self.file
    }
}

impl Library {
    // ---------------------------------------------------------------- lifecycle

    pub async fn open(paths: LibraryPaths) -> Result<Self> {
        Self::open_with(paths, Limits::default()).await
    }

    pub async fn open_with(paths: LibraryPaths, limits: Limits) -> Result<Self> {
        for dir in [&paths.pictures, &paths.data] {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(|e| Error::Setup(format!("can't create {}: {e}", dir.display())))?;
        }
        let lock = LockGuard::shared(&paths.data.join("kura.lock"))?;

        let layout = Layout::new(paths.pictures);
        layout.create_dirs().await?;
        let store = Store::open(&paths.data.join("kura.db")).await?;
        upgrade::run(&store, &layout).await?;
        layout.clear_stale_staging(Duration::from_secs(60 * 60)).await?;
        let thumbs = Thumbs::open(paths.cache).await?;

        Ok(Self { inner: Arc::new(Inner { store, layout, thumbs, limits, _lock: lock }) })
    }

    pub fn limits(&self) -> Limits {
        self.inner.limits
    }

    /// Close the database. Other clones of this handle stop working.
    pub async fn close(self) {
        self.inner.store.close().await;
    }

    // ---------------------------------------------------------------- reads

    /// Images, newest first.
    pub async fn list(&self, filter: &ImageFilter) -> Result<Vec<Image>> {
        let tag = filter.tag.as_ref().map(Tag::as_str);
        self.inner.store.images(Query { tag, ..Query::default() }).await
    }

    /// One image by hash or name (names ignore case).
    pub async fn image(&self, id: impl Into<ImageRef>) -> Result<Image> {
        let id = id.into();
        self.inner
            .store
            .images(Query::by_ref(&id))
            .await?
            .into_iter()
            .next()
            .ok_or(Error::NotFound(id))
    }

    /// Every tag with how many images have it, alphabetically.
    pub async fn tags(&self) -> Result<Vec<TagCount>> {
        self.inner.store.tag_counts().await
    }

    /// Where an image's file is, for local clients that hand it to other
    /// programs (open, clipboard). Read-only: nothing in kura-core writes or
    /// deletes through a path a caller passes in.
    pub fn path_of(&self, image: &Image) -> PathBuf {
        self.inner.layout.image_path(&image.hash, &image.extension)
    }

    /// Open an image's file for streaming. The handle stays readable even if the
    /// image is deleted meanwhile.
    pub async fn open_file(&self, id: impl Into<ImageRef>) -> Result<ImageFile> {
        let image = self.image(id).await?;
        let file = tokio::fs::File::open(self.path_of(&image)).await.internal("opening image file")?;
        let len = file.metadata().await.internal("reading image file")?.len();
        Ok(ImageFile { file, len, media_type: image.media_type(), hash: image.hash })
    }

    // ---------------------------------------------------------------- writes

    /// Store a new image.
    ///
    /// Adding bytes that are already stored returns `Added::Existing` with the
    /// stored image untouched, also when two adds of the same bytes race.
    pub async fn add(&self, source: Source, name: impl Into<NewName>, tags: &[Tag]) -> Result<Added> {
        let name = name.into();
        let hint = source.hint.clone();
        let staged = self.inner.layout.stage(source, self.inner.limits.max_image_bytes).await?;

        // Fast path: no lock needed to see that these bytes are already stored.
        if let Some(existing) = self.find(&staged.hash).await? {
            return Ok(Added::Existing(existing));
        }

        let hash = staged.hash.clone();
        let extension = staged.media_type.extension();
        let mut tx = self.inner.store.begin_write().await?;

        let name = match name {
            NewName::Exactly(name) => name,
            NewName::FromSource => {
                let base = ImageName::slug_from_hint(hint.as_deref());
                let mut candidate = base.clone();
                let mut n = 2;
                while tx.name_taken(&candidate).await? {
                    candidate = base.with_suffix(n);
                    n += 1;
                }
                candidate
            }
        };

        if !tx.insert_image(&hash, &name, extension).await? {
            // Another add stored the same bytes after our fast-path check.
            let existing = tx.resolve(&ImageRef::Hash(hash)).await?;
            return Ok(Added::Existing(existing));
        }
        for tag in tags {
            tx.insert_tag(&hash, tag).await?;
        }
        tx.record(ChangeKind::Added, &hash, Some(name.as_str())).await?;

        // Still holding the write lock: the file appears together with its row.
        self.inner.layout.promote(staged).await?;
        if let Err(e) = tx.commit().await {
            self.collect(&hash, extension).await;
            return Err(e);
        }

        Ok(Added::Created(self.image(&hash).await?))
    }

    /// Rename an image. Only the database changes: files are named by hash.
    /// Renaming to the exact same name changes nothing; a case-only change is a rename.
    pub async fn rename(&self, id: impl Into<ImageRef>, name: ImageName) -> Result<Edited> {
        let id = id.into();
        let mut tx = self.inner.store.begin_write().await?;
        let image = tx.resolve(&id).await?;
        if image.name == name {
            return Ok(Edited { image, changed: false });
        }
        tx.set_name(&image.hash, &name).await?;
        tx.record(ChangeKind::Renamed, &image.hash, Some(name.as_str())).await?;
        tx.commit().await?;
        Ok(Edited { image: self.image(&image.hash).await?, changed: true })
    }

    /// Add tags. Tags the image already has are skipped.
    pub async fn tag(&self, id: impl Into<ImageRef>, tags: &[Tag]) -> Result<Edited> {
        let id = id.into();
        let mut tx = self.inner.store.begin_write().await?;
        let image = tx.resolve(&id).await?;
        let mut changed = false;
        for tag in tags {
            if tx.insert_tag(&image.hash, tag).await? {
                tx.record(ChangeKind::Tagged, &image.hash, Some(tag.as_str())).await?;
                changed = true;
            }
        }
        if !changed {
            return Ok(Edited { image, changed });
        }
        tx.commit().await?;
        Ok(Edited { image: self.image(&image.hash).await?, changed })
    }

    /// Remove a tag. Removing a tag the image doesn't have changes nothing.
    pub async fn untag(&self, id: impl Into<ImageRef>, tag: &Tag) -> Result<Edited> {
        let id = id.into();
        let mut tx = self.inner.store.begin_write().await?;
        let image = tx.resolve(&id).await?;
        if !tx.delete_tag(&image.hash, tag).await? {
            return Ok(Edited { image, changed: false });
        }
        tx.record(ChangeKind::Untagged, &image.hash, Some(tag.as_str())).await?;
        tx.commit().await?;
        Ok(Edited { image: self.image(&image.hash).await?, changed: true })
    }

    /// Delete an image, its tags and its files. Returns what was deleted.
    pub async fn delete(&self, id: impl Into<ImageRef>) -> Result<Image> {
        let id = id.into();
        let mut tx = self.inner.store.begin_write().await?;
        let image = tx.resolve(&id).await?;
        tx.delete_image(&image.hash).await?;
        tx.record(ChangeKind::Deleted, &image.hash, Some(image.name.as_str())).await?;
        tx.commit().await?;

        self.collect(&image.hash, &image.extension).await;
        Ok(image)
    }

    // ---------------------------------------------------------------- derived files

    /// A thumbnail no larger than `size`, made on first use and cached.
    pub async fn thumbnail(&self, id: impl Into<ImageRef>, size: ThumbSize) -> Result<Thumbnail> {
        let image = self.image(id).await?;
        self.inner.thumbs.get_or_create(&image.hash, &self.path_of(&image), size).await
    }

    pub async fn clear_thumbnails(&self) -> Result<()> {
        self.inner.thumbs.clear().await
    }

    // ---------------------------------------------------------------- internals

    async fn find(&self, hash: &ImageHash) -> Result<Option<Image>> {
        Ok(self
            .inner
            .store
            .images(Query { hash: Some(hash.as_str()), ..Query::default() })
            .await?
            .into_iter()
            .next())
    }

    /// Remove an image's files if no row references its hash. Holds the write
    /// lock across the check and the unlink, so a concurrent add of the same
    /// bytes can't lose its file. Best effort: on failure the file stays behind
    /// as an unreferenced orphan, which is harmless.
    async fn collect(&self, hash: &ImageHash, extension: &str) {
        let result: Result<()> = async {
            let mut tx = self.inner.store.begin_write().await?;
            if !tx.is_referenced(hash).await? {
                self.inner.layout.remove_image_file(hash, extension).await?;
            }
            tx.commit().await
        }
        .await;
        if let Err(e) = result {
            eprintln!("kura: leaving file for {hash} behind: {e:?}");
        }
        self.inner.thumbs.remove(hash).await;
    }
}
