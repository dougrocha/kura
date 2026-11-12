use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand};
use kura_core::{ImageName, ImageRef, Tag, Timestamp, parse_created_at};
use kura_server::ServeArgs;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    /// Use the library in this folder instead of your default one
    #[arg(long, global = true)]
    pub library: Option<PathBuf>,

    /// Delete cached thumbnails before running the command
    #[arg(long, global = true)]
    pub clear_cache: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Add an image from a file path or URL
    Add {
        /// A local file path or an http(s) URL
        source: String,
        /// Name for the image (defaults to the file name)
        #[arg(short, long)]
        name: Option<ImageName>,
        /// Tag to add; repeat for several tags
        #[arg(short, long = "tag")]
        tags: Vec<Tag>,
        /// When the image was added, to keep an import's original date:
        /// RFC 3339 (2026-05-14T00:29:43Z) or "2026-05-14 00:29:43" in UTC
        #[arg(long, value_parser = parse_created_at)]
        created_at: Option<Timestamp>,
    },
    /// Remove an image by hash or name, or by a copy of its file
    Remove {
        #[arg(required_unless_present = "file", conflicts_with = "file")]
        image: Option<ImageRef>,
        /// Any copy of the image; it's matched by content
        #[arg(short, long)]
        file: Option<PathBuf>,
    },
    /// List images, newest first
    List {
        #[arg(short, long)]
        tag: Option<Tag>,
    },
    /// List every tag with its image count
    Tags,
    /// Add a tag to an image
    Tag { image: ImageRef, tag: Tag },
    /// Remove a tag from an image
    Untag { image: ImageRef, tag: Tag },
    /// Rename an image
    Rename { image: ImageRef, new_name: ImageName },
    /// Show or set the text written inside an image (searched along with names and tags)
    #[command(group(ArgGroup::new("edit").multiple(false)))]
    Caption {
        image: ImageRef,
        /// Set this as the caption. It wins over the text the Mac app read
        #[arg(group = "edit")]
        text: Option<String>,
        /// Say there is no text in this image, so it isn't read again
        #[arg(long, group = "edit")]
        none: bool,
        /// Drop the caption you set and use the text the Mac app read
        #[arg(long, group = "edit")]
        clear: bool,
        /// Drop the text the Mac app read, so it reads the image again
        #[arg(long, group = "edit")]
        reread: bool,
    },
    /// Browse the library in the terminal
    Tui,
    /// Fuzzy pick an image, copy it, and exit
    Pick,
    /// Check that the database and the files on disk agree
    Doctor {
        /// Also remove what crashes left behind (orphan files, stale uploads, old thumbnails)
        #[arg(long)]
        fix: bool,
    },
    /// Delete the whole library: images, database and thumbnails
    Nuke,
    /// Serve the library over HTTP
    Serve(ServeArgs),
    /// Show or change settings (shared with the Mac app)
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print one setting's value
    Get { key: String },
    /// Change a setting: `kura config set pick.close_after_copy false`
    Set { key: String, value: String },
    /// Put a setting back to its default
    Unset { key: String },
    /// Open the config file in $VISUAL or $EDITOR
    Edit,
    /// Print the config file's path
    Path,
}
