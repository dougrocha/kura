//! Downscaled previews, generated on first use and cached as
//! `<cache>/<hash>-<px>.<png|jpg>`. Derived data: always safe to delete.

use std::path::{Path, PathBuf};

use image::{ImageFormat, ImageReader};

use crate::{
    error::{InvalidInput, Result, ResultExt},
    model::{ImageHash, MediaType},
};

/// How big a thumbnail to make. Both dimensions fit within the size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbSize {
    /// 256 px, for grids of images.
    Grid,
    /// 1000 px, for a single image on screen.
    Preview,
}

impl ThumbSize {
    fn pixels(self) -> u32 {
        match self {
            Self::Grid => 256,
            Self::Preview => 1000,
        }
    }
}

impl std::str::FromStr for ThumbSize {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        match s {
            "grid" => Ok(Self::Grid),
            "preview" => Ok(Self::Preview),
            other => Err(InvalidInput::new("size", format!("'{other}' isn't grid or preview"))),
        }
    }
}

/// A cached thumbnail file. PNG when the image has transparency, JPEG otherwise.
#[derive(Debug, Clone)]
pub struct Thumbnail {
    path: PathBuf,
    media_type: MediaType,
}

impl Thumbnail {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn media_type(&self) -> MediaType {
        self.media_type
    }

    /// Open the thumbnail for streaming.
    pub async fn open(&self) -> Result<tokio::fs::File> {
        tokio::fs::File::open(&self.path).await.internal("opening thumbnail")
    }
}

pub(crate) struct Thumbs {
    dir: PathBuf,
}

impl Thumbs {
    pub(crate) async fn open(dir: PathBuf) -> Result<Self> {
        tokio::fs::create_dir_all(&dir).await.internal("creating thumbnail cache")?;
        Ok(Self { dir })
    }

    pub(crate) async fn get_or_create(&self, hash: &ImageHash, original: &Path, size: ThumbSize) -> Result<Thumbnail> {
        let px = size.pixels();
        for media_type in [MediaType::Png, MediaType::Jpeg] {
            let path = self.path(hash, px, media_type);
            if tokio::fs::try_exists(&path).await.unwrap_or(false) {
                return Ok(Thumbnail { path, media_type });
            }
        }

        let original = original.to_path_buf();
        let png_path = self.path(hash, px, MediaType::Png);
        let jpg_path = self.path(hash, px, MediaType::Jpeg);
        tokio::task::spawn_blocking(move || -> Result<Thumbnail> {
            let image = ImageReader::open(&original)
                .internal("opening image")?
                .with_guessed_format()
                .internal("reading image")?
                .decode()
                .internal("decoding image")?;
            let thumb = image.thumbnail(px, px);

            // JPEG can't store transparency, so those thumbnails are PNGs.
            let (path, media_type, format, thumb) = if thumb.color().has_alpha() {
                (png_path, MediaType::Png, ImageFormat::Png, thumb)
            } else {
                (jpg_path, MediaType::Jpeg, ImageFormat::Jpeg, thumb.to_rgb8().into())
            };

            // Write then rename, so a racing reader never sees half a file.
            let temp = path.with_extension(format!("tmp-{}", std::process::id()));
            thumb.save_with_format(&temp, format).internal("saving thumbnail")?;
            std::fs::rename(&temp, &path).internal("saving thumbnail")?;
            Ok(Thumbnail { path, media_type })
        })
        .await
        .internal("making thumbnail")?
    }

    /// Remove every thumbnail of one image.
    pub(crate) async fn remove(&self, hash: &ImageHash) {
        let prefix = format!("{hash}-");
        let Ok(mut entries) = tokio::fs::read_dir(&self.dir).await else {
            return;
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = tokio::fs::remove_file(entry.path()).await;
            }
        }
    }

    /// Every cached file, with the hash of the image it belongs to (`None` for
    /// files the cache didn't make).
    pub(crate) async fn files(&self) -> Result<Vec<(Option<ImageHash>, PathBuf)>> {
        let mut files = Vec::new();
        let mut entries = tokio::fs::read_dir(&self.dir).await.internal("reading thumbnail cache")?;
        while let Some(entry) = entries.next_entry().await.internal("reading thumbnail cache")? {
            let file_name = entry.file_name();
            let hash =
                file_name.to_str().and_then(|n| n.split_once('-')).and_then(|(stem, _)| stem.parse::<ImageHash>().ok());
            files.push((hash, entry.path()));
        }
        Ok(files)
    }

    pub(crate) async fn clear(&self) -> Result<()> {
        let mut entries = tokio::fs::read_dir(&self.dir).await.internal("reading thumbnail cache")?;
        while let Some(entry) = entries.next_entry().await.internal("reading thumbnail cache")? {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
        Ok(())
    }

    fn path(&self, hash: &ImageHash, px: u32, media_type: MediaType) -> PathBuf {
        self.dir.join(format!("{hash}-{px}.{}", media_type.extension()))
    }
}
