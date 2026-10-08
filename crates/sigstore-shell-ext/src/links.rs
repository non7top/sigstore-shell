//! URLs shown as links. Every one is rebuilt from validated pieces; certificate strings are never passed through.

pub const SIGSTORE_URL: &str = "https://www.sigstore.dev/";
pub const PROJECT_URL: &str = "https://github.com/non7top/sigstore-shell";
const LOG_SEARCH_PREFIX: &str = "https://search.sigstore.dev/?logIndex=";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// SysLink markup as plain text, for when the control class is unavailable.
pub fn strip_markup(markup: &str) -> String {
    markup.replace("<a>", "").replace("</a>", "")
}

pub const FOOTER_MARKUP: &str =
    "Verification uses Sigstore's public roots, with GitHub vouching for \
    which workflow ran. <a>What is Sigstore?</a> <a>About this tab</a>";
pub const FOOTER_URLS: [&str; 2] = [SIGSTORE_URL, PROJECT_URL];

/// The only URLs the page may open.
pub fn openable(url: &str) -> bool {
    url == SIGSTORE_URL
        || url.starts_with("https://github.com/")
        || url
            .strip_prefix(LOG_SEARCH_PREFIX)
            .is_some_and(|n| digits(n) && n.parse::<u64>().is_ok_and(|n| n <= i64::MAX as u64))
}

/// SysLink markup for a row of links, separated by three spaces.
pub fn markup(links: &[Link]) -> String {
    links
        .iter()
        .map(|l| format!("<a>{}</a>", l.label))
        .collect::<Vec<_>>()
        .join("   ")
}

/// Splits labels, given their pixel widths, into consecutive rows that each fit in `max`.
/// A label wider than `max` gets a row of its own. Beyond `max_rows` the last row takes the rest.
pub fn split_rows(
    widths: &[i32],
    gap: i32,
    max: i32,
    max_rows: usize,
) -> Vec<std::ops::Range<usize>> {
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    for (i, w) in widths.iter().enumerate() {
        let needed = if i == start { *w } else { used + gap + *w };
        if i > start && needed > max {
            rows.push(start..i);
            start = i;
            used = *w;
        } else {
            used = needed;
        }
    }
    if start < widths.len() {
        rows.push(start..widths.len());
    }
    if rows.len() > max_rows {
        let end = widths.len();
        rows.truncate(max_rows);
        if let Some(last) = rows.last_mut() {
            last.end = end;
        }
    }
    rows
}

fn name_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && s.chars().any(|c| c != '.')
}

fn repo_ok(repo: &str) -> bool {
    matches!(repo.split_once('/'), Some((o, r)) if name_ok(o) && name_ok(r) && !r.contains('/'))
}

fn commit_ok(sha: &str) -> bool {
    (7..=64).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit())
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.chars().all(|c| c.is_ascii_digit())
}

/// `id` is GitHub's page id for the attestation, which is not part of its API response.
pub fn attestation_url(repo: &str, id: u64) -> Option<String> {
    (repo_ok(repo) && id > 0).then(|| format!("https://github.com/{repo}/attestations/{id}"))
}

/// Opens the entry in Rekor's public search; built from the integer alone.
pub fn log_entry_url(index: u64) -> Option<String> {
    (0 < index && index <= i64::MAX as u64).then(|| format!("{LOG_SEARCH_PREFIX}{index}"))
}

/// First 8 characters of a hex commit id, or None if it is not one.
pub fn short_commit(sha: &str) -> Option<&str> {
    commit_ok(sha).then(|| &sha[..sha.len().min(8)])
}

pub fn commit_url(repo: &str, sha: &str) -> Option<String> {
    (repo_ok(repo) && commit_ok(sha)).then(|| format!("https://github.com/{repo}/commit/{sha}"))
}

/// `path` must be a single workflow file directly under `.github/workflows/`.
pub fn workflow_url(repo: &str, sha: &str, path: &str) -> Option<String> {
    let file = path.strip_prefix(".github/workflows/")?;
    let named = file
        .strip_suffix(".yml")
        .or_else(|| file.strip_suffix(".yaml"))?;
    (repo_ok(repo) && commit_ok(sha) && name_ok(named) && !file.contains('/'))
        .then(|| format!("https://github.com/{repo}/blob/{sha}/.github/workflows/{file}"))
}

/// Accepts only `https://github.com/<repo>/actions/runs/<id>[/attempts/<n>]` for this exact repo.
pub fn run_url(repo: &str, raw: &str) -> Option<String> {
    if !repo_ok(repo) {
        return None;
    }
    let rest = raw
        .strip_prefix("https://github.com/")?
        .strip_prefix(repo)?
        .strip_prefix("/actions/runs/")?;
    match rest.split_once("/attempts/") {
        None if digits(rest) => Some(format!("https://github.com/{repo}/actions/runs/{rest}")),
        Some((id, n)) if digits(id) && digits(n) => Some(format!(
            "https://github.com/{repo}/actions/runs/{id}/attempts/{n}"
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "fc4b137c9e0a5d2b7f3e1a8c6d4b2f0e9a7c5d3b";

    #[test]
    fn builds_the_expected_urls() {
        assert_eq!(
            commit_url("cli/cli", SHA).unwrap(),
            format!("https://github.com/cli/cli/commit/{SHA}")
        );
        assert_eq!(
            workflow_url("cli/cli", SHA, ".github/workflows/release.yml").unwrap(),
            format!("https://github.com/cli/cli/blob/{SHA}/.github/workflows/release.yml")
        );
        assert_eq!(
            run_url(
                "cli/cli",
                "https://github.com/cli/cli/actions/runs/123/attempts/2"
            )
            .unwrap(),
            "https://github.com/cli/cli/actions/runs/123/attempts/2"
        );
        assert_eq!(
            run_url("cli/cli", "https://github.com/cli/cli/actions/runs/123").unwrap(),
            "https://github.com/cli/cli/actions/runs/123"
        );
    }

    #[test]
    fn hostile_repos_make_no_url() {
        for repo in [
            "",
            "cli",
            "cli/",
            "/cli",
            "../cli",
            "cli/..",
            "./.",
            "a/b/c",
            "a/b?x=1",
            "a/b#f",
            "a b/c",
            "a/b\\c",
            "u@evil.com/x",
            "evil.com:80/x",
            "a/b%2e",
            "caf\u{e9}/x",
            "a/b\n",
            "https://evil.com/x",
        ] {
            assert_eq!(commit_url(repo, SHA), None, "{repo:?}");
            assert_eq!(
                workflow_url(repo, SHA, ".github/workflows/a.yml"),
                None,
                "{repo:?}"
            );
            assert_eq!(
                run_url(repo, &format!("https://github.com/{repo}/actions/runs/1")),
                None,
                "{repo:?}"
            );
        }
    }

    #[test]
    fn attestation_and_log_urls_are_built_from_integers() {
        assert_eq!(
            attestation_url("non7top/sigstore-shell", 53_806_567).unwrap(),
            "https://github.com/non7top/sigstore-shell/attestations/53806567"
        );
        assert_eq!(
            log_entry_url(3_140_935_978).unwrap(),
            "https://search.sigstore.dev/?logIndex=3140935978"
        );
        assert!(openable(&log_entry_url(3_140_935_978).unwrap()));
        assert!(openable(&attestation_url("a/b", 1).unwrap()));
        assert_eq!(attestation_url("a/b", 0), None);
        assert_eq!(log_entry_url(0), None);
        assert!(log_entry_url(i64::MAX as u64).is_some());
        assert_eq!(log_entry_url(i64::MAX as u64 + 1), None);
        assert_eq!(log_entry_url(u64::MAX), None);
    }

    #[test]
    fn hostile_repos_make_no_attestation_url() {
        for repo in [
            "",
            "cli",
            "../cli",
            "a/b/c",
            "a/b?x=1",
            "a/b#f",
            "a b/c",
            "u@evil.com/x",
            "evil.com:80/x",
            "a/b%2e",
            "a/b\n",
            "https://evil.com/x",
        ] {
            assert_eq!(attestation_url(repo, 1), None, "{repo:?}");
        }
    }

    #[test]
    fn split_rows_fills_rows_in_order_without_exceeding_the_width() {
        // 100% and 150%: same labels at 6 and 9 px per character, widths measured per label.
        let labels = [
            "Commit",
            "Workflow file",
            "Build run",
            "Attestation",
            "Log entry",
        ];
        for (px, gap, max) in [(6, 18, 357), (9, 27, 536)] {
            let widths: Vec<i32> = labels.iter().map(|l| l.len() as i32 * px).collect();
            let rows = split_rows(&widths, gap, max, 2);
            assert_eq!(rows.iter().map(|r| r.len()).sum::<usize>(), 5);
            assert_eq!(rows[0].start, 0);
            for pair in rows.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
            for r in &rows {
                let total: i32 = widths[r.clone()].iter().sum::<i32>() + gap * (r.len() as i32 - 1);
                assert!(total <= max, "{px}px row {r:?} is {total} > {max}");
            }
        }
        let wide = [100, 100, 100, 100, 100];
        assert_eq!(split_rows(&wide, 10, 450, 2), [0..4, 4..5]);
        assert_eq!(split_rows(&wide, 10, 1000, 2).len(), 1);
        assert_eq!(split_rows(&wide, 10, 50, 9), [0..1, 1..2, 2..3, 3..4, 4..5]);
        assert_eq!(split_rows(&wide, 10, 50, 2), [0..1, 1..5]);
        assert_eq!(
            split_rows(&[], 10, 100, 2),
            Vec::<std::ops::Range<usize>>::new()
        );
    }

    #[test]
    fn five_links_wrap_onto_a_second_row_when_the_labels_are_wide() {
        let widths = [60, 110, 80, 95, 75];
        let rows = split_rows(&widths, 20, 357, 2);
        assert_eq!(rows, [0..3, 3..5]);
    }

    #[test]
    fn hostile_commits_make_no_url() {
        for sha in [
            "",
            "abc",
            "../../x",
            "fc4b137/../..",
            "fc4b137?x",
            "fc4b137#y",
            "zzzzzzzz",
            "fc4b 137c",
            &"a".repeat(65),
        ] {
            assert_eq!(commit_url("cli/cli", sha), None, "{sha:?}");
            assert_eq!(
                workflow_url("cli/cli", sha, ".github/workflows/a.yml"),
                None,
                "{sha:?}"
            );
            assert_eq!(short_commit(sha), None, "{sha:?}");
        }
    }

    #[test]
    fn hostile_workflow_paths_make_no_url() {
        for path in [
            "",
            ".github/workflows/",
            ".github/workflows/../../x.yml",
            ".github/workflows/a/b.yml",
            ".github/workflows/a.txt",
            ".github/workflows/..yml",
            ".github/workflows/a.yml?x=1",
            ".github/workflows/a.yml#x",
            "other/repo/.github/workflows/a.yml",
            "/.github/workflows/a.yml",
            ".github/workflows/a b.yml",
        ] {
            assert_eq!(workflow_url("cli/cli", SHA, path), None, "{path:?}");
        }
    }

    #[test]
    fn hostile_run_urls_make_no_url() {
        for url in [
            "",
            "http://github.com/cli/cli/actions/runs/1",
            "https://github.com.evil.com/cli/cli/actions/runs/1",
            "https://evil.com/cli/cli/actions/runs/1",
            "https://user@github.com/cli/cli/actions/runs/1",
            "https://github.com/other/repo/actions/runs/1",
            "https://github.com/cli/cli/actions/runs/1?x=1",
            "https://github.com/cli/cli/actions/runs/1#x",
            "https://github.com/cli/cli/actions/runs/abc",
            "https://github.com/cli/cli/actions/runs/",
            "https://github.com/cli/cli/actions/runs/1/attempts/",
            "https://github.com/cli/cli/actions/runs/1/../2",
            "https://github.com/cli/cli/actions/runs/1/attempts/1/x",
            "javascript:alert(1)",
            "file:///c:/windows/system32/calc.exe",
        ] {
            assert_eq!(run_url("cli/cli", url), None, "{url:?}");
        }
    }

    #[test]
    fn footer_has_one_url_per_link_and_no_endorsement_wording() {
        assert_eq!(FOOTER_MARKUP.matches("<a>").count(), FOOTER_URLS.len());
        assert!(FOOTER_URLS.iter().all(|u| openable(u)));
        assert_eq!(
            strip_markup(FOOTER_MARKUP),
            "Verification uses Sigstore's public roots, with GitHub vouching for which workflow \
             ran. What is Sigstore? About this tab"
        );
        for word in [
            "official",
            "endorsed",
            "certified",
            "partner",
            "by Sigstore",
            "by GitHub",
        ] {
            assert!(!FOOTER_MARKUP.contains(word), "{word}");
        }
    }

    #[test]
    fn only_https_github_and_sigstore_are_openable() {
        assert!(openable("https://github.com/cli/cli/commit/abcdef1"));
        for u in [
            "http://github.com/x",
            "https://evil.com/",
            "file:///c:/x.exe",
            "https://github.com.evil.com/",
            "https://www.sigstore.dev.evil.com/",
            "https://search.sigstore.dev/",
            "https://search.sigstore.dev/?logIndex=",
            "https://search.sigstore.dev/?logIndex=abc",
            "https://search.sigstore.dev/?logIndex=-1",
            "https://search.sigstore.dev/?logIndex=1&x=2",
            "https://search.sigstore.dev/?logIndex=1#x",
            "https://search.sigstore.dev/?logIndex=99999999999999999999",
            "https://search.sigstore.dev/other?logIndex=1",
            "http://search.sigstore.dev/?logIndex=1",
            "https://search.sigstore.dev.evil.com/?logIndex=1",
            "",
        ] {
            assert!(!openable(u), "{u}");
        }
    }

    #[test]
    fn short_commit_is_eight_characters() {
        assert_eq!(short_commit(SHA), Some("fc4b137c"));
        assert_eq!(short_commit("abcdef1"), Some("abcdef1"));
    }
}
