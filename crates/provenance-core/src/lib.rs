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

pub use github::GithubProvider;
pub use identity::Identity;
pub use pe::{read_claim, PeError, CLAIM_KEY};
pub use provider::{Fetched, Provider, ProviderError, RateLimit};
pub use rekor::RekorProvider;
pub use verify::{load_trusted_root, verify_file, Options, Report, Status, TrustRootSource};

use sha2::{Digest, Sha256};
use std::io;
use std::path::Path;

/// Lower-case hex SHA-256 of a file, streamed.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
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
        std::fs::remove_dir_all(dir).unwrap();
    }
}
