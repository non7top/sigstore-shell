use crate::provider::{Fetched, Provider, ProviderError};
use reqwest::Url;
use sigstore_verify::types::Bundle;
use std::time::Duration;

const MAX_BUNDLE_BYTES: usize = 4 << 20;
const MAX_REDIRECTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(30);

/// A bundle downloaded from an https URL the user gave. It is only a candidate: it goes through
/// the same verification as any other, so neither the URL nor the host proves anything.
pub struct WebBundle {
    url: Url,
    client: reqwest::Client,
}

impl WebBundle {
    /// Only https URLs without a user name or password are accepted.
    pub fn new(url: &str) -> Result<Self, String> {
        Self::build(url, true)
    }

    fn build(url: &str, https_only: bool) -> Result<Self, String> {
        let parsed = Url::parse(url).map_err(|e| format!("'{url}' is not a URL: {e}"))?;
        if !scheme_allowed(&parsed, https_only) {
            return Err(format!("'{url}' is not an https URL"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("a bundle URL must not carry a user name or password".into());
        }
        if parsed.host_str().is_none() {
            return Err(format!("'{url}' has no host"));
        }
        let client = reqwest::Client::builder()
            .user_agent(concat!(
                "sigstore-shell/",
                env!("SIGSTORE_SHELL_VERSION"),
                " (commit ",
                env!("SIGSTORE_SHELL_COMMIT"),
                ")"
            ))
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if !scheme_allowed(attempt.url(), https_only) {
                    attempt.error("redirected to a URL that is not https")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            url: parsed,
            client,
        })
    }

    /// The host the request goes to, for the text that tells the user where the request is sent.
    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    async fn download(&self) -> Result<String, String> {
        let host = self.host();
        let mut resp = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|e| format!("request to {host} failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("{host} returned HTTP {}", resp.status()));
        }
        let too_large =
            || format!("the bundle from {host} is larger than {MAX_BUNDLE_BYTES} bytes");
        if resp
            .content_length()
            .is_some_and(|n| n > MAX_BUNDLE_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("reading from {host} failed: {e}"))?
        {
            if body.len() + chunk.len() > MAX_BUNDLE_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| format!("the bundle from {host} is not text"))
    }
}

fn scheme_allowed(url: &Url, https_only: bool) -> bool {
    url.scheme() == "https" || (!https_only && url.scheme() == "http")
}

#[async_trait::async_trait]
impl Provider for WebBundle {
    fn name(&self) -> &'static str {
        "web-bundle"
    }

    async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
        let text = self.download().await.map_err(ProviderError::failed)?;
        let bundle = Bundle::from_json(&text).map_err(|e| {
            ProviderError::failed(format!("{}: not a Sigstore bundle: {e}", self.host()))
        })?;
        Ok(Fetched {
            bundles: vec![bundle],
            ..Fetched::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_embedded_root, verify_file, Options, Status};
    use std::path::Path;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    /// Answers each request with the next canned response, then closes the connection.
    async fn serve(responses: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(&response).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn redirect(to: &str) -> Vec<u8> {
        format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    }

    fn sums_bundle() -> Vec<u8> {
        std::fs::read(Path::new(FIXTURES).join("SHA256SUMS.cosign.bundle")).unwrap()
    }

    async fn fetch(base: &str, path: &str) -> Result<Fetched, ProviderError> {
        WebBundle::build(&format!("{base}{path}"), false)
            .unwrap()
            .fetch("", None)
            .await
    }

    #[test]
    fn only_plain_https_urls_are_accepted() {
        assert!(WebBundle::new("https://example.org/a.sigstore").is_ok());
        for url in [
            "http://example.org/a.sigstore",
            "ftp://example.org/a",
            "file:///tmp/a.sigstore",
            "https://user:pw@example.org/a",
            "https://user@example.org/a",
            "example.org/a",
            "",
        ] {
            assert!(WebBundle::new(url).is_err(), "{url}");
        }
    }

    #[test]
    fn host_is_the_host_of_the_url() {
        let w =
            WebBundle::new("https://github.com/o/r/releases/download/v1/a.cosign.bundle").unwrap();
        assert_eq!(w.host(), "github.com");
    }

    #[test]
    fn only_https_may_be_the_target_of_a_redirect() {
        let https = Url::parse("https://example.org/").unwrap();
        let http = Url::parse("http://example.org/").unwrap();
        assert!(scheme_allowed(&https, true));
        assert!(!scheme_allowed(&http, true));
        assert!(scheme_allowed(&http, false));
        assert!(!scheme_allowed(
            &Url::parse("file:///etc/passwd").unwrap(),
            false
        ));
    }

    #[tokio::test]
    async fn a_downloaded_bundle_verifies_the_file_it_was_made_for() {
        let base = serve(vec![ok(&sums_bundle())]).await;
        let web = WebBundle::build(&format!("{base}/SHA256SUMS.cosign.bundle"), false).unwrap();
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(web)];
        let r = verify_file(
            &Path::new(FIXTURES).join("SHA256SUMS"),
            &Options::default(),
            &providers,
            &root,
            source,
        )
        .await
        .unwrap();
        assert_eq!(r.status, Status::Verified);
        assert_eq!(r.provider.as_deref(), Some("web-bundle"));
    }

    #[tokio::test]
    async fn a_bundle_for_other_bytes_is_not_verified() {
        let base = serve(vec![ok(&sums_bundle())]).await;
        let web = WebBundle::build(&format!("{base}/b"), false).unwrap();
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(web)];
        let r = verify_file(
            &Path::new(FIXTURES).join("cli-2.102.0-windows-amd64-zip.bundle.json"),
            &Options::default(),
            &providers,
            &root,
            source,
        )
        .await
        .unwrap();
        assert_eq!(r.status, Status::VerificationFailed);
    }

    #[tokio::test]
    async fn a_redirect_is_followed() {
        let base = serve(vec![redirect("/real"), ok(&sums_bundle())]).await;
        assert_eq!(fetch(&base, "/start").await.unwrap().bundles.len(), 1);
    }

    #[tokio::test]
    async fn an_http_error_names_the_host_and_status() {
        let base = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ])
        .await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("127.0.0.1") && e.contains("404"), "{e}");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_by_its_declared_length() {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_BUNDLE_BYTES + 1
        );
        let base = serve(vec![head.into_bytes()]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("larger than"), "{e}");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_while_reading_it() {
        let body = vec![b' '; MAX_BUNDLE_BYTES + 1];
        let chunked = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
            format!("{:x}\r\n", body.len()).into_bytes(),
            body,
            b"\r\n0\r\n\r\n".to_vec(),
        ]
        .concat();
        let base = serve(vec![chunked]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("larger than"), "{e}");
    }

    #[tokio::test]
    async fn something_that_is_not_a_bundle_is_a_failure_not_a_verdict() {
        let base = serve(vec![ok(b"<html>not a bundle</html>")]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("not a Sigstore bundle"), "{e}");
    }
}
