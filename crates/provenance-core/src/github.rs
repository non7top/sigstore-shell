use crate::provider::{Fetched, Provider, ProviderError, RateLimit};
use serde::Deserialize;
use sigstore_verify::types::Bundle;

const API: &str = "https://api.github.com";

pub struct GithubProvider {
    client: reqwest::Client,
    token: Option<String>,
    base: String,
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

pub(crate) fn valid_repo(repo: &str) -> bool {
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
        }
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
        let limits = rate_limit(resp.headers());
        let status = resp.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(Fetched {
                rate_limit: Some(limits),
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
                message: format!("GitHub returned HTTP {status}{hint}: {body}"),
                rate_limit: Some(limits),
            });
        }
        let parsed: Response = resp.json().await.map_err(|e| ProviderError::Failed {
            message: format!("unreadable GitHub response: {e}"),
            rate_limit: Some(limits.clone()),
        })?;
        let mut fetched = Fetched {
            rate_limit: Some(limits),
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
