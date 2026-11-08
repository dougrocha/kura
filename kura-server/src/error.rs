use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kura_core::{Error, ErrorKind};

use crate::wire::ErrorBody;

/// Any kura-core error as an HTTP response: a status from its kind, and a
/// `wire::ErrorBody`. Internal errors are logged and never shown to clients.
///
/// Not covered yet: axum's own rejections (malformed JSON, a missing query
/// parameter) still answer in plain text. Custom extractors that turn those
/// into `ErrorBody` too are the natural next step.
pub(crate) struct ApiError(Error);

impl<E: Into<Error>> From<E> for ApiError {
    fn from(err: E) -> Self {
        Self(err.into())
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
        let kind = self.0.kind();
        if kind == ErrorKind::Internal {
            eprintln!("kura-server: {:?}", self.0);
        }
        let body = ErrorBody { code: self.0.code().to_owned(), message: self.0.to_string() };
        (status(kind), Json(body)).into_response()
    }
}
