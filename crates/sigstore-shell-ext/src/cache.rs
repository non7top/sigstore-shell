use provenance_core::{Report, Status};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

const SCHEMA: u32 = 1;
const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

/// Per-file-hash results on disk. Only outcomes that are stable are stored.
pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    schema: u32,
    cached_at: u64,
    report: Report,
}

fn ttl(status: Status) -> Option<u64> {
    match status {
        Status::Verified | Status::Mismatch => Some(7 * DAY),
        // A signer may attest the file later, so a negative answer goes stale quickly.
        Status::NoAttestation => Some(HOUR),
        Status::LookupFailed | Status::VerificationFailed | Status::NotChecked => None,
    }
}

fn valid_hash(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, sha: &str, rekor: bool) -> Option<PathBuf> {
        valid_hash(sha).then(|| self.dir.join(format!("{sha}-r{}.json", u8::from(rekor))))
    }

    /// A hit needs the same claim as when stored, because the claim decides the mismatch status.
    pub fn get(
        &self,
        sha: &str,
        claimed: Option<&str>,
        rekor: bool,
        now: u64,
    ) -> Option<(Report, u64)> {
        let text = std::fs::read_to_string(self.path(sha, rekor)?).ok()?;
        let entry: Entry = serde_json::from_str(&text).ok()?;
        let age = now.checked_sub(entry.cached_at)?;
        let fresh = ttl(entry.report.status).is_some_and(|ttl| age < ttl);
        let same = entry.schema == SCHEMA
            && entry.report.file_sha256 == sha
            && entry.report.claimed_repo.as_deref() == claimed;
        (fresh && same).then_some((entry.report, entry.cached_at))
    }

    /// Does nothing for statuses that should not be remembered.
    pub fn put(&self, report: &Report, rekor: bool, now: u64) -> io::Result<()> {
        let (Some(_), Some(path)) = (ttl(report.status), self.path(&report.file_sha256, rekor))
        else {
            return Ok(());
        };
        std::fs::create_dir_all(&self.dir)?;
        let entry = Entry {
            schema: SCHEMA,
            cached_at: now,
            report: report.clone(),
        };
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec(&entry)?)?;
        std::fs::rename(tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provenance_core::{Identity, TrustRootSource};

    const SHA: &str = "ae64e556ecc240b200f7eba60d550e4bb60d78e860e69dd88c449405b86067f4";

    fn report(status: Status, claimed: Option<&str>) -> Report {
        Report {
            status,
            file_sha256: SHA.into(),
            claimed_repo: claimed.map(str::to_string),
            queried_repo: claimed.map(str::to_string),
            provider: Some("github".into()),
            identity: Some(Identity {
                repo: Some("cli/cli".into()),
                ..Identity::default()
            }),
            attestation_id: None,
            log_index: None,
            rate_limit: None,
            trust_root: TrustRootSource::Tuf,
            notes: vec!["n".into()],
        }
    }

    fn cache(name: &str) -> (Cache, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ssx-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Cache::new(dir.clone()), dir)
    }

    #[test]
    fn round_trips_a_verified_report() {
        let (c, dir) = cache("rt");
        c.put(&report(Status::Verified, Some("cli/cli")), false, 1000)
            .unwrap();
        let (got, at) = c.get(SHA, Some("cli/cli"), false, 2000).unwrap();
        assert_eq!(at, 1000);
        assert_eq!(got.status, Status::Verified);
        assert_eq!(got.identity.unwrap().repo.as_deref(), Some("cli/cli"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn negative_answer_expires_after_an_hour_but_verified_does_not() {
        let (c, dir) = cache("ttl");
        c.put(&report(Status::NoAttestation, None), false, 0)
            .unwrap();
        assert!(c.get(SHA, None, false, HOUR - 1).is_some());
        assert!(c.get(SHA, None, false, HOUR).is_none());
        c.put(&report(Status::Verified, None), false, 0).unwrap();
        assert!(c.get(SHA, None, false, 6 * DAY).is_some());
        assert!(c.get(SHA, None, false, 7 * DAY).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failures_are_never_stored() {
        let (c, dir) = cache("fail");
        for s in [
            Status::LookupFailed,
            Status::VerificationFailed,
            Status::NotChecked,
        ] {
            c.put(&report(s, None), false, 0).unwrap();
        }
        assert!(!dir.exists());
    }

    #[test]
    fn a_different_claim_or_rekor_setting_misses() {
        let (c, dir) = cache("claim");
        c.put(&report(Status::Verified, Some("cli/cli")), false, 0)
            .unwrap();
        assert!(c.get(SHA, Some("evil/repo"), false, 1).is_none());
        assert!(c.get(SHA, None, false, 1).is_none());
        assert!(c.get(SHA, Some("cli/cli"), true, 1).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clock_going_backwards_and_corrupt_files_miss() {
        let (c, dir) = cache("bad");
        c.put(&report(Status::Verified, None), false, 100).unwrap();
        assert!(c.get(SHA, None, false, 50).is_none());
        std::fs::write(dir.join(format!("{SHA}-r0.json")), "{truncated").unwrap();
        assert!(c.get(SHA, None, false, 200).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_hashes_that_could_escape_the_directory() {
        let (c, _) = cache("path");
        assert!(c.path("../../etc/passwd", false).is_none());
        assert!(c.get(&"A".repeat(64), None, false, 0).is_none());
    }
}
