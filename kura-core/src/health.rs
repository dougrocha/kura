//! `Library::check` and `Library::sweep`: find (and clean up) what a crash or
//! a hand-edited folder can leave behind.
//!
//! The invariants in `library.rs` mean a crash can only ever leave *extra*
//! things: an unreferenced `<hash>.<ext>` file, a staging file, thumbnails of a
//! deleted image. Those are safe to delete, and `sweep` deletes them. A row
//! without its file can only come from outside (someone deleted the file), so
//! that's reported, never "fixed" by dropping the row.

use std::{collections::HashSet, path::PathBuf, time::Duration};

use crate::{model::ImageHash, store::files::PictureScan};

/// Staging files younger than this may belong to an add in progress.
pub(crate) const STALE_STAGING: Duration = Duration::from_secs(60 * 60);
/// How many change-log rows `sweep` keeps. Feeds further behind get `Resync`.
pub(crate) const KEEP_CHANGES: i64 = 10_000;

/// What `check` found. After `sweep`, everything except `missing_files` and
/// `foreign_files` has been removed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Health {
    /// Images whose file is gone. Restore the file, or delete the image.
    pub missing_files: Vec<ImageHash>,
    /// Image files no image uses, left by an interrupted add or delete.
    pub orphan_files: Vec<PathBuf>,
    /// Files in the picture folder that kura didn't name. Never touched.
    pub foreign_files: Vec<PathBuf>,
    /// Staging files of adds that never finished.
    pub stale_staging: Vec<PathBuf>,
    /// Thumbnail-cache files of images that no longer exist, or that the cache
    /// didn't make.
    pub orphan_thumbnails: Vec<PathBuf>,
    /// Change-log rows older than the newest 10,000.
    pub old_changes: u64,
}

impl Health {
    /// Nothing for `sweep` to remove and no missing files. Foreign files don't count.
    pub fn is_clean(&self) -> bool {
        self.missing_files.is_empty()
            && self.orphan_files.is_empty()
            && self.stale_staging.is_empty()
            && self.orphan_thumbnails.is_empty()
            && self.old_changes == 0
    }
}

/// The comparison of rows and files, before anything is removed.
pub(crate) struct Findings {
    pub(crate) health: Health,
    /// `orphan_files` as (hash, extension), for removing them by name.
    pub(crate) orphans: Vec<(ImageHash, String)>,
}

pub(crate) fn compare(
    rows: Vec<(ImageHash, String)>,
    scan: PictureScan,
    thumbnails: Vec<(Option<ImageHash>, PathBuf)>,
    stale_staging: Vec<PathBuf>,
    old_changes: u64,
    path_of: impl Fn(&ImageHash, &str) -> PathBuf,
) -> Findings {
    let on_disk: HashSet<&(ImageHash, String)> = scan.stored.iter().collect();
    let in_db: HashSet<&(ImageHash, String)> = rows.iter().collect();
    let live: HashSet<&ImageHash> = rows.iter().map(|(hash, _)| hash).collect();

    let mut missing_files: Vec<ImageHash> =
        rows.iter().filter(|row| !on_disk.contains(row)).map(|(hash, _)| hash.clone()).collect();
    missing_files.sort_by(|a, b| a.as_str().cmp(b.as_str()));

    let mut orphans: Vec<(ImageHash, String)> =
        scan.stored.iter().filter(|file| !in_db.contains(file)).cloned().collect();
    orphans.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()).then_with(|| a.1.cmp(&b.1)));

    let mut orphan_thumbnails: Vec<PathBuf> = thumbnails
        .into_iter()
        .filter(|(hash, _)| hash.as_ref().is_none_or(|hash| !live.contains(hash)))
        .map(|(_, path)| path)
        .collect();
    orphan_thumbnails.sort();

    let mut foreign_files = scan.foreign;
    foreign_files.sort();
    let mut stale_staging = stale_staging;
    stale_staging.sort();

    Findings {
        health: Health {
            missing_files,
            orphan_files: orphans.iter().map(|(hash, ext)| path_of(hash, ext)).collect(),
            foreign_files,
            stale_staging,
            orphan_thumbnails,
            old_changes,
        },
        orphans,
    }
}
