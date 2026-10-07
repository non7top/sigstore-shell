fn main() {
    println!("cargo:rerun-if-env-changed=PROVENANCE_REPO");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Only release builds set this, so a local build does not claim to come from the repo's workflow.
    if let Ok(repo) = std::env::var("PROVENANCE_REPO") {
        res.set("ProvenanceRepo", &repo);
    }
    res.compile().expect("compile version resource");
}
