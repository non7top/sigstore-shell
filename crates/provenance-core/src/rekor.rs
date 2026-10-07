use crate::provider::{Fetched, Provider, ProviderError};
use sigstore_rekor::{RekorClient, RekorEntryBody};
use sigstore_verify::bundle::BundleV03;
use sigstore_verify::types::{KindVersion, Sha256Hash};

const PUBLIC_REKOR: &str = "https://rekor.sigstore.dev";

/// Rekor v1 search by hash. Only `hashedrekord` entries are indexed by the artifact digest.
pub struct RekorProvider {
    url: String,
}

impl Default for RekorProvider {
    fn default() -> Self {
        Self::new(PUBLIC_REKOR)
    }
}

impl RekorProvider {
    pub fn new(url: &str) -> Self {
        Self { url: url.into() }
    }
}

#[async_trait::async_trait]
impl Provider for RekorProvider {
    fn name(&self) -> &'static str {
        "rekor-v1"
    }

    async fn fetch(&self, digest_hex: &str, _repo: Option<&str>) -> Result<Fetched, ProviderError> {
        let hash = Sha256Hash::from_hex(digest_hex)
            .map_err(|e| ProviderError::failed(format!("bad digest: {e}")))?;
        let client =
            RekorClient::new(&self.url).map_err(|e| ProviderError::failed(e.to_string()))?;
        let uuids = client
            .search_by_hash(&hash)
            .await
            .map_err(|e| ProviderError::failed(format!("Rekor search failed: {e}")))?;
        let mut fetched = Fetched::default();
        for uuid in uuids {
            let entry = client
                .get_entry_by_uuid(&uuid)
                .await
                .map_err(|e| ProviderError::failed(format!("Rekor entry {uuid}: {e}")))?;
            let parsed = RekorEntryBody::parse(&entry.body, KindVersion::HashedRekordV001);
            let Ok(RekorEntryBody::HashedRekordV001(body)) = parsed else {
                fetched.skipped += 1;
                continue;
            };
            let spec = body.spec;
            let (Ok(cert), Ok(tlog)) = (
                spec.signature.public_key.parse_certificate(),
                entry.to_bundle_entry(KindVersion::HashedRekordV001),
            ) else {
                fetched.skipped += 1;
                continue;
            };
            fetched.bundles.push(
                BundleV03::with_certificate_and_signature(cert, spec.signature.content, hash)
                    .with_tlog_entry(tlog)
                    .into_bundle(),
            );
        }
        Ok(fetched)
    }
}
