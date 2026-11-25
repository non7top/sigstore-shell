include!(concat!(env!("OUT_DIR"), "/elf_claim.rs"));

use clap::Parser;
use provenance_core::{
    load_trusted_root, read_bundle_url_file, verdict_text, verify_file, BundleFile, GithubProvider,
    Options, Provider, RekorProvider, Report, Status, WebBundle,
};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    version = provenance_core::VERSION_LONG,
    about = "Check where an exe came from, using its Sigstore attestation"
)]
enum Cli {
    /// Look up and verify the attestation for a file
    Verify {
        file: PathBuf,
        /// owner/repo to ask GitHub about (default: ProvenanceRepo in the file)
        #[arg(long)]
        repo: Option<String>,
        /// Verify against this bundle file instead of asking GitHub (no network lookup)
        #[arg(long)]
        bundle: Option<PathBuf>,
        /// Verify against a bundle downloaded from this https URL instead of asking GitHub (sends a request to that host)
        #[arg(long)]
        bundle_url: Option<String>,
        /// Also download the bundle that the file itself names (its embedded link), after the other lookups (sends a request to that host)
        #[arg(long)]
        web_bundle: bool,
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
    let verdict = verdict_text(report, None);
    let headline = match report.status {
        Status::Verified | Status::Mismatch | Status::NoAttestation => {
            verdict.clone().unwrap_or_default()
        }
        Status::LookupFailed => {
            "LOOKUP FAILED (could not tell whether an attestation exists)".to_string()
        }
        Status::VerificationFailed => "ATTESTATION DID NOT VERIFY".to_string(),
        Status::NotChecked => {
            "NOT CHECKED (no repo to ask about; pass --repo, --bundle, --bundle-url, --web-bundle or --rekor)"
                .to_string()
        }
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
    for note in report
        .notes
        .iter()
        .filter(|n| verdict.as_deref() != Some(n.as_str()))
    {
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
        bundle,
        bundle_url,
        web_bundle,
        rekor,
        json,
    } = Cli::parse();
    let token = std::env::var("GH_TOKEN")
        .ok()
        .filter(|t| !t.is_empty())
        .or_else(|| std::env::var("GITHUB_TOKEN").ok().filter(|t| !t.is_empty()));
    let mut providers: Vec<Box<dyn Provider>> = Vec::new();
    let explicit = bundle.is_some() || bundle_url.is_some();
    if let Some(path) = bundle {
        providers.push(Box::new(BundleFile::new(path)));
    }
    if let Some(url) = bundle_url {
        let web = WebBundle::new(&url)?;
        eprintln!("note: downloading the bundle from {}", web.host());
        providers.push(Box::new(web));
    }
    if !explicit {
        providers.push(Box::new(GithubProvider::new(token)));
        if let Some(sidecar) = BundleFile::beside(&file) {
            providers.push(Box::new(sidecar));
        }
    }
    if web_bundle {
        match read_bundle_url_file(&file) {
            Ok(Some(url)) => match WebBundle::new(&url) {
                Ok(web) => {
                    eprintln!(
                        "note: --web-bundle downloads a bundle from {} only if the lookups before it do not verify the file",
                        web.host()
                    );
                    providers.push(Box::new(web));
                }
                Err(e) => eprintln!("note: --web-bundle skipped, the file's link is unusable: {e}"),
            },
            _ => eprintln!("note: --web-bundle skipped, the file embeds no bundle link"),
        }
    }
    if rekor {
        providers.push(Box::new(RekorProvider::default()));
    }
    let (root, source) = load_trusted_root().await?;
    let report = verify_file(&file, &Options { repo }, &providers, &root, source).await?;
    if json {
        let mut value = serde_json::to_value(&report).map_err(|e| e.to_string())?;
        value["built_by"] = serde_json::json!({
            "name": "sigstore-shell-cli",
            "version": provenance_core::VERSION,
            "commit": provenance_core::COMMIT,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
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
