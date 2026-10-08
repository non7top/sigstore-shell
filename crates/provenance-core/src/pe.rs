use pelite::{pe32, pe64};

use crate::claim_format::PE_KEY;

macro_rules! claim_from {
    ($pe:ident, $image:expr) => {
        (|| -> Result<Option<String>, String> {
            use $pe::Pe;
            let pe = $pe::PeFile::from_bytes($image).map_err(|e| e.to_string())?;
            let mut found = None;
            if let Some(info) = pe.resources().ok().and_then(|r| r.version_info().ok()) {
                for lang in info.translation() {
                    if let Some(value) = info.value(*lang, PE_KEY) {
                        let value = value.trim().to_string();
                        if !value.is_empty() {
                            found = Some(value);
                            break;
                        }
                    }
                }
            }
            Ok(found)
        })()
    };
}

/// Reads the `ProvenanceRepo` version-resource string. Pure parsing, so it works off Windows.
/// `Ok(None)` means a valid PE without that string.
pub fn read_claim(image: &[u8]) -> Result<Option<String>, String> {
    claim_from!(pe64, image).or_else(|first| claim_from!(pe32, image).map_err(|_| first))
}

#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixture {
    fn pad4(buf: &mut Vec<u8>) {
        while !buf.len().is_multiple_of(4) {
            buf.push(0);
        }
    }

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }

    fn block(key: &str, value: &[u8], value_len: u16, kind: u16, children: &[Vec<u8>]) -> Vec<u8> {
        let mut b = vec![0, 0];
        b.extend(value_len.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(utf16z(key));
        pad4(&mut b);
        b.extend(value);
        for child in children {
            pad4(&mut b);
            b.extend(child);
        }
        let len = u16::try_from(b.len()).unwrap().to_le_bytes();
        b[0..2].copy_from_slice(&len);
        b
    }

    fn version_info(strings: &[(&str, &str)]) -> Vec<u8> {
        let mut fixed = Vec::new();
        fixed.extend(0xFEEF_04BDu32.to_le_bytes());
        fixed.extend(0x0001_0000u32.to_le_bytes());
        fixed.resize(52, 0);

        let string_blocks: Vec<Vec<u8>> = strings
            .iter()
            .map(|(k, v)| {
                let value = utf16z(v);
                block(k, &value, u16::try_from(value.len() / 2).unwrap(), 1, &[])
            })
            .collect();
        let table = block("040904b0", &[], 0, 1, &string_blocks);
        let string_file_info = block("StringFileInfo", &[], 0, 1, &[table]);
        let translation = block("Translation", &[0x09, 0x04, 0xb0, 0x04], 4, 0, &[]);
        let var_file_info = block("VarFileInfo", &[], 0, 1, &[translation]);
        block(
            "VS_VERSION_INFO",
            &fixed,
            52,
            0,
            &[string_file_info, var_file_info],
        )
    }

    fn dir(entry_id: u32, target: u32) -> Vec<u8> {
        let mut d = vec![0; 12];
        d.extend(0u16.to_le_bytes());
        d.extend(1u16.to_le_bytes());
        d.extend(entry_id.to_le_bytes());
        d.extend(target.to_le_bytes());
        d
    }

    /// A minimal PE32+ whose only content is a version resource with the given strings.
    pub fn pe_with_version_strings(strings: &[(&str, &str)]) -> Vec<u8> {
        const SECTION_RVA: u32 = 0x1000;
        const RAW_OFFSET: usize = 0x200;
        const DATA_OFFSET: u32 = 0x58;

        let info = version_info(strings);
        let mut rsrc = Vec::new();
        rsrc.extend(dir(16, 0x8000_0000 | 0x18));
        rsrc.extend(dir(1, 0x8000_0000 | 0x30));
        rsrc.extend(dir(0x409, 0x48));
        rsrc.extend((SECTION_RVA + DATA_OFFSET).to_le_bytes());
        rsrc.extend(u32::try_from(info.len()).unwrap().to_le_bytes());
        rsrc.extend(0u32.to_le_bytes());
        rsrc.extend(0u32.to_le_bytes());
        assert_eq!(rsrc.len() as u32, DATA_OFFSET);
        rsrc.extend(&info);
        let raw_size = rsrc.len().div_ceil(0x200) * 0x200;
        let virtual_size = u32::try_from(rsrc.len()).unwrap();

        let mut img = vec![0u8; RAW_OFFSET];
        img[0..2].copy_from_slice(b"MZ");
        img[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        img[0x40..0x44].copy_from_slice(b"PE\0\0");
        let coff = 0x44;
        img[coff..coff + 2].copy_from_slice(&0x8664u16.to_le_bytes());
        img[coff + 2..coff + 4].copy_from_slice(&1u16.to_le_bytes());
        img[coff + 16..coff + 18].copy_from_slice(&240u16.to_le_bytes());
        img[coff + 18..coff + 20].copy_from_slice(&0x22u16.to_le_bytes());
        let opt = coff + 20;
        let put32 = |img: &mut Vec<u8>, at: usize, v: u32| {
            img[opt + at..opt + at + 4].copy_from_slice(&v.to_le_bytes());
        };
        img[opt..opt + 2].copy_from_slice(&0x20bu16.to_le_bytes());
        img[opt + 24..opt + 32].copy_from_slice(&0x1_4000_0000u64.to_le_bytes());
        put32(&mut img, 32, 0x1000);
        put32(&mut img, 36, 0x200);
        put32(&mut img, 40, 6);
        put32(&mut img, 48, 6);
        put32(&mut img, 56, SECTION_RVA + 0x1000);
        put32(&mut img, 60, 0x200);
        img[opt + 68..opt + 70].copy_from_slice(&3u16.to_le_bytes());
        put32(&mut img, 108, 16);
        put32(&mut img, 112 + 2 * 8, SECTION_RVA);
        put32(&mut img, 112 + 2 * 8 + 4, virtual_size);

        let sec = opt + 240;
        img[sec..sec + 5].copy_from_slice(b".rsrc");
        img[sec + 8..sec + 12].copy_from_slice(&virtual_size.to_le_bytes());
        img[sec + 12..sec + 16].copy_from_slice(&SECTION_RVA.to_le_bytes());
        img[sec + 16..sec + 20].copy_from_slice(&u32::try_from(raw_size).unwrap().to_le_bytes());
        img[sec + 20..sec + 24].copy_from_slice(&u32::try_from(RAW_OFFSET).unwrap().to_le_bytes());
        img[sec + 36..sec + 40].copy_from_slice(&0x4000_0040u32.to_le_bytes());

        img.extend(&rsrc);
        img.resize(RAW_OFFSET + raw_size, 0);
        img
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::pe_with_version_strings;
    use super::*;

    #[test]
    fn reads_claim_from_version_resource() {
        let pe = pe_with_version_strings(&[
            ("ProductName", "Demo"),
            ("ProvenanceRepo", "non7top/promptloom"),
        ]);
        assert_eq!(
            read_claim(&pe).unwrap().as_deref(),
            Some("non7top/promptloom")
        );
    }

    #[test]
    fn missing_key_is_none() {
        let pe = pe_with_version_strings(&[("ProductName", "Demo")]);
        assert_eq!(read_claim(&pe).unwrap(), None);
    }
}
