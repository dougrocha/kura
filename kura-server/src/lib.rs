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
//! | GET    | /tags                       | every tag with its image count                    |
//! | GET    | /events?since=              | Server-Sent Events: every edit as it happens      |
//!
//! Adding bytes that are already stored answers 200 with the existing image
//! instead of 201, so uploads are safe to retry. Every error, including a
//! malformed request or an unknown path, is a JSON `wire::ErrorBody`.
//! Hash file URLs send `ETag` and `Cache-Control: immutable`, and honor
//! `If-None-Match` (304) and single `Range` requests (206).

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
}

/// Open the library at `paths` and serve it until the process exits.
pub async fn run(paths: LibraryPaths, args: ServeArgs) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let library = Library::open(paths).await?;
    let health = library.sweep().await?;
    let removed = health.orphan_files.len() + health.stale_staging.len() + health.orphan_thumbnails.len();
    if removed > 0 {
        println!("kura-server: removed {removed} leftover file(s) from earlier crashes");
    }
    if !health.missing_files.is_empty() {
        println!("kura-server: {} image(s) are missing their file; run `kura doctor`", health.missing_files.len());
    }
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    println!("kura-server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(library)).await?;
    Ok(())
}

pub fn router(library: Library) -> Router {
    routes::router().with_state(library)
}
