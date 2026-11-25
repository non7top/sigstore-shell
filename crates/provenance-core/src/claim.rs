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
