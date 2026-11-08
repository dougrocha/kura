use std::convert::Infallible;

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{
        IntoResponse, Redirect, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, post},
};
use futures::{Stream, StreamExt};
use kura_core::{
    Added, ChangeSeq, FetchPolicy, HttpUrl, ImageFilter, ImageName, ImageRef, Library, NewName, Source, Tag, ThumbSize,
    WatchEvent,
};
use tokio_util::io::ReaderStream;

use crate::{
    error::{ApiError, ApiResult},
    extract::{self, Json as JsonBody, Path, Query},
    file, wire,
};

pub(crate) fn router() -> Router<Library> {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/images", get(list_images).post(upload_image))
        .route("/imports", post(import_image))
        .route("/images/{id}", get(get_image).patch(rename_image).delete(delete_image))
        .route("/images/{id}/file", get(image_file))
        .route("/images/{id}/thumbnail", get(image_thumbnail))
        .route("/images/{id}/tags", post(add_tag))
        .route("/images/{id}/tags/{tag}", delete(remove_tag))
        .route("/tags", get(list_tags))
        .route("/events", get(events))
        .fallback(extract::no_route)
        .method_not_allowed_fallback(extract::wrong_method)
}

fn image_json(image: &kura_core::Image) -> Json<wire::Image> {
    Json(image.into())
}

/// 201 for a new image, 200 when the bytes were already stored.
fn added_response(added: Added) -> (StatusCode, Json<wire::Image>) {
    let status = if added.is_created() { StatusCode::CREATED } else { StatusCode::OK };
    (status, image_json(added.image()))
}

fn optional_name(name: Option<&str>) -> Result<NewName, ApiError> {
    Ok(name.map(ImageName::parse).transpose()?.into())
}

async fn list_images(
    State(lib): State<Library>,
    Query(q): Query<wire::ListQuery>,
) -> ApiResult<Json<Vec<wire::Image>>> {
    let mut filter = ImageFilter::all();
    if let Some(tag) = q.tag.as_deref() {
        filter = filter.tag(Tag::parse(tag)?);
    }
    let images = lib.list(&filter).await?;
    Ok(Json(images.iter().map(Into::into).collect()))
}

async fn upload_image(
    State(lib): State<Library>,
    Query(q): Query<wire::UploadQuery>,
    body: Body,
) -> ApiResult<(StatusCode, Json<wire::Image>)> {
    let name = optional_name(q.name.as_deref())?;
    let tags = Tag::parse_list(&q.tags)?;
    // The body streams into kura-core, which enforces the size limit as it reads.
    let source = Source::stream(body.into_data_stream(), q.file_name);
    Ok(added_response(lib.add(source, name, &tags).await?))
}

async fn import_image(
    State(lib): State<Library>,
    JsonBody(body): JsonBody<wire::Import>,
) -> ApiResult<(StatusCode, Json<wire::Image>)> {
    let url = HttpUrl::parse(&body.url)?;
    let name = optional_name(body.name.as_deref())?;
    let tags = body.tags.iter().map(|t| Tag::parse(t)).collect::<Result<Vec<_>, _>>()?;
    // Callers are remote: only public addresses, never the server's own network.
    let source = Source::fetch(&url, &FetchPolicy::public_only()).await?;
    Ok(added_response(lib.add(source, name, &tags).await?))
}

async fn get_image(State(lib): State<Library>, Path(id): Path<String>) -> ApiResult<Json<wire::Image>> {
    let id: ImageRef = id.parse()?;
    Ok(image_json(&lib.image(id).await?))
}

async fn rename_image(
    State(lib): State<Library>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<wire::Rename>,
) -> ApiResult<Json<wire::Image>> {
    let id: ImageRef = id.parse()?;
    let edited = lib.rename(id, ImageName::parse(&body.name)?).await?;
    Ok(image_json(&edited.image))
}

async fn delete_image(State(lib): State<Library>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let id: ImageRef = id.parse()?;
    lib.delete(id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn image_file(State(lib): State<Library>, Path(id): Path<String>, headers: HeaderMap) -> ApiResult<Response> {
    let id: ImageRef = id.parse()?;
    if let ImageRef::Name(_) = id {
        // Names change; send clients to the hash URL, whose bytes never do.
        let image = lib.image(id).await?;
        return Ok(Redirect::temporary(&wire::file_url(image.hash())).into_response());
    }

    file::respond(lib.open_file(id).await?, &headers).await
}

async fn image_thumbnail(
    State(lib): State<Library>,
    Path(id): Path<String>,
    Query(q): Query<wire::ThumbnailQuery>,
) -> ApiResult<Response> {
    let id: ImageRef = id.parse()?;
    let size = match q.size.as_deref() {
        Some(size) => size.parse::<ThumbSize>()?,
        None => ThumbSize::Preview,
    };
    let thumb = lib.thumbnail(id, size).await?;
    let file = thumb.open().await?;
    Ok(([(header::CONTENT_TYPE, thumb.media_type().as_str())], Body::from_stream(ReaderStream::new(file)))
        .into_response())
}

async fn add_tag(
    State(lib): State<Library>,
    Path(id): Path<String>,
    JsonBody(body): JsonBody<wire::AddTag>,
) -> ApiResult<Json<wire::Image>> {
    let id: ImageRef = id.parse()?;
    let edited = lib.tag(id, &[Tag::parse(&body.tag)?]).await?;
    Ok(image_json(&edited.image))
}

async fn remove_tag(
    State(lib): State<Library>,
    Path((id, tag)): Path<(String, String)>,
) -> ApiResult<Json<wire::Image>> {
    let id: ImageRef = id.parse()?;
    let edited = lib.untag(id, &Tag::parse(&tag)?).await?;
    Ok(image_json(&edited.image))
}

async fn list_tags(State(lib): State<Library>) -> ApiResult<Json<Vec<wire::TagCount>>> {
    let tags = lib.tags().await?;
    Ok(Json(tags.into_iter().map(|t| wire::TagCount { tag: t.tag.to_string(), count: t.count }).collect()))
}

/// Server-Sent Events: every edit, live. Reconnecting clients send
/// `Last-Event-ID` and pick up where they left off.
async fn events(
    State(lib): State<Library>,
    Query(q): Query<wire::EventsQuery>,
    headers: HeaderMap,
) -> ApiResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    let last_event_id = headers.get("last-event-id").and_then(|v| v.to_str().ok());
    let since = match last_event_id {
        Some(id) => Some(id.trim().parse::<u64>().map_err(|_| {
            ApiError::request(StatusCode::BAD_REQUEST, "invalid_request", "Last-Event-ID must be a change seq")
        })?),
        None => q.since,
    };
    let feed = lib.watch(since.map(ChangeSeq)).await?;
    // A failed read ends the stream; the client reconnects with Last-Event-ID.
    let events = feed
        .take_while(|event| {
            if let Err(e) = event {
                eprintln!("kura-server: change feed: {e:?}");
            }
            futures::future::ready(event.is_ok())
        })
        .filter_map(|event| futures::future::ready(event.ok().map(|event| Ok(sse_event(&event)))));
    Ok(Sse::new(events).keep_alive(KeepAlive::default()))
}

fn sse_event(event: &WatchEvent) -> Event {
    match event {
        WatchEvent::Change(change) => {
            let data = serde_json::to_string(&wire::Change::from(change)).expect("plain data serializes");
            Event::default().id(change.seq.0.to_string()).event(change.kind.as_str()).data(data)
        }
        WatchEvent::Resync => Event::default().event("resync").data(""),
    }
}
