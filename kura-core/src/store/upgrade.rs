//! Bring any existing library to the current shape. Run by `Library::open`;
//! every step is safe to re-run after a crash.
//!
//! 1. `legacy_pre_0002`: Rust, only while `images.file_path` exists (libraries
//!    from before migrations).
//! 2. `sqlx::migrate!()`: 0001 adopts the old schema, 0002 switches to hash file
//!    names (unchanged: migrations are checksummed), 0003 adds the change log.

use std::{collections::HashSet, path::Path};

use sqlx::{SqlitePool, migrate::MigrateError};

use crate::{
    error::{Error, Result, ResultExt, internal},
    model::{ImageName, MediaType},
    store::{Store, files::Layout},
};

pub(crate) async fn run(store: &Store, layout: &Layout) -> Result<()> {
    legacy_pre_0002(store.pool(), layout).await?;
    sqlx::migrate!().run(store.pool()).await.map_err(|e| match e {
        MigrateError::VersionMissing(v) => Error::Setup(format!(
            "this library was upgraded by a newer version of kura (migration {v}); update kura to open it"
        )),
        e => internal("running migrations", e),
    })
}

/// Libraries from before migrations stored files under their name, with the path
/// in `images.file_path`, and only required *file names* to be unique. Make the
/// data fit migration 0002 before it runs:
///
/// - move each file to `<hash>.<ext>`, taking the extension from the file's
///   contents when it can be read. Old rows hold *absolute* paths, but the old
///   code always stored files in the picture folder, so only the file name is
///   used, joined to *this* library's picture folder. A moved or copied library
///   never touches files outside its own folder;
/// - rewrite `file_path` to the bare `<hash>.<ext>`, so 0002's SQL ("text after
///   the last dot") derives exactly the extension used here;
/// - make names valid and unique ignoring case (`Cat`, `cat` → `Cat`, `cat-2`,
///   oldest keeps its name). Without this, 0002's unique index fails on every
///   open and the library can't be opened.
///
/// Re-running after a crash finds files already at `<hash>.*` and plans the same rows.
async fn legacy_pre_0002(pool: &SqlitePool, layout: &Layout) -> Result<()> {
    // Unchecked queries: the compile-time schema no longer has `file_path`.
    let has_legacy_column: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('images') WHERE name = 'file_path'",
    )
    .fetch_one(pool)
    .await
    .internal("inspecting schema")?;
    if !has_legacy_column {
        return Ok(());
    }

    let rows: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id, hash, name, file_path FROM images ORDER BY id")
            .fetch_all(pool)
            .await
            .internal("reading legacy images")?;

    let mut taken = HashSet::new();
    let mut plan = Vec::with_capacity(rows.len());
    for (id, hash, name, file_path) in rows {
        let old_path = match Path::new(&file_path).file_name() {
            Some(file_name) => layout.pictures_file(file_name),
            None => layout.pictures_file(std::ffi::OsStr::new(&hash)),
        };
        let old_path = old_path.as_path();
        let extension = legacy_extension(layout, &hash, old_path).await;
        let target = layout.image_path(&crate::model::ImageHash::from_row(hash.clone()), &extension);

        if exists(old_path).await && !exists(&target).await {
            tokio::fs::rename(old_path, &target).await.internal("moving legacy image")?;
        }

        let base = ImageName::parse(&name).unwrap_or_else(|_| ImageName::slug_from_hint(Some(&name)));
        let mut unique = base.clone();
        let mut n = 2;
        while !taken.insert(unique.as_str().to_ascii_lowercase()) {
            unique = base.with_suffix(n);
            n += 1;
        }

        plan.push((id, unique, format!("{hash}.{extension}")));
    }

    let mut tx = pool.begin().await.internal("upgrading legacy library")?;
    for (id, name, file_name) in plan {
        sqlx::query("UPDATE images SET name = ?, file_path = ? WHERE id = ?")
            .bind(name.as_str())
            .bind(file_name)
            .bind(id)
            .execute(&mut *tx)
            .await
            .internal("upgrading legacy image")?;
    }
    tx.commit().await.internal("upgrading legacy library")
}

/// The extension a legacy image will be stored under: from its contents if the
/// file can be read, else from wherever an interrupted run already moved it, else
/// its old extension.
async fn legacy_extension(layout: &Layout, hash: &str, old_path: &Path) -> String {
    let old_extension = old_path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .filter(|e| !e.is_empty())
        .unwrap_or_else(|| "bin".to_owned());

    if let Some(media) = sniff_file(old_path).await {
        return media.extension().to_owned();
    }

    let hash = crate::model::ImageHash::from_row(hash.to_owned());
    for candidate in ["png", "jpg", "gif", "webp", old_extension.as_str()] {
        if exists(&layout.image_path(&hash, candidate)).await {
            return candidate.to_owned();
        }
    }
    old_extension
}

async fn sniff_file(path: &Path) -> Option<MediaType> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await.ok()?;
    let mut head = [0; 12];
    let mut len = 0;
    while len < head.len() {
        let n = file.read(&mut head[len..]).await.ok()?;
        if n == 0 {
            break;
        }
        len += n;
    }
    MediaType::from_bytes(&head[..len]).ok()
}

async fn exists(path: &Path) -> bool {
    tokio::fs::try_exists(path).await.unwrap_or(false)
}
