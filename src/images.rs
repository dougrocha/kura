use std::{
    fs,
    path::{Path, PathBuf},
};

use jiff::civil::DateTime;
use miette::{IntoDiagnostic, Result, miette};

use hako::mime_type::MimeType;

use crate::{
    State,
    tags::Tag,
    types::{CreatedAt, ImageHash},
};

#[derive(Debug)]
pub struct Image {
    pub id: i64,
    pub hash: ImageHash,
    pub name: String,
    pub file_path: PathBuf,
    pub created_at: CreatedAt,
}

#[derive(Debug)]
pub struct ImageWithTags {
    pub image: Image,
    pub tags: Vec<Tag>,
}

fn parse_created_at(s: String) -> Result<CreatedAt> {
    DateTime::strptime("%Y-%m-%d %H:%M:%S", &s)
        .map(CreatedAt)
        .into_diagnostic()
}

impl Image {
    pub fn mime_type(&self) -> Option<MimeType> {
        self.file_path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(|e| MimeType::from_extension(e).ok())
    }

    pub async fn create_table(state: &State) -> Result<()> {
        sqlx::query!(
            "CREATE TABLE IF NOT EXISTS images (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hash TEXT NOT NULL UNIQUE,
                name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            )"
        )
        .execute(&state.db_pool)
        .await
        .into_diagnostic()?;

        Ok(())
    }

    pub async fn all_with_tag(state: &State, tag: &str) -> Result<Vec<ImageWithTags>> {
        let rows = sqlx::query!(
            "SELECT i.id, i.hash, i.name, i.file_path, i.created_at,
                t.id AS tag_id, t.image_hash AS tag_image_hash, t.tag, t.created_at AS tag_created_at
             FROM images i
             LEFT JOIN tags t ON t.image_hash = i.hash
             WHERE i.hash IN (SELECT image_hash FROM tags WHERE tag = ?)
             ORDER BY i.hash",
            tag
        )
        .fetch_all(&state.db_pool)
        .await
        .into_diagnostic()?;

        let mapped = rows
            .into_iter()
            .map(|row| -> Result<(Self, Option<Tag>)> {
                let tag = row
                    .tag_id
                    .map(|id| -> Result<Tag> {
                        Ok(Tag {
                            id,
                            image_hash: ImageHash(row.tag_image_hash.unwrap()),
                            tag: row.tag.unwrap(),
                            created_at: parse_created_at(row.tag_created_at.unwrap())?,
                        })
                    })
                    .transpose()?;
                Ok((
                    Self {
                        id: row.id.unwrap(),
                        hash: ImageHash(row.hash),
                        name: row.name,
                        file_path: PathBuf::from(row.file_path),
                        created_at: parse_created_at(row.created_at)?,
                    },
                    tag,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::collect_images(mapped))
    }

    pub async fn all(state: &State) -> Result<Vec<ImageWithTags>> {
        let rows = sqlx::query!(
            "SELECT i.id, i.hash, i.name, i.file_path, i.created_at,
                t.id AS tag_id, t.image_hash AS tag_image_hash, t.tag, t.created_at AS tag_created_at
             FROM images i
             LEFT JOIN tags t ON t.image_hash = i.hash
             ORDER BY i.hash"
        )
        .fetch_all(&state.db_pool)
        .await
        .into_diagnostic()?;

        let mapped = rows
            .into_iter()
            .map(|row| -> Result<(Self, Option<Tag>)> {
                let tag = row
                    .tag_id
                    .map(|id| -> Result<Tag> {
                        Ok(Tag {
                            id,
                            image_hash: ImageHash(row.tag_image_hash.unwrap()),
                            tag: row.tag.unwrap(),
                            created_at: parse_created_at(row.tag_created_at.unwrap())?,
                        })
                    })
                    .transpose()?;
                Ok((
                    Self {
                        id: row.id.unwrap(),
                        hash: ImageHash(row.hash),
                        name: row.name,
                        file_path: PathBuf::from(row.file_path),
                        created_at: parse_created_at(row.created_at)?,
                    },
                    tag,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::collect_images(mapped))
    }

    pub async fn find_by_file_path(
        state: &State,
        file_path: &Path,
    ) -> Result<Option<ImageWithTags>> {
        let path_str = file_path
            .to_str()
            .ok_or_else(|| miette!("Invalid file path"))?;

        let rows = sqlx::query!(
            "SELECT i.id, i.hash, i.name, i.file_path, i.created_at,
                t.id AS tag_id, t.image_hash AS tag_image_hash, t.tag, t.created_at AS tag_created_at
             FROM images i
             LEFT JOIN tags t ON t.image_hash = i.hash
             WHERE i.file_path = ?",
            path_str
        )
        .fetch_all(&state.db_pool)
        .await
        .into_diagnostic()?;

        let mapped = rows
            .into_iter()
            .map(|row| -> Result<(Self, Option<Tag>)> {
                let tag = row
                    .tag_id
                    .map(|id| -> Result<Tag> {
                        Ok(Tag {
                            id,
                            image_hash: ImageHash(row.tag_image_hash.unwrap()),
                            tag: row.tag.unwrap(),
                            created_at: parse_created_at(row.tag_created_at.unwrap())?,
                        })
                    })
                    .transpose()?;
                Ok((
                    Self {
                        id: row.id,
                        hash: ImageHash(row.hash),
                        name: row.name,
                        file_path: PathBuf::from(row.file_path),
                        created_at: parse_created_at(row.created_at)?,
                    },
                    tag,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::collect_images(mapped).into_iter().next())
    }

    pub async fn find_by_hash_or_name(
        state: &State,
        hash_or_name: &str,
    ) -> Result<Option<ImageWithTags>> {
        let rows = sqlx::query!(
            "SELECT i.id, i.hash, i.name, i.file_path, i.created_at,
                t.id AS tag_id, t.image_hash AS tag_image_hash, t.tag, t.created_at AS tag_created_at
             FROM images i
             LEFT JOIN tags t ON t.image_hash = i.hash
             WHERE i.hash = ? OR i.name = ?",
            hash_or_name,
            hash_or_name
        )
        .fetch_all(&state.db_pool)
        .await
        .into_diagnostic()?;

        let mapped = rows
            .into_iter()
            .map(|row| -> Result<(Self, Option<Tag>)> {
                let tag = row
                    .tag_id
                    .map(|id| -> Result<Tag> {
                        Ok(Tag {
                            id,
                            image_hash: ImageHash(row.tag_image_hash.unwrap()),
                            tag: row.tag.unwrap(),
                            created_at: parse_created_at(row.tag_created_at.unwrap())?,
                        })
                    })
                    .transpose()?;
                Ok((
                    Self {
                        id: row.id,
                        hash: ImageHash(row.hash),
                        name: row.name,
                        file_path: PathBuf::from(row.file_path),
                        created_at: parse_created_at(row.created_at)?,
                    },
                    tag,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self::collect_images(mapped).into_iter().next())
    }

    pub async fn rename(&mut self, state: &State, new_name: &str) -> Result<()> {
        let new_file_path = self
            .file_path
            .with_file_name(new_name.replace(' ', "-").to_lowercase())
            .with_extension(self.file_path.extension().unwrap_or_default());

        if fs::exists(&new_file_path).into_diagnostic()? {
            return Err(miette!("An image with name {} already exists", new_name));
        }

        let new_path_str = new_file_path.to_str().unwrap();
        let hash = self.hash.as_str();

        sqlx::query!(
            "UPDATE images SET name = ?, file_path = ? WHERE hash = ?",
            new_name,
            new_path_str,
            hash
        )
        .execute(&state.db_pool)
        .await
        .into_diagnostic()?;

        fs::rename(&self.file_path, &new_file_path).into_diagnostic()?;
        self.name = new_name.to_string();
        self.file_path = new_file_path;
        Ok(())
    }

    pub async fn delete(&self, state: &State) -> Result<()> {
        let mut tx = state.db_pool.begin().await.into_diagnostic()?;
        let hash = self.hash.as_str();

        sqlx::query!("DELETE FROM tags WHERE image_hash = ?", hash)
            .execute(&mut *tx)
            .await
            .into_diagnostic()?;

        sqlx::query!("DELETE FROM images WHERE hash = ?", hash)
            .execute(&mut *tx)
            .await
            .into_diagnostic()?;

        tx.commit().await.into_diagnostic()?;

        if fs::exists(&self.file_path).into_diagnostic()? {
            fs::remove_file(&self.file_path).into_diagnostic()?;
        }

        Ok(())
    }

    fn collect_images(rows: Vec<(Self, Option<Tag>)>) -> Vec<ImageWithTags> {
        let mut images: Vec<ImageWithTags> = Vec::new();

        for (img, tag) in rows {
            if images.last().is_none_or(|last| last.image.hash != img.hash) {
                images.push(ImageWithTags {
                    image: img,
                    tags: vec![],
                });
            }
            if let Some(t) = tag {
                images.last_mut().unwrap().tags.push(t);
            }
        }

        images
    }
}

#[derive(Debug)]
pub struct NewImage<'a> {
    hash: ImageHash,
    name: &'a str,
    file_path: &'a str,
}

impl<'a> NewImage<'a> {
    pub fn new(hash: ImageHash, name: &'a str, file_path: &'a str) -> Self {
        Self {
            hash,
            name,
            file_path,
        }
    }

    pub async fn insert(&self, state: &State) -> Result<i64> {
        let hash = self.hash.as_str();

        let result = sqlx::query!(
            "INSERT INTO images (hash, name, file_path, created_at) VALUES (?, ?, ?, datetime('now'))",
            hash,
            self.name,
            self.file_path
        )
        .execute(&state.db_pool)
        .await
        .into_diagnostic()?;

        Ok(result.last_insert_rowid())
    }
}
