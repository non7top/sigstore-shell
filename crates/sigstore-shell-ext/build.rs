#[path = "../provenance-core/src/claim_format.rs"]
mod claim_format;

fn main() {
    println!("cargo:rerun-if-env-changed=PROVENANCE_REPO");
    println!("cargo:rerun-if-env-changed=RELEASE_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Only release builds set this, so a local build does not claim to come from the repo's workflow.
    if let Ok(repo) = std::env::var("PROVENANCE_REPO") {
        res.set(claim_format::PE_KEY, &repo);
    }
    // The crate version stays fixed: release-please cannot update Cargo.lock, and --locked builds need it to match.
    if let Ok(version) = std::env::var("RELEASE_VERSION") {
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
    }
    res.compile().expect("compile version resource");
}
