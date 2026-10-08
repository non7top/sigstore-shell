use crate::settings::app_dir;
use provenance_core::Bundle;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;

const SCHEMA: u32 = 2;
const DAY: u64 = 24 * 3600;
/// A freshness hint only: an entry past it is looked up again, one within it is still re-verified.
const TTL: u64 = 7 * DAY;
/// Bundles are about 14 KB; anything much larger is not one.
const MAX_FILE: u64 = 1 << 20;

/// Attestation bundles on disk, one per file hash. A bundle is evidence that is checked again on
/// every use; nothing here is a verdict, so a file planted in this directory cannot make a result.
pub struct Cache {
    dir: PathBuf,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    schema: u32,
    sha256: String,
    fetched_at: u64,
    /// The `owner/repo` asked about when the bundle was fetched.
    repo: Option<String>,
    provider: String,
    /// Link hint only; not covered by the bundle's signature.
    attestation_id: Option<u64>,
    bundle: serde_json::Value,
}

/// A parsed entry; still unverified.
pub struct Stored {
    pub bundle: Bundle,
    pub fetched_at: u64,
    pub provider: String,
    pub attestation_id: Option<u64>,
}

fn valid_hash(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Cache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `%LOCALAPPDATA%\sigstore-shell\cache`, if the profile directory is known.
    pub fn user() -> Option<Self> {
        app_dir().map(|d| Self::new(d.join("cache")))
    }

    fn path(&self, sha: &str) -> Option<PathBuf> {
        valid_hash(sha).then(|| self.dir.join(format!("{sha}.json")))
    }

    /// Verdict files from before bundles were cached; never read, removed when seen.
    fn legacy_paths(&self, sha: &str) -> [PathBuf; 2] {
        [0, 1].map(|rekor| self.dir.join(format!("{sha}-r{rekor}.json")))
    }

    fn read(&self, sha: &str) -> Option<Entry> {
        let path = self.path(sha)?;
        let meta = std::fs::metadata(&path).ok()?;
        let entry = (meta.len() <= MAX_FILE)
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
            .and_then(|text| serde_json::from_str::<Entry>(&text).ok())
            .filter(|e| e.schema == SCHEMA && e.sha256 == sha);
        if entry.is_none() {
            let _ = std::fs::remove_file(path);
        }
        entry
    }

    /// An unreadable or misfiled entry is deleted. One fetched for another repo, or past the
    /// freshness hint, is a miss and is left for the next lookup to replace.
    pub fn get(&self, sha: &str, repo: Option<&str>, now: u64) -> Option<Stored> {
        let entry = self.read(sha)?;
        let Ok(bundle) = Bundle::from_json(&entry.bundle.to_string()) else {
            let _ = self.remove(sha);
            return None;
        };
        let fresh = now
            .checked_sub(entry.fetched_at)
            .is_some_and(|age| age < TTL);
        (fresh && entry.repo.as_deref() == repo).then_some(Stored {
            bundle,
            fetched_at: entry.fetched_at,
            provider: entry.provider,
            attestation_id: entry.attestation_id,
        })
    }

    pub fn put(
        &self,
        sha: &str,
        repo: Option<&str>,
        provider: &str,
        attestation_id: Option<u64>,
        bundle: &Bundle,
        now: u64,
    ) -> io::Result<()> {
        let Some(path) = self.path(sha) else {
            return Ok(());
        };
        let bundle = serde_json::from_str(&bundle.to_json().map_err(io::Error::other)?)?;
        let entry = Entry {
            schema: SCHEMA,
            sha256: sha.to_string(),
            fetched_at: now,
            repo: repo.map(str::to_string),
            provider: provider.to_string(),
            attestation_id,
            bundle,
        };
        std::fs::create_dir_all(&self.dir)?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        std::fs::write(&tmp, serde_json::to_vec(&entry)?)?;
        std::fs::rename(tmp, path)?;
        for old in self.legacy_paths(sha) {
            let _ = std::fs::remove_file(old);
        }
        Ok(())
    }

    /// Deletes this hash's entry and nothing else.
    pub fn remove(&self, sha: &str) -> io::Result<()> {
        let Some(path) = self.path(sha) else {
            return Ok(());
        };
        for file in std::iter::once(path).chain(self.legacy_paths(sha)) {
            match std::fs::remove_file(file) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLE: &str = include_str!(
        "../../provenance-core/tests/fixtures/cli-2.102.0-windows-amd64-zip.bundle.json"
    );
    const SHA: &str = "ae64e556ecc240b200f7eba60d550e4bb60d78e860e69dd88c449405b86067f4";
    const OTHER: &str = "00000000000000000000000000000000000000000000000000000000000000aa";

    fn bundle() -> Bundle {
        Bundle::from_json(BUNDLE).unwrap()
    }

    fn cache(name: &str) -> (Cache, PathBuf) {
        let dir = std::env::temp_dir().join(format!("ssx-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        (Cache::new(dir.clone()), dir)
    }

    fn put(c: &Cache, repo: Option<&str>, now: u64) {
        c.put(SHA, repo, "github", Some(7), &bundle(), now).unwrap();
    }

    fn file(dir: &std::path::Path) -> PathBuf {
        dir.join(format!("{SHA}.json"))
    }

    #[test]
    fn round_trips_a_bundle_that_still_parses_and_serializes_the_same() {
        let (c, dir) = cache("rt");
        put(&c, Some("cli/cli"), 1000);
        let got = c.get(SHA, Some("cli/cli"), 2000).unwrap();
        assert_eq!(got.fetched_at, 1000);
        assert_eq!(got.provider, "github");
        assert_eq!(got.attestation_id, Some(7));
        assert_eq!(got.bundle, bundle());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn nothing_but_the_bundle_and_its_bookkeeping_is_stored() {
        let (c, dir) = cache("shape");
        put(&c, Some("cli/cli"), 1);
        let text = std::fs::read_to_string(file(&dir)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "attestation_id",
                "bundle",
                "fetched_at",
                "provider",
                "repo",
                "schema",
                "sha256"
            ]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_week_is_a_freshness_hint_and_a_stale_entry_is_kept_for_replacement() {
        let (c, dir) = cache("ttl");
        put(&c, None, 0);
        assert!(c.get(SHA, None, 7 * DAY - 1).is_some());
        assert!(c.get(SHA, None, 7 * DAY).is_none());
        assert!(file(&dir).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_different_repo_misses_without_deleting() {
        let (c, dir) = cache("repo");
        put(&c, Some("cli/cli"), 0);
        assert!(c.get(SHA, Some("evil/repo"), 1).is_none());
        assert!(c.get(SHA, None, 1).is_none());
        assert!(file(&dir).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clock_going_backwards_misses() {
        let (c, dir) = cache("clock");
        put(&c, None, 100);
        assert!(c.get(SHA, None, 50).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_misfiled_oversized_and_old_format_entries_are_deleted() {
        let (c, dir) = cache("bad");
        put(&c, None, 0);
        let good = std::fs::read_to_string(file(&dir)).unwrap();
        let cases: Vec<(&str, String)> = vec![
            ("truncated", good[..good.len() / 2].to_string()),
            ("empty", String::new()),
            ("not json", "\u{0}\u{1}".into()),
            (
                "another file's entry",
                good.replace(SHA, OTHER),
            ),
            ("other schema", good.replace("\"schema\":2", "\"schema\":1")),
            (
                "verdict fields smuggled in",
                good.replacen('{', "{\"status\":\"verified\",\"identity\":{\"repo\":\"evil/repo\"},", 1),
            ),
            (
                "the old verdict format",
                format!(
                    "{{\"schema\":1,\"cached_at\":1,\"report\":{{\"status\":\"verified\",\"file_sha256\":\"{SHA}\"}}}}"
                ),
            ),
            ("not a bundle", {
                let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
                v["bundle"] = serde_json::json!({"mediaType": "x"});
                v.to_string()
            }),
            ("oversized", " ".repeat(MAX_FILE as usize + 1)),
        ];
        for (name, text) in cases {
            std::fs::write(file(&dir), &text).unwrap();
            assert!(c.get(SHA, None, 1).is_none(), "{name}");
            assert!(!file(&dir).exists(), "{name} was not deleted");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_deletes_only_this_hashs_entry_and_its_old_verdict_files() {
        let (c, dir) = cache("rm");
        put(&c, None, 0);
        c.put(OTHER, None, "github", None, &bundle(), 0).unwrap();
        std::fs::write(dir.join(format!("{SHA}-r0.json")), "{}").unwrap();
        c.remove(SHA).unwrap();
        c.remove(SHA).unwrap();
        assert!(!file(&dir).exists());
        assert!(!dir.join(format!("{SHA}-r0.json")).exists());
        assert!(dir.join(format!("{OTHER}.json")).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn writing_an_entry_removes_old_verdict_files_for_the_hash() {
        let (c, dir) = cache("legacy");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{SHA}-r1.json")), "{}").unwrap();
        put(&c, None, 0);
        assert!(!dir.join(format!("{SHA}-r1.json")).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_hashes_that_could_escape_the_directory() {
        let (c, _) = cache("path");
        assert!(c.path("../../etc/passwd").is_none());
        assert!(c.get(&"A".repeat(64), None, 0).is_none());
        assert!(c.remove("../x").is_ok());
        assert!(c.put("../x", None, "github", None, &bundle(), 0).is_ok());
    }
}
