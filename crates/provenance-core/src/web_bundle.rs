use crate::provider::{Fetched, Provider, ProviderError};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Url;
use sigstore_verify::types::Bundle;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

const MAX_BUNDLE_BYTES: usize = 4 << 20;
const MAX_REDIRECTS: usize = 5;
const TIMEOUT: Duration = Duration::from_secs(30);

/// A bundle downloaded from an https URL the user gave. It is only a candidate: it goes through
/// the same verification as any other, so neither the URL nor the host proves anything.
pub struct WebBundle {
    url: Url,
    client: reqwest::Client,
}

impl WebBundle {
    /// A URL the user typed: only https, no user name or password.
    pub fn new(url: &str) -> Result<Self, String> {
        Self::build(url, true, false)
    }

    /// A link read from the file being checked, which the file's author controls: like [`Self::new`], and
    /// it never connects to a loopback, private or link-local address, also not through DNS or a redirect.
    pub fn from_file_link(url: &str) -> Result<Self, String> {
        Self::build(url, true, true)
    }

    fn build(url: &str, https_only: bool, public_only: bool) -> Result<Self, String> {
        let parsed = Url::parse(url).map_err(|e| format!("'{url}' is not a URL: {e}"))?;
        if !scheme_allowed(&parsed, https_only) {
            return Err(format!("'{url}' is not an https URL"));
        }
        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err("a bundle URL must not carry a user name or password".into());
        }
        if parsed.host_str().is_none() {
            return Err(format!("'{url}' has no host"));
        }
        if public_only {
            redirect_target_allowed(&parsed, true)?;
        }
        let mut builder = reqwest::Client::builder();
        if public_only {
            builder = builder.dns_resolver(PublicOnly);
        }
        let client = builder
            .user_agent(concat!(
                "sigstore-shell/",
                env!("SIGSTORE_SHELL_VERSION"),
                " (commit ",
                env!("SIGSTORE_SHELL_COMMIT"),
                ")"
            ))
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error("too many redirects")
                } else if !scheme_allowed(attempt.url(), https_only) {
                    attempt.error("redirected to a URL that is not https")
                } else if let Err(why) = redirect_target_allowed(attempt.url(), public_only) {
                    attempt.error(why)
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            url: parsed,
            client,
        })
    }

    /// The host the request goes to, for the text that tells the user where the request is sent.
    pub fn host(&self) -> &str {
        self.url.host_str().unwrap_or_default()
    }

    async fn download(&self) -> Result<String, String> {
        let host = self.host();
        let mut resp = self
            .client
            .get(self.url.clone())
            .send()
            .await
            .map_err(|e| format!("request to {host} failed: {}", error_chain(&e)))?;
        if !resp.status().is_success() {
            return Err(format!("{host} returned HTTP {}", resp.status()));
        }
        let too_large =
            || format!("the bundle from {host} is larger than {MAX_BUNDLE_BYTES} bytes");
        if resp
            .content_length()
            .is_some_and(|n| n > MAX_BUNDLE_BYTES as u64)
        {
            return Err(too_large());
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("reading from {host} failed: {e}"))?
        {
            if body.len() + chunk.len() > MAX_BUNDLE_BYTES {
                return Err(too_large());
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| format!("the bundle from {host} is not text"))
    }
}

/// Where a GitHub release usually keeps the bundle of one of its files, guessed from the repo the file
/// claims, its `ProductVersion` and its file name: `releases/download/v<version>/<name>.cosign.bundle`.
/// A hint that is wrong for a renamed file or another tag style; the download is verified like any other.
pub fn guess_release_bundle_url(
    repo: &str,
    product_version: &str,
    file_name: &str,
) -> Option<String> {
    let version_ok = product_version.len() <= 64
        && product_version.starts_with(|c: char| c.is_ascii_digit())
        && product_version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'-' | b'_'));
    if !crate::valid_repo(repo) || !version_ok || file_name.is_empty() {
        return None;
    }
    let mut url = Url::parse(&format!(
        "https://github.com/{repo}/releases/download/v{product_version}/"
    ))
    .ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(&format!("{file_name}.cosign.bundle"));
    Some(url.to_string())
}

/// reqwest's own text stops at "error sending request"; the cause is in the sources.
fn error_chain(e: &dyn std::error::Error) -> String {
    let mut text = e.to_string();
    let mut source = e.source();
    while let Some(s) = source {
        text.push_str(": ");
        text.push_str(&s.to_string());
        source = s.source();
    }
    text
}

/// An address literal in the URL never goes through DNS, so the resolver cannot see it.
fn redirect_target_allowed(url: &Url, public_only: bool) -> Result<(), String> {
    if !public_only {
        return Ok(());
    }
    let host = url.host_str().unwrap_or_default();
    match host.trim_matches(['[', ']']).parse::<IpAddr>() {
        Ok(ip) if !public_ip(ip) => Err(format!(
            "{host} is a loopback, private or link-local address"
        )),
        _ => Ok(()),
    }
}

fn public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => public_v4(a),
        IpAddr::V6(a) => public_v6(a),
    }
}

fn public_v4(a: Ipv4Addr) -> bool {
    let o = a.octets();
    let shared = o[0] == 100 && o[1] & 0xc0 == 64;
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || shared
        || o[0] == 0
        || o[0] >= 240)
}

fn public_v6(a: Ipv6Addr) -> bool {
    if let Some(v4) = a.to_ipv4_mapped() {
        return public_v4(v4);
    }
    let s = a.segments();
    // 64:ff9b::/96 carries an IPv4 address in its last 32 bits.
    if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
        let [hi, lo] = [s[6].to_be_bytes(), s[7].to_be_bytes()];
        return public_v4(Ipv4Addr::new(hi[0], hi[1], lo[0], lo[1]));
    }
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_multicast()
        || s[0] & 0xfe00 == 0xfc00
        || s[0] & 0xffc0 == 0xfe80)
}

/// Resolves a host name and keeps only its public addresses, so a name that points inside the network
/// (or is rebound there) cannot be reached.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        Box::pin(async move {
            let public: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|a| public_ip(a.ip()))
                .collect();
            if public.is_empty() {
                return Err(format!("{host} has no public address").into());
            }
            Ok(Box::new(public.into_iter()) as Addrs)
        })
    }
}

fn scheme_allowed(url: &Url, https_only: bool) -> bool {
    url.scheme() == "https" || (!https_only && url.scheme() == "http")
}

#[async_trait::async_trait]
impl Provider for WebBundle {
    fn name(&self) -> &'static str {
        "web-bundle"
    }

    async fn fetch(&self, _: &str, _: Option<&str>) -> Result<Fetched, ProviderError> {
        let text = self.download().await.map_err(ProviderError::failed)?;
        let bundle = Bundle::from_json(&text).map_err(|e| {
            ProviderError::failed(format!("{}: not a Sigstore bundle: {e}", self.host()))
        })?;
        Ok(Fetched {
            bundles: vec![bundle],
            ..Fetched::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{load_embedded_root, verify_file, Options, Status};
    use std::path::Path;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

    /// Answers each request with the next canned response, then closes the connection.
    async fn serve(responses: Vec<Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for response in responses {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(&response).await;
                let _ = socket.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    fn ok(body: &[u8]) -> Vec<u8> {
        let mut r = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        r.extend_from_slice(body);
        r
    }

    fn redirect(to: &str) -> Vec<u8> {
        format!("HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .into_bytes()
    }

    fn sums_bundle() -> Vec<u8> {
        std::fs::read(Path::new(FIXTURES).join("SHA256SUMS.cosign.bundle")).unwrap()
    }

    async fn fetch(base: &str, path: &str) -> Result<Fetched, ProviderError> {
        WebBundle::build(&format!("{base}{path}"), false, false)
            .unwrap()
            .fetch("", None)
            .await
    }

    #[test]
    fn only_plain_https_urls_are_accepted() {
        assert!(WebBundle::new("https://example.org/a.sigstore").is_ok());
        for url in [
            "http://example.org/a.sigstore",
            "ftp://example.org/a",
            "file:///tmp/a.sigstore",
            "https://user:pw@example.org/a",
            "https://user@example.org/a",
            "example.org/a",
            "",
        ] {
            assert!(WebBundle::new(url).is_err(), "{url}");
        }
    }

    #[test]
    fn the_release_bundle_url_is_guessed_from_repo_version_and_name() {
        assert_eq!(
            guess_release_bundle_url("non7top/sigstore-shell", "0.3.0", "sigstore-shell-cli.exe"),
            Some("https://github.com/non7top/sigstore-shell/releases/download/v0.3.0/sigstore-shell-cli.exe.cosign.bundle".into())
        );
        let spaced = guess_release_bundle_url("o/r", "1.2.3-rc1", "my app (1)#x?.exe").unwrap();
        assert!(
            spaced.ends_with("/v1.2.3-rc1/my%20app%20(1)%23x%3F.exe.cosign.bundle"),
            "{spaced}"
        );
        assert!(WebBundle::new(&spaced).is_ok());
    }

    #[test]
    fn nothing_odd_makes_it_into_a_guessed_url() {
        for (repo, version, name) in [
            ("not a repo", "1.0", "a.exe"),
            ("o/r", "", "a.exe"),
            ("o/r", "v1.0", "a.exe"),
            ("o/r", "1.0/../x", "a.exe"),
            ("o/r", "1.0 beta", "a.exe"),
            ("o/r", "1.0", ""),
            ("o/r", &"1".repeat(65), "a.exe"),
        ] {
            assert_eq!(
                guess_release_bundle_url(repo, version, name),
                None,
                "{repo} {version} {name}"
            );
        }
        let slash = guess_release_bundle_url("o/r", "1.0", "../a.exe").unwrap();
        assert!(slash.ends_with("/v1.0/..%2Fa.exe.cosign.bundle"), "{slash}");
    }

    #[test]
    fn host_is_the_host_of_the_url() {
        let w =
            WebBundle::new("https://github.com/o/r/releases/download/v1/a.cosign.bundle").unwrap();
        assert_eq!(w.host(), "github.com");
    }

    #[test]
    fn only_https_may_be_the_target_of_a_redirect() {
        let https = Url::parse("https://example.org/").unwrap();
        let http = Url::parse("http://example.org/").unwrap();
        assert!(scheme_allowed(&https, true));
        assert!(!scheme_allowed(&http, true));
        assert!(scheme_allowed(&http, false));
        assert!(!scheme_allowed(
            &Url::parse("file:///etc/passwd").unwrap(),
            false
        ));
    }

    #[test]
    fn a_file_link_to_an_address_inside_the_network_is_refused() {
        for url in [
            "https://127.0.0.1/a",
            "https://127.1/a",
            "https://2130706433/a",
            "https://0.0.0.0/a",
            "https://10.1.2.3/a",
            "https://172.16.0.1/a",
            "https://192.168.1.1/a",
            "https://169.254.169.254/latest/meta-data",
            "https://100.64.0.1/a",
            "https://[::1]/a",
            "https://[::]/a",
            "https://[fd00::1]/a",
            "https://[fe80::1]/a",
            "https://[::ffff:127.0.0.1]/a",
            "https://[::ffff:10.0.0.1]/a",
            "https://[64:ff9b::7f00:1]/a",
        ] {
            assert!(WebBundle::from_file_link(url).is_err(), "{url}");
        }
        for url in [
            "https://example.org/a",
            "https://8.8.8.8/a",
            "https://[2606:4700::1111]/a",
            "https://github.com/o/r/releases/download/v1/a.cosign.bundle",
        ] {
            assert!(WebBundle::from_file_link(url).is_ok(), "{url}");
        }
    }

    #[test]
    fn a_link_the_user_typed_may_point_inside_the_network() {
        assert!(WebBundle::new("https://127.0.0.1/a").is_ok());
        assert!(WebBundle::new("https://intranet.example/a").is_ok());
    }

    #[test]
    fn a_redirect_to_an_address_inside_the_network_is_refused_for_a_file_link() {
        let inside = Url::parse("https://169.254.169.254/x").unwrap();
        assert!(redirect_target_allowed(&inside, true).is_err());
        assert!(redirect_target_allowed(&inside, false).is_ok());
        let outside = Url::parse("https://example.org/x").unwrap();
        assert!(redirect_target_allowed(&outside, true).is_ok());
    }

    #[tokio::test]
    async fn a_name_that_resolves_inside_the_network_is_not_connected_to() {
        let e = WebBundle::from_file_link("https://localhost:9/a")
            .unwrap()
            .fetch("", None)
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("no public address"), "{e}");
    }

    #[tokio::test]
    async fn a_downloaded_bundle_verifies_the_file_it_was_made_for() {
        let base = serve(vec![ok(&sums_bundle())]).await;
        let web =
            WebBundle::build(&format!("{base}/SHA256SUMS.cosign.bundle"), false, false).unwrap();
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(web)];
        let r = verify_file(
            &Path::new(FIXTURES).join("SHA256SUMS"),
            &Options::default(),
            &providers,
            &root,
            source,
        )
        .await
        .unwrap();
        assert_eq!(r.status, Status::Verified);
        assert_eq!(r.provider.as_deref(), Some("web-bundle"));
    }

    #[tokio::test]
    async fn a_bundle_for_other_bytes_is_not_verified() {
        let base = serve(vec![ok(&sums_bundle())]).await;
        let web = WebBundle::build(&format!("{base}/b"), false, false).unwrap();
        let (root, source) = load_embedded_root().unwrap();
        let providers: Vec<Box<dyn Provider>> = vec![Box::new(web)];
        let r = verify_file(
            &Path::new(FIXTURES).join("cli-2.102.0-windows-amd64-zip.bundle.json"),
            &Options::default(),
            &providers,
            &root,
            source,
        )
        .await
        .unwrap();
        assert_eq!(r.status, Status::VerificationFailed);
    }

    #[tokio::test]
    async fn a_redirect_is_followed() {
        let base = serve(vec![redirect("/real"), ok(&sums_bundle())]).await;
        assert_eq!(fetch(&base, "/start").await.unwrap().bundles.len(), 1);
    }

    #[tokio::test]
    async fn an_http_error_names_the_host_and_status() {
        let base = serve(vec![
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
        ])
        .await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("127.0.0.1") && e.contains("404"), "{e}");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_by_its_declared_length() {
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            MAX_BUNDLE_BYTES + 1
        );
        let base = serve(vec![head.into_bytes()]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("larger than"), "{e}");
    }

    #[tokio::test]
    async fn a_body_over_the_limit_is_refused_while_reading_it() {
        let body = vec![b' '; MAX_BUNDLE_BYTES + 1];
        let chunked = [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec(),
            format!("{:x}\r\n", body.len()).into_bytes(),
            body,
            b"\r\n0\r\n\r\n".to_vec(),
        ]
        .concat();
        let base = serve(vec![chunked]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("larger than"), "{e}");
    }

    #[tokio::test]
    async fn something_that_is_not_a_bundle_is_a_failure_not_a_verdict() {
        let base = serve(vec![ok(b"<html>not a bundle</html>")]).await;
        let e = fetch(&base, "/x").await.unwrap_err().to_string();
        assert!(e.contains("not a Sigstore bundle"), "{e}");
    }
}
