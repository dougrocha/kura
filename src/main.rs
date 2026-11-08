mod cli;
mod image_protocol_cache;
mod tui;

use clap::{CommandFactory, Parser};
use kura_core::{
    Added, FetchPolicy, HttpUrl, ImageFilter, ImageHash, ImageRef, Library, LibraryPaths, Source,
};
use miette::{IntoDiagnostic, Result, miette};

use crate::{
    cli::{Cli, Commands},
    tui::KuraApp,
};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let Some(command) = cli.command else {
        // Bare `kura` only prints help; it never creates a library.
        Cli::command().print_help().into_diagnostic()?;
        println!();
        return Ok(());
    };

    let paths = LibraryPaths::resolve(cli.library.as_deref()).into_diagnostic()?;

    match command {
        Commands::Nuke => paths.destroy().into_diagnostic()?,
        Commands::Serve(args) => kura_server::run(paths, args).await.map_err(|e| miette!("{e}"))?,
        command => {
            let lib = Library::open(paths).await.into_diagnostic()?;
            if cli.clear_cache {
                lib.clear_thumbnails().await.into_diagnostic()?;
            }
            if let Commands::Tui = command {
                let app = KuraApp::new(lib).await.into_diagnostic()?;
                hako::run(app).await.into_diagnostic()?;
            } else {
                run(&lib, command).await.into_diagnostic()?;
            }
        }
    }

    Ok(())
}

async fn run(lib: &Library, command: Commands) -> kura_core::Result<()> {
    match command {
        Commands::Add { source, name, tags } => {
            // The CLI acts as the local user, so it may read local files and
            // fetch from the local network.
            let source = match HttpUrl::parse(&source) {
                Ok(url) => Source::fetch(&url, &FetchPolicy::trusted_local_user()).await?,
                Err(_) => Source::path(&source).await?,
            };
            match lib.add(source, name, &tags).await? {
                Added::Created(image) => println!("Added: {} ({})", image.name(), image.hash()),
                Added::Existing(image) => {
                    println!("Already stored as '{}' ({})", image.name(), image.hash());
                }
            }
        }
        Commands::Remove { image, file } => {
            let target = match (image, file) {
                (Some(image), _) => image,
                (None, Some(path)) => ImageRef::Hash(ImageHash::of_file(&path).await?),
                (None, None) => unreachable!("clap requires one of them"),
            };
            let removed = lib.delete(target).await?;
            println!("Removed: {} ({})", removed.name(), removed.hash());
        }
        Commands::List { tag } => {
            let filter = tag.map_or_else(ImageFilter::all, |t| ImageFilter::all().tag(t));
            let images = lib.list(&filter).await?;
            if images.is_empty() {
                println!("No images found.");
            }
            for image in images {
                if image.tags().is_empty() {
                    println!("{}", image.name());
                } else {
                    let tags: Vec<&str> = image.tags().iter().map(|t| t.as_str()).collect();
                    println!("{}  [{}]", image.name(), tags.join(", "));
                }
            }
        }
        Commands::Tags => {
            for tag in lib.tags().await? {
                println!("{} ({})", tag.tag, tag.count);
            }
        }
        Commands::Tag { image, tag } => {
            let edited = lib.tag(image, std::slice::from_ref(&tag)).await?;
            if edited.changed {
                println!("Tagged {} with {tag}", edited.image.name());
            } else {
                println!("{} is already tagged {tag}", edited.image.name());
            }
        }
        Commands::Untag { image, tag } => {
            let edited = lib.untag(image, &tag).await?;
            if edited.changed {
                println!("Removed tag '{tag}' from {}", edited.image.name());
            } else {
                println!("{} isn't tagged '{tag}'", edited.image.name());
            }
        }
        Commands::Rename { image, new_name } => {
            let edited = lib.rename(image, new_name).await?;
            println!("Renamed to {}", edited.image.name());
        }
        Commands::Tui | Commands::Nuke | Commands::Serve(_) => unreachable!("handled in main"),
    }
    Ok(())
}
