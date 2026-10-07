//! Read an exe's claimed repo, look up its Sigstore attestation by hash and verify it.
//!
//! The identity in a [`Report`] always comes from the verified certificate; the repo
//! embedded in the file is only a claim to compare against.

mod github;
mod identity;
mod pe;
mod provider;
mod rekor;
mod verify;

pub use github::{valid_repo, GithubProvider};
pub use identity::Identity;
#[cfg(feature = "test-fixtures")]
pub use pe::fixture;
pub use pe::{read_claim, read_claim_file, PeError, CLAIM_KEY};
pub use provider::{Fetched, Provider, ProviderError, RateLimit};
pub use rekor::RekorProvider;
pub use sigstore_verify::trust_root::TrustedRoot;
pub use verify::{
    load_embedded_root, load_trusted_root, verify_digest, verify_file, Options, Report, Status,
    TrustRootSource,
};

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Lower-case hex SHA-256 of a file, streamed.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    Ok(sha256_file_with(path, |_| true)?.expect("callback never stops"))
}

/// Streamed SHA-256 that calls `keep_going(bytes_hashed_so_far)` per chunk; `None` if it returned false.
pub fn sha256_file_with(
    path: &Path,
    mut keep_going: impl FnMut(u64) -> bool,
) -> io::Result<Option<String>> {
    use io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            return Ok(Some(hex::encode(hasher.finalize())));
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        if !keep_going(done) {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_input() {
        let dir = std::env::temp_dir().join(format!("pc-sha-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc");
        std::fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(sha256_file_with(&file, |_| false).unwrap(), None);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
