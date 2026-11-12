//! HTTP API for a Kura library. Each handler parses its inputs, makes one
//! `Library` call and returns `wire` JSON.
//!
//! | Method | Path                        | Does                                              |
//! |--------|-----------------------------|---------------------------------------------------|
//! | GET    | /health                     | liveness check                                    |
//! | GET    | /images?tag=                | list images, newest first                         |
//! | POST   | /images?name=&tags=a,b      | upload the raw image as the body (streamed)       |
//! | POST   | /imports                    | add from a public URL: `{"url","name"?,"tags"?}`  |
//! | GET    | /images/{id}                | one image; `id` is a hash or a name               |
//! | PATCH  | /images/{id}                | rename: `{"name"}`                                |
//! | DELETE | /images/{id}                | delete                                            |
//! | GET    | /images/{id}/file           | the file; a name redirects to the hash URL        |
//! | GET    | /images/{id}/thumbnail      | `?size=grid` (256 px) or `preview` (1000 px)      |
//! | POST   | /images/{id}/tags           | add a tag: `{"tag"}`                              |
//! | DELETE | /images/{id}/tags/{tag}     | remove a tag (succeeds if it wasn't there)        |
//! | GET    | /search?q=                  | ranked search over names, tags and captions       |
//! | PUT    | /images/{id}/captions/{src} | set a caption: `{"text","engine"?}`, src `ocr`\|`manual` |
//! | DELETE | /images/{id}/captions/{src} | clear one source's caption (succeeds if none)     |
//! | GET    | /tags                       | every tag with its image count                    |
//! | GET    | /events?since=              | Server-Sent Events: every edit as it happens      |
//! | GET    | /config                     | the shared config file (only with `--config-api`) |
//! | PATCH  | /config                     | change one setting: `{"key","value"}`, null unsets |
//!
//! Adding bytes that are already stored answers 200 with the existing image
//! instead of 201, so uploads are safe to retry. Every error, including a
//! malformed request or an unknown path, is a JSON `wire::ErrorBody`.
//! Hash file URLs send `ETag` and `Cache-Control: immutable`, and honor
//! `If-None-Match` (304) and single `Range` requests (206).

mod config;
mod error;
mod extract;
mod file;
mod routes;
pub mod wire;

use std::net::SocketAddr;

use axum::Router;
use kura_core::{Library, LibraryPaths};

/// Options shared by `kura serve` and the `kura-server` binary.
#[derive(Debug, Clone, clap::Args)]
pub struct ServeArgs {
    /// Address to listen on. Keep it on localhost and put `tailscale serve`
    /// (or another proxy) in front to share it.
    #[arg(short, long, default_value = "127.0.0.1:7878")]
    pub bind: SocketAddr,

    /// Exit when stdin closes. For an app that runs the server as a child
    /// process: the app holds the other end, so when it quits or crashes, its
    /// server goes too instead of lingering in the background.
    #[arg(long, hide = true)]
    pub exit_with_stdin: bool,

    /// Serve `GET`/`PATCH /config` for the Mac app's Settings. Off for a plain
    /// `kura serve`, which may be shared over the network.
    #[arg(long, hide = true)]
    pub config_api: bool,
}

/// Open the library at `paths` and serve it until the process exits.
pub async fn run(paths: LibraryPaths, args: ServeArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pictures = paths.pictures.clone();
    let library = Library::open(paths).await?;
    let health = library.sweep().await?;
    let removed = health.orphan_files.len() + health.stale_staging.len() + health.orphan_thumbnails.len();
    if removed > 0 {
        println!("kura-server: removed {removed} leftover file(s) from earlier crashes");
    }
    if !health.missing_files.is_empty() {
        println!("kura-server: {} image(s) are missing their file; run `kura doctor`", health.missing_files.len());
    }
    if args.exit_with_stdin {
        tokio::spawn(async {
            let _ = tokio::io::copy(&mut tokio::io::stdin(), &mut tokio::io::sink()).await;
            // SQLite (WAL) and the add pipeline are crash-safe, so a hard exit is fine.
            std::process::exit(0);
        });
    }
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    // Apps that start the server read this line to learn the port (`--bind 127.0.0.1:0`).
    println!("kura-server listening on http://{}", listener.local_addr()?);
    let mut app = router(library);
    if args.config_api {
        app = app.merge(config::router(config::ConfigState { file: kura_core::ConfigFile::locate()?, pictures }));
    }
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn router(library: Library) -> Router {
    routes::router().with_state(library)
}
