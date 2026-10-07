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
    pub rate_limit: Option<RateLimit>,
    pub skipped: usize,
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
