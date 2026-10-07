use provenance_core::{valid_repo, Identity, PeError, Report, Status};

/// What the file says about itself. Never trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Absent,
    Repo(String),
    Invalid(String),
    Unreadable,
}

impl Claim {
    pub fn from_read(read: Result<Option<String>, PeError>) -> Self {
        match read {
            Ok(None) => Self::Absent,
            Ok(Some(c)) if valid_repo(&c) => Self::Repo(c),
            Ok(Some(c)) => Self::Invalid(c),
            Err(_) => Self::Unreadable,
        }
    }

    pub fn repo(&self) -> Option<&str> {
        match self {
            Self::Repo(r) => Some(r),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Hashing { percent: u8 },
    Lookup,
}

#[derive(Debug, Clone)]
pub enum State {
    Idle,
    Running { job: u64, phase: Phase },
    Done(Box<Report>),
    Cancelled,
    Failed(String),
}

#[derive(Debug)]
pub enum Event {
    Start {
        job: u64,
    },
    Progress {
        job: u64,
        phase: Phase,
    },
    Finished {
        job: u64,
        result: Result<Box<Report>, String>,
    },
    Cancel,
}

impl State {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }

    /// Events for a job other than the running one are dropped, so a cancelled job's late
    /// result cannot overwrite a newer state.
    pub fn apply(self, event: Event) -> State {
        match (self, event) {
            (state @ State::Running { .. }, Event::Start { .. }) => state,
            (_, Event::Start { job }) => State::Running {
                job,
                phase: Phase::Hashing { percent: 0 },
            },
            (State::Running { job, .. }, Event::Progress { job: j, phase }) if job == j => {
                State::Running { job, phase }
            }
            (State::Running { job, .. }, Event::Finished { job: j, result }) if job == j => {
                match result {
                    Ok(report) => State::Done(report),
                    Err(e) => State::Failed(e),
                }
            }
            (State::Running { .. }, Event::Cancel) => State::Cancelled,
            (state, _) => state,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    pub claim_line: String,
    pub headline: String,
    pub rows: Vec<(String, String)>,
    pub notes: Vec<String>,
    pub consent: String,
    pub can_verify: bool,
    pub running: bool,
    pub progress_text: String,
}

impl View {
    /// Text for the read-only details box, with CRLF as Win32 edit controls expect.
    pub fn details_text(&self) -> String {
        let rows = self.rows.iter().map(|(k, v)| format!("{k}: {v}"));
        let notes = self.notes.iter().map(|n| format!("Note: {n}"));
        rows.chain(notes).collect::<Vec<_>>().join("\r\n")
    }
}

pub fn provider_label(name: &str) -> &str {
    match name {
        "github" => "GitHub attestations (api.github.com)",
        "rekor-v1" => "Rekor v1 public log (rekor.sigstore.dev)",
        other => other,
    }
}

/// `2026-09-30T12:34:56.789Z` becomes `2026-09-30 12:34:56 UTC`; anything else is shown as is.
pub fn format_time(ts: &str) -> String {
    let Some(body) = ts.strip_suffix('Z') else {
        return ts.to_string();
    };
    let body = body.split('.').next().unwrap_or(body);
    format!("{} UTC", body.replacen('T', " ", 1))
}

pub fn format_epoch(secs: u64) -> String {
    i64::try_from(secs)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .map_or_else(|| secs.to_string(), |t| format_time(&t.to_string()))
}

fn claim_line(claim: &Claim) -> String {
    match claim {
        Claim::Absent => "No provenance information in this file.".into(),
        Claim::Unreadable => {
            "No provenance information in this file (its version information could not be read)."
                .into()
        }
        Claim::Repo(r) => format!("Claimed repository: {r}\r\n(claimed, not verified)"),
        Claim::Invalid(c) => {
            format!("Claimed repository: {c}\r\n(claimed, not verified; not a valid owner/repo)")
        }
    }
}

fn consent(rekor: bool) -> String {
    let mut s = String::from(
        "Pressing Verify hashes this file and sends its SHA-256 hash to GitHub \
         (api.github.com), and downloads Sigstore's public trust root.",
    );
    if rekor {
        s.push_str(
            " Rekor search is enabled in settings.json, so the hash also goes to rekor.sigstore.dev.",
        );
    }
    s
}

fn identity_rows(id: &Identity) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    let mut add = |label: &str, value: &Option<String>| {
        if let Some(v) = value {
            rows.push((label.to_string(), v.clone()));
        }
    };
    add("Repository", &id.repo);
    add("Owner", &id.owner);
    add("Workflow", &id.workflow);
    add("Commit", &id.commit);
    add("Ref", &id.git_ref);
    add("Signed", &id.signed_at.as_deref().map(format_time));
    add("Signer", &id.san);
    rows
}

fn done_view(report: &Report, claim: &Claim) -> (String, Vec<(String, String)>) {
    let mut rows = Vec::new();
    let headline = match report.status {
        Status::Verified => "Verified: this file's Sigstore attestation checks out. \
            The details below come from its signing certificate."
            .to_string(),
        Status::Mismatch => {
            let actual = report
                .identity
                .as_ref()
                .and_then(|i| i.repo.as_deref())
                .unwrap_or("an unknown repository");
            let claimed = claim
                .repo()
                .or(report.queried_repo.as_deref())
                .unwrap_or("?");
            format!(
                "Mismatch: the file claims {claimed}, but its attestation was signed for {actual}."
            )
        }
        Status::NoAttestation => "No attestation found for this file. \
            This does not mean the file is unsafe; most files have none."
            .to_string(),
        Status::LookupFailed => {
            let limited = report
                .rate_limit
                .as_ref()
                .is_some_and(|r| r.remaining == Some(0));
            if limited {
                "Lookup failed: GitHub's request limit is used up. Try again later.".to_string()
            } else {
                "Lookup failed: could not tell whether an attestation exists.".to_string()
            }
        }
        Status::VerificationFailed => {
            "Verification failed: an attestation exists but did not verify. Do not rely on it."
                .to_string()
        }
        Status::NotChecked => "Nothing was looked up.".to_string(),
    };
    if let Some(id) = &report.identity {
        rows.extend(identity_rows(id));
    }
    if let Some(p) = &report.provider {
        rows.push(("Answered by".into(), provider_label(p).into()));
    }
    rows.push(("SHA-256".into(), report.file_sha256.clone()));
    (headline, rows)
}

pub fn view(claim: &Claim, state: &State, rekor: bool) -> View {
    let can_search = claim.repo().is_some() || rekor;
    let running = state.is_running();
    let mut v = View {
        claim_line: claim_line(claim),
        headline: String::new(),
        rows: Vec::new(),
        notes: Vec::new(),
        consent: consent(rekor),
        can_verify: can_search && !running,
        running,
        progress_text: String::new(),
    };
    match state {
        State::Idle if !can_search => {
            v.headline = "Nothing to look up: this file names no repository and Rekor search \
                is off."
                .into();
        }
        State::Idle => {
            v.headline = "Not checked yet. Press Verify to look up this file's attestation.".into();
        }
        State::Running { phase, .. } => {
            v.headline = "Verifying...".into();
            v.progress_text = match phase {
                Phase::Hashing { percent } => format!("Hashing the file: {percent}%"),
                Phase::Lookup => "Looking up and verifying the attestation".into(),
            };
        }
        State::Done(report) => {
            let (headline, rows) = done_view(report, claim);
            v.headline = headline;
            v.rows = rows;
            v.notes = report.notes.clone();
        }
        State::Cancelled => v.headline = "Cancelled. Nothing was verified.".into(),
        State::Failed(e) => v.headline = format!("Could not verify: {e}"),
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use provenance_core::{RateLimit, TrustRootSource};

    fn report(status: Status) -> Report {
        Report {
            status,
            file_sha256: "ab".repeat(32),
            claimed_repo: Some("cli/cli".into()),
            queried_repo: Some("cli/cli".into()),
            provider: Some("github".into()),
            identity: None,
            rate_limit: None,
            trust_root: TrustRootSource::Tuf,
            notes: vec![],
        }
    }

    fn verified() -> Report {
        Report {
            identity: Some(Identity {
                repo: Some("cli/cli".into()),
                owner: Some("cli".into()),
                workflow: Some(".github/workflows/deployment.yml".into()),
                commit: Some("fc4b137c".into()),
                signed_at: Some("2026-09-30T12:34:56.789Z".into()),
                ..Identity::default()
            }),
            ..report(Status::Verified)
        }
    }

    fn running(job: u64) -> State {
        State::Idle.apply(Event::Start { job })
    }

    #[test]
    fn claim_is_read_and_validated() {
        assert_eq!(Claim::from_read(Ok(None)), Claim::Absent);
        assert_eq!(
            Claim::from_read(Ok(Some("a/b".into()))),
            Claim::Repo("a/b".into())
        );
        assert_eq!(
            Claim::from_read(Ok(Some("a/../b".into()))),
            Claim::Invalid("a/../b".into())
        );
        let err = provenance_core::read_claim(b"junk").unwrap_err();
        assert_eq!(Claim::from_read(Err(err)), Claim::Unreadable);
    }

    #[test]
    fn opening_the_page_shows_the_claim_labelled_unverified() {
        let v = view(
            &Claim::Repo("non7top/promptloom".into()),
            &State::Idle,
            false,
        );
        assert!(v.claim_line.contains("non7top/promptloom"));
        assert!(v.claim_line.contains("claimed, not verified"));
        assert!(v.can_verify);
        assert!(v.rows.is_empty());
    }

    #[test]
    fn no_claim_says_so_and_cannot_verify_without_rekor() {
        let v = view(&Claim::Absent, &State::Idle, false);
        assert_eq!(v.claim_line, "No provenance information in this file.");
        assert!(!v.can_verify);
        assert!(view(&Claim::Absent, &State::Idle, true).can_verify);
    }

    #[test]
    fn an_invalid_claim_is_shown_but_not_queried() {
        let v = view(&Claim::Invalid("x".into()), &State::Idle, false);
        assert!(v.claim_line.contains("not a valid owner/repo"));
        assert!(!v.can_verify);
    }

    #[test]
    fn consent_text_names_rekor_only_when_enabled() {
        assert!(!view(&Claim::Absent, &State::Idle, false)
            .consent
            .contains("rekor"));
        assert!(view(&Claim::Absent, &State::Idle, true)
            .consent
            .contains("rekor.sigstore.dev"));
    }

    #[test]
    fn happy_path_through_the_state_machine() {
        let s = running(1);
        assert!(s.is_running());
        let s = s.apply(Event::Progress {
            job: 1,
            phase: Phase::Hashing { percent: 40 },
        });
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        assert!(v.running && !v.can_verify);
        assert_eq!(v.progress_text, "Hashing the file: 40%");
        let s = s.apply(Event::Finished {
            job: 1,
            result: Ok(Box::new(verified())),
        });
        assert!(matches!(s, State::Done(_)));
    }

    #[test]
    fn verified_view_lists_certificate_identity_and_provider() {
        let s = State::Done(Box::new(verified()));
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        let text = v.details_text();
        assert!(v.headline.starts_with("Verified"));
        assert!(text.contains("Repository: cli/cli"));
        assert!(text.contains("Owner: cli"));
        assert!(text.contains("Workflow: .github/workflows/deployment.yml"));
        assert!(text.contains("Commit: fc4b137c"));
        assert!(text.contains("Signed: 2026-09-30 12:34:56 UTC"));
        assert!(text.contains("Answered by: GitHub attestations"));
        assert!(text.contains("\r\n"));
    }

    #[test]
    fn mismatch_names_both_repos() {
        let r = Report {
            status: Status::Mismatch,
            claimed_repo: Some("evil/repo".into()),
            ..verified()
        };
        let v = view(
            &Claim::Repo("evil/repo".into()),
            &State::Done(Box::new(r)),
            false,
        );
        assert!(v.headline.contains("evil/repo") && v.headline.contains("cli/cli"));
    }

    #[test]
    fn each_failure_status_has_its_own_wording() {
        let h = |s| view(&Claim::Absent, &State::Done(Box::new(report(s))), true).headline;
        assert!(h(Status::NoAttestation).contains("not mean the file is unsafe"));
        assert!(h(Status::LookupFailed).starts_with("Lookup failed"));
        assert!(h(Status::VerificationFailed).starts_with("Verification failed"));
        assert!(h(Status::NotChecked).contains("Nothing was looked up"));
    }

    #[test]
    fn exhausted_rate_limit_is_called_out() {
        let r = Report {
            rate_limit: Some(RateLimit {
                remaining: Some(0),
                ..RateLimit::default()
            }),
            ..report(Status::LookupFailed)
        };
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        assert!(v.headline.contains("request limit"));
    }

    #[test]
    fn cancel_then_late_result_is_ignored() {
        let s = running(1).apply(Event::Cancel);
        assert!(matches!(s, State::Cancelled));
        let s = s.apply(Event::Finished {
            job: 1,
            result: Ok(Box::new(verified())),
        });
        assert!(matches!(s, State::Cancelled));
    }

    #[test]
    fn stale_job_events_are_ignored_after_restart() {
        let s = running(1)
            .apply(Event::Cancel)
            .apply(Event::Start { job: 2 });
        let s = s.apply(Event::Finished {
            job: 1,
            result: Ok(Box::new(verified())),
        });
        assert!(matches!(s, State::Running { job: 2, .. }));
    }

    #[test]
    fn start_while_running_is_a_no_op_and_cancel_while_idle_too() {
        let s = running(1).apply(Event::Start { job: 2 });
        assert!(matches!(s, State::Running { job: 1, .. }));
        assert!(matches!(State::Idle.apply(Event::Cancel), State::Idle));
    }

    #[test]
    fn failure_can_be_retried() {
        let s = running(1).apply(Event::Finished {
            job: 1,
            result: Err("disk".into()),
        });
        let v = view(&Claim::Repo("a/b".into()), &s, false);
        assert!(v.headline.contains("disk") && v.can_verify);
        assert!(s.apply(Event::Start { job: 2 }).is_running());
    }

    #[test]
    fn time_formatting() {
        assert_eq!(
            format_time("2026-09-30T12:34:56Z"),
            "2026-09-30 12:34:56 UTC"
        );
        assert_eq!(format_time("garbage"), "garbage");
        assert_eq!(format_epoch(0), "1970-01-01 00:00:00 UTC");
    }
}
