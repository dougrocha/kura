use std::{io::Write, path::Path, process::{Command, Stdio}};

use crate::mime_type::MimeTypeError;

pub trait Clipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError>;
    fn write_image(&self, path: &Path) -> Result<(), ClipboardError>;
}

pub fn clipboard() -> Result<Box<dyn Clipboard>, ClipboardError> {
    #[cfg(target_os = "linux")]
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        return Ok(Box::new(WaylandClipboard));
    }

    Err(ClipboardError::UnsupportedPlatform)
}

struct WaylandClipboard;

impl Clipboard for WaylandClipboard {
    fn write(&self, text: &str) -> Result<(), ClipboardError> {
        let mut child = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }

        Ok(())
    }

    fn write_image(&self, path: &Path) -> Result<(), ClipboardError> {
        let uri = format!("file://{}\n", path.display());

        let mut child = Command::new("wl-copy")
            .arg("--type")
            .arg("text/uri-list")
            .stdin(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(uri.as_bytes())?;
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("file has no extension")]
    MissingExtension,
    #[error(transparent)]
    MimeType(#[from] MimeTypeError),
    #[error("no supported clipboard backend for this platform")]
    UnsupportedPlatform,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
