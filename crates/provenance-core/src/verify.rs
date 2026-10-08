use crate::claim::read_claim_file;
use crate::github::valid_repo;
use crate::identity::Identity;
use crate::provider::{Provider, ProviderError, RateLimit};
use serde::{Deserialize, Serialize};
use sigstore_verify::trust_root::{TrustedRoot, SIGSTORE_PRODUCTION_TRUSTED_ROOT};
use sigstore_verify::types::{Bundle, Sha256Hash};
use sigstore_verify::{VerificationPolicy, Verifier};
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Verified,
    /// Providers answered and found nothing. Not a statement that the file is unsafe.
    NoAttestation,
    /// Verified, but the certificate names a different repo than the file or `--repo` claimed.
    Mismatch,
    LookupFailed,
    /// An attestation was returned but did not verify against the Sigstore trust root.
    VerificationFailed,
    /// No repo to ask about and no search provider enabled.
    NotChecked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrustRootSource {
    /// Fetched and validated through Sigstore's TUF repository.
    Tuf,
    /// Compiled into the sigstore-trust-root crate; may be stale.
    Embedded,
}

#[derive(Debug, Default)]
pub struct Options {
    /// Repo to ask about instead of the one embedded in the file.
    pub repo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub status: Status,
    pub file_sha256: String,
    pub claimed_repo: Option<String>,
    pub queried_repo: Option<String>,
    pub provider: Option<String>,
    /// Present for `verified` and `mismatch`, from the certificate only.
    pub identity: Option<Identity>,
    pub rate_limit: Option<RateLimit>,
    pub trust_root: TrustRootSource,
    pub notes: Vec<String>,
}

pub async fn load_trusted_root() -> Result<(TrustedRoot, TrustRootSource), String> {
    match TrustedRoot::production().await {
        Ok(root) => Ok((root, TrustRootSource::Tuf)),
        Err(_) => load_embedded_root(),
    }
}

pub fn load_embedded_root() -> Result<(TrustedRoot, TrustRootSource), String> {
    TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT)
        .map(|root| (root, TrustRootSource::Embedded))
        .map_err(|e| format!("no usable Sigstore trust root: {e}"))
}

fn same_repo(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

fn verify_bundle(
    verifier: &Verifier,
    digest: Sha256Hash,
    bundle: &Bundle,
) -> Result<Identity, String> {
    let result = verifier
        .verify(digest, bundle, &VerificationPolicy::any_identity())
        .map_err(|e| e.to_string())?;
    let cert = result
        .certificate()
        .filter(|_| result.certificate_verified())
        .ok_or("bundle carries no verified signing certificate")?;
    let signed_at = result
        .verified_timestamps()
        .iter()
        .min()
        .copied()
        .or(result.integrated_time())
        .map(|t| t.to_string());
    Ok(Identity::from_certificate(cert, signed_at))
}

pub async fn verify_file(
    path: &Path,
    options: &Options,
    providers: &[Box<dyn Provider>],
    root: &TrustedRoot,
    root_source: TrustRootSource,
) -> Result<Report, String> {
    let digest_hex = crate::sha256_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut notes = Vec::new();

    let claimed_repo = match read_claim_file(path) {
        Ok(claim) => claim,
        Err(e) => {
            notes.push(e.to_string());
            None
        }
    };
    if let Some(claim) = &claimed_repo {
        if !valid_repo(claim) {
            notes.push(format!(
                "embedded claim '{claim}' is not an owner/repo; ignoring it"
            ));
        }
    }
    let claimed_repo = claimed_repo.filter(|c| valid_repo(c));
    verify_digest(
        &digest_hex,
        claimed_repo,
        notes,
        options,
        providers,
        root,
        root_source,
    )
    .await
}

/// Looks up and verifies an already-hashed file. `claimed_repo` must already be validated.
pub async fn verify_digest(
    digest_hex: &str,
    claimed_repo: Option<String>,
    notes: Vec<String>,
    options: &Options,
    providers: &[Box<dyn Provider>],
    root: &TrustedRoot,
    root_source: TrustRootSource,
) -> Result<Report, String> {
    let digest = Sha256Hash::from_hex(digest_hex).map_err(|e| e.to_string())?;
    let queried_repo = options.repo.clone().or_else(|| claimed_repo.clone());

    let mut report = Report {
        status: Status::NotChecked,
        file_sha256: digest_hex.to_string(),
        claimed_repo: claimed_repo.clone(),
        queried_repo: queried_repo.clone(),
        provider: None,
        identity: None,
        rate_limit: None,
        trust_root: root_source,
        notes,
    };
    let verifier = Verifier::new(root).map_err(|e| format!("trust root unusable: {e}"))?;

    let mut answered_empty = false;
    let mut failed = false;
    let mut invalid = false;
    for provider in providers {
        match provider.fetch(digest_hex, queried_repo.as_deref()).await {
            Err(ProviderError::NotApplicable(why)) => {
                report.notes.push(format!("{}: {why}", provider.name()));
            }
            Err(ProviderError::Failed {
                message,
                rate_limit,
            }) => {
                failed = true;
                report.rate_limit = rate_limit.or(report.rate_limit.take());
                report.notes.push(format!("{}: {message}", provider.name()));
            }
            Ok(fetched) => {
                if fetched.rate_limit.is_some() {
                    report.rate_limit = fetched.rate_limit;
                }
                if fetched.skipped > 0 {
                    report.notes.push(format!(
                        "{}: ignored {} entries that could not be turned into a bundle",
                        provider.name(),
                        fetched.skipped
                    ));
                }
                if fetched.bundles.is_empty() {
                    answered_empty = true;
                    continue;
                }
                let mut rejected = Vec::new();
                for bundle in &fetched.bundles {
                    match verify_bundle(&verifier, digest, bundle) {
                        Ok(identity) => {
                            if !rejected.is_empty() {
                                report.notes.push(format!(
                                    "{}: {} other attestation(s) did not verify against the public Sigstore root: {}",
                                    provider.name(),
                                    rejected.len(),
                                    rejected[0]
                                ));
                            }
                            report.provider = Some(provider.name().to_string());
                            let expected = options.repo.as_ref().or(claimed_repo.as_ref());
                            let mismatch = match (&identity.repo, expected) {
                                (Some(actual), Some(want)) => !same_repo(actual, want),
                                (None, Some(_)) => true,
                                _ => false,
                            };
                            report.status = if mismatch {
                                Status::Mismatch
                            } else {
                                Status::Verified
                            };
                            report.identity = Some(identity);
                            return Ok(report);
                        }
                        Err(e) => {
                            invalid = true;
                            rejected.push(e);
                        }
                    }
                }
                for e in rejected {
                    report.notes.push(format!(
                        "{}: attestation did not verify: {e}",
                        provider.name()
                    ));
                }
            }
        }
    }

    report.status = if invalid {
        Status::VerificationFailed
    } else if failed {
        Status::LookupFailed
    } else if answered_empty {
        Status::NoAttestation
    } else {
        Status::NotChecked
    };
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Fetched;

    const BUNDLE: &str =
        include_str!("../tests/fixtures/cli-2.102.0-windows-amd64-zip.bundle.json");
    const DIGEST: &str = "ae64e556ecc240b200f7eba60d550e4bb60d78e860e69dd88c449405b86067f4";

    enum Mock {
        Bundle,
        Empty,
        Fail,
    }

    #[async_trait::async_trait]
    impl Provider for Mock {
        fn name(&self) -> &'static str {
            "mock"
        }
        async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            match self {
                Mock::Bundle => Ok(Fetched {
                    bundles: vec![Bundle::from_json(BUNDLE).unwrap()],
                    ..Fetched::default()
                }),
                Mock::Empty => Ok(Fetched::default()),
                Mock::Fail => Err(ProviderError::failed("offline")),
            }
        }
    }

    async fn run(mock: Mock, digest: &str, claim: Option<&str>) -> Report {
        let (root, src) = (
            TrustedRoot::from_json(SIGSTORE_PRODUCTION_TRUSTED_ROOT).unwrap(),
            TrustRootSource::Embedded,
        );
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(mock)];
        verify_digest(
            digest,
            claim.map(str::to_string),
            vec![],
            &Options::default(),
            &providers,
            &root,
            src,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn real_bundle_verifies_and_identity_comes_from_certificate() {
        let r = run(Mock::Bundle, DIGEST, Some("CLI/cli")).await;
        assert_eq!(r.status, Status::Verified);
        let id = r.identity.unwrap();
        assert_eq!(id.repo.as_deref(), Some("cli/cli"));
        assert_eq!(id.owner.as_deref(), Some("cli"));
        assert_eq!(
            id.workflow.as_deref(),
            Some(".github/workflows/deployment.yml")
        );
        assert_eq!(
            id.commit.as_deref(),
            Some("fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd")
        );
        assert!(id.signed_at.is_some());
    }

    #[tokio::test]
    async fn claim_for_another_repo_is_a_mismatch() {
        let r = run(Mock::Bundle, DIGEST, Some("evil/repo")).await;
        assert_eq!(r.status, Status::Mismatch);
        assert_eq!(r.identity.unwrap().repo.as_deref(), Some("cli/cli"));
    }

    #[tokio::test]
    async fn bundle_for_a_different_digest_does_not_verify() {
        let other = "00".repeat(32);
        let r = run(Mock::Bundle, &other, Some("cli/cli")).await;
        assert_eq!(r.status, Status::VerificationFailed);
        assert!(r.identity.is_none());
    }

    #[tokio::test]
    async fn empty_answer_is_no_attestation() {
        let r = run(Mock::Empty, DIGEST, Some("cli/cli")).await;
        assert_eq!(r.status, Status::NoAttestation);
    }

    #[tokio::test]
    async fn provider_error_is_lookup_failed_not_no_attestation() {
        let r = run(Mock::Fail, DIGEST, Some("cli/cli")).await;
        assert_eq!(r.status, Status::LookupFailed);
    }
}
