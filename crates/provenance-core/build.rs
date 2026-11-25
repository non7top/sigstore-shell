#[path = "build_id.rs"]
mod build_id;

fn main() {
    // The crate version stays fixed: release-please cannot update Cargo.lock, and --locked builds need it to match.
    let version =
        build_id::release_version().unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap());
    println!("cargo:rustc-env=SIGSTORE_SHELL_VERSION={version}");
    println!(
        "cargo:rustc-env=SIGSTORE_SHELL_COMMIT={}",
        build_id::commit()
    );
}
