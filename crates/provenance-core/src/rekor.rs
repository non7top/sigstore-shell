use crate::provider::{Fetched, Provider, ProviderError};
use sigstore_rekor::{RekorClient, RekorEntryBody};
use sigstore_verify::bundle::BundleV03;
use sigstore_verify::types::{KindVersion, Sha256Hash};

const PUBLIC_REKOR: &str = "https://rekor.sigstore.dev";

/// Anyone can sign any hash, so the number of entries for one is not under the file's control.
const MAX_ENTRIES: usize = 25;

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
        if uuids.len() > MAX_ENTRIES {
            fetched.failures.push(format!(
                "Rekor lists {} entries for this hash; only the first {MAX_ENTRIES} were read",
                uuids.len()
            ));
        }
        for uuid in uuids.iter().take(MAX_ENTRIES) {
            let entry = match client.get_entry_by_uuid(uuid).await {
                Ok(entry) => entry,
                Err(e) => {
                    fetched.failures.push(format!("Rekor entry {uuid}: {e}"));
                    continue;
                }
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serves one canned response per connection, in order; returns the base URL and a hit counter.
    fn serve(responses: Vec<String>) -> (String, Arc<AtomicUsize>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for response in responses {
                let Ok((mut conn, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 4096];
                let _ = conn.read(&mut buf);
                counter.fetch_add(1, Ordering::SeqCst);
                let _ = conn.write_all(response.as_bytes());
            }
        });
        (base, hits)
    }

    fn json(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    const E500: &str =
        "HTTP/1.1 500 Internal Server Error\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

    fn index(count: usize) -> String {
        let uuids: Vec<String> = (0..count)
            .map(|i| format!("\"{}{i:016x}\"", "a".repeat(64)))
            .collect();
        json(&format!("[{}]", uuids.join(",")))
    }

    #[tokio::test]
    async fn an_entry_that_cannot_be_fetched_is_reported_and_the_rest_are_still_read() {
        let mut responses = vec![index(3)];
        responses.extend((0..3).map(|_| E500.to_string()));
        let (base, hits) = serve(responses);
        let fetched = RekorProvider::new(&base)
            .fetch(&"b".repeat(64), None)
            .await
            .unwrap();
        assert_eq!(fetched.failures.len(), 3, "{:?}", fetched.failures);
        assert!(
            fetched.failures[0].contains("500"),
            "{:?}",
            fetched.failures
        );
        assert_eq!(hits.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn no_more_than_the_cap_of_entries_is_fetched_and_the_rest_is_reported() {
        let total = MAX_ENTRIES + 10;
        let mut responses = vec![index(total)];
        responses.extend((0..total).map(|_| E500.to_string()));
        let (base, hits) = serve(responses);
        let fetched = RekorProvider::new(&base)
            .fetch(&"b".repeat(64), None)
            .await
            .unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 1 + MAX_ENTRIES);
        assert_eq!(fetched.failures.len(), 1 + MAX_ENTRIES);
        assert!(fetched.failures[0].contains(&format!("only the first {MAX_ENTRIES}")));
    }
}
