use std::{
    fs,
    path::{Path, PathBuf},
};

use image::{DynamicImage, ImageFormat};
use miette::{IntoDiagnostic, Result};

const MAX_WIDTH: u32 = 1000;
const MAX_HEIGHT: u32 = 1000;

#[derive(Clone)]
pub struct ImageCache {
    cache_dir: PathBuf,
}

impl ImageCache {
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(&cache_dir).into_diagnostic()?;
        Ok(Self { cache_dir })
    }

    pub fn clear(&self) -> Result<()> {
        fs::remove_dir_all(&self.cache_dir).into_diagnostic()?;
        fs::create_dir_all(&self.cache_dir).into_diagnostic()?;
        Ok(())
    }

    pub fn load_or_cache(&self, hash: &str, original: &Path) -> Result<DynamicImage> {
        let cache_path = self.cache_dir.join(format!("{hash}.jpg"));

        if cache_path.exists()
            && let Ok(img) = image::open(&cache_path)
        {
            return Ok(img);
        }

        let img = image::open(original).into_diagnostic()?;
        let downscaled = img.thumbnail(MAX_WIDTH, MAX_HEIGHT);
        let _ = downscaled.save_with_format(&cache_path, ImageFormat::Jpeg);
        Ok(downscaled)
    }
}
