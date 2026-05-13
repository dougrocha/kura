use std::{
    fs,
    path::{Path, PathBuf},
};

use miette::{IntoDiagnostic, Result, miette};
use rusqlite::named_params;
use serde::{Deserialize, Serialize};

use crate::{
    State,
    types::{CreatedAt, ImageHash},
    tags::Tag,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub hash: ImageHash,
    pub name: String,
    pub file_path: PathBuf,
    pub created_at: CreatedAt,
    #[serde(default, skip_deserializing)]
    pub tags: Vec<Tag>,
}

impl Image {
    pub fn create_table(state: &State) -> Result<()> {
        state
            .db
            .execute(
                "CREATE TABLE IF NOT EXISTS images (
                hash TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                file_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
                (),
            )
            .into_diagnostic()?;
        Ok(())
    }

    pub fn all(state: &State) -> Result<Vec<Image>> {
        let mut img_stmt = state
            .db
            .prepare("SELECT hash, name, file_path, created_at FROM images")
            .into_diagnostic()?;
        let mut images: Vec<Image> =
            serde_rusqlite::from_rows(img_stmt.query([]).into_diagnostic()?)
                .collect::<Result<_, _>>()
                .into_diagnostic()?;

        let mut tags_by_hash = Tag::all_by_hash(state)?;

        for img in &mut images {
            if let Some(tags) = tags_by_hash.remove(&img.hash) {
                img.tags = tags;
            }
        }

        Ok(images)
    }

    pub fn find_by_file_path(state: &State, file_path: &Path) -> Result<Option<Image>> {
        let path_str = file_path
            .to_str()
            .ok_or_else(|| miette!("Invalid file path"))?;

        Self::find_by_field(state, "file_path", path_str)
    }

    fn find_by_field(state: &State, field: &str, value: &str) -> Result<Option<Image>> {
        let sql =
            format!("SELECT hash, name, file_path, created_at FROM images WHERE {field} = :value");
        let mut img_stmt = state.db.prepare(&sql).into_diagnostic()?;
        let mut image = serde_rusqlite::from_rows::<Image>(
            img_stmt
                .query(named_params! { ":value": value })
                .into_diagnostic()?,
        )
        .next()
        .transpose()
        .into_diagnostic()?;

        if let Some(ref mut img) = image {
            let mut tag_stmt = state
                .db
                .prepare(
                    "SELECT id, image_hash, tag, created_at FROM tags WHERE image_hash = :hash",
                )
                .into_diagnostic()?;
            img.tags = serde_rusqlite::from_rows(
                tag_stmt
                    .query(named_params! { ":hash": &img.hash })
                    .into_diagnostic()?,
            )
            .collect::<Result<_, _>>()
            .into_diagnostic()?;
        }

        Ok(image)
    }

    pub fn delete(&self, state: &State) -> Result<()> {
        state
            .db
            .execute(
                "DELETE FROM tags WHERE image_hash = :hash",
                named_params! { ":hash": self.hash },
            )
            .into_diagnostic()?;

        state
            .db
            .execute(
                "DELETE FROM images WHERE hash = :hash",
                named_params! { ":hash": self.hash },
            )
            .into_diagnostic()?;

        if fs::exists(&self.file_path).into_diagnostic()? {
            fs::remove_file(&self.file_path).into_diagnostic()?;
        }

        Ok(())
    }

    pub fn find_by_hash_or_name(state: &State, hash_or_name: &str) -> Result<Option<Image>> {
        if let Some(img) = Self::find_by_field(state, "hash", hash_or_name)? {
            return Ok(Some(img));
        }

        Self::find_by_field(state, "name", hash_or_name)
    }
}

#[derive(Debug, Serialize)]
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
}

impl<'a> NewImage<'a> {
    pub fn insert(&self, state: &State) -> Result<()> {
        state.db.execute(
            "INSERT INTO images (hash, name, file_path, created_at) \
                 VALUES (:hash, :name, :file_path, datetime('now'))",
            named_params! { ":hash": self.hash, ":name": self.name, ":file_path": self.file_path },
        ).into_diagnostic()?;
        Ok(())
    }
}
