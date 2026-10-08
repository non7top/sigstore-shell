include!(concat!(env!("OUT_DIR"), "/elf_claim.rs"));

use clap::Parser;
use provenance_core::{
    load_trusted_root, verify_file, GithubProvider, Options, Provider, RekorProvider, Report,
    Status,
};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version,
    about = "Check where an exe came from, using its Sigstore attestation"
)]
enum Cli {
    /// Look up and verify the attestation for a file
    Verify {
        file: PathBuf,
        /// owner/repo to ask GitHub about (default: ProvenanceRepo in the file)
        #[arg(long)]
        repo: Option<String>,
        /// Also search the public Rekor v1 log by hash (sends the hash to a second service)
        #[arg(long)]
        rekor: bool,
        #[arg(long)]
        json: bool,
    },
}

fn exit_code(status: Status) -> u8 {
    match status {
        Status::Verified => 0,
        Status::NoAttestation => 10,
        Status::Mismatch => 11,
        Status::LookupFailed => 12,
        Status::VerificationFailed => 13,
        Status::NotChecked => 14,
    }
}

fn print_text(report: &Report) {
    let headline = match report.status {
        Status::Verified => "VERIFIED",
        Status::NoAttestation => "NO ATTESTATION FOUND (this does not mean the file is unsafe)",
        Status::Mismatch => "MISMATCH: certificate names a different repo than claimed",
        Status::LookupFailed => "LOOKUP FAILED (could not tell whether an attestation exists)",
        Status::VerificationFailed => "ATTESTATION DID NOT VERIFY",
        Status::NotChecked => "NOT CHECKED (no repo to ask about; pass --repo or --rekor)",
    };
    println!("{headline}");
    println!("sha256:  {}", report.file_sha256);
    match &report.claimed_repo {
        Some(c) => println!("claimed: {c} (embedded in the file, not verified)"),
        None => println!("claimed: none"),
    }
    if let Some(p) = &report.provider {
        println!("answered by: {p}");
    }
    if let Some(id) = &report.identity {
        println!("-- from the certificate --");
        let rows = [
            ("repo", &id.repo),
            ("owner", &id.owner),
            ("workflow", &id.workflow),
            ("ref", &id.git_ref),
            ("commit", &id.commit),
            ("signed", &id.signed_at),
            ("trigger", &id.trigger),
            ("runner", &id.runner),
            ("run", &id.run_url),
            ("issuer", &id.issuer),
            ("signer", &id.san),
        ];
        for (name, value) in rows {
            if let Some(v) = value {
                println!("{name:<9}{v}");
            }
        }
    }
    for note in &report.notes {
        println!("note: {note}");
    }
    if let Some(r) = &report.rate_limit {
        println!(
            "rate limit: {}/{} remaining ({}), resets at epoch {}",
            r.remaining.map_or("?".into(), |v| v.to_string()),
            r.limit.map_or("?".into(), |v| v.to_string()),
            r.resource.as_deref().unwrap_or("?"),
            r.reset_epoch.map_or("?".into(), |v| v.to_string()),
        );
    }
}

async fn run() -> Result<u8, String> {
    let Cli::Verify {
        file,
        repo,
        rekor,
        json,
    } = Cli::parse();
    let token = std::env::var("GH_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()));
    let mut providers: Vec<Box<dyn Provider>> = vec![Box::new(GithubProvider::new(token))];
    if rekor {
        providers.push(Box::new(RekorProvider::default()));
    }
    let (root, source) = load_trusted_root().await?;
    let report = verify_file(&file, &Options { repo }, &providers, &root, source).await?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?
        );
    } else {
        print_text(&report);
    }
    Ok(exit_code(report.status))
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}
