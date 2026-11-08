//! Serving an image file: `ETag`, `If-None-Match` (304) and `Range` (206/416).
//!
//! Hash URLs are immutable, so the hash is a strong ETag and the response may
//! be cached forever. That's also what makes ranges safe to resume: a client
//! holding part of `<hash>` can never get the rest of a different file.

use std::io::SeekFrom;

use axum::{
    body::Body,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use kura_core::ImageFile;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

use crate::error::ApiResult;

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// The bytes of `[start, end]`, inclusive, like `Content-Range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ByteRange {
    pub(crate) start: u64,
    pub(crate) end: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum RangeRequest {
    /// No `Range`, or one we serve whole (several ranges, another unit, junk).
    Whole,
    Part(ByteRange),
    Unsatisfiable,
}

/// Parse a `Range` header for a file of `len` bytes. Only a single
/// `bytes=` range is honored; anything else falls back to the whole file,
/// which RFC 9110 allows a server to do.
pub(crate) fn parse_range(value: &str, len: u64) -> RangeRequest {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return RangeRequest::Whole;
    };
    if spec.contains(',') {
        return RangeRequest::Whole;
    }
    let Some((first, last)) = spec.trim().split_once('-') else {
        return RangeRequest::Whole;
    };
    let (first, last) = (first.trim(), last.trim());

    let range = match (first.parse::<u64>(), last.parse::<u64>()) {
        // `bytes=-500`: the last 500 bytes.
        (Err(_), Ok(suffix)) if first.is_empty() => {
            if suffix == 0 || len == 0 {
                return RangeRequest::Unsatisfiable;
            }
            ByteRange { start: len.saturating_sub(suffix), end: len - 1 }
        }
        // `bytes=500-`: from 500 to the end.
        (Ok(start), Err(_)) if last.is_empty() => ByteRange { start, end: len.saturating_sub(1) },
        (Ok(start), Ok(end)) if start <= end => ByteRange { start, end: end.min(len.saturating_sub(1)) },
        _ => return RangeRequest::Whole,
    };
    if range.start >= len { RangeRequest::Unsatisfiable } else { RangeRequest::Part(range) }
}

/// Does an `If-None-Match` / `If-Range` value name `etag`? Weak comparison, as
/// RFC 9110 requires for `If-None-Match`.
fn matches(value: &str, etag: &str) -> bool {
    value.split(',').map(str::trim).any(|tag| tag == "*" || tag.trim_start_matches("W/") == etag)
}

pub(crate) async fn respond(file: ImageFile, request: &HeaderMap) -> ApiResult<Response> {
    let etag = format!("\"{}\"", file.hash());
    let len = file.len();
    let header = |name| request.get(name).and_then(|v: &HeaderValue| v.to_str().ok());

    let mut headers = HeaderMap::new();
    headers.insert(header::ETAG, HeaderValue::try_from(etag.as_str()).expect("hex is a valid header"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(IMMUTABLE));
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));

    if header(header::IF_NONE_MATCH).is_some_and(|v| matches(v, &etag)) {
        return Ok((StatusCode::NOT_MODIFIED, headers).into_response());
    }

    let content_type = file.media_type().map_or("application/octet-stream", |m| m.as_str());
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));

    // `If-Range` with another validator means "my partial copy is stale": send it all.
    let range_still_valid = header(header::IF_RANGE).is_none_or(|v| v.trim() == etag);
    let range = match header(header::RANGE) {
        Some(value) if range_still_valid => parse_range(value, len),
        _ => RangeRequest::Whole,
    };

    let mut file = file.into_file();
    match range {
        RangeRequest::Whole => {
            headers.insert(header::CONTENT_LENGTH, len.into());
            Ok((headers, Body::from_stream(ReaderStream::new(file))).into_response())
        }
        RangeRequest::Part(ByteRange { start, end }) => {
            file.seek(SeekFrom::Start(start)).await.map_err(seek_failed)?;
            let part_len = end - start + 1;
            headers.insert(header::CONTENT_LENGTH, part_len.into());
            let content_range = format!("bytes {start}-{end}/{len}");
            headers.insert(header::CONTENT_RANGE, HeaderValue::try_from(content_range).expect("digits"));
            let body = Body::from_stream(ReaderStream::new(file.take(part_len)));
            Ok((StatusCode::PARTIAL_CONTENT, headers, body).into_response())
        }
        RangeRequest::Unsatisfiable => {
            headers.remove(header::CONTENT_TYPE);
            let content_range = format!("bytes */{len}");
            headers.insert(header::CONTENT_RANGE, HeaderValue::try_from(content_range).expect("digits"));
            Ok((StatusCode::RANGE_NOT_SATISFIABLE, headers).into_response())
        }
    }
}

/// A failed seek on a file we just opened is a server problem, not the client's.
fn seek_failed(e: std::io::Error) -> crate::error::ApiError {
    eprintln!("kura-server: seeking image file: {e}");
    crate::error::ApiError::request(StatusCode::INTERNAL_SERVER_ERROR, "internal", "internal error")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(start: u64, end: u64) -> RangeRequest {
        RangeRequest::Part(ByteRange { start, end })
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-99", 1000), part(0, 99));
        assert_eq!(parse_range("bytes=900-", 1000), part(900, 999));
        assert_eq!(parse_range("bytes=-100", 1000), part(900, 999));
        assert_eq!(parse_range("bytes=-5000", 1000), part(0, 999));
        assert_eq!(parse_range("bytes=990-5000", 1000), part(990, 999));
        assert_eq!(parse_range("bytes=1000-", 1000), RangeRequest::Unsatisfiable);
        assert_eq!(parse_range("bytes=-0", 1000), RangeRequest::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-", 0), RangeRequest::Unsatisfiable);
        for whole in ["bytes=0-1,5-6", "items=0-1", "bytes=5-1", "bytes=x-y", "bytes=-", "0-1"] {
            assert_eq!(parse_range(whole, 1000), RangeRequest::Whole, "{whole}");
        }
    }

    #[test]
    fn etags() {
        assert!(matches("\"abc\"", "\"abc\""));
        assert!(matches("W/\"abc\"", "\"abc\""));
        assert!(matches("\"x\", \"abc\"", "\"abc\""));
        assert!(matches("*", "\"abc\""));
        assert!(!matches("\"abd\"", "\"abc\""));
    }
}
