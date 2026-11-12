mod cli;
mod image_protocol_cache;
mod tui;

use clap::{CommandFactory, Parser};
use kura_core::{
    Added, CaptionEdit, CaptionSource, CaptionText, ConfigFile, FetchPolicy, HttpUrl, ImageFilter, ImageHash, ImageRef,
    InvalidInput, Library, LibraryPaths, Outcome, Source,
};
use miette::{IntoDiagnostic, Result, miette};

use crate::{
    cli::{Cli, Commands, ConfigAction},
    tui::{KuraApp, Mode},
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

    let config_file = ConfigFile::locate().into_diagnostic()?;
    // Before loading it: `kura config edit` must work on a broken file.
    if let Commands::Config { action } = command {
        return config(&config_file, action);
    }
    let config = config_file.load().into_diagnostic()?;
    let paths = LibraryPaths::resolve(cli.library.as_deref(), &config).into_diagnostic()?;

    match command {
        Commands::Nuke => paths.destroy().into_diagnostic()?,
        Commands::Serve(args) => kura_server::run(paths, args).await.map_err(|e| miette!("{e}"))?,
        command => {
            let lib = Library::open(paths).await.into_diagnostic()?;
            if cli.clear_cache {
                lib.clear_thumbnails().await.into_diagnostic()?;
            }
            match command {
                Commands::Tui | Commands::Pick => {
                    let mode = if let Commands::Pick = command { Mode::Pick } else { Mode::Browse };
                    let app = KuraApp::new(lib, mode, config.close_after_copy).await.into_diagnostic()?;
                    hako::run(app).await.into_diagnostic()?;
                }
                command => run(&lib, command).await.into_diagnostic()?,
            }
        }
    }

    Ok(())
}

async fn run(lib: &Library, command: Commands) -> kura_core::Result<()> {
    match command {
        Commands::Add { source, name, tags, created_at } => {
            // The CLI acts as the local user, so it may read local files and
            // fetch from the local network.
            let source = match HttpUrl::parse(&source) {
                Ok(url) => Source::fetch(&url, &FetchPolicy::trusted_local_user()).await?,
                Err(_) => Source::path(&source).await?,
            };
            match lib.add(source, name, &tags, created_at).await? {
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
        Commands::Caption { image, text, none, clear, reread } => {
            let edit = match (text, none, clear, reread) {
                (Some(text), ..) => match CaptionText::normalize(&text) {
                    Outcome::Blank => {
                        let reason = "can't be empty (use --none to say there is no text)".to_owned();
                        return Err(InvalidInput { field: "caption", reason }.into());
                    }
                    outcome => CaptionEdit::Manual(outcome),
                },
                (None, true, ..) => CaptionEdit::Manual(Outcome::Blank),
                (None, _, true, _) => CaptionEdit::Clear(CaptionSource::Manual),
                (None, _, _, true) => CaptionEdit::Clear(CaptionSource::Ocr),
                (None, false, false, false) => {
                    let image = lib.image(image).await?;
                    match image.caption().map(kura_core::Reading::outcome) {
                        None => println!("(not read yet)"),
                        Some(Outcome::Blank) => println!("(no text)"),
                        Some(Outcome::Text(text)) => println!("{text}"),
                    }
                    return Ok(());
                }
            };
            let edited = lib.set_caption(image, edit).await?;
            let name = edited.image.name();
            println!(
                "{}",
                if edited.changed {
                    format!("Updated the caption of {name}")
                } else {
                    format!("{name} already has that caption")
                }
            );
        }
        Commands::Doctor { fix } => doctor(lib, fix).await?,
        Commands::Tui | Commands::Pick | Commands::Nuke | Commands::Serve(_) | Commands::Config { .. } => {
            unreachable!("handled in main")
        }
    }
    Ok(())
}

fn config(file: &ConfigFile, action: Option<ConfigAction>) -> Result<()> {
    match action {
        None => {
            println!("{}\n", file.path().display());
            for key in kura_core::KEYS {
                let value = file.get(key.name).into_diagnostic()?;
                let shown = value.unwrap_or_else(|| "(default)".to_owned());
                println!("{:<26} {:<12} {}", key.name, shown, key.about);
            }
            // Catch a broken file here too, not only when a command fails.
            file.load().into_diagnostic()?;
        }
        Some(ConfigAction::Get { key }) => match file.get(&key).into_diagnostic()? {
            Some(value) => println!("{value}"),
            None => println!("(default)"),
        },
        Some(ConfigAction::Set { key, value }) => file.set(&key, &value).into_diagnostic()?,
        Some(ConfigAction::Unset { key }) => file.unset(&key).into_diagnostic()?,
        Some(ConfigAction::Path) => println!("{}", file.path().display()),
        Some(ConfigAction::Edit) => {
            file.create_template().into_diagnostic()?;
            let path = file.path();
            let editor = std::env::var("VISUAL")
                .or_else(|_| std::env::var("EDITOR"))
                .ok()
                .filter(|e| !e.trim().is_empty())
                .unwrap_or_else(|| "vi".to_owned());
            // Through the shell, so EDITOR="code --wait" works.
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(format!("{editor} \"$0\""))
                .arg(path)
                .status()
                .into_diagnostic()?;
            if !status.success() {
                return Err(miette!("{editor} exited with {status}"));
            }
            file.load().into_diagnostic()?;
        }
    }
    Ok(())
}

async fn doctor(lib: &Library, fix: bool) -> kura_core::Result<()> {
    let health = if fix { lib.sweep().await? } else { lib.check().await? };
    let action = if fix { "removed" } else { "found" };
    let report = |label: &str, paths: Vec<String>| {
        if !paths.is_empty() {
            println!("{label} ({}):", paths.len());
            for path in paths {
                println!("  {path}");
            }
        }
    };
    let show = |paths: &[std::path::PathBuf]| paths.iter().map(|p| p.display().to_string()).collect();

    let missing = health.missing_files.iter().map(ToString::to_string).collect();
    report("Images whose file is missing (restore it, or `kura remove <hash>`)", missing);
    report(&format!("Orphan image files, {action}"), show(&health.orphan_files));
    report(&format!("Unfinished uploads, {action}"), show(&health.stale_staging));
    report(&format!("Stale thumbnails, {action}"), show(&health.orphan_thumbnails));
    report("Files kura didn't create (left alone)", show(&health.foreign_files));
    if health.old_changes > 0 {
        println!("Old change-log entries, {action}: {}", health.old_changes);
    }

    let sweepable = !health.orphan_files.is_empty()
        || !health.stale_staging.is_empty()
        || !health.orphan_thumbnails.is_empty()
        || health.old_changes > 0;
    if health.is_clean() {
        println!("Library is healthy.");
    } else if !fix && sweepable {
        println!("Run `kura doctor --fix` to clean up.");
    }
    Ok(())
}
