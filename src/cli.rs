use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    Add {
        name: String,
        #[arg(short, long)]
        file_path: Option<PathBuf>,
        #[arg(short, long)]
        url: Option<String>,
    },
    Remove {
        hash_or_name: Option<String>,
        file_path: Option<PathBuf>,
    },
    List {
        #[arg(short, long)]
        tag: Option<String>,
    },
    Tag {
        hash_or_name: String,
        tag: String,
    },
    Untag {
        hash_or_name: String,
        tag: String,
    },
    Rename {
        old_name: String,
        new_name: String,
    },
    Tui,
    Nuke,
    Serve {
        #[arg(short, long, default_value_t = 7878)]
        port: u16,
    },
}
