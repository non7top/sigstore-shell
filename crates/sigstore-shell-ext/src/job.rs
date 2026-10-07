use crate::cache::Cache;
use crate::model::{format_epoch, Phase};
use provenance_core::{
    sha256_file_with, verify_digest, Options, Provider, Report, TrustRootSource, TrustedRoot,
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
    pub rekor: bool,
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

/// Hash, consult the cache, then look up and verify. Checks `cancel` between hash chunks
/// and aborts an in-flight lookup when it is set.
pub async fn run<F, Fut>(
    input: &JobInput<'_>,
    cache: Option<&Cache>,
    providers: &[Box<dyn Provider>],
    load_root: F,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Phase) + Sync),
) -> Result<Report, JobError>
where
    F: FnOnce() -> Fut,
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

    let claimed = input.claimed_repo.as_deref();
    if let Some((mut report, cached_at)) =
        cache.and_then(|c| c.get(&sha, claimed, input.rekor, now_secs()))
    {
        report.notes.push(format!(
            "Result cached from a check on {}.",
            format_epoch(cached_at)
        ));
        return Ok(report);
    }

    progress(Phase::Lookup);
    let lookup = async {
        let (root, source) = load_root().await?;
        verify_digest(
            &sha,
            input.claimed_repo.clone(),
            Vec::new(),
            &Options::default(),
            providers,
            &root,
            source,
        )
        .await
    };
    let report = tokio::select! {
        r = lookup => r.map_err(JobError::Failed)?,
        () = cancelled(cancel) => return Err(JobError::Cancelled),
    };
    if let Some(cache) = cache {
        // A cache that cannot be written only costs a repeat lookup.
        let _ = cache.put(&report, input.rekor, now_secs());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use provenance_core::{load_embedded_root, Fetched, ProviderError, Status};
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    struct Counting {
        calls: Arc<AtomicUsize>,
        answer: fn() -> Result<Fetched, ProviderError>,
    }

    #[async_trait::async_trait]
    impl Provider for Counting {
        fn name(&self) -> &'static str {
            "mock"
        }
        async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            (self.answer)()
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

    async fn root() -> Result<(TrustedRoot, TrustRootSource), String> {
        load_embedded_root()
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
    }

    #[test]
    fn second_run_is_served_from_cache_without_a_lookup() {
        let dir = tmp("cache");
        let file = dir.join("a.exe");
        std::fs::write(&file, b"abc").unwrap();
        let cache = Cache::new(dir.join("cache"));
        let calls = Arc::new(AtomicUsize::new(0));
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Counting {
            calls: calls.clone(),
            answer: || Ok(Fetched::default()),
        })];
        let input = JobInput {
            path: &file,
            claimed_repo: Some("a/b".into()),
            rekor: false,
        };
        let cancel = AtomicBool::new(false);
        let phases = Mutex::new(Vec::new());
        let progress = |p| phases.lock().unwrap().push(p);
        let run_once = || {
            rt().block_on(run(
                &input,
                Some(&cache),
                &providers,
                root,
                &cancel,
                &progress,
            ))
        };

        let first = run_once().unwrap();
        assert_eq!(first.status, Status::NoAttestation);
        assert_eq!(
            first.file_sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(phases.lock().unwrap().contains(&Phase::Lookup));

        let second = run_once().unwrap();
        assert_eq!(second.status, Status::NoAttestation);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(second.notes.iter().any(|n| n.starts_with("Result cached")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn lookup_failure_is_not_cached() {
        let dir = tmp("fail");
        let file = dir.join("a.exe");
        std::fs::write(&file, b"abc").unwrap();
        let cache = Cache::new(dir.join("cache"));
        let calls = Arc::new(AtomicUsize::new(0));
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(Counting {
            calls: calls.clone(),
            answer: || Err(ProviderError::failed("offline")),
        })];
        let input = JobInput {
            path: &file,
            claimed_repo: Some("a/b".into()),
            rekor: false,
        };
        let cancel = AtomicBool::new(false);
        for _ in 0..2 {
            let r = rt()
                .block_on(run(
                    &input,
                    Some(&cache),
                    &providers,
                    root,
                    &cancel,
                    &|_| {},
                ))
                .unwrap();
            assert_eq!(r.status, Status::LookupFailed);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancel_before_hashing_finishes() {
        let dir = tmp("cancel-hash");
        let file = dir.join("a.exe");
        std::fs::write(&file, b"abc").unwrap();
        let input = JobInput {
            path: &file,
            claimed_repo: None,
            rekor: false,
        };
        let cancel = AtomicBool::new(true);
        let r = rt().block_on(run(&input, None, &[], root, &cancel, &|_| {}));
        assert_eq!(r.unwrap_err(), JobError::Cancelled);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cancel_aborts_a_hanging_lookup() {
        let dir = tmp("cancel-net");
        let file = dir.join("a.exe");
        std::fs::write(&file, b"abc").unwrap();
        let input = JobInput {
            path: &file,
            claimed_repo: Some("a/b".into()),
            rekor: false,
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
            rekor: false,
        };
        let cancel = AtomicBool::new(false);
        let r = rt().block_on(run(&input, None, &[], root, &cancel, &|_| {}));
        assert!(matches!(r, Err(JobError::Failed(_))));
    }
}
