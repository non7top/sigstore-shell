use serde::{Deserialize, Serialize};
use sigstore_verify::crypto::CertificateInfo;
use sigstore_verify::SubjectAltName;

/// What the Fulcio certificate says about the build. Every field is a certificate claim.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Identity {
    pub repo: Option<String>,
    pub owner: Option<String>,
    pub workflow: Option<String>,
    pub git_ref: Option<String>,
    pub commit: Option<String>,
    pub signed_at: Option<String>,
    pub trigger: Option<String>,
    pub runner: Option<String>,
    pub run_url: Option<String>,
    pub issuer: Option<String>,
    pub san: Option<String>,
}

fn strip_host(url: &str) -> Option<&str> {
    url.strip_prefix("https://github.com/")
        .map(|s| s.trim_end_matches('/'))
}

impl Identity {
    pub(crate) fn from_certificate(cert: &CertificateInfo, signed_at: Option<String>) -> Self {
        let ci = &cert.ci_claims;
        let legacy = &ci.deprecated_github;
        let repo = ci
            .source_repository_uri
            .as_deref()
            .and_then(strip_host)
            .map(str::to_string)
            .or_else(|| legacy.workflow_repository.clone());
        let owner = ci
            .source_repository_owner_uri
            .as_deref()
            .and_then(strip_host)
            .map(str::to_string)
            .or_else(|| repo.as_ref()?.split('/').next().map(str::to_string));
        let san = cert.identity.as_ref().map(|i| i.to_string());
        let san_uri = match &cert.identity {
            Some(SubjectAltName::Uri(u)) => Some(u.clone()),
            _ => None,
        };
        let workflow_uri = ci.build_config_uri.clone().or(san_uri);
        let (workflow, ref_from_uri) = match workflow_uri.as_deref() {
            Some(uri) => {
                let uri = strip_host(uri).unwrap_or(uri);
                let path = repo
                    .as_deref()
                    .and_then(|r| uri.strip_prefix(r))
                    .map(|p| p.trim_start_matches('/'))
                    .unwrap_or(uri);
                match path.split_once('@') {
                    Some((w, r)) => (Some(w.to_string()), Some(r.to_string())),
                    None => (Some(path.to_string()), None),
                }
            }
            None => (None, None),
        };
        Self {
            repo,
            owner,
            workflow,
            git_ref: ci
                .source_repository_ref
                .clone()
                .or(legacy.workflow_ref.clone())
                .or(ref_from_uri),
            commit: ci
                .source_repository_digest
                .clone()
                .or_else(|| legacy.workflow_sha.clone()),
            signed_at,
            trigger: ci
                .build_trigger
                .clone()
                .or_else(|| legacy.workflow_trigger.clone()),
            runner: ci.runner_environment.clone(),
            run_url: ci.run_invocation_uri.clone(),
            issuer: cert.issuer.clone(),
            san,
        }
    }
}
