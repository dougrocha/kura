use std::collections::HashMap;

use miette::{IntoDiagnostic, Result};
use rusqlite::named_params;
use serde::{Deserialize, Serialize};

use crate::{
    State,
    types::{CreatedAt, ImageHash},
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Tag {
    id: u32,
    image_hash: ImageHash,
    tag: String,
    created_at: CreatedAt,
}

impl Tag {
    pub fn all_by_hash(state: &State) -> Result<HashMap<ImageHash, Vec<Tag>>> {
        let mut stmt = state
            .db
            .prepare("SELECT id, image_hash, tag, created_at FROM tags")
            .into_diagnostic()?;
        let deser_rows = serde_rusqlite::from_rows::<Tag>(stmt.query([]).into_diagnostic()?);

        let mut tags_by_hash: HashMap<ImageHash, Vec<Tag>> = HashMap::new();
        for tag in deser_rows {
            let tag = tag.into_diagnostic()?;
            tags_by_hash
                .entry(tag.image_hash.clone())
                .or_default()
                .push(tag);
        }

        Ok(tags_by_hash)
    }
}

impl Tag {
    pub fn create_table(state: &State) -> Result<()> {
        state
            .db
            .execute(
                "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                image_hash TEXT NOT NULL REFERENCES images(hash),
                tag TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(image_hash, tag)
            )",
                (),
            )
            .into_diagnostic()?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct NewTag<'a> {
    image_hash: &'a ImageHash,
    tag: &'a str,
}

impl<'a> NewTag<'a> {
    pub fn new(image_hash: &'a ImageHash, tag: &'a str) -> Self {
        Self { image_hash, tag }
    }
}

impl<'a> NewTag<'a> {
    pub fn insert(&self, state: &State) -> Result<()> {
        state
            .db
            .execute(
                "INSERT OR IGNORE INTO tags (image_hash, tag, created_at) \
                 VALUES (:image_hash, :tag, datetime('now'))",
                named_params! { ":image_hash": self.image_hash, ":tag": self.tag },
            )
            .into_diagnostic()?;
        Ok(())
    }
}
