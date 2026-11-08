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
    image::RgbImage::from_pixel(4, 4, image::Rgb([seed, 0, 0]))
        .write_to(&mut out, image::ImageFormat::Png)
        .unwrap();
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
