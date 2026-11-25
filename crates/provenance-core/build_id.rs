#![allow(dead_code)]
//! Shared by the build scripts: the release version and commit baked into each binary.

use std::process::Command;

fn env_nonempty(name: &str) -> Option<String> {
    println!("cargo:rerun-if-env-changed={name}");
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The release tag without its "v"; None for a local build.
pub fn release_version() -> Option<String> {
    env_nonempty("RELEASE_VERSION")
}

/// Where the release workflow publishes the cosign bundle of the release asset `asset`; None unless
/// both PROVENANCE_REPO and RELEASE_VERSION are set, as in a release build.
pub fn bundle_url(asset: &str) -> Option<String> {
    let repo = env_nonempty("PROVENANCE_REPO")?;
    let version = release_version()?;
    Some(format!(
        "https://github.com/{repo}/releases/download/v{version}/{asset}.cosign.bundle"
    ))
}

/// RELEASE_COMMIT, else the checked-out commit, else "unknown" (a build without .git).
pub fn commit() -> String {
    if let Some(commit) = env_nonempty("RELEASE_COMMIT") {
        return commit;
    }
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        println!("cargo:rerun-if-changed={dir}/HEAD");
        if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
            let path = format!("{dir}/{branch}");
            if std::path::Path::new(&path).exists() {
                println!("cargo:rerun-if-changed={path}");
            }
        }
    }
    git(&["rev-parse", "HEAD"])
        .filter(|c| c.len() == 40 && c.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".to_string())
}

/// The commit as a version-resource value, or None when it is not known.
pub fn known_commit() -> Option<String> {
    Some(commit()).filter(|c| c != "unknown")
}
