//! Where new image bytes come from. Every source becomes the same thing, a byte
//! stream plus an optional name hint, so `Library::add` has one ingest path
//! (hash, sniff and size limit while staging). Callers choose `path`, `fetch`,
//! `stream` or `bytes` explicitly; the core never guesses from a string.

use std::{
    io,
    net::IpAddr,
    path::{Path, PathBuf},
    pin::Pin,
    time::Duration,
};

use bytes::Bytes;
use futures::{Stream, TryStreamExt};
use reqwest::Url;
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;

use crate::error::{Error, FetchFailure, InvalidInput, Result};

/// New image bytes, not yet read.
pub struct Source {
    pub(crate) reader: Pin<Box<dyn AsyncRead + Send>>,
    /// File name or URL, used only for `NewName::FromSource`.
    pub(crate) hint: Option<String>,
    pub(crate) origin: Origin,
}

/// Where the bytes come from, so a failed read becomes the right error.
pub(crate) enum Origin {
    File(PathBuf),
    Url(String),
    Upload,
}

impl Origin {
    pub(crate) fn read_error(&self, err: io::Error) -> Error {
        match self {
            Self::File(path) => Error::ReadSource {
                what: format!("'{}'", path.display()),
                reason: err.to_string(),
            },
            Self::Url(url) => Error::Fetch { url: url.clone(), reason: FetchFailure::Network(err.to_string()) },
            Self::Upload => Error::ReadSource { what: "the upload".into(), reason: err.to_string() },
        }
    }
}

impl Source {
    /// A local file. Only for clients acting as the local user (the CLI); the
    /// server never calls this, so remote callers can't read the server's disk.
    pub async fn path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let origin = Origin::File(path.clone());
        let file = tokio::fs::File::open(&path).await.map_err(|e| origin.read_error(e))?;
        Ok(Self {
            reader: Box::pin(file),
            hint: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            origin,
        })
    }

    /// An HTTP(S) URL fetched under `policy`. Returns once the response headers
    /// arrive; the body streams during staging and is still capped by
    /// `Limits::max_image_bytes`.
    pub async fn fetch(url: &HttpUrl, policy: &FetchPolicy) -> Result<Self> {
        let fail = |reason| Error::Fetch { url: url.as_str().to_owned(), reason };

        if !policy.allow_private {
            ensure_public(&url.0).await.map_err(fail)?;
        }

        let client = reqwest::Client::builder()
            .timeout(policy.timeout)
            // A redirect could point anywhere, including private addresses, so
            // only follow redirects when private destinations are allowed anyway.
            .redirect(if policy.allow_private {
                reqwest::redirect::Policy::limited(5)
            } else {
                reqwest::redirect::Policy::none()
            })
            .build()
            .map_err(|e| fail(FetchFailure::Network(e.to_string())))?;

        let response = client.get(url.0.clone()).send().await.map_err(|e| {
            fail(if e.is_timeout() { FetchFailure::Timeout } else { FetchFailure::Network(e.to_string()) })
        })?;
        if !response.status().is_success() {
            return Err(fail(FetchFailure::Status(response.status().as_u16())));
        }

        let stream = response.bytes_stream().map_err(io::Error::other);
        Ok(Self {
            reader: Box::pin(StreamReader::new(stream)),
            hint: Some(url.0.path().to_owned()),
            origin: Origin::Url(url.as_str().to_owned()),
        })
    }

    /// Any byte stream, such as an HTTP request body. Nothing is buffered.
    pub fn stream<S, E>(stream: S, hint: Option<String>) -> Self
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Send + 'static,
        E: Into<Box<dyn std::error::Error + Send + Sync>> + 'static,
    {
        let stream = stream.map_err(io::Error::other);
        Self { reader: Box::pin(StreamReader::new(stream)), hint, origin: Origin::Upload }
    }

    /// In-memory bytes.
    pub fn bytes(data: impl Into<Bytes>, hint: Option<String>) -> Self {
        let data: Bytes = data.into();
        Self { reader: Box::pin(io::Cursor::new(data)), hint, origin: Origin::Upload }
    }
}

/// An absolute `http` or `https` URL with a host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpUrl(Url);

impl HttpUrl {
    pub fn parse(s: &str) -> Result<Self, InvalidInput> {
        let url = Url::parse(s.trim()).map_err(|e| InvalidInput::new("url", e.to_string()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(InvalidInput::new("url", "must start with http:// or https://"));
        }
        if url.host().is_none() {
            return Err(InvalidInput::new("url", "must have a host"));
        }
        Ok(Self(url))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::str::FromStr for HttpUrl {
    type Err = InvalidInput;

    fn from_str(s: &str) -> Result<Self, InvalidInput> {
        Self::parse(s)
    }
}

/// Which URLs kura-core may fetch for a caller.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    pub timeout: Duration,
    /// Allow loopback, private, link-local and tailnet destinations.
    pub allow_private: bool,
}

impl FetchPolicy {
    /// The CLI: the user can already reach anything on their own network.
    pub fn trusted_local_user() -> Self {
        Self { timeout: Duration::from_secs(60), allow_private: true }
    }

    /// The server: callers are remote, so only the public internet (an SSRF guard).
    pub fn public_only() -> Self {
        Self { timeout: Duration::from_secs(30), allow_private: false }
    }
}

/// Resolve the URL's host and refuse it if any address isn't public.
///
/// Known gap: the HTTP client resolves the name again, so a DNS server that
/// answers differently the second time (DNS rebinding) could slip past this.
/// Closing that needs a custom resolver on the client.
async fn ensure_public(url: &Url) -> std::result::Result<(), FetchFailure> {
    let host = url.host_str().ok_or(FetchFailure::Forbidden)?;
    let port = url.port_or_known_default().unwrap_or(80);
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| FetchFailure::Network(e.to_string()))?;

    let mut any = false;
    for addr in addrs {
        any = true;
        if !is_public(addr.ip()) {
            return Err(FetchFailure::Forbidden);
        }
    }
    if any { Ok(()) } else { Err(FetchFailure::Network("host has no addresses".into())) }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            let cgnat = a == 100 && (64..128).contains(&b); // 100.64.0.0/10, used by Tailscale
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || cgnat)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            let unique_local = first & 0xfe00 == 0xfc00; // fc00::/7, includes Tailscale's fd7a::/48
            let link_local = first & 0xffc0 == 0xfe80;
            !(v6.is_loopback() || v6.is_unspecified() || unique_local || link_local)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_not_public() {
        for ip in ["127.0.0.1", "10.1.2.3", "192.168.1.1", "100.101.102.103", "169.254.1.1", "::1", "fd7a:115c:a1e0::1", "::ffff:127.0.0.1"] {
            assert!(!is_public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn urls_must_be_http() {
        assert!(HttpUrl::parse("https://example.com/cat.png").is_ok());
        assert!(HttpUrl::parse("/etc/passwd").is_err());
        assert!(HttpUrl::parse("file:///etc/passwd").is_err());
    }
}
