use crate::links::{
    attestation_url, commit_url, log_entry_url, run_url, short_commit, workflow_url, Link,
};
use provenance_core::{valid_repo, ClaimError, Identity, Report, Status};

/// What the file says about itself. Never trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    Absent,
    Repo(String),
    Invalid(String),
    Unreadable,
}

impl Claim {
    pub fn from_read(read: Result<Option<String>, ClaimError>) -> Self {
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

/// Which indicator the page shows beside the verdict; colour is chosen by the UI, never the only signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    None,
    Good,
    Bad,
    Neutral,
    Warn,
}

impl Outcome {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Good => "\u{2714}",
            Self::Bad => "\u{2716}",
            Self::Neutral => "\u{2013}",
            Self::Warn => "\u{26A0}",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Normal,
    Bad,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    /// Bold repository name: the file's claim, or after a result the certificate's repository.
    pub repo: String,
    pub repo_tone: Tone,
    /// Normal-weight `@ short-commit` on the same line as the repo.
    pub repo_suffix: String,
    /// Normal-weight line under the repo: "(claimed, not verified)", or why there is no repo.
    pub repo_note: String,
    pub outcome: Outcome,
    pub headline: String,
    pub links: Vec<Link>,
    pub rows: Vec<(String, String)>,
    pub notes: Vec<String>,
    /// Reset time of the GitHub window, or that the answer came from the cache.
    pub rate_line: Option<String>,
    pub sha256: Option<String>,
    pub signer: Option<String>,
    pub consent: String,
    pub show_consent: bool,
    pub verify_label: String,
    pub can_verify: bool,
    pub running: bool,
    pub progress_text: String,
}

/// GitHub's count from the last fresh response seen by this tab; never stored anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    pub limit: u64,
    pub remaining: u64,
}

impl Rate {
    /// None for cache hits (their headers are old) and for answers without both numbers.
    pub fn from_report(report: &Report) -> Option<Self> {
        if is_cached(report) {
            return None;
        }
        let r = report.rate_limit.as_ref()?;
        Some(Self {
            limit: r.limit?,
            remaining: r.remaining?,
        })
    }
}

const CACHED_NOTE: &str = "Result cached from a check on ";

fn is_cached(report: &Report) -> bool {
    report.notes.iter().any(|n| n.starts_with(CACHED_NOTE))
}

/// The cache note carries UTC (`2026-10-08 04:24:11 UTC.`); show it in local time.
fn cached_line(report: &Report, tz: &jiff::tz::TimeZone) -> Option<String> {
    let note = report.notes.iter().find(|n| n.starts_with(CACHED_NOTE))?;
    let stamp = note[CACHED_NOTE.len()..]
        .trim_end_matches('.')
        .trim_end_matches(" UTC");
    let when = jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M:%S", stamp)
        .ok()
        .and_then(|d| d.to_zoned(jiff::tz::TimeZone::UTC).ok())
        .map(|z| {
            z.with_time_zone(tz.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| stamp.to_string());
    Some(format!("Cached result from {when}; no GitHub request used"))
}

/// Everything besides the file's own state that the view depends on.
#[derive(Debug, Clone)]
pub struct Env {
    pub rate: Option<Rate>,
    pub tz: jiff::tz::TimeZone,
}

impl Env {
    pub fn local(rate: Option<Rate>) -> Self {
        Self {
            rate,
            tz: jiff::tz::TimeZone::system(),
        }
    }
}

impl Default for Env {
    fn default() -> Self {
        Self {
            rate: None,
            tz: jiff::tz::TimeZone::UTC,
        }
    }
}

/// Characters that fit on a details line in the monospace box without wrapping.
const LINE_WIDTH: usize = 52;
const LABEL_WIDTH: usize = 11;

/// Breaks after the last `/`, `@` or space that fits, so URLs split at path separators.
fn wrap_value(value: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = value.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    while chars.len() - start > width {
        let window = &chars[start..start + width];
        let cut = window
            .iter()
            .rposition(|c| matches!(c, '/' | '@' | ' '))
            .map_or(width, |i| i + 1);
        out.push(chars[start..start + cut].iter().collect());
        start += cut;
    }
    out.push(chars[start..].iter().collect());
    out
}

fn push_row(lines: &mut Vec<String>, key: &str, value: &str) {
    let label = format!("{key}:");
    let pad = LABEL_WIDTH.max(label.len() + 1);
    if pad + value.chars().count() <= LINE_WIDTH {
        lines.push(format!("{label:<pad$}{value}"));
        return;
    }
    lines.push(label);
    let parts = if key == "SHA-256" && value.len() == 64 {
        vec![value[..32].to_string(), value[32..].to_string()]
    } else {
        wrap_value(value, LINE_WIDTH - 2)
    };
    lines.extend(parts.into_iter().map(|p| format!("  {p}")));
}

impl View {
    /// Text for the read-only details box, with CRLF as Win32 edit controls expect.
    pub fn details_text(&self) -> String {
        let mut lines = Vec::new();
        for (k, v) in &self.rows {
            push_row(&mut lines, k, v);
        }
        lines.extend(self.notes.iter().map(|n| format!("Note: {n}")));
        lines.join("\r\n")
    }

    pub fn has_details(&self) -> bool {
        !self.rows.is_empty() || !self.notes.is_empty()
    }
}

const LOW_REMAINING: u64 = 10;

fn reset_clock(epoch: u64, tz: &jiff::tz::TimeZone) -> Option<String> {
    let ts = jiff::Timestamp::from_second(i64::try_from(epoch).ok()?).ok()?;
    Some(ts.to_zoned(tz.clone()).strftime("%H:%M").to_string())
}

/// Plain label until a lookup in this tab has shown the count, then e.g. "Verify  52/60".
pub fn verify_label(base: &str, rate: Option<Rate>) -> String {
    match rate {
        Some(r) => format!("{base}  {}/{}", r.remaining, r.limit),
        None => base.to_string(),
    }
}

/// The count is per IP and shared with other tools, so it is never worded as this app's own quota.
fn rate_line(report: &Report, tz: &jiff::tz::TimeZone) -> Option<String> {
    if is_cached(report) {
        return cached_line(report, tz);
    }
    if matches!(report.provider.as_deref(), Some(p) if p != "github") {
        return None;
    }
    let at = reset_clock(report.rate_limit.as_ref()?.reset_epoch?, tz)?;
    Some(format!("GitHub API limit (this IP) resets at {at}"))
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

fn claim_parts(claim: &Claim) -> (String, String) {
    match claim {
        Claim::Absent => (
            String::new(),
            "No provenance information in this file.".into(),
        ),
        Claim::Unreadable => (
            String::new(),
            "No provenance information in this file (could not read it).".into(),
        ),
        Claim::Repo(r) => (r.clone(), "(claimed, not verified)".into()),
        Claim::Invalid(c) => (
            c.clone(),
            "(claimed, not verified; not a valid owner/repo)".into(),
        ),
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
    add("Owner", &id.owner);
    add("Workflow", &id.workflow);
    add("Commit", &id.commit);
    add("Ref", &id.git_ref);
    add("Signed", &id.signed_at.as_deref().map(format_time));
    add("Signer", &id.san);
    rows
}

struct RepoLine {
    name: String,
    tone: Tone,
    suffix: String,
    note: String,
}

struct Done {
    outcome: Outcome,
    headline: String,
    /// Set when the certificate names a repository to show instead of the claim.
    repo: Option<RepoLine>,
    links: Vec<Link>,
    rows: Vec<(String, String)>,
}

fn done_view(report: &Report, claim: &Claim, env: &Env) -> Done {
    let mut rows = Vec::new();
    let mut outcome = Outcome::Bad;
    let headline = match report.status {
        Status::Verified => {
            outcome = Outcome::Good;
            "Verified: this file's Sigstore attestation checks out.".to_string()
        }
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
        Status::NoAttestation => {
            outcome = Outcome::Neutral;
            "No attestation found for this file. \
            This does not mean the file is unsafe; most files have none."
                .to_string()
        }
        Status::LookupFailed => {
            outcome = Outcome::Warn;
            let rate = report.rate_limit.as_ref();
            let left = rate.and_then(|r| r.remaining);
            match left {
                Some(0) => {
                    let again = rate
                        .and_then(|r| r.reset_epoch)
                        .and_then(|e| reset_clock(e, &env.tz))
                        .map_or(String::new(), |t| format!(" after {t}"));
                    format!("Lookup failed: GitHub's request limit is used up. Try again{again}.")
                }
                Some(n) if n <= LOW_REMAINING => format!(
                    "Lookup failed: could not tell whether an attestation exists. \
                     Only {n} GitHub requests are left this hour for this IP."
                ),
                _ => "Lookup failed: could not tell whether an attestation exists.".to_string(),
            }
        }
        Status::VerificationFailed => {
            "Verification failed: an attestation exists but did not verify. Do not rely on it."
                .to_string()
        }
        Status::NotChecked => {
            outcome = Outcome::Neutral;
            "Nothing was looked up.".to_string()
        }
    };
    let mut repo = None;
    let mut links = Vec::new();
    if let Some(id) = &report.identity {
        rows.extend(identity_rows(id));
        if let Some(cert_repo) = &id.repo {
            let short = id.commit.as_deref().and_then(short_commit);
            let suffix = short.map_or(String::new(), |c| format!("@ {c}"));
            let line = |tone, note| RepoLine {
                name: cert_repo.clone(),
                tone,
                suffix: suffix.clone(),
                note,
            };
            match report.status {
                Status::Verified => repo = Some(line(Tone::Normal, String::new())),
                Status::Mismatch => {
                    let claimed = claim
                        .repo()
                        .or(report.queried_repo.as_deref())
                        .unwrap_or("?");
                    let note = format!("signed for this repository; the file claims {claimed}");
                    repo = Some(line(Tone::Bad, note));
                }
                _ => {}
            }
        }
        if report.status == Status::Verified {
            links = build_links(id, report);
        }
    }
    if let Some(p) = &report.provider {
        rows.push(("Answered by".into(), provider_label(p).into()));
    }
    rows.push(("SHA-256".into(), report.file_sha256.clone()));
    Done {
        outcome,
        headline,
        repo,
        links,
        rows,
    }
}

fn build_links(id: &Identity, report: &Report) -> Vec<Link> {
    let repo = id.repo.as_deref();
    let commit = id.commit.as_deref();
    let (repo_commit, workflow, run) = match (repo, commit) {
        (Some(repo), Some(commit)) => (
            commit_url(repo, commit),
            id.workflow
                .as_deref()
                .and_then(|w| workflow_url(repo, commit, w)),
            id.run_url.as_deref().and_then(|u| run_url(repo, u)),
        ),
        _ => (None, None, None),
    };
    let candidates = [
        ("Commit", repo_commit),
        ("Workflow file", workflow),
        ("Build run", run),
        (
            "Attestation",
            repo.zip(report.attestation_id)
                .and_then(|(repo, id)| attestation_url(repo, id)),
        ),
        ("Log entry", report.log_index.and_then(log_entry_url)),
    ];
    candidates
        .into_iter()
        .filter_map(|(label, url)| {
            url.map(|url| Link {
                label: label.into(),
                url,
            })
        })
        .collect()
}

pub fn view(claim: &Claim, state: &State, rekor: bool) -> View {
    view_in(claim, state, rekor, &Env::default())
}

pub fn view_in(claim: &Claim, state: &State, rekor: bool, env: &Env) -> View {
    let can_search = claim.repo().is_some() || rekor;
    let running = state.is_running();
    let (repo, repo_note) = claim_parts(claim);
    let mut v = View {
        repo,
        repo_tone: Tone::Normal,
        repo_suffix: String::new(),
        repo_note,
        outcome: Outcome::None,
        headline: String::new(),
        links: Vec::new(),
        rows: Vec::new(),
        notes: Vec::new(),
        rate_line: None,
        sha256: None,
        signer: None,
        consent: consent(rekor),
        show_consent: true,
        verify_label: verify_label("Verify", env.rate),
        can_verify: can_search && !running,
        running,
        progress_text: String::new(),
    };
    match state {
        State::Idle if !can_search => {
            v.headline = "Nothing to look up: this file names no repository and Rekor search \
                is off."
                .into();
            v.show_consent = false;
        }
        State::Idle => {}
        State::Running { phase, .. } => {
            v.headline = "Verifying...".into();
            v.progress_text = match phase {
                Phase::Hashing { percent } => format!("Hashing the file: {percent}%"),
                Phase::Lookup => "Looking up and verifying the attestation".into(),
            };
        }
        State::Done(report) => {
            let d = done_view(report, claim, env);
            if let Some(line) = d.repo {
                v.repo = line.name;
                v.repo_tone = line.tone;
                v.repo_suffix = line.suffix;
                v.repo_note = line.note;
            }
            v.outcome = d.outcome;
            v.headline = d.headline;
            v.links = d.links;
            v.rows = d.rows;
            v.notes = report
                .notes
                .iter()
                .filter(|n| !n.starts_with(CACHED_NOTE))
                .cloned()
                .collect();
            v.rate_line = rate_line(report, &env.tz);
            v.sha256 = Some(report.file_sha256.clone());
            v.signer = report.identity.as_ref().and_then(|i| i.san.clone());
            v.show_consent = false;
            v.verify_label = verify_label("Verify again", env.rate);
        }
        State::Cancelled => v.headline = "Cancelled. Nothing was verified.".into(),
        State::Failed(e) => {
            v.outcome = Outcome::Warn;
            v.headline = format!("Could not verify: {e}");
            v.show_consent = false;
            v.verify_label = verify_label("Verify again", env.rate);
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::links::{markup, openable};
    use provenance_core::{RateLimit, TrustRootSource};

    fn report(status: Status) -> Report {
        Report {
            status,
            file_sha256: "ab".repeat(32),
            claimed_repo: Some("cli/cli".into()),
            queried_repo: Some("cli/cli".into()),
            provider: Some("github".into()),
            identity: None,
            attestation_id: None,
            log_index: None,
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
                commit: Some("fc4b137c9e0a5d2b7f3e1a8c6d4b2f0e9a7c5d3b".into()),
                git_ref: Some("refs/heads/trunk".into()),
                run_url: Some("https://github.com/cli/cli/actions/runs/42/attempts/1".into()),
                san: Some(
                    "https://github.com/cli/cli/.github/workflows/deployment.yml@refs/heads/trunk"
                        .into(),
                ),
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
        let err = provenance_core::read_claim(b"MZ").unwrap_err();
        assert_eq!(Claim::from_read(Err(err)), Claim::Unreadable);
    }

    #[test]
    fn opening_the_page_shows_the_claim_labelled_unverified() {
        let v = view(
            &Claim::Repo("non7top/promptloom".into()),
            &State::Idle,
            false,
        );
        assert_eq!(v.repo, "non7top/promptloom");
        assert_eq!(v.repo_tone, Tone::Normal);
        assert_eq!(v.repo_note, "(claimed, not verified)");
        assert_eq!(v.headline, "");
        assert!(v.show_consent);
        assert!(v.can_verify);
        assert!(v.rows.is_empty());
    }

    #[test]
    fn no_claim_says_so_and_cannot_verify_without_rekor() {
        let v = view(&Claim::Absent, &State::Idle, false);
        assert_eq!(v.repo, "");
        assert_eq!(v.repo_note, "No provenance information in this file.");
        assert!(!v.can_verify);
        assert!(view(&Claim::Absent, &State::Idle, true).can_verify);
    }

    #[test]
    fn an_invalid_claim_is_shown_but_not_queried() {
        let v = view(&Claim::Invalid("x".into()), &State::Idle, false);
        assert!(v.repo_note.contains("not a valid owner/repo"));
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
    fn verified_view_puts_repo_and_short_commit_up_top() {
        let s = State::Done(Box::new(verified()));
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        assert_eq!(v.outcome, Outcome::Good);
        assert!(v.headline.starts_with("Verified") && v.headline.contains("checks out"));
        assert!(!v.headline.to_lowercase().contains("safe"));
        assert_eq!(v.repo, "cli/cli");
        assert_eq!(v.repo_tone, Tone::Normal);
        assert_eq!(v.repo_suffix, "@ fc4b137c");
        assert_eq!(v.repo_note, "");
    }

    #[test]
    fn verified_view_lists_details_below() {
        let s = State::Done(Box::new(verified()));
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        let text = v.details_text();
        assert!(text.contains("Owner:     cli"));
        assert!(text.contains("Workflow:  .github/workflows/deployment.yml"));
        assert!(text.contains("Commit:    fc4b137c9e0a5d2b7f3e1a8c6d4b2f0e9a7c5d3b"));
        assert!(text.contains("Ref:       refs/heads/trunk"));
        assert!(text.contains("Signed:    2026-09-30 12:34:56 UTC"));
        assert!(text.contains("Signer:\r\n  https://github.com/cli/cli/"));
        assert!(text.contains("Answered by: GitHub attestations"));
        assert!(!text.contains("Repository"));
        assert!(text.contains("\r\n"));
    }

    #[test]
    fn no_details_line_is_wider_than_the_box() {
        let mut r = verified();
        r.identity.as_mut().unwrap().san = Some(format!("https://github.com/{}", "a/".repeat(60)));
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        for line in v.details_text().split("\r\n") {
            assert!(line.chars().count() <= LINE_WIDTH, "{line}");
        }
    }

    #[test]
    fn wrap_value_breaks_at_separators_and_never_loses_text() {
        let url =
            "https://github.com/non7top/promptloom/.github/workflows/release.yml@refs/heads/master";
        let parts = wrap_value(url, 50);
        assert_eq!(parts.concat(), url);
        assert!(parts.iter().all(|p| p.chars().count() <= 50));
        assert!(parts[0].ends_with('/'));
        assert_eq!(wrap_value(&"x".repeat(120), 50).concat(), "x".repeat(120));
        assert_eq!(wrap_value("short", 50), ["short"]);
    }

    #[test]
    fn sha256_is_split_in_two_lines_but_copied_whole() {
        let s = State::Done(Box::new(verified()));
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        let text = v.details_text();
        assert!(text.contains(&format!(
            "SHA-256:\r\n  {}\r\n  {}",
            "ab".repeat(16),
            "ab".repeat(16)
        )));
        assert_eq!(v.sha256, Some("ab".repeat(32)));
        assert!(v.signer.unwrap().ends_with("@refs/heads/trunk"));
    }

    #[test]
    fn verified_view_links_commit_workflow_and_run() {
        let s = State::Done(Box::new(verified()));
        let v = view(&Claim::Repo("cli/cli".into()), &s, false);
        let urls: Vec<_> = v.links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                "https://github.com/cli/cli/commit/fc4b137c9e0a5d2b7f3e1a8c6d4b2f0e9a7c5d3b",
                "https://github.com/cli/cli/blob/fc4b137c9e0a5d2b7f3e1a8c6d4b2f0e9a7c5d3b/.github/workflows/deployment.yml",
                "https://github.com/cli/cli/actions/runs/42/attempts/1",
            ]
        );
        assert_eq!(
            markup(&v.links),
            "<a>Commit</a>   <a>Workflow file</a>   <a>Build run</a>"
        );
    }

    #[test]
    fn attestation_and_log_entry_links_follow_the_build_links() {
        let r = Report {
            attestation_id: Some(53_806_567),
            log_index: Some(3_140_935_978),
            ..verified()
        };
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        let labels: Vec<_> = v.links.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(
            labels,
            [
                "Commit",
                "Workflow file",
                "Build run",
                "Attestation",
                "Log entry"
            ]
        );
        assert_eq!(
            v.links[3].url,
            "https://github.com/cli/cli/attestations/53806567"
        );
        assert_eq!(
            v.links[4].url,
            "https://search.sigstore.dev/?logIndex=3140935978"
        );
        assert!(v.links.iter().all(|l| openable(&l.url)));
    }

    #[test]
    fn the_demo_report_fixture_shows_all_five_links() {
        let r: Report =
            serde_json::from_str(include_str!("../tests/fixtures/demo-report.json")).unwrap();
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        assert_eq!(v.links.len(), 5);
    }

    #[test]
    fn the_new_links_appear_only_when_their_data_exists() {
        let only = |attestation_id, log_index| {
            let r = Report {
                attestation_id,
                log_index,
                ..verified()
            };
            let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
            v.links.len()
        };
        assert_eq!(only(None, None), 3);
        assert_eq!(only(Some(7), None), 4);
        assert_eq!(only(None, Some(9)), 4);
        assert_eq!(only(Some(0), Some(0)), 3);
        assert_eq!(only(Some(1), Some(u64::MAX)), 4);
    }

    #[test]
    fn a_hostile_certificate_repo_gets_no_attestation_link() {
        let mut r = Report {
            attestation_id: Some(1),
            log_index: Some(1),
            ..verified()
        };
        r.identity.as_mut().unwrap().repo = Some("cli/cli/../../evil".into());
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        let labels: Vec<_> = v.links.iter().map(|l| l.label.as_str()).collect();
        assert_eq!(labels, ["Log entry"]);
    }

    #[test]
    fn a_mismatch_shows_no_links_even_with_ids() {
        let r = Report {
            status: Status::Mismatch,
            attestation_id: Some(1),
            log_index: Some(1),
            ..verified()
        };
        let v = view(&Claim::Repo("x/y".into()), &State::Done(Box::new(r)), false);
        assert!(v.links.is_empty());
    }

    #[test]
    fn hostile_certificate_values_yield_no_links() {
        let mut r = verified();
        let id = r.identity.as_mut().unwrap();
        id.workflow = Some("../../evil".into());
        id.run_url = Some("https://evil.example/cli/cli/actions/runs/1".into());
        let v = view(&Claim::Absent, &State::Done(Box::new(r.clone())), true);
        assert_eq!(v.links.len(), 1);
        r.identity.as_mut().unwrap().repo = Some("cli/cli?x=1".into());
        let v = view(&Claim::Absent, &State::Done(Box::new(r)), true);
        assert!(v.links.is_empty());
    }

    #[test]
    fn only_a_verified_result_gets_links() {
        let r = Report {
            status: Status::Mismatch,
            ..verified()
        };
        let v = view(&Claim::Repo("x/y".into()), &State::Done(Box::new(r)), false);
        assert!(v.links.is_empty());
        assert_eq!(v.outcome, Outcome::Bad);
    }

    #[test]
    fn a_result_hides_the_consent_and_relabels_the_button() {
        let before = view(&Claim::Repo("cli/cli".into()), &State::Idle, false);
        assert!(before.show_consent);
        assert_eq!(before.verify_label, "Verify");
        assert_eq!(before.headline, "");
        assert_eq!(before.outcome, Outcome::None);
        let done = view(
            &Claim::Repo("cli/cli".into()),
            &State::Done(Box::new(verified())),
            false,
        );
        assert!(!done.show_consent);
        assert_eq!(done.verify_label, "Verify again");
        assert!(done.can_verify);
    }

    #[test]
    fn outcome_per_status() {
        let o = |s| view(&Claim::Absent, &State::Done(Box::new(report(s))), true).outcome;
        assert_eq!(o(Status::Verified), Outcome::Good);
        assert_eq!(o(Status::Mismatch), Outcome::Bad);
        assert_eq!(o(Status::VerificationFailed), Outcome::Bad);
        assert_eq!(o(Status::NoAttestation), Outcome::Neutral);
        assert_eq!(o(Status::LookupFailed), Outcome::Warn);
        assert_ne!(Outcome::Neutral.glyph(), Outcome::Bad.glyph());
        assert_eq!(Outcome::None.glyph(), "");
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
        assert_eq!(v.repo, "cli/cli");
        assert_eq!(v.repo_tone, Tone::Bad);
        assert!(v.repo_note.contains("evil/repo"));
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

    fn limited(remaining: Option<u64>, reset: Option<u64>) -> Report {
        Report {
            rate_limit: Some(RateLimit {
                limit: Some(60),
                remaining,
                reset_epoch: reset,
                resource: Some("core".into()),
            }),
            ..report(Status::NoAttestation)
        }
    }

    // 2026-10-08 05:42:00 UTC
    const RESET: u64 = 1_791_438_120;

    fn headline(r: Report) -> String {
        view_in(
            &Claim::Absent,
            &State::Done(Box::new(r)),
            true,
            &Env::default(),
        )
        .headline
    }

    #[test]
    fn rate_line_is_only_the_reset_time_in_local_time() {
        let utc = jiff::tz::TimeZone::UTC;
        let r = limited(Some(52), Some(RESET));
        assert_eq!(
            rate_line(&r, &utc).unwrap(),
            "GitHub API limit (this IP) resets at 05:42"
        );
        let plus2 = jiff::tz::TimeZone::fixed(jiff::tz::offset(2));
        assert!(rate_line(&r, &plus2).unwrap().ends_with("07:42"));
    }

    #[test]
    fn rate_line_without_headers_shows_nothing() {
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(rate_line(&report(Status::LookupFailed), &utc), None);
        assert_eq!(rate_line(&limited(Some(5), None), &utc), None);
    }

    #[test]
    fn cached_result_says_no_request_was_used() {
        let mut r = limited(Some(52), Some(RESET));
        r.notes
            .push("Result cached from a check on 2026-10-08 04:24:11 UTC.".into());
        assert_eq!(
            rate_line(&r, &jiff::tz::TimeZone::UTC).unwrap(),
            "Cached result from 2026-10-08 04:24; no GitHub request used"
        );
        let plus2 = jiff::tz::TimeZone::fixed(jiff::tz::offset(2));
        assert!(rate_line(&r, &plus2).unwrap().contains("06:24"));
        let v = view_in(
            &Claim::Absent,
            &State::Done(Box::new(r)),
            true,
            &Env::default(),
        );
        assert!(v.notes.is_empty());
    }

    #[test]
    fn a_retry_note_stays_in_the_details_box() {
        let mut r = limited(Some(52), Some(RESET));
        r.notes
            .push("github: the server returned an error; retried once".into());
        let v = view_in(
            &Claim::Absent,
            &State::Done(Box::new(r)),
            true,
            &Env::default(),
        );
        assert!(v.details_text().contains("retried once"));
    }

    #[test]
    fn rekor_answers_get_no_github_line() {
        let mut r = limited(Some(52), Some(RESET));
        r.provider = Some("rekor-v1".into());
        assert_eq!(rate_line(&r, &jiff::tz::TimeZone::UTC), None);
    }

    #[test]
    fn exhausted_and_low_quota_are_spelled_out_next_to_the_verdict() {
        let failed = |rem, reset| Report {
            status: Status::LookupFailed,
            ..limited(rem, reset)
        };
        assert_eq!(
            headline(failed(Some(0), Some(RESET))),
            "Lookup failed: GitHub's request limit is used up. Try again after 05:42."
        );
        assert_eq!(
            headline(failed(Some(0), None)),
            "Lookup failed: GitHub's request limit is used up. Try again."
        );
        assert!(headline(failed(Some(3), None))
            .contains("Only 3 GitHub requests are left this hour for this IP"));
        assert!(headline(failed(Some(10), None)).contains("Only 10"));
        assert!(!headline(failed(Some(11), None)).contains("Only"));
        assert_eq!(
            headline(failed(None, None)),
            "Lookup failed: could not tell whether an attestation exists."
        );
    }

    #[test]
    fn verify_button_label_carries_the_counter() {
        let rate = |remaining| {
            Some(Rate {
                limit: 60,
                remaining,
            })
        };
        assert_eq!(verify_label("Verify", None), "Verify");
        assert_eq!(verify_label("Verify", rate(52)), "Verify  52/60");
        assert_eq!(
            verify_label("Verify again", rate(51)),
            "Verify again  51/60"
        );
        assert_eq!(verify_label("Verify", rate(0)), "Verify  0/60");
    }

    #[test]
    fn only_a_fresh_answer_with_both_numbers_sets_the_counter() {
        let full = limited(Some(52), Some(RESET));
        assert_eq!(
            Rate::from_report(&full),
            Some(Rate {
                limit: 60,
                remaining: 52
            })
        );
        assert_eq!(Rate::from_report(&limited(None, Some(RESET))), None);
        assert_eq!(Rate::from_report(&report(Status::LookupFailed)), None);
        let mut cached = full;
        cached.notes.push("Result cached from a check on x.".into());
        assert_eq!(Rate::from_report(&cached), None);
    }

    #[test]
    fn the_counter_reaches_the_view_label() {
        let env = Env {
            rate: Some(Rate {
                limit: 60,
                remaining: 51,
            }),
            tz: jiff::tz::TimeZone::UTC,
        };
        let claim = Claim::Repo("cli/cli".into());
        assert_eq!(
            view_in(&claim, &State::Idle, false, &env).verify_label,
            "Verify  51/60"
        );
        let done = State::Done(Box::new(verified()));
        assert_eq!(
            view_in(&claim, &done, false, &env).verify_label,
            "Verify again  51/60"
        );
        assert_eq!(view(&claim, &State::Idle, false).verify_label, "Verify");
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
