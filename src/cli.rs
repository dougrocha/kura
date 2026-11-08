use std::path::PathBuf;

use clap::{Parser, Subcommand};
use kura_core::{ImageName, ImageRef, Tag};
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
    /// Browse the library in the terminal
    Tui,
    /// Delete the whole library: images, database and thumbnails
    Nuke,
    /// Serve the library over HTTP
    Serve(ServeArgs),
}
