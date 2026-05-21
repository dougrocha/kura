use std::{
    fs,
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
    path::PathBuf,
};

use clap::{CommandFactory, Parser};
use hako::mime_type::MimeType;
use kura::{
    State,
    cli::{Cli, Commands},
    images::{Image, NewImage},
    tags::{NewTag, Tag},
    tui::KuraApp,
    types::ImageHash,
};
use miette::{IntoDiagnostic, Result, miette};
use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() -> Result<()> {
    let state = State::new()?;

    Image::create_table(&state).await?;
    Tag::create_table(&state).await?;

    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Tui) => {
            hako::run(KuraApp::new(state).await?).await.into_diagnostic()?;
        }
        Some(Commands::Nuke) => state.prune().await?,
        Some(Commands::Add {
            name,
            file_path,
            url,
        }) => {
            let (data, extension) = match (file_path, url) {
                (Some(_), Some(_)) => return Err(miette!("Cannot provide both --file and --url")),
                (None, None) => return Err(miette!("Must provide either --file or --url")),
                (None, Some(url)) => fetch_remote_image(url).await?,
                (Some(file_path), None) => get_local_image(file_path)?,
            };

            let hash = ImageHash(hex::encode(Sha256::digest(&data)));

            if let Some(existing) = Image::find_by_hash_or_name(&state, hash.as_str()).await? {
                return Err(miette!(
                    "Duplicate image: already stored as '{}'",
                    existing.image.name
                ));
            }

            let mut dest = state
                .picture_dir
                .join(name.replace(' ', "-").to_lowercase());
            dest.set_extension(&extension);
            if fs::exists(&dest).into_diagnostic()? {
                return Err(miette!("An image with name {} already exists", name));
            }

            fs::write(&dest, &data).into_diagnostic()?;
            let new_image = NewImage::new(hash.clone(), name, dest.to_str().unwrap());
            new_image.insert(&state).await?;
            println!("Added: {name} ({hash})");
        }
        Some(Commands::Remove {
            hash_or_name,
            file_path,
        }) => {
            let image = if let Some(hash_or_name) = hash_or_name {
                Image::find_by_hash_or_name(&state, hash_or_name).await?
            } else if let Some(file_path) = file_path {
                let file_path = if file_path.is_absolute() {
                    file_path.clone()
                } else {
                    std::env::current_dir().into_diagnostic()?.join(file_path)
                };
                Image::find_by_file_path(&state, &file_path).await?
            } else {
                return Err(miette!("Must provide hash, name, or file_path to remove"));
            };

            let image = image.ok_or_else(|| miette!("Image not found"))?;
            image.image.delete(&state).await?;
            println!("Removed: {} ({})", image.image.name, image.image.hash);
        }
        Some(Commands::List { tag }) => {
            let images = match tag {
                Some(t) => Image::all_with_tag(&state, t).await?,
                None => Image::all(&state).await?,
            };
            if images.is_empty() {
                println!("No images found.");
            } else {
                for img in &images {
                    let tags = if img.tags.is_empty() {
                        String::new()
                    } else {
                        format!("  [{}]", img.tags.iter().map(|t| t.tag.as_str()).collect::<Vec<_>>().join(", "))
                    };
                    println!("{}{}", img.image.name, tags);
                }
            }
        }
        Some(Commands::Tag { hash_or_name, tag }) => {
            let image = Image::find_by_hash_or_name(&state, hash_or_name)
                .await?
                .ok_or_else(|| miette!("Image not found"))?;

            NewTag::new(&image.image.hash, tag).insert(&state).await?;
            println!("Tagged {} with {}", image.image.name, tag);
        }
        Some(Commands::Untag { hash_or_name, tag }) => {
            let image = Image::find_by_hash_or_name(&state, hash_or_name)
                .await?
                .ok_or_else(|| miette!("Image not found"))?;

            let hash = image.image.hash.as_str();
            let tag = tag.as_str();
            sqlx::query!(
                "DELETE FROM tags WHERE image_hash = ? AND tag = ?",
                hash,
                tag
            )
            .execute(&state.db_pool)
            .await
            .into_diagnostic()?;

            println!("Removed tag '{}' from {}", tag, image.image.name);
        }
        Some(Commands::Rename { old_name, new_name }) => {
            let mut image = Image::find_by_hash_or_name(&state, old_name)
                .await?
                .ok_or_else(|| miette!("Image not found"))?;
            image.image.rename(&state, new_name).await?;
            println!("Renamed {} → {}", old_name, new_name);
        }
        Some(Commands::Serve { port }) => {
            let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();
            for stream in listener.incoming() {
                handle_connection(stream.unwrap());
            }
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().into_diagnostic()?;
            println!();
        }
    }

    Ok(())
}

fn get_local_image(file_path: &PathBuf) -> Result<(Vec<u8>, String)> {
    let file_path = if file_path.is_absolute() {
        file_path.clone()
    } else {
        std::env::current_dir().into_diagnostic()?.join(file_path)
    };

    if fs::exists(&file_path).is_ok_and(|x| !x) {
        return Err(miette!("File does not exist"));
    }

    let extension = file_path
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| MimeType::from_extension(ext).is_ok())
        .ok_or_else(|| miette!("File needs to be a valid image"))?
        .to_string();
    let data = fs::read(&file_path).into_diagnostic()?;
    Ok((data, extension))
}

async fn fetch_remote_image(url: &str) -> Result<(Vec<u8>, String)> {
    let resp = reqwest::get(url).await.into_diagnostic()?;
    if !resp.status().is_success() {
        return Err(miette!("Failed to fetch image: HTTP {}", resp.status()));
    }
    let data = resp.bytes().await.into_diagnostic()?.to_vec();

    let extension = MimeType::from_bytes(&data)
        .into_diagnostic()?
        .extension()
        .to_string();

    Ok((data, extension))
}

// TODO: Allow people to use server with this program
fn handle_connection(mut stream: TcpStream) {
    let buf_reader = BufReader::new(&stream);
    let request_line = buf_reader.lines().next().unwrap().unwrap();

    if request_line == "GET / HTTP/1.1" {
        let status_line = "HTTP/1.1 200 OK";
        let contents = "{\"image\": \"file_path.jpg\"}";
        let length = contents.len();
        let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
        stream.write_all(response.as_bytes()).unwrap();
    } else {
        let status_line = "HTTP/1.1 404 NOT FOUND";
        let contents = "Not here brev";
        let length = contents.len();
        let response = format!("{status_line}\r\nContent-Length: {length}\r\n\r\n{contents}");
        stream.write_all(response.as_bytes()).unwrap();
    }
}
