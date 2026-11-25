use crate::{elf, pe};

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ClaimError(String);

/// Reads the embedded repo claim from a PE (`MZ`) or ELF (`\x7fELF`) image, chosen by magic.
/// `Ok(None)` means a valid file without a claim; the value is an unverified hint.
pub fn read_claim(image: &[u8]) -> Result<Option<String>, ClaimError> {
    if image.starts_with(b"MZ") {
        pe::read_claim(image).map_err(|e| ClaimError(format!("not a readable PE file: {e}")))
    } else if image.starts_with(b"\x7fELF") {
        elf::read_claim(image).map_err(|e| ClaimError(format!("not a readable ELF file: {e}")))
    } else {
        Ok(None)
    }
}

/// Reads the optional embedded bundle link: the `ProvenanceBundle` string of a PE or the note of
/// that name in an ELF. It is only a hint where a bundle might be fetched from, never evidence.
pub fn read_bundle_url(image: &[u8]) -> Result<Option<String>, ClaimError> {
    if image.starts_with(b"MZ") {
        pe::read_bundle_url(image).map_err(|e| ClaimError(format!("not a readable PE file: {e}")))
    } else if image.starts_with(b"\x7fELF") {
        elf::read_bundle_url(image).map_err(|e| ClaimError(format!("not a readable ELF file: {e}")))
    } else {
        Ok(None)
    }
}

/// Like [`read_bundle_url`] but maps the file instead of reading it.
pub fn read_bundle_url_file(path: &std::path::Path) -> Result<Option<String>, ClaimError> {
    let map =
        pelite::FileMap::open(path).map_err(|e| ClaimError(format!("{}: {e}", path.display())))?;
    read_bundle_url(map.as_ref())
}

/// The bundle link to try for a file: the one it embeds, else, for a PE with a repo claim and a
/// `ProductVersion`, the usual release URL guessed from them (see [`guess_release_bundle_url`]).
pub fn bundle_link_file(path: &std::path::Path) -> Option<String> {
    let map = pelite::FileMap::open(path).ok()?;
    let image = map.as_ref();
    if let Ok(Some(url)) = read_bundle_url(image) {
        return Some(url);
    }
    if !image.starts_with(b"MZ") {
        return None;
    }
    let repo = pe::read_claim(image).ok()??;
    let version = pe::read_product_version(image).ok()??;
    crate::web_bundle::guess_release_bundle_url(&repo, &version, path.file_name()?.to_str()?)
}

/// Like [`read_claim`] but maps the file instead of reading it, so a large exe is not loaded.
pub fn read_claim_file(path: &std::path::Path) -> Result<Option<String>, ClaimError> {
    let map =
        pelite::FileMap::open(path).map_err(|e| ClaimError(format!("{}: {e}", path.display())))?;
    read_claim(map.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elf::fixture::{elf_with_claim, elf_with_notes};
    use crate::pe::fixture::pe_with_version_strings;

    #[test]
    fn dispatches_by_magic() {
        let pe = pe_with_version_strings(&[("ProvenanceRepo", "a/pe")]);
        assert_eq!(read_claim(&pe).unwrap().as_deref(), Some("a/pe"));
        let elf = elf_with_claim(Some("a/elf"));
        assert_eq!(read_claim(&elf).unwrap().as_deref(), Some("a/elf"));
    }

    #[test]
    fn bundle_links_are_read_from_both_formats() {
        let url = "https://github.com/o/r/releases/download/v1/app.cosign.bundle";
        let pe = pe_with_version_strings(&[("ProvenanceRepo", "o/r"), ("ProvenanceBundle", url)]);
        assert_eq!(read_bundle_url(&pe).unwrap().as_deref(), Some(url));
        assert_eq!(read_claim(&pe).unwrap().as_deref(), Some("o/r"));
        let elf = elf_with_notes(&[("ProvenanceRepo", "o/r"), ("ProvenanceBundle", url)]);
        assert_eq!(read_bundle_url(&elf).unwrap().as_deref(), Some(url));
        let none = pe_with_version_strings(&[("ProvenanceRepo", "o/r")]);
        assert_eq!(read_bundle_url(&none).unwrap(), None);
        assert_eq!(read_bundle_url(b"hello").unwrap(), None);
    }

    #[test]
    fn the_embedded_link_wins_over_the_guess_and_only_a_pe_with_a_repo_is_guessed() {
        let dir = std::env::temp_dir().join(format!("pc-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link = "https://example.org/x.cosign.bundle";
        let write = |name: &str, bytes: Vec<u8>| {
            let path = dir.join(name);
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let both = write(
            "a.exe",
            pe_with_version_strings(&[
                ("ProvenanceRepo", "o/r"),
                ("ProductVersion", "1.2.3"),
                ("ProvenanceBundle", link),
            ]),
        );
        assert_eq!(bundle_link_file(&both).as_deref(), Some(link));
        let guess = write(
            "b.exe",
            pe_with_version_strings(&[("ProvenanceRepo", "o/r"), ("ProductVersion", "1.2.3")]),
        );
        assert_eq!(
            bundle_link_file(&guess).as_deref(),
            Some("https://github.com/o/r/releases/download/v1.2.3/b.exe.cosign.bundle")
        );
        let no_repo = write(
            "c.exe",
            pe_with_version_strings(&[("ProductVersion", "1.2.3")]),
        );
        assert_eq!(bundle_link_file(&no_repo), None);
        let no_version = write(
            "d.exe",
            pe_with_version_strings(&[("ProvenanceRepo", "o/r")]),
        );
        assert_eq!(bundle_link_file(&no_version), None);
        let elf = write("e", elf_with_claim(Some("o/r")));
        assert_eq!(bundle_link_file(&elf), None);
        assert_eq!(bundle_link_file(&dir.join("missing.exe")), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_magic_has_no_claim() {
        assert_eq!(read_claim(b"hello world").unwrap(), None);
        assert_eq!(read_claim(b"").unwrap(), None);
    }

    #[test]
    fn truncated_headers_are_errors() {
        assert!(read_claim(b"MZ").is_err());
        assert!(read_claim(b"\x7fELF\x02\x01").is_err());
    }

    #[test]
    fn reads_claims_from_files_on_disk() {
        let dir = std::env::temp_dir().join(format!("pc-claim-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app.exe");
        std::fs::write(&exe, pe_with_version_strings(&[("ProvenanceRepo", "a/b")])).unwrap();
        assert_eq!(read_claim_file(&exe).unwrap().as_deref(), Some("a/b"));
        let bin = dir.join("app");
        std::fs::write(&bin, elf_with_claim(Some("c/d"))).unwrap();
        assert_eq!(read_claim_file(&bin).unwrap().as_deref(), Some("c/d"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
