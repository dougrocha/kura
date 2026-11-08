//! Drop-in replacements for axum's `Json`, `Query` and `Path` extractors whose
//! rejections are `ApiError`s, so a malformed request gets the same JSON error
//! body as everything else instead of axum's plain text. Handlers use these;
//! responses still use `axum::Json`.

use axum::{
    extract::{FromRequest, FromRequestParts, Request},
    http::{StatusCode, request::Parts},
};
use serde::de::DeserializeOwned;

use crate::error::ApiError;

/// The `code` of every rejected request. The status says which way it was bad
/// (400 bad JSON or query, 415 missing `Content-Type: application/json`, ...).
const INVALID_REQUEST: &str = "invalid_request";

fn rejected(status: StatusCode, message: String) -> ApiError {
    ApiError::request(status, INVALID_REQUEST, message)
}

/// A JSON request body.
pub(crate) struct Json<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Json<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match axum::Json::<T>::from_request(req, state).await {
            Ok(axum::Json(value)) => Ok(Self(value)),
            Err(r) => Err(rejected(r.status(), r.body_text())),
        }
    }
}

/// Query-string parameters.
pub(crate) struct Query<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for Query<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Self(value)),
            Err(r) => Err(rejected(r.status(), r.body_text())),
        }
    }
}

/// Path parameters.
pub(crate) struct Path<T>(pub T);

impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for Path<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
        match axum::extract::Path::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Path(value)) => Ok(Self(value)),
            Err(r) => Err(rejected(r.status(), r.body_text())),
        }
    }
}

/// Answers requests no route matches.
pub(crate) async fn no_route() -> ApiError {
    ApiError::request(StatusCode::NOT_FOUND, "no_route", "no such endpoint")
}

/// Answers a known path with the wrong method.
pub(crate) async fn wrong_method() -> ApiError {
    ApiError::request(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed", "this endpoint doesn't take that method")
}
