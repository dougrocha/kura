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
    /// The effective caption (manual over ocr). `null` means not read yet.
    pub caption: Option<Caption>,
}

/// The text written inside an image. `text` is `""` when it was read and has none.
#[derive(Debug, Serialize, Deserialize)]
pub struct Caption {
    pub text: String,
    /// `ocr` or `manual`.
    pub source: String,
    /// The recognizer of an ocr caption (`vision:1`); `null` for manual.
    pub engine: Option<String>,
}

impl From<&kura_core::Reading> for Caption {
    fn from(reading: &kura_core::Reading) -> Self {
        Self {
            text: reading.outcome().text().to_owned(),
            source: reading.source().to_string(),
            engine: reading.engine().map(ToString::to_string),
        }
    }
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
            caption: image.caption().map(Into::into),
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
    /// RFC 3339 or "YYYY-MM-DD HH:MM:SS" (UTC); defaults to now.
    pub created_at: Option<String>,
}

/// `POST /imports`: add an image from a public URL.
#[derive(Debug, Serialize, Deserialize)]
pub struct Import {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// RFC 3339 or "YYYY-MM-DD HH:MM:SS" (UTC); defaults to now.
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Rename {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AddTag {
    pub tag: String,
}

/// `PUT /images/{id}/captions/{source}`.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptionBody {
    /// Raw text; the server normalizes it. Blank means "there is no text".
    pub text: String,
    /// Required for `ocr`, not allowed for `manual`.
    #[serde(default)]
    pub engine: Option<String>,
}

/// `GET /search?q=`
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

/// The ranked answer to `GET /search`: hashes only, best first. A client holds
/// the images already, so each hit is about 70 bytes.
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResponse {
    /// The change-log position of this answer (the number SSE `id:` carries).
    pub seq: u64,
    pub hits: Vec<Hit>,
    /// Images in the library.
    pub images: usize,
    /// Images with no caption yet: searchable by name and tags only.
    pub unread: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Hit {
    pub hash: String,
    /// Only when the caption took part in the match; omitted otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<Snippet>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Snippet {
    /// Caption text comes before the first segment.
    pub leading_cut: bool,
    pub segments: Vec<Segment>,
    /// Caption text comes after the last segment.
    pub trailing_cut: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Segment {
    pub text: String,
    pub matched: bool,
}

impl From<&kura_core::SearchResults> for SearchResponse {
    fn from(results: &kura_core::SearchResults) -> Self {
        Self {
            seq: results.seq.0,
            hits: results
                .hits
                .iter()
                .map(|hit| Hit {
                    hash: hit.image().hash().to_string(),
                    snippet: hit.snippet().map(|s| Snippet {
                        leading_cut: s.leading_cut,
                        segments: s
                            .segments
                            .iter()
                            .map(|seg| Segment { text: seg.text.clone(), matched: seg.matched })
                            .collect(),
                        trailing_cut: s.trailing_cut,
                    }),
                })
                .collect(),
            images: results.coverage.images,
            unread: results.coverage.unread,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ThumbnailQuery {
    /// `grid` (256 px) or `preview` (1000 px, the default).
    pub size: Option<String>,
}

/// `GET /events` sends one of these as the `data` of each Server-Sent Event.
/// The event's `id` is `seq` and its `event` is `kind`. A `resync` event (no
/// data) means: reload with `GET /images`, then keep listening.
#[derive(Debug, Serialize, Deserialize)]
pub struct Change {
    pub seq: u64,
    /// `added`, `renamed`, `tagged`, `untagged`, `deleted` or `captioned`.
    pub kind: String,
    pub hash: String,
    /// The new name (added, renamed), the tag (tagged, untagged), the old
    /// name (deleted), or `ocr`, `manual`, `cleared:ocr`, `cleared:manual`
    /// (captioned; never the text).
    pub detail: Option<String>,
    pub at: Timestamp,
}

impl From<&kura_core::Change> for Change {
    fn from(change: &kura_core::Change) -> Self {
        Self {
            seq: change.seq.0,
            kind: change.kind.as_str().to_owned(),
            hash: change.image.to_string(),
            detail: change.detail.clone(),
            at: change.at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Resume after this seq. The `Last-Event-ID` header, which browsers and
    /// SSE clients send on reconnect, takes precedence.
    pub since: Option<u64>,
}

/// `GET /config`: the shared config file (`kura config`), for the Mac app's Settings.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    /// Where the file is.
    pub path: String,
    /// `library` as written in the file (`~/Memes`); `None` when unset.
    pub library: Option<String>,
    /// The images folder this server is using now.
    pub library_folder: String,
    pub close_after_copy: bool,
    pub relaunch_when_hidden: bool,
}

/// `PATCH /config`: one change, as in `kura config set`. `value: null` unsets.
#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigChange {
    pub key: String,
    pub value: Option<String>,
}
