use crate::provider::{Fetched, Provider, ProviderError};
use sigstore_verify::types::Bundle;
use std::path::{Path, PathBuf};

const MAX_BUNDLE_BYTES: u64 = 4 << 20;
const SIDECAR_SUFFIXES: [&str; 2] = [".sigstore", ".cosign.bundle"];

/// A bundle read from a file on disk. It is only a candidate: it goes through the same
/// verification as a downloaded one, so the file's name or location proves nothing.
pub struct BundleFile {
    path: PathBuf,
}

impl BundleFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The `FILE.sigstore` or `FILE.cosign.bundle` that sits next to `file`, if one exists.
    pub fn beside(file: &Path) -> Option<Self> {
        SIDECAR_SUFFIXES
            .iter()
            .map(|suffix| {
                let mut name = file.as_os_str().to_owned();
                name.push(suffix);
                PathBuf::from(name)
            })
            .find(|candidate| candidate.is_file())
            .map(Self::new)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn load(&self) -> Result<Bundle, String> {
        let shown = self.path.display();
        let len = std::fs::metadata(&self.path)
            .map_err(|e| format!("{shown}: {e}"))?
            .len();
        if len > MAX_BUNDLE_BYTES {
            return Err(format!("{shown}: {len} bytes is too large for a bundle"));
        }
        let text = std::fs::read_to_string(&self.path).map_err(|e| format!("{shown}: {e}"))?;
        Bundle::from_json(&text).map_err(|e| format!("{shown}: not a Sigstore bundle: {e}"))
    }
}

#[async_trait::async_trait]
impl Provider for BundleFile {
    fn name(&self) -> &'static str {
        "bundle-file"
    }

    async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
        let bundle = self.load().map_err(ProviderError::failed)?;
        Ok(Fetched {
            bundles: vec![bundle],
            ..Fetched::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_embedded_root, verify_digest, verify_file, Options, Report, Status};

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    /// A real `cosign sign-blob` bundle (messageSignature) for the SHA256SUMS of release v0.1.0.
    const SUMS: &str = "SHA256SUMS";
    const SUMS_BUNDLE: &str = "SHA256SUMS.cosign.bundle";
    /// A real attestation bundle (DSSE) for a GitHub CLI release zip.
    const DSSE_BUNDLE: &str = "cli-2.102.0-windows-amd64-zip.bundle.json";
    const DSSE_DIGEST: &str = "ae64e556ecc240b200f7eba60d550e4bb60d78e860e69dd88c449405b86067f4";

    fn fixture(name: &str) -> PathBuf {
        Path::new(FIXTURES).join(name)
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pc-bundle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn verify(file: &Path, bundle: &Path) -> Report {
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(BundleFile::new(bundle))];
        verify_file(file, &Options::default(), &providers, &root, source)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_message_signature_bundle_verifies_the_signed_file() {
        let r = verify(&fixture(SUMS), &fixture(SUMS_BUNDLE)).await;
        assert_eq!(r.status, Status::Verified);
        assert_eq!(r.provider.as_deref(), Some("bundle-file"));
        assert_eq!(
            r.identity.unwrap().repo.as_deref(),
            Some("non7top/sigstore-shell")
        );
    }

    #[tokio::test]
    async fn a_dsse_bundle_verifies_a_digest_it_names() {
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> =
            vec![Box::new(BundleFile::new(fixture(DSSE_BUNDLE)))];
        let r = verify_digest(
            DSSE_DIGEST,
            None,
            vec![],
            &Options::default(),
            &providers,
            &root,
            source,
        )
        .await
        .unwrap();
        assert_eq!(r.status, Status::Verified);
        assert_eq!(r.identity.unwrap().repo.as_deref(), Some("cli/cli"));
    }

    #[tokio::test]
    async fn a_bundle_for_other_bytes_is_verification_failed_not_verified() {
        let dir = scratch("tampered");
        let mut bytes = std::fs::read(fixture(SUMS)).unwrap();
        bytes[0] ^= 1;
        let altered = dir.join(SUMS);
        std::fs::write(&altered, bytes).unwrap();
        let r = verify(&altered, &fixture(SUMS_BUNDLE)).await;
        assert_eq!(r.status, Status::VerificationFailed);
        assert!(r.identity.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_missing_or_malformed_bundle_is_a_failed_lookup_with_the_reason() {
        let dir = scratch("bad");
        let r = verify(&fixture(SUMS), &dir.join("nope.sigstore")).await;
        assert_eq!(r.status, Status::LookupFailed);
        assert!(r.notes.iter().any(|n| n.starts_with("bundle-file:")));

        let garbage = dir.join("garbage.sigstore");
        std::fs::write(&garbage, "not json").unwrap();
        let r = verify(&fixture(SUMS), &garbage).await;
        assert_eq!(r.status, Status::LookupFailed);
        assert!(r.notes.iter().any(|n| n.contains("not a Sigstore bundle")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_oversized_file_is_refused_without_parsing() {
        let dir = scratch("big");
        let big = dir.join("big.sigstore");
        std::fs::write(&big, vec![b' '; MAX_BUNDLE_BYTES as usize + 1]).unwrap();
        let err = BundleFile::new(&big).load().unwrap_err();
        assert!(err.contains("too large"), "{err}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn beside_finds_either_sidecar_name_and_nothing_else() {
        let dir = scratch("beside");
        let file = dir.join("app.exe");
        std::fs::write(&file, b"x").unwrap();
        assert!(BundleFile::beside(&file).is_none());

        std::fs::write(dir.join("app.exe.cosign.bundle"), "{}").unwrap();
        let found = BundleFile::beside(&file).unwrap();
        assert_eq!(found.path(), dir.join("app.exe.cosign.bundle"));

        std::fs::write(dir.join("app.exe.sigstore"), "{}").unwrap();
        let found = BundleFile::beside(&file).unwrap();
        assert_eq!(found.path(), dir.join("app.exe.sigstore"));

        std::fs::write(dir.join("other.exe.sigstore"), "{}").unwrap();
        assert!(BundleFile::beside(&dir.join("other2.exe")).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
