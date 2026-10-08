//! URLs shown as links. Every one is rebuilt from validated pieces; certificate strings are never passed through.

pub const SIGSTORE_URL: &str = "https://www.sigstore.dev/";
pub const PROJECT_URL: &str = "https://github.com/non7top/sigstore-shell";

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
    "Verified against Sigstore's public roots, with GitHub vouching for \
    which workflow ran. <a>What is Sigstore?</a> <a>About this tab</a>";
pub const FOOTER_URLS: [&str; 2] = [SIGSTORE_URL, PROJECT_URL];

/// The only URLs the page may open.
pub fn openable(url: &str) -> bool {
    url == SIGSTORE_URL || url.starts_with("https://github.com/")
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
            "Verified against Sigstore's public roots, with GitHub vouching for which workflow \
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
