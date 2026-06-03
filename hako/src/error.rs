use crate::clipboard::ClipboardError;
use crate::mime_type::MimeTypeError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Clipboard(#[from] ClipboardError),
    #[error(transparent)]
    MimeType(#[from] MimeTypeError),
}

pub type Result<T> = std::result::Result<T, Error>;
