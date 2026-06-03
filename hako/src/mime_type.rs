pub enum MimeType {
    Png,
    Jpeg,
    Gif,
    WebP,
}

impl MimeType {
    pub fn from_extension(ext: &str) -> Result<Self, MimeTypeError> {
        match ext.to_ascii_lowercase().as_str() {
            "png" => Ok(Self::Png),
            "jpg" | "jpeg" => Ok(Self::Jpeg),
            "gif" => Ok(Self::Gif),
            "webp" => Ok(Self::WebP),
            other => Err(MimeTypeError::UnsupportedExtension(other.to_owned())),
        }
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, MimeTypeError> {
        match data {
            [0x89, 0x50, 0x4E, 0x47, ..] => Ok(Self::Png),
            [0xFF, 0xD8, 0xFF, ..] => Ok(Self::Jpeg),
            [b'G', b'I', b'F', b'8', ..] => Ok(Self::Gif),
            [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Ok(Self::WebP),
            _ => Err(MimeTypeError::UnknownFormat),
        }
    }

    pub fn from_content_type(ct: &str) -> Result<Self, MimeTypeError> {
        let ct = ct.split(';').next().unwrap_or(ct).trim();
        match ct {
            "image/png" => Ok(Self::Png),
            "image/jpeg" | "image/jpg" => Ok(Self::Jpeg),
            "image/gif" => Ok(Self::Gif),
            "image/webp" => Ok(Self::WebP),
            other => Err(MimeTypeError::UnsupportedExtension(other.to_owned())),
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::WebP => "webp",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Gif => "image/gif",
            Self::WebP => "image/webp",
        }
    }

    pub fn is_animated(&self) -> bool {
        matches!(self, Self::Gif | Self::WebP)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MimeTypeError {
    #[error("unsupported image extension: {0}")]
    UnsupportedExtension(String),
    #[error("could not determine image format from file contents")]
    UnknownFormat,
}
