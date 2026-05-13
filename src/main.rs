use std::{
    fs,
    io::{BufReader, prelude::*},
    net::{TcpListener, TcpStream},
    path::PathBuf,
};

use clap::{CommandFactory, Parser, Subcommand};
use kura::{
    State,
    types::ImageHash,
    images::{Image, NewImage},
    tags::{NewTag, Tag},
};
use miette::{IntoDiagnostic, Result, miette};
use rusqlite::named_params;
use sha2::{Digest, Sha256};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
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
    Nuke,
    Serve {
        #[arg(short, long, default_value_t = 7878)]
        port: u16,
    },
}

fn main() -> Result<()> {
    let mut state = State::new()?;

    Image::create_table(&state)?;
    Tag::create_table(&state)?;

    let cli = Cli::parse();

    match &cli.command {
        Some(Commands::Nuke) => state.prune()?,
        Some(command) => handle_command(&mut state, command)?,
        None => {
            let mut cmd = Cli::command();
            cmd.print_help().into_diagnostic()?;
            println!();
        }
    }

    Ok(())
}

fn handle_command(state: &mut State, command: &Commands) -> Result<()> {
    match command {
        Commands::Nuke => unreachable!("Command is not handled"),
        Commands::Add {
            name,
            file_path,
            url,
        } => {
            let (data, extension) = match (file_path, url) {
                (Some(_), Some(_)) => return Err(miette!("Cannot provide both --file and --url")),
                (None, None) => return Err(miette!("Must provide either --file or --url")),
                (None, Some(url)) => fetch_remote_image(url)?,
                (Some(file_path), None) => get_local_image(file_path)?,
            };

            let mut dest = state
                .picture_dir
                .join(name.replace(' ', "-").to_lowercase());
            dest.set_extension(&extension);
            if fs::exists(&dest).into_diagnostic()? {
                return Err(miette!("An image with name {} already exists", name));
            }

            fs::write(&dest, &data).into_diagnostic()?;

            let hash = ImageHash(hex::encode(Sha256::digest(data)));
            let new_image = NewImage::new(hash.clone(), name, dest.to_str().unwrap());
            new_image.insert(state)?;
            println!("Added: {name} ({hash})");
        }
        Commands::Remove {
            hash_or_name,
            file_path,
        } => {
            let image = if let Some(hash_or_name) = hash_or_name {
                Image::find_by_hash_or_name(state, hash_or_name)?
            } else if let Some(file_path) = file_path {
                let file_path = if file_path.is_absolute() {
                    file_path.clone()
                } else {
                    std::env::current_dir().into_diagnostic()?.join(file_path)
                };
                Image::find_by_file_path(state, &file_path)?
            } else {
                return Err(miette!("Must provide hash, name, or file_path to remove"));
            };

            let image = image.ok_or_else(|| miette!("Image not found"))?;
            image.delete(state)?;
            println!("Removed: {} ({})", image.name, image.hash);
        }
        Commands::List { tag } => {
            let images = Image::all(state)?;

            println!("Searching for {tag:#?}, result:");
            println!("{images:#?}");
        }
        Commands::Serve { port } => {
            let listener = TcpListener::bind(format!("127.0.0.1:{port}")).unwrap();

            for stream in listener.incoming() {
                let stream = stream.unwrap();

                handle_connection(stream);
            }
        }
        Commands::Tag { hash_or_name, tag } => {
            let image = Image::find_by_hash_or_name(state, hash_or_name)?
                .ok_or_else(|| miette!("Image not found"))?;

            let new_tag = NewTag::new(&image.hash, tag);
            new_tag.insert(state)?;

            println!("Tagged {} with {}", image.name, tag);
        }
        Commands::Untag { hash_or_name, tag } => {
            let image = Image::find_by_hash_or_name(state, hash_or_name)?
                .ok_or_else(|| miette!("Image not found"))?;

            state
                .db
                .execute(
                    "DELETE FROM tags WHERE image_hash = :hash AND tag = :tag",
                    named_params! { ":hash": image.hash, ":tag": tag },
                )
                .into_diagnostic()?;

            println!("Removed tag '{}' from {}", tag, image.name);
        }
    }

    Ok(())
}

fn get_local_image(file_path: &PathBuf) -> Result<(Vec<u8>, String), miette::Error> {
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
        .filter(|ext| matches!(*ext, "png" | "jpg" | "jpeg" | "gif"))
        .ok_or_else(|| miette!("File needs to be a valid image"))?
        .to_string();
    let data = fs::read(&file_path).into_diagnostic()?;
    Ok((data, extension))
}

fn fetch_remote_image(url: &String) -> Result<(Vec<u8>, String), miette::Error> {
    let resp = reqwest::blocking::get(url).into_diagnostic()?;
    let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).cloned();
    let data = resp.bytes().into_diagnostic()?.to_vec();
    let extension = PathBuf::from(url.split('?').next().unwrap_or(url))
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| matches!(*ext, "png" | "jpg" | "jpeg" | "gif"))
        .map(|s| s.to_string())
        .or_else(|| {
            content_type.and_then(|ct| {
                ct.to_str().ok().and_then(|ct| match ct {
                    "image/png" => Some("png".to_string()),
                    "image/jpeg" | "image/jpg" => Some("jpg".to_string()),
                    "image/gif" => Some("gif".to_string()),
                    _ => None,
                })
            })
        })
        .ok_or_else(|| miette!("Could not determine image extension from URL or Content-Type"))?;
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
