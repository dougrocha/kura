//! All SQL lives here. `Store::begin_write` is the only way to get a writable
//! transaction, and every row-writing method lives on `WriteTx`, so each write
//! can record its change in the same transaction.

pub(crate) mod files;
pub(crate) mod upgrade;

use std::path::Path;

use jiff::{Timestamp, civil::DateTime, tz::TimeZone};
use sqlx::{
    Sqlite, SqliteExecutor, SqlitePool, Transaction,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};

use crate::{
    error::{Error, Result, ResultExt, internal},
    model::{Image, ImageHash, ImageName, ImageRef, Tag, TagCount},
};

#[derive(Clone)]
pub(crate) struct Store {
    pool: SqlitePool,
}

/// Narrows the one image query. All `None` means every image.
#[derive(Default, Clone, Copy)]
pub(crate) struct Query<'a> {
    pub(crate) hash: Option<&'a str>,
    pub(crate) name: Option<&'a str>,
    pub(crate) tag: Option<&'a str>,
}

impl<'a> Query<'a> {
    pub(crate) fn by_ref(r: &'a ImageRef) -> Self {
        match r {
            ImageRef::Hash(h) => Self { hash: Some(h.as_str()), ..Self::default() },
            ImageRef::Name(n) => Self { name: Some(n.as_str()), ..Self::default() },
        }
    }
}

struct ImageRow {
    hash: String,
    name: String,
    extension: String,
    created_at: String,
    tags: String,
}

impl ImageRow {
    fn into_image(self) -> Result<Image> {
        let tags: Vec<String> = serde_json::from_str(&self.tags).internal("decoding tags")?;
        Ok(Image {
            hash: ImageHash::from_row(self.hash),
            name: ImageName::from_row(self.name),
            extension: self.extension,
            created_at: parse_sqlite_time(&self.created_at)?,
            tags: tags.into_iter().map(Tag::from_row).collect(),
        })
    }
}

/// SQLite's `datetime('now')` is UTC without a zone marker.
fn parse_sqlite_time(s: &str) -> Result<Timestamp> {
    let civil = DateTime::strptime("%Y-%m-%d %H:%M:%S", s).internal("parsing timestamp")?;
    Ok(civil.to_zoned(TimeZone::UTC).internal("parsing timestamp")?.timestamp())
}

/// The one image query, shared by every read. Tags come back as a JSON array.
async fn select_images(executor: impl SqliteExecutor<'_>, q: Query<'_>) -> Result<Vec<Image>> {
    sqlx::query_as!(
        ImageRow,
        r#"SELECT i.hash AS "hash!", i.name AS "name!", i.extension AS "extension!",
                  i.created_at AS "created_at!",
                  (SELECT json_group_array(tag)
                   FROM (SELECT tag FROM tags WHERE image_hash = i.hash ORDER BY tag)) AS "tags!: String"
           FROM images i
           WHERE (?1 IS NULL OR i.hash = ?1)
             AND (?2 IS NULL OR i.name = ?2 COLLATE NOCASE)
             AND (?3 IS NULL OR EXISTS (SELECT 1 FROM tags t WHERE t.image_hash = i.hash AND t.tag = ?3))
           ORDER BY i.created_at DESC, i.id DESC"#,
        q.hash,
        q.name,
        q.tag
    )
    .fetch_all(executor)
    .await
    .internal("loading images")?
    .into_iter()
    .map(ImageRow::into_image)
    .collect()
}

fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error().is_some_and(|e| e.is_unique_violation())
}

impl Store {
    pub(crate) async fn open(db_path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(db_path)
            .create_if_missing(true)
            // Readers and one writer at a time: the TUI, CLI and server can share a library.
            .journal_mode(SqliteJournalMode::Wal)
            // Required for ON DELETE CASCADE; sqlx's default, stated because we rely on it.
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(10));
        let pool = SqlitePool::connect_with(options).await.internal("opening database")?;
        Ok(Self { pool })
    }

    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub(crate) async fn images(&self, q: Query<'_>) -> Result<Vec<Image>> {
        select_images(&self.pool, q).await
    }

    pub(crate) async fn tag_counts(&self) -> Result<Vec<TagCount>> {
        let rows = sqlx::query!(
            r#"SELECT tag AS "tag!", COUNT(*) AS "count!: i64" FROM tags GROUP BY tag ORDER BY tag"#
        )
        .fetch_all(&self.pool)
        .await
        .internal("loading tags")?;
        Ok(rows
            .into_iter()
            .map(|r| TagCount { tag: Tag::from_row(r.tag), count: r.count.unsigned_abs() })
            .collect())
    }

    /// Start a write. `BEGIN IMMEDIATE` takes SQLite's write lock up front, so
    /// writes from every process serialize here and checks inside the
    /// transaction can't be raced. File creation and deletion happen while this
    /// lock is held (see `Library::add` and `Library::collect`).
    pub(crate) async fn begin_write(&self) -> Result<WriteTx> {
        let tx = self
            .pool
            .begin_with("BEGIN IMMEDIATE")
            .await
            .internal("starting write")?;
        Ok(WriteTx { tx })
    }

    pub(crate) async fn close(&self) {
        self.pool.close().await;
    }
}

/// What a change-log row records.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ChangeKind {
    Added,
    Renamed,
    Tagged,
    Untagged,
    Deleted,
}

impl ChangeKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Renamed => "renamed",
            Self::Tagged => "tagged",
            Self::Untagged => "untagged",
            Self::Deleted => "deleted",
        }
    }
}

pub(crate) struct WriteTx {
    tx: Transaction<'static, Sqlite>,
}

impl WriteTx {
    pub(crate) async fn images(&mut self, q: Query<'_>) -> Result<Vec<Image>> {
        select_images(&mut *self.tx, q).await
    }

    pub(crate) async fn resolve(&mut self, r: &ImageRef) -> Result<Image> {
        self.images(Query::by_ref(r))
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| Error::NotFound(r.clone()))
    }

    pub(crate) async fn name_taken(&mut self, name: &ImageName) -> Result<bool> {
        let name = name.as_str();
        let taken = sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM images WHERE name = ? COLLATE NOCASE) AS "taken!: bool""#,
            name
        )
        .fetch_one(&mut *self.tx)
        .await
        .internal("checking name")?;
        Ok(taken)
    }

    /// Insert an image row. Returns `false` if the hash is already stored (another
    /// add got there first); a taken name is `NameTaken`.
    pub(crate) async fn insert_image(&mut self, hash: &ImageHash, name: &ImageName, extension: &str) -> Result<bool> {
        let (hash_str, name_str) = (hash.as_str(), name.as_str());
        let result = sqlx::query!(
            "INSERT INTO images (hash, name, extension, created_at)
             VALUES (?, ?, ?, datetime('now'))
             ON CONFLICT(hash) DO NOTHING",
            hash_str,
            name_str,
            extension
        )
        .execute(&mut *self.tx)
        .await
        .map_err(|e| self::name_error(e, name))?;
        Ok(result.rows_affected() == 1)
    }

    pub(crate) async fn set_name(&mut self, hash: &ImageHash, name: &ImageName) -> Result<()> {
        let (hash_str, name_str) = (hash.as_str(), name.as_str());
        sqlx::query!("UPDATE images SET name = ? WHERE hash = ?", name_str, hash_str)
            .execute(&mut *self.tx)
            .await
            .map_err(|e| self::name_error(e, name))?;
        Ok(())
    }

    /// Returns whether the tag was new.
    pub(crate) async fn insert_tag(&mut self, hash: &ImageHash, tag: &Tag) -> Result<bool> {
        let (hash_str, tag_str) = (hash.as_str(), tag.as_str());
        let result = sqlx::query!(
            "INSERT OR IGNORE INTO tags (image_hash, tag, created_at) VALUES (?, ?, datetime('now'))",
            hash_str,
            tag_str
        )
        .execute(&mut *self.tx)
        .await
        .internal("adding tag")?;
        Ok(result.rows_affected() == 1)
    }

    /// Returns whether the image had the tag.
    pub(crate) async fn delete_tag(&mut self, hash: &ImageHash, tag: &Tag) -> Result<bool> {
        let (hash_str, tag_str) = (hash.as_str(), tag.as_str());
        let result = sqlx::query!("DELETE FROM tags WHERE image_hash = ? AND tag = ?", hash_str, tag_str)
            .execute(&mut *self.tx)
            .await
            .internal("removing tag")?;
        Ok(result.rows_affected() > 0)
    }

    /// Delete an image row; its tags cascade.
    pub(crate) async fn delete_image(&mut self, hash: &ImageHash) -> Result<()> {
        let hash_str = hash.as_str();
        sqlx::query!("DELETE FROM images WHERE hash = ?", hash_str)
            .execute(&mut *self.tx)
            .await
            .internal("deleting image")?;
        Ok(())
    }

    pub(crate) async fn is_referenced(&mut self, hash: &ImageHash) -> Result<bool> {
        let hash_str = hash.as_str();
        let referenced = sqlx::query_scalar!(
            r#"SELECT EXISTS(SELECT 1 FROM images WHERE hash = ?) AS "referenced!: bool""#,
            hash_str
        )
        .fetch_one(&mut *self.tx)
        .await
        .internal("checking image")?;
        Ok(referenced)
    }

    /// Append to the change log, in this transaction.
    pub(crate) async fn record(&mut self, kind: ChangeKind, hash: &ImageHash, detail: Option<&str>) -> Result<()> {
        let (kind_str, hash_str) = (kind.as_str(), hash.as_str());
        sqlx::query!(
            "INSERT INTO changes (kind, image_hash, detail) VALUES (?, ?, ?)",
            kind_str,
            hash_str,
            detail
        )
        .execute(&mut *self.tx)
        .await
        .internal("recording change")?;
        Ok(())
    }

    pub(crate) async fn commit(self) -> Result<()> {
        self.tx.commit().await.internal("committing")
    }
}

/// Within a write, the name index is the only unique constraint an insert or
/// rename can hit (hash conflicts are handled by `ON CONFLICT`).
fn name_error(err: sqlx::Error, name: &ImageName) -> Error {
    if is_unique_violation(&err) {
        Error::NameTaken(name.clone())
    } else {
        internal("writing image", err)
    }
}
