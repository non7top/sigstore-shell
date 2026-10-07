//! Writes a minimal PE whose version resource has `ProvenanceRepo`, for smoke tests.
//! Usage: fixture_exe <out.exe> <owner/repo>

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, out, repo] = args.as_slice() else {
        eprintln!("usage: fixture_exe <out.exe> <owner/repo>");
        std::process::exit(2);
    };
    let pe = provenance_core::fixture::pe_with_version_strings(&[("ProvenanceRepo", repo)]);
    std::fs::write(out, pe).expect("write fixture");
}
