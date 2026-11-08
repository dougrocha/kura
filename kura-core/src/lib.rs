//! kura-core: one library of memes on disk, behind one handle.
//!
//! ```text
//! lib.rs        re-exports only: this list is the whole public API
//! library.rs    Library (every operation), LibraryPaths, Limits, ImageFile
//! changes.rs    the change feed: Change, ChangeSeq, WatchEvent (Library::watch)
//! health.rs     Health: what Library::check finds and Library::sweep cleans
//! model.rs      parsed value types: ImageHash, ImageName, Tag, ImageRef, Image, ...
//! source.rs     Source (bytes to add), HttpUrl, FetchPolicy
//! thumbs.rs     ThumbSize, Thumbnail, the thumbnail cache
//! error.rs      Error, ErrorKind, InvalidInput
//! store/        private: all SQL (mod.rs), files and locks (files.rs), upgrades (upgrade.rs)
//! ```
//!
//! Every module is private, so nothing outside this crate can reach the
//! database pool, build a path that a write trusts, or skip validation.

mod changes;
mod error;
mod health;
mod library;
mod model;
mod source;
mod store;
mod thumbs;

pub use changes::{Change, ChangeKind, ChangeSeq, WatchEvent};
pub use error::{Error, ErrorKind, FetchFailure, InvalidInput, Result};
pub use health::Health;
pub use library::{ImageFile, Library, LibraryPaths, Limits};
pub use model::{Added, Edited, Image, ImageFilter, ImageHash, ImageName, ImageRef, MediaType, NewName, Tag, TagCount};
pub use source::{FetchPolicy, HttpUrl, Source};
pub use thumbs::{ThumbSize, Thumbnail};

/// Re-exported so clients name timestamps without depending on jiff themselves.
pub use jiff::Timestamp;
