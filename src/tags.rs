use miette::{IntoDiagnostic, Result};

use crate::{
    State,
    types::{CreatedAt, ImageHash},
};

#[derive(Debug)]
pub struct Tag {
    pub id: i64,
    pub image_hash: ImageHash,
    pub tag: String,
    pub created_at: CreatedAt,
}

impl Tag {
    pub async fn create_table(state: &State) -> Result<()> {
        sqlx::query!(
            "CREATE TABLE IF NOT EXISTS tags (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                image_hash TEXT NOT NULL REFERENCES images(hash) ON DELETE CASCADE,
                tag TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(image_hash, tag)
            )"
        )
        .execute(&state.db_pool)
        .await
        .into_diagnostic()?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct NewTag<'a> {
    image_hash: &'a ImageHash,
    tag: &'a str,
}

impl<'a> NewTag<'a> {
    pub fn new(image_hash: &'a ImageHash, tag: &'a str) -> Self {
        Self { image_hash, tag }
    }

    pub async fn insert(&self, state: &State) -> Result<()> {
        let hash = self.image_hash.as_str();
        sqlx::query!(
            "INSERT OR IGNORE INTO tags (image_hash, tag, created_at) VALUES (?, ?, datetime('now'))",
            hash,
            self.tag
        )
        .execute(&state.db_pool)
        .await
        .into_diagnostic()?;

        Ok(())
    }
}
