use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kura_core::{Error, ErrorKind};

use crate::wire::ErrorBody;

/// Every failure the API answers with, always as a `wire::ErrorBody`.
pub(crate) enum ApiError {
    /// A kura-core error: the status comes from its kind. Internal errors are
    /// logged and never shown to clients.
    Core(Error),
    /// The request itself was malformed (axum's extractors rejected it, or no
    /// route matched). Built by `extract.rs` and the router fallbacks.
    Request { status: StatusCode, code: &'static str, message: String },
}

// No `From` impls for axum's rejection types: this blanket impl would overlap
// them (kura-core could add `From<JsonRejection> for Error`), so rejections go
// through `ApiError::request` instead.
impl<E: Into<Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self::Core(err.into())
    }
}

impl ApiError {
    pub(crate) fn request(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self::Request { status, code, message: message.into() }
    }
}

pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// Exhaustive on purpose: a new `ErrorKind` doesn't compile until it has a status.
fn status(kind: ErrorKind) -> StatusCode {
    match kind {
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::Conflict => StatusCode::CONFLICT,
        ErrorKind::Invalid => StatusCode::BAD_REQUEST,
        ErrorKind::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
        ErrorKind::Unsupported => StatusCode::UNSUPPORTED_MEDIA_TYPE,
        ErrorKind::Forbidden => StatusCode::FORBIDDEN,
        ErrorKind::Upstream => StatusCode::BAD_GATEWAY,
        ErrorKind::Busy => StatusCode::SERVICE_UNAVAILABLE,
        ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::Core(err) => {
                let kind = err.kind();
                if kind == ErrorKind::Internal {
                    eprintln!("kura-server: {err:?}");
                }
                (status(kind), ErrorBody { code: err.code().to_owned(), message: err.to_string() })
            }
            Self::Request { status, code, message } => (status, ErrorBody { code: code.to_owned(), message }),
        };
        (status, Json(body)).into_response()
    }
}
