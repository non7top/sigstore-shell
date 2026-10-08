use serde::{Deserialize, Serialize};
use sigstore_verify::types::Bundle;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RateLimit {
    pub limit: Option<u64>,
    pub remaining: Option<u64>,
    pub reset_epoch: Option<u64>,
    pub resource: Option<String>,
}

/// Candidate bundles from a provider; nothing here is trusted until verified.
#[derive(Debug, Default)]
pub struct Fetched {
    pub bundles: Vec<Bundle>,
    /// Provider-side page id per bundle, same order; shorter than `bundles` means unknown. A link hint only.
    pub attestation_ids: Vec<Option<u64>>,
    pub rate_limit: Option<RateLimit>,
    pub skipped: usize,
    /// The provider hit a server error and asked again once.
    pub retried: bool,
}

impl Fetched {
    pub fn attestation_id(&self, bundle_index: usize) -> Option<u64> {
        self.attestation_ids.get(bundle_index).copied().flatten()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("not applicable: {0}")]
    NotApplicable(String),
    #[error("{message}")]
    Failed {
        message: String,
        rate_limit: Option<RateLimit>,
    },
}

impl ProviderError {
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
            rate_limit: None,
        }
    }
}

#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;

    /// `repo` is the `owner/repo` to ask about, if known.
    async fn fetch(&self, digest_hex: &str, repo: Option<&str>) -> Result<Fetched, ProviderError>;
}

/// A bundle kept from an earlier lookup, offered as if a provider had just returned it. Verifying
/// it goes through the same path as a fresh answer, so nothing about it is taken on trust.
pub struct StoredBundle {
    name: &'static str,
    bundle: Bundle,
    attestation_id: Option<u64>,
}

impl StoredBundle {
    /// `provider` is the name the bundle came from; an unknown one is reported as `cache`.
    pub fn new(provider: &str, bundle: Bundle, attestation_id: Option<u64>) -> Self {
        let name = match provider {
            "github" => "github",
            "rekor-v1" => "rekor-v1",
            _ => "cache",
        };
        Self {
            name,
            bundle,
            attestation_id,
        }
    }
}

#[async_trait::async_trait]
impl Provider for StoredBundle {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
        Ok(Fetched {
            bundles: vec![self.bundle.clone()],
            attestation_ids: vec![self.attestation_id],
            ..Fetched::default()
        })
    }
}
