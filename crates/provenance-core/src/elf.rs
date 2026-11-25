use crate::claim_format::{ELF_BUNDLE_NOTE_NAME, ELF_NOTE_NAME, ELF_NOTE_TYPE};

const SHT_NOTE: u32 = 7;
const MAX_CLAIM: usize = 256;
const MAX_BUNDLE_URL: usize = 512;

struct Reader<'a> {
    data: &'a [u8],
    big: bool,
}

impl Reader<'_> {
    fn bytes(&self, at: u64, len: u64) -> Option<&[u8]> {
        let end = at.checked_add(len)?;
        self.data
            .get(usize::try_from(at).ok()?..usize::try_from(end).ok()?)
    }
    fn u16(&self, at: u64) -> Option<u16> {
        let b = self.bytes(at, 2)?.try_into().ok()?;
        Some(if self.big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        })
    }
    fn u32(&self, at: u64) -> Option<u32> {
        let b = self.bytes(at, 4)?.try_into().ok()?;
        Some(if self.big {
            u32::from_be_bytes(b)
        } else {
            u32::from_le_bytes(b)
        })
    }
    fn u64(&self, at: u64) -> Option<u64> {
        let b = self.bytes(at, 8)?.try_into().ok()?;
        Some(if self.big {
            u64::from_be_bytes(b)
        } else {
            u64::from_le_bytes(b)
        })
    }
}

fn align4(n: u64) -> u64 {
    (n + 3) & !3
}

/// Finds the note called `name` in the SHT_NOTE sections. `None` if malformed or absent.
fn find(image: &[u8], name: &str, max: usize) -> Option<Option<String>> {
    let is64 = match *image.get(4)? {
        1 => false,
        2 => true,
        _ => return None,
    };
    let big = match *image.get(5)? {
        1 => false,
        2 => true,
        _ => return None,
    };
    let r = Reader { data: image, big };
    let (shoff, shentsize, shnum) = if is64 {
        (r.u64(0x28)?, r.u16(0x3a)?, r.u16(0x3c)?)
    } else {
        (u64::from(r.u32(0x20)?), r.u16(0x2e)?, r.u16(0x30)?)
    };
    if shoff == 0 || shnum == 0 {
        return Some(None);
    }
    let min_ent = if is64 { 64 } else { 40 };
    if u64::from(shentsize) < min_ent {
        return None;
    }
    for i in 0..u64::from(shnum) {
        let sh = shoff.checked_add(i * u64::from(shentsize))?;
        if r.u32(sh + 4)? != SHT_NOTE {
            continue;
        }
        let (off, size) = if is64 {
            (r.u64(sh + 0x18)?, r.u64(sh + 0x20)?)
        } else {
            (u64::from(r.u32(sh + 0x10)?), u64::from(r.u32(sh + 0x14)?))
        };
        let notes = r.bytes(off, size)?;
        if let Some(claim) = scan_notes(notes, big, name, max) {
            return Some(Some(claim));
        }
    }
    Some(None)
}

fn scan_notes(notes: &[u8], big: bool, wanted: &str, max: usize) -> Option<String> {
    let r = Reader { data: notes, big };
    let mut at = 0u64;
    while at + 12 <= notes.len() as u64 {
        let namesz = u64::from(r.u32(at)?);
        let descsz = u64::from(r.u32(at + 4)?);
        let kind = r.u32(at + 8)?;
        let name_at = at + 12;
        let desc_at = name_at.checked_add(align4(namesz))?;
        let next = desc_at.checked_add(align4(descsz))?;
        let name = r.bytes(name_at, namesz)?;
        if kind == ELF_NOTE_TYPE
            && name.strip_suffix(&[0]) == Some(wanted.as_bytes())
            && descsz <= max as u64
        {
            let desc = r.bytes(desc_at, descsz)?;
            let value = std::str::from_utf8(desc).ok()?.trim_matches('\0').trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
        at = next;
    }
    None
}

fn read_note(image: &[u8], name: &str, max: usize) -> Result<Option<String>, String> {
    if !image.starts_with(b"\x7fELF") {
        return Err("not an ELF file".into());
    }
    find(image, name, max).ok_or_else(|| "malformed ELF file".into())
}

/// Reads the provenance note. `Err` means the file is not a parseable ELF; a valid ELF without the note is `Ok(None)`.
pub fn read_claim(image: &[u8]) -> Result<Option<String>, String> {
    read_note(image, ELF_NOTE_NAME, MAX_CLAIM)
}

/// Reads the optional bundle link note, with the same meaning of `Ok(None)`.
pub fn read_bundle_url(image: &[u8]) -> Result<Option<String>, String> {
    read_note(image, ELF_BUNDLE_NOTE_NAME, MAX_BUNDLE_URL)
}

#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixture {
    use crate::claim_format::{ELF_NOTE_NAME, ELF_NOTE_TYPE};

    pub fn elf_with_claim(claim: Option<&str>) -> Vec<u8> {
        match claim {
            Some(c) => elf_with_notes(&[(ELF_NOTE_NAME, c)]),
            None => elf_with_notes(&[]),
        }
    }

    fn pad4(buf: &mut Vec<u8>) {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }

    /// A minimal little-endian ELF64 with one SHT_NOTE section holding the given (name, value) notes.
    pub fn elf_with_notes(notes: &[(&str, &str)]) -> Vec<u8> {
        let mut note = Vec::new();
        for (name, value) in notes {
            note.extend(u32::try_from(name.len() + 1).unwrap().to_le_bytes());
            note.extend(u32::try_from(value.len()).unwrap().to_le_bytes());
            note.extend(ELF_NOTE_TYPE.to_le_bytes());
            note.extend(name.as_bytes());
            note.push(0);
            pad4(&mut note);
            note.extend(value.as_bytes());
            pad4(&mut note);
        }
        let mut img = vec![0u8; 64];
        img[..4].copy_from_slice(b"\x7fELF");
        img[4] = 2;
        img[5] = 1;
        img[6] = 1;
        img[0x10..0x12].copy_from_slice(&3u16.to_le_bytes());
        img[0x12..0x14].copy_from_slice(&62u16.to_le_bytes());
        let note_off = img.len() as u64;
        img.extend(&note);
        pad4(&mut img);
        let shoff = img.len() as u64;
        img.extend([0u8; 64]);
        let mut sh = [0u8; 64];
        sh[4..8].copy_from_slice(&7u32.to_le_bytes());
        sh[0x18..0x20].copy_from_slice(&note_off.to_le_bytes());
        sh[0x20..0x28].copy_from_slice(&(note.len() as u64).to_le_bytes());
        sh[0x30..0x38].copy_from_slice(&4u64.to_le_bytes());
        img.extend(sh);
        img[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        img[0x34..0x36].copy_from_slice(&64u16.to_le_bytes());
        img[0x3a..0x3c].copy_from_slice(&64u16.to_le_bytes());
        img[0x3c..0x3e].copy_from_slice(&2u16.to_le_bytes());
        img
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{elf_with_claim, elf_with_notes};
    use super::*;

    #[test]
    fn reads_the_note() {
        let elf = elf_with_claim(Some("non7top/sigstore-shell"));
        assert_eq!(
            read_claim(&elf).unwrap().as_deref(),
            Some("non7top/sigstore-shell")
        );
    }

    #[test]
    fn odd_length_value_is_padded_and_read() {
        let elf = elf_with_claim(Some("a/bc"));
        assert_eq!(read_claim(&elf).unwrap().as_deref(), Some("a/bc"));
        let elf = elf_with_claim(Some("a/b"));
        assert_eq!(read_claim(&elf).unwrap().as_deref(), Some("a/b"));
    }

    #[test]
    fn elf_without_the_note_is_none() {
        assert_eq!(read_claim(&elf_with_claim(None)).unwrap(), None);
    }

    #[test]
    fn every_truncation_is_an_error_or_none_never_a_panic() {
        let elf = elf_with_claim(Some("non7top/sigstore-shell"));
        for len in 0..elf.len() {
            let _ = read_claim(&elf[..len]);
        }
    }

    #[test]
    fn every_corrupted_byte_is_handled_without_panic() {
        let elf = elf_with_claim(Some("non7top/sigstore-shell"));
        for i in 0..elf.len() {
            for v in [0u8, 0x7f, 0xff] {
                let mut bad = elf.clone();
                bad[i] = v;
                let _ = read_claim(&bad);
            }
        }
    }

    #[test]
    fn huge_note_sizes_are_rejected() {
        let mut elf = elf_with_claim(Some("a/b"));
        let note_off = 64;
        elf[note_off + 4..note_off + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(read_claim(&elf).unwrap(), None);
    }

    #[test]
    fn non_utf8_value_is_ignored() {
        let mut elf = elf_with_claim(Some("a/b"));
        let desc = 64 + 12 + 16;
        elf[desc] = 0xff;
        assert_eq!(read_claim(&elf).unwrap(), None);
    }

    #[test]
    fn the_claim_and_the_bundle_link_are_separate_notes_in_either_order() {
        let url = "https://github.com/o/r/releases/download/v1/app.cosign.bundle";
        for notes in [
            [(ELF_NOTE_NAME, "o/r"), (ELF_BUNDLE_NOTE_NAME, url)],
            [(ELF_BUNDLE_NOTE_NAME, url), (ELF_NOTE_NAME, "o/r")],
        ] {
            let elf = elf_with_notes(&notes);
            assert_eq!(read_claim(&elf).unwrap().as_deref(), Some("o/r"));
            assert_eq!(read_bundle_url(&elf).unwrap().as_deref(), Some(url));
        }
        let only_repo = elf_with_claim(Some("o/r"));
        assert_eq!(read_bundle_url(&only_repo).unwrap(), None);
    }

    #[test]
    fn a_bundle_link_over_the_limit_is_ignored() {
        let long = format!("https://example.org/{}", "a".repeat(MAX_BUNDLE_URL));
        let elf = elf_with_notes(&[(ELF_BUNDLE_NOTE_NAME, &long)]);
        assert_eq!(read_bundle_url(&elf).unwrap(), None);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(read_claim(b"hello").is_err());
        assert!(read_claim(b"\x7fELF").is_err());
        assert!(read_claim(b"\x7fELF\x09\x01").is_err());
    }
}
