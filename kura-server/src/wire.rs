//! The JSON the API sends and receives. Public, so tests and a future Rust
//! client share one definition; kura-core stays free of serde.

use kura_core::{ImageHash, Timestamp};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Image {
    pub hash: String,
    pub name: String,
    /// `None` only for legacy images in an unsupported format.
    pub mime_type: Option<String>,
    /// RFC 3339, UTC.
    pub created_at: Timestamp,
    pub tags: Vec<String>,
    /// Immutable: the bytes at a hash URL never change.
    pub file_url: String,
    pub thumbnail_url: String,
}

impl From<&kura_core::Image> for Image {
    fn from(image: &kura_core::Image) -> Self {
        Self {
            hash: image.hash().to_string(),
            name: image.name().to_string(),
            mime_type: image.media_type().map(|m| m.as_str().to_owned()),
            created_at: image.created_at(),
            tags: image.tags().iter().map(ToString::to_string).collect(),
            file_url: file_url(image.hash()),
            thumbnail_url: format!("/images/{}/thumbnail", image.hash()),
        }
    }
}

pub fn file_url(hash: &ImageHash) -> String {
    format!("/images/{hash}/file")
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TagCount {
    pub tag: String,
    pub count: u64,
}

/// Every error response has this shape. `code` is stable; `message` is for people.
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub tag: Option<String>,
}

/// `POST /images?name=&tags=a,b&file_name=` with the raw image as the body.
#[derive(Debug, Deserialize)]
pub struct UploadQuery {
    /// Without a name, one is derived from `file_name`.
    pub name: Option<String>,
    /// Comma-separated.
    #[serde(default)]
    pub tags: String,
    pub file_name: Option<String>,
}

/// `POST /imports`: add an image from a public URL.
#[derive(Debug, Serialize, Deserialize)]
pub struct Import {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rename {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddTag {
    pub tag: String,
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    /// `grid` (256 px) or `preview` (1000 px, the default).
    pub size: Option<String>,
}
