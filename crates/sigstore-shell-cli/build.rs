#[path = "../provenance-core/build_id.rs"]
mod build_id;
#[path = "../provenance-core/src/claim_format.rs"]
mod claim_format;

use std::path::PathBuf;

/// The note bytes, laid out as in project.md ("Embedded claim"), in the target's byte order.
fn note(name: &str, value: &str, big: bool) -> Vec<u8> {
    let word = |n: usize| {
        let n = u32::try_from(n).expect("note field fits u32");
        if big {
            n.to_be_bytes()
        } else {
            n.to_le_bytes()
        }
    };
    let pad = |v: &mut Vec<u8>| v.resize(v.len().div_ceil(4) * 4, 0);
    let mut b = Vec::new();
    b.extend(word(name.len() + 1));
    b.extend(word(value.len()));
    b.extend(word(claim_format::ELF_NOTE_TYPE as usize));
    b.extend(name.as_bytes());
    b.push(0);
    pad(&mut b);
    b.extend(value.as_bytes());
    pad(&mut b);
    b
}

fn main() {
    println!("cargo:rerun-if-env-changed=PROVENANCE_REPO");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let repo = std::env::var("PROVENANCE_REPO").ok();
    // Always written, empty off Linux or without a repo, so main.rs can include! it unconditionally.
    let mut src = String::new();
    if os == "linux" {
        if let Some(repo) = &repo {
            let big = std::env::var("CARGO_CFG_TARGET_ENDIAN").as_deref() == Ok("big");
            let mut bytes = note(claim_format::ELF_NOTE_NAME, repo, big);
            if let Some(url) = build_id::bundle_url("sigstore-shell-cli") {
                bytes.extend(note(claim_format::ELF_BUNDLE_NOTE_NAME, &url, big));
            }
            src = format!(
                "#[repr(C, align(4))]\nstruct Note([u8; {len}]);\n\
                 #[used]\n#[link_section = \"{section}\"]\nstatic PROVENANCE_NOTE: Note = Note({bytes:?});\n",
                len = bytes.len(),
                section = claim_format::ELF_SECTION,
            );
        }
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("elf_claim.rs");
    std::fs::write(out, src).expect("write elf_claim.rs");
    if os != "windows" {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    // Only release builds set this, so a local build does not claim to come from the repo's workflow.
    if let Some(repo) = &repo {
        res.set(claim_format::PE_KEY, repo);
    }
    if let Some(url) = build_id::bundle_url("sigstore-shell-cli.exe") {
        res.set(claim_format::PE_BUNDLE_KEY, &url);
    }
    // The crate version stays fixed: release-please cannot update Cargo.lock, and --locked builds need it to match.
    if let Some(version) = build_id::release_version() {
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
    }
    if let Some(commit) = build_id::known_commit() {
        res.set("BuildCommit", &commit);
    }
    res.compile().expect("compile version resource");
}
