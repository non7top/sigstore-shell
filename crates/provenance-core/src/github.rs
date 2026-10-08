use crate::provider::{Fetched, Provider, ProviderError, RateLimit};
use serde::Deserialize;
use sigstore_verify::types::Bundle;
use std::time::Duration;

const API: &str = "https://api.github.com";

pub struct GithubProvider {
    client: reqwest::Client,
    token: Option<String>,
    base: String,
    retry_delay: Duration,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    attestations: Vec<Attestation>,
}

#[derive(Deserialize)]
struct Attestation {
    bundle: Option<serde_json::Value>,
    bundle_url: Option<String>,
}

pub fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!((parts.next(), parts.next(), parts.next()), (Some(o), Some(r), None) if ok(o) && ok(r))
}

impl GithubProvider {
    pub fn new(token: Option<String>) -> Self {
        Self::with_base(token, API)
    }

    pub fn with_base(token: Option<String>, base: &str) -> Self {
        let client = reqwest::Client::builder()
            .user_agent(concat!("sigstore-shell/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("static client config");
        Self {
            client,
            token: token.filter(|t| !t.is_empty()),
            base: base.trim_end_matches('/').to_string(),
            retry_delay: Duration::from_secs(1),
        }
    }

    pub fn with_retry_delay(mut self, delay: Duration) -> Self {
        self.retry_delay = delay;
        self
    }

    pub fn authenticated(&self) -> bool {
        self.token.is_some()
    }
}

fn rate_limit(headers: &reqwest::header::HeaderMap) -> RateLimit {
    let num = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
    };
    RateLimit {
        limit: num("x-ratelimit-limit"),
        remaining: num("x-ratelimit-remaining"),
        reset_epoch: num("x-ratelimit-reset"),
        resource: headers
            .get("x-ratelimit-resource")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string),
    }
}

impl GithubProvider {
    /// One retry after a short pause, for a 5xx only: 4xx and rate limiting would just repeat.
    async fn get_with_retry(&self, url: &str) -> Result<(reqwest::Response, bool), ProviderError> {
        let mut retried = false;
        loop {
            let mut req = self
                .client
                .get(url)
                .header("Accept", "application/vnd.github+json")
                .header("X-GitHub-Api-Version", "2022-11-28");
            if let Some(token) = &self.token {
                req = req.bearer_auth(token);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| ProviderError::failed(format!("request to GitHub failed: {e}")))?;
            if resp.status().is_server_error() && !retried {
                retried = true;
                tokio::time::sleep(self.retry_delay).await;
                continue;
            }
            return Ok((resp, retried));
        }
    }

    /// The API now returns a short-lived blob URL instead of the bundle; the blob is
    /// Snappy-compressed JSON and must not receive the GitHub token.
    async fn download_bundle(&self, url: &str) -> Option<Bundle> {
        let resp = self.client.get(url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body = resp.bytes().await.ok()?;
        let json = if body.first() == Some(&b'{') {
            body.to_vec()
        } else {
            snap::raw::Decoder::new().decompress_vec(&body).ok()?
        };
        Bundle::from_json(std::str::from_utf8(&json).ok()?).ok()
    }
}

#[async_trait::async_trait]
impl Provider for GithubProvider {
    fn name(&self) -> &'static str {
        "github"
    }

    async fn fetch(&self, digest_hex: &str, repo: Option<&str>) -> Result<Fetched, ProviderError> {
        let repo = repo.ok_or_else(|| {
            ProviderError::NotApplicable("no repo claimed in the file and none given".into())
        })?;
        if !valid_repo(repo) {
            return Err(ProviderError::failed(format!(
                "'{repo}' is not an owner/repo"
            )));
        }
        let url = format!(
            "{}/repos/{repo}/attestations/sha256:{digest_hex}?per_page=100",
            self.base
        );
        let (resp, retried) = self.get_with_retry(&url).await?;
        let limits = rate_limit(resp.headers());
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Fetched {
                rate_limit: Some(limits),
                retried,
                ..Fetched::default()
            });
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            let hint = if status == reqwest::StatusCode::FORBIDDEN
                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
            {
                if limits.remaining == Some(0) {
                    " (rate limited)"
                } else {
                    " (forbidden)"
                }
            } else {
                ""
            };
            let body: String = body.chars().take(200).collect();
            return Err(ProviderError::Failed {
                message: format!(
                    "GitHub returned HTTP {status}{hint}: {body}{}",
                    if retried { " (retried once)" } else { "" }
                ),
                rate_limit: Some(limits),
            });
        }
        let parsed: Response = resp.json().await.map_err(|e| ProviderError::Failed {
            message: format!("unreadable GitHub response: {e}"),
            rate_limit: Some(limits.clone()),
        })?;
        let mut fetched = Fetched {
            rate_limit: Some(limits),
            retried,
            ..Fetched::default()
        };
        for attestation in parsed.attestations {
            let parsed = match (attestation.bundle, attestation.bundle_url) {
                (Some(inline), _) => Bundle::from_json(&inline.to_string()).ok(),
                (None, Some(url)) => self.download_bundle(&url).await,
                (None, None) => None,
            };
            match parsed {
                Some(bundle) => fetched.bundles.push(bundle),
                None => fetched.skipped += 1,
            }
        }
        Ok(fetched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_validation() {
        assert!(valid_repo("cli/cli"));
        assert!(valid_repo("non7top/prompt-loom.x"));
        assert!(!valid_repo("cli"));
        assert!(!valid_repo("a/b/c"));
        assert!(!valid_repo("a/../b?x=1"));
        assert!(!valid_repo("/b"));
    }

    /// Serves one canned response per connection, in order; returns the base URL and a hit counter.
    fn serve(
        responses: Vec<&'static str>,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut conn, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = conn.write_all(response.as_bytes());
            }
        });
        (base, hits)
    }

    const OK: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 20\r\nconnection: close\r\n\r\n{\"attestations\":[]}  ";
    const E503: &str =
        "HTTP/1.1 503 Service Unavailable\r\ncontent-length: 4\r\nconnection: close\r\n\r\nbusy";
    const E404: &str = "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";
    const E403: &str = "HTTP/1.1 403 Forbidden\r\nx-ratelimit-remaining: 0\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

    fn provider(base: &str) -> GithubProvider {
        GithubProvider::with_base(None, base).with_retry_delay(Duration::from_millis(10))
    }

    #[tokio::test]
    async fn a_503_is_retried_once_and_the_retry_is_reported() {
        let (base, hits) = serve(vec![E503, OK]);
        let fetched = provider(&base)
            .fetch(&"a".repeat(64), Some("cli/cli"))
            .await
            .unwrap();
        assert!(fetched.retried);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn two_503s_fail_after_exactly_one_retry() {
        let (base, hits) = serve(vec![E503, E503, OK]);
        let err = provider(&base)
            .fetch(&"a".repeat(64), Some("cli/cli"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("503") && err.to_string().contains("retried once"));
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn client_errors_and_rate_limits_are_not_retried() {
        for response in [E403, E404] {
            let (base, hits) = serve(vec![response, OK]);
            let _ = provider(&base)
                .fetch(&"a".repeat(64), Some("cli/cli"))
                .await;
            assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
    }

    #[tokio::test]
    async fn a_clean_answer_is_not_marked_retried() {
        let (base, _) = serve(vec![OK]);
        let fetched = provider(&base)
            .fetch(&"a".repeat(64), Some("cli/cli"))
            .await
            .unwrap();
        assert!(!fetched.retried);
    }

    #[test]
    fn parses_rate_limit_headers() {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-ratelimit-limit", "60".parse().unwrap());
        h.insert("x-ratelimit-remaining", "59".parse().unwrap());
        h.insert("x-ratelimit-reset", "1700000000".parse().unwrap());
        h.insert("x-ratelimit-resource", "core".parse().unwrap());
        let r = rate_limit(&h);
        assert_eq!(r.limit, Some(60));
        assert_eq!(r.remaining, Some(59));
        assert_eq!(r.reset_epoch, Some(1_700_000_000));
        assert_eq!(r.resource.as_deref(), Some("core"));
    }
}
