use std::{collections::HashMap, str::FromStr};

use strum::{Display, EnumString};

#[derive(Debug, PartialEq, Display, EnumString)]
#[strum(ascii_case_insensitive)]
pub enum Method {
    Get,
    Post,
    Patch,
    Put,
    Delete,
}

#[derive(Debug, PartialEq, Display, EnumString)]
pub enum HttpVersion {
    #[strum(serialize = "1.0")]
    Http1_0,
    #[strum(serialize = "1.1")]
    Http1_1,
}

pub struct Request {
    pub method: Method,
    pub path: String,
    pub http_version: HttpVersion,

    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl FromStr for Request {
    type Err = ParseRequestError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s.as_bytes())
    }
}

impl TryFrom<&[u8]> for Request {
    type Error = ParseRequestError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        // head ends at the first CRLF CRLF, the rest is body
        let head_end = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or(ParseRequestError::MissingCrlf)?;
        let head = std::str::from_utf8(&bytes[..head_end])
            .map_err(|_| ParseRequestError::InvalidEncoding)?;
        let body_bytes = &bytes[head_end + 4..];

        let mut lines = head.split("\r\n");

        // parse first request line
        let request_line = lines
            .next()
            .filter(|l| !l.is_empty())
            .ok_or(ParseRequestError::MissingRequestLine)?;
        let mut parts = request_line.split(' ');

        let method = Method::from_str(parts.next().ok_or(ParseRequestError::UnexpectedEof)?)
            .map_err(|_| ParseRequestError::InvalidMethod)?;

        let path = parts
            .next()
            .ok_or(ParseRequestError::UnexpectedEof)
            .and_then(|p| {
                if p.starts_with('/') {
                    Ok(p.to_string())
                } else {
                    Err(ParseRequestError::InvalidUri)
                }
            })?;

        let version_part = parts.next().ok_or(ParseRequestError::UnexpectedEof)?;
        let version_str = version_part
            .strip_prefix("HTTP/")
            .ok_or(ParseRequestError::InvalidVersion)?;
        let http_version = version_str
            .parse::<HttpVersion>()
            .map_err(|_| ParseRequestError::InvalidVersion)?;

        if parts.next().is_some() {
            return Err(ParseRequestError::InvalidRequestLine);
        }

        // parse headers, repeated names are combined with ", "
        let mut headers: HashMap<String, String> = HashMap::new();
        for line in lines {
            let (name, value) = line.split_once(':').ok_or_else(|| {
                ParseRequestError::InvalidHeader("missing ':' in header".to_string())
            })?;

            let name = name.trim().to_lowercase();
            if name.is_empty() {
                return Err(ParseRequestError::InvalidHeader(
                    "empty header name".to_string(),
                ));
            }
            let value = value.trim();

            headers
                .entry(name)
                .and_modify(|v| {
                    v.push_str(", ");
                    v.push_str(value);
                })
                .or_insert_with(|| value.to_string());
        }

        // body is exactly content-length bytes, extra bytes are ignored
        let body = match headers.get("content-length") {
            Some(len) => {
                let expected: usize = len.parse().map_err(|_| {
                    ParseRequestError::InvalidHeader("invalid content-length".to_string())
                })?;
                if body_bytes.len() < expected {
                    return Err(ParseRequestError::IncompleteBody);
                }
                body_bytes[..expected].to_vec()
            }
            None => Vec::new(),
        };

        Ok(Self {
            method,
            path,
            http_version,

            headers,
            body,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseRequestError {
    #[error("invalid method")]
    InvalidMethod,
    #[error("invalid uri")]
    InvalidUri,
    #[error("invalid version")]
    InvalidVersion,
    #[error("unexpected eof")]
    UnexpectedEof,
    #[error("missing CRLF")]
    MissingCrlf,
    #[error("missing request line")]
    MissingRequestLine,
    #[error("invalid request line")]
    InvalidRequestLine,
    #[error("invalid utf-8 in request head")]
    InvalidEncoding,
    #[error("invalid header: {0}")]
    InvalidHeader(String),
    #[error("incomplete body")]
    IncompleteBody,
}

#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("not found")]
    NotFound,
    #[error("method not allowed")]
    MethodNotAllowed,
}

pub type Handler = Box<dyn Fn(Request) -> Response + Send + Sync>;

pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Self {
        todo!()
    }

    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        todo!()
    }

    pub fn body(mut self, body: impl Into<Vec<u8>>) -> Self {
        todo!()
    }

    /// Serializes the status line, headers (including content-length) and body.
    pub fn to_bytes(&self) -> Vec<u8> {
        todo!()
    }
}

impl From<RouterError> for Response {
    fn from(err: RouterError) -> Self {
        todo!()
    }
}

pub struct Route {
    pub method: Method,
    pub path: String,
    handler: Handler,
}

impl Route {
    pub fn new(
        method: Method,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> Self {
        todo!()
    }

    pub fn matches(&self, method: &Method, path: &str) -> bool {
        todo!()
    }

    pub fn call(&self, req: Request) -> Response {
        todo!()
    }
}

#[derive(Default)]
pub struct Router {
    routes: Vec<Route>,
}

impl Router {
    pub fn new() -> Self {
        todo!()
    }

    pub fn route(
        &mut self,
        method: Method,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    pub fn get(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    pub fn post(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    pub fn put(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    pub fn patch(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    pub fn delete(
        &mut self,
        path: impl Into<String>,
        handler: impl Fn(Request) -> Response + Send + Sync + 'static,
    ) -> &mut Self {
        todo!()
    }

    /// Dispatches to the first matching route; `NotFound` if no path matches, `MethodNotAllowed` if the path matches under a different method.
    pub fn handle(&self, req: Request) -> Result<Response, RouterError> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_get_with_headers() {
        let req = Request::try_from(
            &b"GET /users HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\n\r\n"[..],
        )
        .unwrap();
        assert_eq!(req.method, Method::Get);
        assert_eq!(req.path, "/users");
        assert_eq!(req.http_version, HttpVersion::Http1_1);
        assert_eq!(req.headers["host"], "example.com");
        assert_eq!(req.headers["accept"], "*/*");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parses_post_with_content_length_body() {
        let req = "POST /items HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello"
            .parse::<Request>()
            .unwrap();
        assert_eq!(req.method, Method::Post);
        assert_eq!(req.body, b"hello");
    }

    #[test]
    fn parses_binary_body() {
        let bytes = b"POST /upload HTTP/1.1\r\ncontent-length: 4\r\n\r\n\xff\xfe\x00\x80";
        let req = Request::try_from(&bytes[..]).unwrap();
        assert_eq!(req.body, vec![0xff, 0xfe, 0x00, 0x80]);
    }

    #[test]
    fn bare_lf_is_missing_crlf() {
        assert!(matches!(
            Request::try_from(&b"GET / HTTP/1.1\nHost: x\n\n"[..]),
            Err(ParseRequestError::MissingCrlf)
        ));
    }

    #[test]
    fn trailing_junk_on_request_line_is_invalid() {
        assert!(matches!(
            "GET / HTTP/1.1 garbage\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::InvalidRequestLine)
        ));
    }

    #[test]
    fn empty_request_line_is_missing() {
        assert!(matches!(
            "\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::MissingRequestLine)
        ));
    }

    #[test]
    fn invalid_method() {
        assert!(matches!(
            "FOO / HTTP/1.1\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::InvalidMethod)
        ));
    }

    #[test]
    fn path_must_start_with_slash() {
        assert!(matches!(
            "GET index HTTP/1.1\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::InvalidUri)
        ));
    }

    #[test]
    fn bad_version() {
        assert!(matches!(
            "GET / HTTP/2.0\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::InvalidVersion)
        ));
    }

    #[test]
    fn incomplete_body() {
        assert!(matches!(
            "POST / HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc".parse::<Request>(),
            Err(ParseRequestError::IncompleteBody)
        ));
    }

    #[test]
    fn extra_bytes_beyond_content_length_are_ignored() {
        let req = "POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\nabcGET / HTTP/1.1\r\n\r\n"
            .parse::<Request>()
            .unwrap();
        assert_eq!(req.body, b"abc");
    }

    #[test]
    fn duplicate_headers_are_combined() {
        let req = "GET / HTTP/1.1\r\nAccept: a\r\naccept: b\r\n\r\n"
            .parse::<Request>()
            .unwrap();
        assert_eq!(req.headers["accept"], "a, b");
    }

    #[test]
    fn header_without_colon_is_invalid() {
        assert!(matches!(
            "GET / HTTP/1.1\r\nHost example\r\n\r\n".parse::<Request>(),
            Err(ParseRequestError::InvalidHeader(msg)) if msg == "missing ':' in header"
        ));
    }

    #[test]
    fn method_is_case_insensitive() {
        let req = "get / HTTP/1.1\r\n\r\n".parse::<Request>().unwrap();
        assert_eq!(req.method, Method::Get);
    }
}
