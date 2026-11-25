//! The `Zone.Identifier` stream that browsers attach to a downloaded file on NTFS. What it says
//! about where the file came from is a hint for the user to look at, never evidence.

use reqwest::Url;

/// The most of the stream that is read; a real one is a few hundred bytes.
pub const MAX_ZONE_BYTES: u64 = 4096;

/// Where the download says it came from, taken from `ReferrerUrl` only: `github.com/owner/repo` for a
/// GitHub page, otherwise the page's host. `HostUrl` is never used: it is a short-lived signed link
/// whose query carries tokens, and it must not reach the screen, a log or an issue.
pub fn referrer_from_zone_identifier(text: &str) -> Option<String> {
    let referrer = text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        key.trim()
            .eq_ignore_ascii_case("ReferrerUrl")
            .then(|| value.trim())
    })?;
    let url = Url::parse(referrer).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let host_ok = !host.is_empty()
        && host.len() <= 100
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'));
    if !host_ok {
        return None;
    }
    if host == "github.com" || host == "www.github.com" {
        let mut parts = url.path_segments()?;
        let (owner, repo) = (parts.next()?, parts.next()?);
        let repo_path = format!("{owner}/{repo}");
        return crate::valid_repo(&repo_path).then(|| format!("github.com/{repo_path}"));
    }
    Some(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(referrer: &str) -> String {
        format!(
            "[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl={referrer}\r\nHostUrl=https://release-assets.githubusercontent.com/github-production-release-asset/1/2?sig=SECRET&jwt=TOKEN\r\n"
        )
    }

    #[test]
    fn a_github_release_page_gives_owner_and_repo() {
        assert_eq!(
            referrer_from_zone_identifier(&zone("https://github.com/o/r/releases/tag/v1.0"))
                .as_deref(),
            Some("github.com/o/r")
        );
        assert_eq!(
            referrer_from_zone_identifier(&zone("https://github.com/o/r/")).as_deref(),
            Some("github.com/o/r")
        );
    }

    #[test]
    fn another_site_gives_only_its_host() {
        assert_eq!(
            referrer_from_zone_identifier(&zone("https://Example.org/downloads/?id=7#x"))
                .as_deref(),
            Some("example.org")
        );
    }

    #[test]
    fn the_signed_host_link_is_never_used_or_echoed() {
        let text = "[ZoneTransfer]\nZoneId=3\nHostUrl=https://release-assets.githubusercontent.com/x?sig=SECRET\n";
        assert_eq!(referrer_from_zone_identifier(text), None);
        for referrer in ["https://github.com/o/r", "https://example.org/"] {
            let out = referrer_from_zone_identifier(&zone(referrer)).unwrap();
            assert!(!out.contains("SECRET") && !out.contains("TOKEN") && !out.contains("sig="));
        }
    }

    #[test]
    fn odd_or_hostile_referrers_give_nothing() {
        for referrer in [
            "",
            "about:internet",
            "http://github.com/o/r",
            "file:///C:/x",
            "https://github.com/o",
            "https://github.com/o/r%2F..",
            "https://github.com/../x",
            "https://user:pw@example.org/",
            "https://exa mple.org/",
            "not a url",
        ] {
            assert_eq!(
                referrer_from_zone_identifier(&zone(referrer)),
                None,
                "{referrer:?}"
            );
        }
        assert_eq!(referrer_from_zone_identifier(""), None);
        assert_eq!(referrer_from_zone_identifier("ZoneId=3"), None);
    }

    #[test]
    fn the_key_is_matched_loosely_and_the_first_one_wins() {
        let text = "referrerurl = https://github.com/a/b\nReferrerUrl=https://github.com/c/d\n";
        assert_eq!(
            referrer_from_zone_identifier(text).as_deref(),
            Some("github.com/a/b")
        );
    }
}
