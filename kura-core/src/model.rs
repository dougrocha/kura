//! Value types. Each one is built only by parsing (at a client boundary) or by the
//! store (trusted rows), so holding one is proof its rules hold.

use std::{fmt, path::Path, str::FromStr};

use hako::mime_type::MimeType;
use jiff::Timestamp;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use crate::error::{Error, InvalidInput, Result};

/// The image formats Kura stores, detected from file contents.
pub type MediaType = MimeType;

const NAME_MAX_CHARS: usize = 200;
const TAG_MAX_CHARS: usize = 64;

// ------------------------------------------------------------------ identity

/// SHA-256 of an image's bytes: 64 lowercase hex chars. Identifies an image and
/// names its file. The field is private, so a path like `../x` can't be one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageHash(String);

impl ImageHash {
    pub fn of_bytes(data: &[u8]) -> Self {
        Self(hex::encode(Sha256::digest(data)))
    }

    /// Hash a local file by streaming it (`kura remove --file`).
    pub async fn of_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let read_error = |e: std::io::Error| Error::ReadSource {
            what: format!("'{}'", path.display()),
            reason: e.to_string(),
        };
        let mut file = tokio::fs::File::open(path).await.map_err(read_error)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0; 64 * 1024];
        loop {
            let n = file.read(&mut buf).await.map_err(read_error)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Self::from_digest(hasher))
    }

    pub(crate) fn from_digest(hasher: Sha256) -> Self {
        Self(hex::encode(hasher.finalize()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Rows are trusted: they were validated on the way in.
    pub(crate) fn from_row(s: String) -> Self {
        Self(s)
    }

    fn is_hash_shaped(s: &str) -> bool {
        s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

impl FromStr for ImageHash {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        if Self::is_hash_shaped(s) {
            Ok(Self(s.to_ascii_lowercase()))
        } else {
            Err(InvalidInput::new("hash", "expected 64 hex characters"))
        }
    }
}

impl fmt::Display for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A user-facing image name: trimmed, 1 to 200 characters, no control characters,
/// and never hash-shaped, so [`ImageRef`] parsing is unambiguous. Uniqueness
/// (ignoring ASCII case) is the database's job.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ImageName(String);

impl ImageName {
    pub fn parse(s: &str) -> Result<Self, InvalidInput> {
        let s = clean_text(s, "name", NAME_MAX_CHARS)?;
        if ImageHash::is_hash_shaped(&s) {
            return Err(InvalidInput::new("name", "can't look like an image hash"));
        }
        Ok(Self(s))
    }

    /// Slug of a file name or URL ("Grumpy Cat.png" → "grumpy-cat"); "image" when empty.
    pub(crate) fn slug_from_hint(hint: Option<&str>) -> Self {
        let last_segment = hint
            .unwrap_or_default()
            .split(['?', '#'])
            .next()
            .unwrap_or_default()
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default();
        let stem = last_segment.split('.').next().unwrap_or_default();
        let slug: String = stem
            .chars()
            .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("-");
        let slug: String = slug.chars().take(NAME_MAX_CHARS).collect();

        Self::parse(&slug).unwrap_or_else(|_| Self("image".to_owned()))
    }

    /// `cat` → `cat-2`, `cat-3`, … for collisions on derived names.
    pub(crate) fn with_suffix(&self, n: u32) -> Self {
        let suffix = format!("-{n}");
        let keep = NAME_MAX_CHARS - suffix.len();
        let base: String = self.0.chars().take(keep).collect();
        Self(format!("{base}{suffix}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_row(s: String) -> Self {
        Self(s)
    }
}

impl FromStr for ImageName {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        Self::parse(s)
    }
}

impl fmt::Display for ImageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A tag: trimmed, 1 to 64 characters, no control characters and no commas
/// (lists of tags are comma-separated in query strings). Case-sensitive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(String);

impl Tag {
    pub fn parse(s: &str) -> Result<Self, InvalidInput> {
        let s = clean_text(s, "tag", TAG_MAX_CHARS)?;
        if s.contains(',') {
            return Err(InvalidInput::new("tag", "can't contain commas"));
        }
        Ok(Self(s))
    }

    /// Parse a comma-separated list, skipping empty entries ("a, b,," → [a, b]).
    pub fn parse_list(s: &str) -> Result<Vec<Self>, InvalidInput> {
        s.split(',')
            .filter(|t| !t.trim().is_empty())
            .map(Self::parse)
            .collect()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn from_row(s: String) -> Self {
        Self(s)
    }
}

impl FromStr for Tag {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        Self::parse(s)
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The text rule names and tags share.
fn clean_text(s: &str, field: &'static str, max_chars: usize) -> Result<String, InvalidInput> {
    let s = s.trim();
    if s.is_empty() {
        return Err(InvalidInput::new(field, "can't be empty"));
    }
    if s.chars().any(char::is_control) {
        return Err(InvalidInput::new(field, "can't contain control characters"));
    }
    if s.chars().count() > max_chars {
        return Err(InvalidInput::new(field, format!("can't be longer than {max_chars} characters")));
    }
    Ok(s.to_owned())
}

/// How a caller points at an image: 64 hex characters are a hash, anything else
/// that is a valid name is a name. Never ambiguous, because names can't be hash-shaped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageRef {
    Hash(ImageHash),
    Name(ImageName),
}

impl FromStr for ImageRef {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        match s.trim().parse::<ImageHash>() {
            Ok(hash) => Ok(Self::Hash(hash)),
            Err(_) => ImageName::parse(s).map(Self::Name),
        }
    }
}

impl From<ImageHash> for ImageRef {
    fn from(h: ImageHash) -> Self {
        Self::Hash(h)
    }
}

impl From<&ImageHash> for ImageRef {
    fn from(h: &ImageHash) -> Self {
        Self::Hash(h.clone())
    }
}

impl From<ImageName> for ImageRef {
    fn from(n: ImageName) -> Self {
        Self::Name(n)
    }
}

impl From<&ImageRef> for ImageRef {
    fn from(r: &ImageRef) -> Self {
        r.clone()
    }
}

impl From<&Image> for ImageRef {
    fn from(image: &Image) -> Self {
        Self::Hash(image.hash.clone())
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hash(h) => h.fmt(f),
            Self::Name(n) => n.fmt(f),
        }
    }
}

// ------------------------------------------------------------------ snapshots

/// A read-only snapshot of one image. Never authoritative: every operation takes
/// an [`ImageRef`] and re-resolves it inside its own transaction, so a stale or
/// hand-built snapshot can't cause a wrong write.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub(crate) hash: ImageHash,
    pub(crate) name: ImageName,
    /// The file's extension. Always a [`MediaType`] extension for images added by
    /// this version; legacy rows may hold whatever their old file name had.
    pub(crate) extension: String,
    pub(crate) created_at: Timestamp,
    /// Sorted.
    pub(crate) tags: Vec<Tag>,
}

impl Image {
    pub fn hash(&self) -> &ImageHash {
        &self.hash
    }

    pub fn name(&self) -> &ImageName {
        &self.name
    }

    /// `None` only for legacy rows whose file extension isn't a supported format.
    pub fn media_type(&self) -> Option<MediaType> {
        MediaType::from_extension(&self.extension).ok()
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }

    pub fn tags(&self) -> &[Tag] {
        &self.tags
    }
}

/// A tag and how many images carry it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagCount {
    pub tag: Tag,
    pub count: u64,
}

/// Which images `Library::list` returns, newest first. Grows through builder
/// methods (paging, search) without breaking callers.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ImageFilter {
    pub(crate) tag: Option<Tag>,
}

impl ImageFilter {
    pub fn all() -> Self {
        Self::default()
    }

    /// Only images that carry `tag`.
    pub fn tag(mut self, tag: Tag) -> Self {
        self.tag = Some(tag);
        self
    }
}

/// What to call a new image.
#[derive(Debug, Clone)]
pub enum NewName {
    /// Exactly this name; if it's taken, `add` fails with `Error::NameTaken`.
    Exactly(ImageName),
    /// Derived from the source's file name or URL. On a collision, `-2`, `-3`, …
    /// are tried, so a derived name never fails with `NameTaken`.
    FromSource,
}

impl From<ImageName> for NewName {
    fn from(n: ImageName) -> Self {
        Self::Exactly(n)
    }
}

impl From<Option<ImageName>> for NewName {
    fn from(n: Option<ImageName>) -> Self {
        n.map_or(Self::FromSource, Self::Exactly)
    }
}

/// Result of `Library::add`. Adding bytes that are already stored is not an error:
/// it returns the existing image untouched, which makes upload retries safe.
#[derive(Debug, Clone)]
#[must_use]
pub enum Added {
    Created(Image),
    Existing(Image),
}

impl Added {
    pub fn image(&self) -> &Image {
        match self {
            Self::Created(i) | Self::Existing(i) => i,
        }
    }

    pub fn into_image(self) -> Image {
        match self {
            Self::Created(i) | Self::Existing(i) => i,
        }
    }

    pub fn is_created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

/// Result of an idempotent edit (rename, tag, untag): the image afterwards, and
/// whether anything changed. Unchanged edits write nothing.
#[derive(Debug, Clone)]
#[must_use]
pub struct Edited {
    pub image: Image,
    pub changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refs_are_never_ambiguous() {
        let hash = "a".repeat(64);
        assert!(matches!(hash.parse::<ImageRef>(), Ok(ImageRef::Hash(_))));
        assert!(matches!("Grumpy Cat".parse::<ImageRef>(), Ok(ImageRef::Name(_))));
        assert!(ImageName::parse(&hash).is_err());
        assert!("".parse::<ImageRef>().is_err());
    }

    #[test]
    fn names_and_tags_are_cleaned() {
        assert_eq!(ImageName::parse("  cat ").unwrap().as_str(), "cat");
        assert!(ImageName::parse("a\nb").is_err());
        assert!(ImageName::parse(&"x".repeat(201)).is_err());
        assert!(Tag::parse("a,b").is_err());
        assert_eq!(
            Tag::parse_list("funny, cats,,").unwrap(),
            vec![Tag::parse("funny").unwrap(), Tag::parse("cats").unwrap()]
        );
    }

    #[test]
    fn slugs() {
        let slug = |s: &str| ImageName::slug_from_hint(Some(s)).as_str().to_owned();
        assert_eq!(slug("/tmp/Grumpy Cat.png"), "grumpy-cat");
        assert_eq!(slug("https://x.test/a/IMG_2199.JPG?size=large"), "img-2199");
        assert_eq!(slug("..."), "image");
        assert_eq!(ImageName::slug_from_hint(None).as_str(), "image");
        assert_eq!(ImageName::parse("cat").unwrap().with_suffix(2).as_str(), "cat-2");
    }
}
