//! Errors. Domain variants only: sqlx, io and image errors never appear in the
//! public API. They are boxed into the opaque [`Internal`], so their versions are
//! not part of kura-core's interface. Presentation (miette) is the CLI's job.

use std::fmt;

use crate::model::{ImageName, ImageRef};

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("no image matches '{0}'")]
    NotFound(ImageRef),

    #[error("an image named '{0}' already exists")]
    NameTaken(ImageName),

    #[error(transparent)]
    Invalid(#[from] InvalidInput),

    #[error("not a supported image (png, jpeg, gif or webp)")]
    UnsupportedMedia,

    #[error("image is larger than the {limit}-byte limit")]
    TooLarge { limit: u64 },

    /// Reading a caller-named source failed (a local file, or an upload that broke off).
    #[error("could not read {what}: {reason}")]
    ReadSource { what: String, reason: String },

    /// Fetching a URL failed or was refused by the [`FetchPolicy`](crate::FetchPolicy).
    #[error("could not fetch {url}: {reason}")]
    Fetch { url: String, reason: FetchFailure },

    /// `LibraryPaths::destroy` while another process (server, TUI) has the library open.
    #[error("the library is open in another process")]
    LibraryInUse,

    /// Bad environment on open: no home dir, unwritable dir, newer schema than this binary.
    #[error("{0}")]
    Setup(String),

    /// Everything a caller can't act on: database, I/O, image codec. Displays as
    /// "internal error"; the source chain is for logs.
    #[error("internal error")]
    Internal(#[source] Internal),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// What a client does about an error. Match it without a `_` arm, so adding a
/// kind breaks the server's status table at compile time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NotFound,
    Conflict,
    Invalid,
    TooLarge,
    Unsupported,
    Forbidden,
    Upstream,
    Busy,
    Internal,
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        match self {
            Self::NotFound(_) => ErrorKind::NotFound,
            Self::NameTaken(_) => ErrorKind::Conflict,
            Self::Invalid(_) | Self::ReadSource { .. } => ErrorKind::Invalid,
            Self::UnsupportedMedia => ErrorKind::Unsupported,
            Self::TooLarge { .. } => ErrorKind::TooLarge,
            Self::Fetch { reason: FetchFailure::Forbidden, .. } => ErrorKind::Forbidden,
            Self::Fetch { .. } => ErrorKind::Upstream,
            Self::LibraryInUse => ErrorKind::Busy,
            Self::Setup(_) | Self::Internal(_) => ErrorKind::Internal,
        }
    }

    /// A stable machine-readable code for wire bodies.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::NameTaken(_) => "name_taken",
            Self::Invalid(_) => "invalid_input",
            Self::UnsupportedMedia => "unsupported_media",
            Self::TooLarge { .. } => "too_large",
            Self::ReadSource { .. } => "unreadable_source",
            Self::Fetch { reason: FetchFailure::Forbidden, .. } => "fetch_forbidden",
            Self::Fetch { .. } => "fetch_failed",
            Self::LibraryInUse => "library_in_use",
            Self::Setup(_) | Self::Internal(_) => "internal",
        }
    }
}

/// A value that failed to parse at a boundary (CLI argument, HTTP input).
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid {field}: {reason}")]
pub struct InvalidInput {
    pub field: &'static str,
    pub reason: String,
}

impl InvalidInput {
    pub(crate) fn new(field: &'static str, reason: impl Into<String>) -> Self {
        Self { field, reason: reason.into() }
    }
}

#[derive(Debug, Clone)]
pub enum FetchFailure {
    /// The policy refused the destination (loopback, private or tailnet address).
    Forbidden,
    Timeout,
    Status(u16),
    Network(String),
}

impl fmt::Display for FetchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden => f.write_str("destination is not a public address"),
            Self::Timeout => f.write_str("timed out"),
            Self::Status(code) => write!(f, "HTTP {code}"),
            Self::Network(msg) => f.write_str(msg),
        }
    }
}

/// Opaque wrapper over sqlx/io/image errors plus what was being done. Built only
/// inside the crate (`.internal("...")`); there is no public `From<sqlx::Error>`.
pub struct Internal {
    context: &'static str,
    source: Box<dyn std::error::Error + Send + Sync>,
}

impl fmt::Debug for Internal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {:?}", self.context, self.source)
    }
}

impl fmt::Display for Internal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.source)
    }
}

impl std::error::Error for Internal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

pub(crate) fn internal(context: &'static str, source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Error {
    Error::Internal(Internal { context, source: source.into() })
}

/// `.internal("inserting image")?` everywhere inside the crate.
pub(crate) trait ResultExt<T> {
    fn internal(self, context: &'static str) -> Result<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> ResultExt<T> for Result<T, E> {
    fn internal(self, context: &'static str) -> Result<T> {
        self.map_err(|e| internal(context, e))
    }
}
