use crate::cache::Cache;
use crate::model::{format_epoch, Phase, CACHED_NOTE};
use provenance_core::{
    sha256_file_with, verify_digest, verify_digest_keeping_bundle, Options, Provider, Report,
    Status, StoredBundle, TrustRootSource, TrustedRoot,
};
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, PartialEq, Eq)]
pub enum JobError {
    Cancelled,
    Failed(String),
}

pub struct JobInput<'a> {
    pub path: &'a Path,
    /// Already validated as `owner/repo`.
    pub claimed_repo: Option<String>,
    /// Skip the cache and ask the providers now.
    pub refresh: bool,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

async fn cancelled(flag: &AtomicBool) {
    while !flag.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Hash, then [`resolve`]. Checks `cancel` between hash chunks.
pub async fn run<F, Fut>(
    input: &JobInput<'_>,
    cache: Option<&Cache>,
    providers: &[Box<dyn Provider>],
    load_root: F,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Phase) + Sync),
) -> Result<Report, JobError>
where
    F: Fn(bool) -> Fut,
    Fut: Future<Output = Result<(TrustedRoot, TrustRootSource), String>>,
{
    let total = std::fs::metadata(input.path)
        .map_err(|e| JobError::Failed(format!("{}: {e}", input.path.display())))?
        .len()
        .max(1);
    let sha = sha256_file_with(input.path, |done| {
        progress(Phase::Hashing {
            percent: u8::try_from(done.min(total) * 100 / total).unwrap_or(100),
        });
        !cancel.load(Ordering::Relaxed)
    })
    .map_err(|e| JobError::Failed(format!("{}: {e}", input.path.display())))?
    .ok_or(JobError::Cancelled)?;
    resolve(
        &sha,
        input.claimed_repo.as_deref(),
        input.refresh,
        cache,
        providers,
        load_root,
        cancel,
        progress,
    )
    .await
}

/// A cached bundle is verified again offline (`load_root(true)`) and only then used. An entry that
/// does not verify is deleted and the lookup goes online (`load_root(false)`).
#[allow(clippy::too_many_arguments)]
pub async fn resolve<F, Fut>(
    sha: &str,
    claimed: Option<&str>,
    refresh: bool,
    cache: Option<&Cache>,
    providers: &[Box<dyn Provider>],
    load_root: F,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Phase) + Sync),
) -> Result<Report, JobError>
where
    F: Fn(bool) -> Fut,
    Fut: Future<Output = Result<(TrustedRoot, TrustRootSource), String>>,
{
    if let (Some(cache), false) = (cache, refresh) {
        if let Some(report) = from_cache(cache, sha, claimed, &load_root).await {
            return Ok(report);
        }
    }

    progress(Phase::Lookup);
    let lookup = async {
        let (root, source) = load_root(false).await?;
        verify_digest_keeping_bundle(
            sha,
            claimed.map(str::to_string),
            Vec::new(),
            &Options::default(),
            providers,
            &root,
            source,
        )
        .await
    };
    let (report, bundle) = tokio::select! {
        r = lookup => r.map_err(JobError::Failed)?,
        () = cancelled(cancel) => return Err(JobError::Cancelled),
    };
    if let Some(cache) = cache {
        remember(cache, &report, bundle.as_ref());
    }
    Ok(report)
}

async fn from_cache<F, Fut>(
    cache: &Cache,
    sha: &str,
    claimed: Option<&str>,
    load_root: &F,
) -> Option<Report>
where
    F: Fn(bool) -> Fut,
    Fut: Future<Output = Result<(TrustedRoot, TrustRootSource), String>>,
{
    let stored = cache.get(sha, claimed, now_secs())?;
    // Without a usable root the entry cannot be judged, so it is left alone for a later try.
    let (root, source) = load_root(true).await.ok()?;
    let from_disk: Vec<Box<dyn Provider>> = vec![Box::new(StoredBundle::new(
        &stored.provider,
        stored.bundle,
        stored.attestation_id,
    ))];
    let mut report = verify_digest(
        sha,
        claimed.map(str::to_string),
        Vec::new(),
        &Options::default(),
        &from_disk,
        &root,
        source,
    )
    .await
    .ok()?;
    if matches!(report.status, Status::Verified | Status::Mismatch) {
        report
            .notes
            .push(format!("{CACHED_NOTE}{}.", format_epoch(stored.fetched_at)));
        Some(report)
    } else {
        let _ = cache.remove(sha);
        None
    }
}

/// Only a bundle that just verified is stored; a definite "none" drops what was kept. A cache that
/// cannot be written only costs a repeat lookup.
fn remember(cache: &Cache, report: &Report, bundle: Option<&provenance_core::Bundle>) {
    match (report.status, bundle) {
        (Status::Verified | Status::Mismatch, Some(bundle)) => {
            let _ = cache.put(
                &report.file_sha256,
                report.queried_repo.as_deref(),
                report.provider.as_deref().unwrap_or("github"),
                report.attestation_id,
                bundle,
                now_secs(),
            );
        }
        (Status::NoAttestation, _) => {
            let _ = cache.remove(&report.file_sha256);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use provenance_core::{load_embedded_root, Bundle, Fetched, ProviderError};
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    const BUNDLE: &str = include_str!(
        "../../provenance-core/tests/fixtures/cli-2.102.0-windows-amd64-zip.bundle.json"
    );
    /// The SHA-256 the fixture bundle attests.
    const SHA: &str = "ae64e556ecc240b200f7eba60d550e4bb60d78e860e69dd88c449405b86067f4";
    const OTHER: &str = "00000000000000000000000000000000000000000000000000000000000000aa";

    struct Counting {
        calls: Arc<AtomicUsize>,
        answer: fn() -> Result<Fetched, ProviderError>,
    }

    #[async_trait::async_trait]
    impl Provider for Counting {
        fn name(&self) -> &'static str {
            "github"
        }
        async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.answer)()
        }
    }

    fn serves_the_bundle() -> Result<Fetched, ProviderError> {
        Ok(Fetched {
            bundles: vec![Bundle::from_json(BUNDLE).unwrap()],
            attestation_ids: vec![Some(53_806_567)],
            ..Fetched::default()
        })
    }

    fn serves_nothing() -> Result<Fetched, ProviderError> {
        Ok(Fetched::default())
    }

    fn counting(
        answer: fn() -> Result<Fetched, ProviderError>,
    ) -> (Vec<Box<dyn Provider>>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Counting {
            calls: calls.clone(),
            answer,
        })];
        (providers, calls)
    }

    /// Stands in for "no network": any use fails the test.
    struct Panics;

    #[async_trait::async_trait]
    impl Provider for Panics {
        fn name(&self) -> &'static str {
            "panics"
        }
        async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            panic!("a cache hit must not ask a provider");
        }
    }

    struct Hang;

    #[async_trait::async_trait]
    impl Provider for Hang {
        fn name(&self) -> &'static str {
            "hang"
        }
        async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            std::future::pending().await
        }
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ssx-job-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn root(_offline: bool) -> Result<(TrustedRoot, TrustRootSource), String> {
        load_embedded_root()
    }

    /// Fails the test if anything needs the network-backed trust root.
    async fn offline_root(offline: bool) -> Result<(TrustedRoot, TrustRootSource), String> {
        assert!(offline, "tried to load the trust root over the network");
        load_embedded_root()
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    fn resolve_with<F, Fut>(
        sha: &str,
        claimed: Option<&str>,
        refresh: bool,
        cache: &Cache,
        providers: &[Box<dyn Provider>],
        load_root: F,
    ) -> Report
    where
        F: Fn(bool) -> Fut,
        Fut: Future<Output = Result<(TrustedRoot, TrustRootSource), String>>,
    {
        let cancel = AtomicBool::new(false);
        rt().block_on(resolve(
            sha,
            claimed,
            refresh,
            Some(cache),
            providers,
            load_root,
            &cancel,
            &|_| {},
        ))
        .unwrap()
    }

    fn entry_file(cache_dir: &Path, sha: &str) -> std::path::PathBuf {
        cache_dir.join(format!("{sha}.json"))
    }

    fn seed(cache: &Cache, sha: &str, repo: Option<&str>, fetched_at: u64) {
        let bundle = Bundle::from_json(BUNDLE).unwrap();
        cache
            .put(sha, repo, "github", Some(53_806_567), &bundle, fetched_at)
            .unwrap();
    }

    fn edit_entry(dir: &Path, sha: &str, edit: impl FnOnce(&mut serde_json::Value)) {
        let path = entry_file(dir, sha);
        let mut v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        edit(&mut v);
        std::fs::write(path, v.to_string()).unwrap();
    }

    fn flip(v: &mut serde_json::Value, pointer: &str) {
        let text = v.pointer(pointer).unwrap().as_str().unwrap().to_string();
        let mid = text.len() / 2;
        let replacement = if text.as_bytes()[mid] == b'A' {
            "B"
        } else {
            "A"
        };
        let tampered = format!("{}{replacement}{}", &text[..mid], &text[mid + 1..]);
        *v.pointer_mut(pointer).unwrap() = tampered.into();
    }

    #[test]
    fn a_verified_lookup_stores_the_bundle_and_the_next_one_is_served_from_it() {
        let dir = tmp("hit");
        let cache = Cache::new(dir.join("cache"));
        let (providers, calls) = counting(serves_the_bundle);

        let first = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(first.status, Status::Verified);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!first.notes.iter().any(|n| n.starts_with(CACHED_NOTE)));
        assert!(entry_file(&dir.join("cache"), SHA).exists());

        let second = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(second.status, Status::Verified);
        assert!(second.notes.iter().any(|n| n.starts_with(CACHED_NOTE)));
        assert_eq!(second.identity.unwrap().repo.as_deref(), Some("cli/cli"));
        assert_eq!(second.attestation_id, Some(53_806_567));
        assert_eq!(second.log_index, Some(3_010_358_693));
        assert_eq!(second.provider.as_deref(), Some("github"));
        assert_eq!(second.rate_limit, None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_cache_hit_verifies_an_old_bundle_with_no_network_and_no_provider() {
        let dir = tmp("offline");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Panics)];
        // The fixture's Fulcio certificate expired long ago; the signed log time vouches for it.
        let r = resolve_with(
            SHA,
            Some("cli/cli"),
            false,
            &cache,
            &providers,
            offline_root,
        );
        assert_eq!(r.status, Status::Verified);
        assert_eq!(
            r.identity.unwrap().commit.as_deref(),
            Some("fc4b137cdef0a6bd28fd461b7cf9c84a5812a8cd")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_valid_bundle_whose_certificate_names_another_repo_is_a_mismatch() {
        let dir = tmp("mismatch");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("evil/repo"), now_secs());
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Panics)];
        let r = resolve_with(
            SHA,
            Some("evil/repo"),
            false,
            &cache,
            &providers,
            offline_root,
        );
        assert_eq!(r.status, Status::Mismatch);
        assert_eq!(r.identity.unwrap().repo.as_deref(), Some("cli/cli"));
        assert!(entry_file(&dir.join("cache"), SHA).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn assert_forgery_is_dropped(name: &str, edit: impl FnOnce(&mut serde_json::Value)) {
        let dir = tmp(name);
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        edit_entry(&dir.join("cache"), SHA, edit);
        let (providers, calls) = counting(serves_nothing);
        let r = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(
            r.status,
            Status::NoAttestation,
            "{name}: not served from disk"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "{name}: went online");
        assert!(
            !entry_file(&dir.join("cache"), SHA).exists(),
            "{name}: not deleted"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn forged_entries_are_ignored_deleted_and_looked_up_online() {
        assert_forgery_is_dropped("cert", |v| {
            flip(v, "/bundle/verificationMaterial/certificate/rawBytes")
        });
        assert_forgery_is_dropped("payload", |v| flip(v, "/bundle/dsseEnvelope/payload"));
        assert_forgery_is_dropped("signature", |v| {
            flip(v, "/bundle/dsseEnvelope/signatures/0/sig")
        });
        assert_forgery_is_dropped("verdict fields", |v| {
            v["status"] = "verified".into();
            v["identity"] = serde_json::json!({"repo": "evil/repo"});
        });
        assert_forgery_is_dropped("emptied bundle", |v| v["bundle"] = serde_json::json!({}));
    }

    #[test]
    fn edited_bookkeeping_cannot_change_what_the_certificate_says() {
        let dir = tmp("bookkeeping");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        edit_entry(&dir.join("cache"), SHA, |v| {
            v["provider"] = "rekor-v1".into();
            v["attestation_id"] = 1.into();
        });
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Panics)];
        let r = resolve_with(
            SHA,
            Some("cli/cli"),
            false,
            &cache,
            &providers,
            offline_root,
        );
        assert_eq!(r.status, Status::Verified);
        assert_eq!(r.identity.unwrap().repo.as_deref(), Some("cli/cli"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_bundle_filed_under_another_files_hash_does_not_verify() {
        let dir = tmp("other-hash");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, OTHER, Some("cli/cli"), now_secs());
        let (providers, calls) = counting(serves_nothing);
        let r = resolve_with(OTHER, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(r.status, Status::NoAttestation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!entry_file(&dir.join("cache"), OTHER).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_truncated_entry_file_is_ignored_and_deleted() {
        let dir = tmp("truncated");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        let path = entry_file(&dir.join("cache"), SHA);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::write(&path, &text[..text.len() / 2]).unwrap();
        let (providers, calls) = counting(serves_nothing);
        let r = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(r.status, Status::NoAttestation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_planted_old_format_verdict_is_never_read() {
        let dir = tmp("legacy");
        let cache_dir = dir.join("cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let verdict = format!(
            r#"{{"schema":1,"cached_at":{},"report":{{"status":"verified","file_sha256":"{SHA}"}}}}"#,
            now_secs()
        );
        std::fs::write(cache_dir.join(format!("{SHA}-r0.json")), &verdict).unwrap();
        std::fs::write(entry_file(&cache_dir, SHA), &verdict).unwrap();
        let cache = Cache::new(cache_dir);
        let (providers, calls) = counting(serves_nothing);
        let r = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(r.status, Status::NoAttestation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_entry_past_the_week_is_looked_up_again_and_replaced() {
        let dir = tmp("stale");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs() - 8 * 24 * 3600);
        let (providers, calls) = counting(serves_the_bundle);
        let r = resolve_with(SHA, Some("cli/cli"), false, &cache, &providers, root);
        assert_eq!(r.status, Status::Verified);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(cache.get(SHA, Some("cli/cli"), now_secs()).is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn check_online_skips_the_cache_and_replaces_the_bundle() {
        let dir = tmp("refresh");
        let cache = Cache::new(dir.join("cache"));
        let old = now_secs() - 3600;
        seed(&cache, SHA, Some("cli/cli"), old);
        let (providers, calls) = counting(serves_the_bundle);
        let r = resolve_with(SHA, Some("cli/cli"), true, &cache, &providers, root);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(r.status, Status::Verified);
        assert!(!r.notes.iter().any(|n| n.starts_with(CACHED_NOTE)));
        let stored = cache.get(SHA, Some("cli/cli"), now_secs()).unwrap();
        assert!(stored.fetched_at > old);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_online_answer_of_none_drops_the_cached_bundle() {
        let dir = tmp("withdrawn");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        let (providers, _) = counting(serves_nothing);
        let r = resolve_with(SHA, Some("cli/cli"), true, &cache, &providers, root);
        assert_eq!(r.status, Status::NoAttestation);
        assert!(!entry_file(&dir.join("cache"), SHA).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_entry_that_cannot_be_judged_without_a_root_is_kept() {
        let dir = tmp("no-root");
        let cache = Cache::new(dir.join("cache"));
        seed(&cache, SHA, Some("cli/cli"), now_secs());
        let (providers, calls) = counting(serves_nothing);
        let r = resolve_with(
            SHA,
            Some("cli/cli"),
            false,
            &cache,
            &providers,
            |offline| async move {
                if offline {
                    Err("no root".to_string())
                } else {
                    load_embedded_root()
                }
            },
        );
        assert_eq!(r.status, Status::NoAttestation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn no_attestation_and_failures_are_never_cached() {
        let dir = tmp("neg");
        let cache = Cache::new(dir.join("cache"));
        let (empty, _) = counting(serves_nothing);
        let (failing, _) = counting(|| Err(ProviderError::failed("offline")));
        let (bad, _) = counting(|| {
            let mut fetched = serves_the_bundle().unwrap();
            fetched.bundles.clear();
            fetched.attestation_ids.clear();
            Ok(fetched)
        });
        for providers in [&empty, &failing, &bad] {
            resolve_with(SHA, Some("cli/cli"), false, &cache, providers, root);
        }
        // A bundle that does not verify for this file is not stored either.
        let (wrong_file, _) = counting(serves_the_bundle);
        let r = resolve_with(OTHER, Some("cli/cli"), false, &cache, &wrong_file, root);
        assert_eq!(r.status, Status::VerificationFailed);
        assert!(!dir.join("cache").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn file_job(dir: &Path) -> std::path::PathBuf {
        let file = dir.join("a.exe");
        std::fs::write(&file, b"abc").unwrap();
        file
    }

    #[test]
    fn run_hashes_the_file_and_reports_hashing_then_lookup() {
        let dir = tmp("run");
        let file = file_job(&dir);
        let (providers, _) = counting(serves_nothing);
        let input = JobInput {
            path: &file,
            claimed_repo: Some("a/b".into()),
            refresh: false,
        };
        let cancel = AtomicBool::new(false);
        let phases = Mutex::new(Vec::new());
        let progress = |p| phases.lock().unwrap().push(p);
        let r = rt()
            .block_on(run(&input, None, &providers, root, &cancel, &progress))
            .unwrap();
        assert_eq!(r.status, Status::NoAttestation);
        assert_eq!(
            r.file_sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(phases.lock().unwrap().contains(&Phase::Lookup));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancel_before_hashing_finishes() {
        let dir = tmp("cancel-hash");
        let file = file_job(&dir);
        let input = JobInput {
            path: &file,
            claimed_repo: None,
            refresh: false,
        };
        let cancel = AtomicBool::new(true);
        let r = rt().block_on(run(&input, None, &[], root, &cancel, &|_| {}));
        assert_eq!(r.unwrap_err(), JobError::Cancelled);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancel_aborts_a_hanging_lookup() {
        let dir = tmp("cancel-net");
        let file = file_job(&dir);
        let input = JobInput {
            path: &file,
            claimed_repo: Some("a/b".into()),
            refresh: false,
        };
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Hang)];
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let progress = move |p| {
            if p == Phase::Lookup {
                let flag = flag.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    flag.store(true, Ordering::Relaxed);
                });
            }
        };
        let r = rt().block_on(run(&input, None, &providers, root, &cancel, &progress));
        assert_eq!(r.unwrap_err(), JobError::Cancelled);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_is_a_failure_not_a_panic() {
        let input = JobInput {
            path: Path::new("/nonexistent/a.exe"),
            claimed_repo: None,
            refresh: false,
        };
        let cancel = AtomicBool::new(false);
        let r = rt().block_on(run(&input, None, &[], root, &cancel, &|_| {}));
        assert!(matches!(r, Err(JobError::Failed(_))));
    }
}
