use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode, header},
};
use http_body_util::BodyExt;
use kura_core::{Library, LibraryPaths};
use serde_json::{Value, json};
use tower::ServiceExt;

/// A 4x4 PNG whose pixels depend on `seed`.
fn png(seed: u8) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(4, 4, image::Rgb([seed, 0, 0])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

async fn app() -> (Router, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let lib = Library::open(LibraryPaths::under(tmp.path())).await.unwrap();
    (kura_server::router(lib), tmp)
}

struct Reply {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

async fn send(app: &Router, method: Method, uri: &str, body: impl Into<Body>) -> Reply {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body.into())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Reply { status, headers, body }
}

#[tokio::test]
async fn upload_list_tag_rename_delete() {
    let (app, _tmp) = app().await;

    let reply = send(&app, Method::POST, "/images?name=Cat&tags=funny,cats", png(1)).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    let image = reply.json();
    assert_eq!(image["name"], "Cat");
    assert_eq!(image["tags"], json!(["cats", "funny"]));
    assert_eq!(image["mime_type"], "image/png");
    assert!(image["created_at"].as_str().unwrap().ends_with('Z'));
    let hash = image["hash"].as_str().unwrap().to_owned();

    // Files: names redirect to the immutable hash URL, which streams the bytes.
    let reply = send(&app, Method::GET, "/images/cat/file", Body::empty()).await;
    assert_eq!(reply.status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(reply.headers[header::LOCATION], format!("/images/{hash}/file"));
    let reply = send(&app, Method::GET, &format!("/images/{hash}/file"), Body::empty()).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(reply.body, png(1));

    // Thumbnails.
    let reply = send(&app, Method::GET, &format!("/images/{hash}/thumbnail?size=grid"), Body::empty()).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.headers[header::CONTENT_TYPE], "image/jpeg");

    // Re-uploading the same bytes is a safe retry; a taken name is a conflict.
    let reply = send(&app, Method::POST, "/images?name=Other", png(1)).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["name"], "Cat");
    let reply = send(&app, Method::POST, "/images?name=cat", png(2)).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.json()["code"], "name_taken");

    // Without a name, one is derived from file_name.
    let reply = send(&app, Method::POST, "/images?file_name=Smug%20Dog.png", png(3)).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.json()["name"], "smug-dog");

    // Bad input is a JSON error with a stable code.
    let reply = send(&app, Method::POST, "/images?name=x", "hello").await;
    assert_eq!(reply.status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(reply.json()["code"], "unsupported_media");
    let reply = send(&app, Method::POST, "/images?name=x&tags=a,,b,%20", png(4)).await;
    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.json()["tags"], json!(["a", "b"]));

    // Tags are idempotent.
    let reply = send(&app, Method::POST, &format!("/images/{hash}/tags"), r#"{"tag":"reaction"}"#).await;
    assert_eq!(reply.json()["tags"], json!(["cats", "funny", "reaction"]));
    for _ in 0..2 {
        let reply = send(&app, Method::DELETE, "/images/cat/tags/funny", Body::empty()).await;
        assert_eq!(reply.status, StatusCode::OK);
        assert_eq!(reply.json()["tags"], json!(["cats", "reaction"]));
    }
    let reply = send(&app, Method::GET, "/images?tag=cats", Body::empty()).await;
    assert_eq!(reply.json().as_array().unwrap().len(), 1);
    let reply = send(&app, Method::GET, "/tags", Body::empty()).await;
    assert_eq!(
        reply.json(),
        json!([{"tag": "a", "count": 1}, {"tag": "b", "count": 1}, {"tag": "cats", "count": 1}, {"tag": "reaction", "count": 1}])
    );

    // Rename, then delete.
    let reply = send(&app, Method::PATCH, "/images/cat", r#"{"name":"Smug Cat"}"#).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.json()["name"], "Smug Cat");
    let reply = send(&app, Method::DELETE, "/images/smug%20cat", Body::empty()).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    let reply = send(&app, Method::GET, &format!("/images/{hash}"), Body::empty()).await;
    assert_eq!(reply.status, StatusCode::NOT_FOUND);
    assert_eq!(reply.json()["code"], "not_found");
}

#[tokio::test]
async fn imports_refuse_private_addresses() {
    let (app, _tmp) = app().await;

    let reply = send(&app, Method::POST, "/imports", r#"{"url":"/etc/passwd"}"#).await;
    assert_eq!(reply.status, StatusCode::BAD_REQUEST);

    for url in ["http://127.0.0.1:7878/health", "http://localhost/x.png", "http://100.100.100.100/x.png"] {
        let reply = send(&app, Method::POST, "/imports", json!({ "url": url }).to_string()).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{url}");
        assert_eq!(reply.json()["code"], "fetch_forbidden");
    }
}

async fn get_with(app: &Router, uri: &str, headers: &[(header::HeaderName, &str)]) -> Reply {
    let mut request = Request::builder().uri(uri);
    for (name, value) in headers {
        request = request.header(name, *value);
    }
    let response = app.clone().oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Reply { status, headers, body }
}

#[tokio::test]
async fn files_are_cacheable_and_resumable() {
    let (app, _tmp) = app().await;
    let bytes = png(1);
    let hash =
        send(&app, Method::POST, "/images?name=cat", bytes.clone()).await.json()["hash"].as_str().unwrap().to_owned();
    let url = format!("/images/{hash}/file");
    let etag = format!("\"{hash}\"");

    let whole = get_with(&app, &url, &[]).await;
    assert_eq!(whole.status, StatusCode::OK);
    assert_eq!(whole.headers[header::ETAG], etag.as_str());
    assert_eq!(whole.headers[header::ACCEPT_RANGES], "bytes");
    assert!(whole.headers[header::CACHE_CONTROL].to_str().unwrap().contains("immutable"));

    let cached = get_with(&app, &url, &[(header::IF_NONE_MATCH, &etag)]).await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.body.is_empty());

    let len = bytes.len();
    let part = get_with(&app, &url, &[(header::RANGE, "bytes=10-19")]).await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.headers[header::CONTENT_RANGE], format!("bytes 10-19/{len}"));
    assert_eq!(part.body, &bytes[10..20]);

    let tail = get_with(&app, &url, &[(header::RANGE, "bytes=-5")]).await;
    assert_eq!(tail.body, &bytes[len - 5..]);

    // A stale If-Range means the client's partial copy is of something else.
    let stale = get_with(&app, &url, &[(header::RANGE, "bytes=10-19"), (header::IF_RANGE, "\"other\"")]).await;
    assert_eq!((stale.status, stale.body.len()), (StatusCode::OK, len));

    let beyond = get_with(&app, &url, &[(header::RANGE, &format!("bytes={len}-"))]).await;
    assert_eq!(beyond.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(beyond.headers[header::CONTENT_RANGE], format!("bytes */{len}"));
}

#[tokio::test]
async fn malformed_requests_get_json_errors() {
    let (app, _tmp) = app().await;
    send(&app, Method::POST, "/images?name=cat", png(1)).await;

    for (method, uri, body, status) in [
        (Method::PATCH, "/images/cat", "{not json", StatusCode::BAD_REQUEST),
        (Method::PATCH, "/images/cat", r#"{"nmae":"x"}"#, StatusCode::UNPROCESSABLE_ENTITY),
        (Method::GET, "/events?since=soon", "", StatusCode::BAD_REQUEST),
    ] {
        let reply = send(&app, method, uri, body).await;
        assert_eq!(reply.status, status, "{uri} {body}");
        assert_eq!(reply.json()["code"], "invalid_request");
    }

    let reply = send(&app, Method::GET, "/nope", Body::empty()).await;
    assert_eq!((reply.status, reply.json()["code"].clone()), (StatusCode::NOT_FOUND, json!("no_route")));
    let reply = send(&app, Method::PUT, "/tags", Body::empty()).await;
    assert_eq!(reply.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(reply.json()["code"], "method_not_allowed");
}

/// Read SSE frames until `count` `data:` lines have arrived.
async fn read_events(body: &mut Body, count: usize) -> Vec<(String, String, String)> {
    let mut text = String::new();
    loop {
        let events: Vec<(String, String, String)> = text
            .split("\n\n")
            .filter_map(|block| {
                let field =
                    |name: &str| block.lines().find_map(|l| l.strip_prefix(name).map(|v| v.trim_start().to_owned()));
                Some((field("id:").unwrap_or_default(), field("event:")?, field("data:").unwrap_or_default()))
            })
            .collect();
        if events.len() >= count {
            return events;
        }
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
            .await
            .expect("no event within 5 s")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            text.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
}

#[tokio::test]
async fn events_stream_edits_and_resume_from_last_event_id() {
    let (app, _tmp) = app().await;

    let response = app.clone().oneshot(Request::get("/events").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut live = response.into_body();

    let hash = send(&app, Method::POST, "/images?name=cat", png(1)).await.json()["hash"].as_str().unwrap().to_owned();
    send(&app, Method::POST, "/images/cat/tags", r#"{"tag":"funny"}"#).await;

    let events = read_events(&mut live, 2).await;
    assert_eq!(events[0].1, "added");
    let added: Value = serde_json::from_str(&events[0].2).unwrap();
    assert_eq!((added["hash"].as_str(), added["detail"].as_str()), (Some(hash.as_str()), Some("cat")));
    assert_eq!(events[1].1, "tagged");

    // Reconnecting with the first id replays only the second event.
    let request = Request::get("/events").header("last-event-id", &events[0].0).body(Body::empty()).unwrap();
    let mut resumed = app.clone().oneshot(request).await.unwrap().into_body();
    let replay = read_events(&mut resumed, 1).await;
    assert_eq!(replay[0], events[1]);
}
