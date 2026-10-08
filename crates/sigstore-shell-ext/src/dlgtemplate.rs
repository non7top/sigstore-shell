//! The property page layout as an in-memory DLGTEMPLATE, so the DLL needs no resource file.

pub const IDC_REPO: u16 = 101;
pub const IDC_HEADLINE: u16 = 102;
pub const IDC_DETAILS: u16 = 103;
pub const IDC_CONSENT: u16 = 104;
pub const IDC_PROGRESS: u16 = 105;
pub const IDC_PROGRESS_TEXT: u16 = 106;
pub const IDC_VERIFY: u16 = 107;
pub const IDC_CANCEL: u16 = 108;
pub const IDC_GLYPH: u16 = 109;
pub const IDC_REPO_NOTE: u16 = 110;
pub const IDC_EXPLAIN: u16 = 111;
pub const IDC_COPY_SHA: u16 = 112;
pub const IDC_COPY_SIGNER: u16 = 113;
/// SysLink controls are created in code, so the page still opens where the class is missing.
pub const IDC_LINKS: u16 = 114;
pub const IDC_FOOTER: u16 = 115;
/// Shows the outcome icon; the text glyph control takes over when high contrast is on.
pub const IDC_ICON: u16 = 116;
pub const IDC_RATE: u16 = 117;
pub const IDC_SEPARATOR: u16 = 118;
pub const IDC_REPO_SUFFIX: u16 = 119;

pub const MARGIN: i16 = 7;
pub const CONTENT_WIDTH: i16 = 238;
pub const VERDICT_ROW: i16 = 40;
pub const REPO_ROW: i16 = 15;
pub const REPO_WIDTH: i16 = 153;

pub const ICON_SLOT: i16 = 17;

/// RT_GROUP_ICON ids written by build.rs.
pub const ICON_VERIFIED: u16 = 201;
pub const ICON_FAILED: u16 = 202;
pub const ICON_NEUTRAL: u16 = 203;
pub const ICON_WARNING: u16 = 204;

pub const LINKS_RECT: (i16, i16, i16, i16) = (7, 66, 238, 10);
pub const FOOTER_RECT: (i16, i16, i16, i16) = (7, 197, 238, 19);
pub const HEADLINE_RECT: (i16, i16, i16, i16) = (MARGIN, VERDICT_ROW, CONTENT_WIDTH, 24);

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const DS_3DLOOK: u32 = 0x0004;
const DS_SETFONT: u32 = 0x0040;
const DS_CONTROL: u32 = 0x0400;
const SS_NOPREFIX: u32 = 0x0080;
const SS_CENTER: u32 = 0x0001;
const SS_OWNERDRAW: u32 = 0x000D;
const SS_ETCHEDHORZ: u32 = 0x0010;
const SS_ENDELLIPSIS: u32 = 0x4000;
const ES_MULTILINE: u32 = 0x0004;
const ES_AUTOVSCROLL: u32 = 0x0040;
const ES_READONLY: u32 = 0x0800;
const BS_DEFPUSHBUTTON: u32 = 0x0001;
const PBS_MARQUEE: u32 = 0x0008;

enum Class {
    Button,
    Edit,
    Static,
    Named(&'static str),
}

struct Item {
    class: Class,
    text: &'static str,
    id: u16,
    style: u32,
    ex_style: u32,
    rect: (i16, i16, i16, i16),
}

fn utf16z(buf: &mut Vec<u8>, s: &str) {
    for unit in s.encode_utf16().chain([0]) {
        buf.extend(unit.to_le_bytes());
    }
}

fn align4(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

fn items() -> Vec<Item> {
    let vis = WS_CHILD | WS_VISIBLE;
    let item = |class, text, id, style, rect| Item {
        class,
        text,
        id,
        style,
        ex_style: 0,
        rect,
    };
    let label = |id, style, rect| item(Class::Static, "", id, style | SS_NOPREFIX, rect);
    vec![
        item(
            Class::Button,
            "Verify",
            IDC_VERIFY,
            vis | WS_TABSTOP | BS_DEFPUSHBUTTON,
            (165, 13, 80, 14),
        ),
        item(
            Class::Button,
            "Cancel",
            IDC_CANCEL,
            WS_CHILD | WS_TABSTOP,
            (165, 13, 80, 14),
        ),
        item(
            Class::Static,
            "Provenance: which repo and workflow published this file.",
            IDC_EXPLAIN,
            vis | SS_NOPREFIX,
            (7, 3, 238, 9),
        ),
        label(
            IDC_REPO,
            vis | SS_ENDELLIPSIS,
            (7, REPO_ROW, REPO_WIDTH, 12),
        ),
        label(IDC_REPO_SUFFIX, vis, (7, REPO_ROW, 40, 12)),
        label(IDC_REPO_NOTE, vis, (7, 28, 238, 9)),
        Item {
            ex_style: WS_EX_TRANSPARENT,
            ..label(IDC_GLYPH, WS_CHILD | SS_CENTER, (7, 13, 14, 14))
        },
        label(IDC_ICON, WS_CHILD | SS_OWNERDRAW, (7, 13, 14, 14)),
        label(IDC_HEADLINE, vis, HEADLINE_RECT),
        label(IDC_CONSENT, vis, (7, VERDICT_ROW, 238, 24)),
        item(
            Class::Named("msctls_progress32"),
            "",
            IDC_PROGRESS,
            WS_CHILD | PBS_MARQUEE,
            (7, 66, 238, 8),
        ),
        label(IDC_PROGRESS_TEXT, vis, (7, 76, 238, 10)),
        item(
            Class::Edit,
            "",
            IDC_DETAILS,
            WS_CHILD
                | WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | ES_MULTILINE
                | ES_AUTOVSCROLL
                | ES_READONLY,
            (7, 78, 238, 84),
        ),
        item(
            Class::Button,
            "Copy SHA-256",
            IDC_COPY_SHA,
            WS_CHILD | WS_TABSTOP,
            (7, 165, 62, 14),
        ),
        item(
            Class::Button,
            "Copy signer",
            IDC_COPY_SIGNER,
            WS_CHILD | WS_TABSTOP,
            (73, 165, 56, 14),
        ),
        label(IDC_RATE, vis, (7, 181, 238, 9)),
        item(
            Class::Static,
            "",
            IDC_SEPARATOR,
            vis | SS_ETCHEDHORZ,
            (7, 194, 238, 1),
        ),
    ]
}

/// Little-endian DLGTEMPLATE followed by its DLGITEMTEMPLATEs.
pub fn build() -> Vec<u8> {
    let items = items();
    let mut b = Vec::new();
    let style = WS_CHILD | WS_CLIPCHILDREN | DS_SETFONT | DS_CONTROL | DS_3DLOOK;
    b.extend(style.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(u16::try_from(items.len()).unwrap().to_le_bytes());
    for v in [0i16, 0, 252, 218] {
        b.extend(v.to_le_bytes());
    }
    b.extend(0u16.to_le_bytes());
    b.extend(0u16.to_le_bytes());
    utf16z(&mut b, "");
    b.extend(8u16.to_le_bytes());
    utf16z(&mut b, "MS Shell Dlg");
    for item in &items {
        align4(&mut b);
        b.extend(item.style.to_le_bytes());
        b.extend(item.ex_style.to_le_bytes());
        for v in [item.rect.0, item.rect.1, item.rect.2, item.rect.3] {
            b.extend(v.to_le_bytes());
        }
        b.extend(item.id.to_le_bytes());
        match item.class {
            Class::Button => b.extend([0xFF, 0xFF, 0x80, 0x00]),
            Class::Edit => b.extend([0xFF, 0xFF, 0x81, 0x00]),
            Class::Static => b.extend([0xFF, 0xFF, 0x82, 0x00]),
            Class::Named(name) => utf16z(&mut b, name),
        }
        utf16z(&mut b, item.text);
        b.extend(0u16.to_le_bytes());
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_counts_every_control() {
        let b = build();
        assert_eq!(u16::from_le_bytes([b[8], b[9]]) as usize, items().len());
    }

    #[test]
    fn walks_cleanly_to_the_end() {
        let b = build();
        // 18 fixed bytes, menu, class and empty title (2 each), font size, font name.
        let mut at = 18 + 6 + 2 + ("MS Shell Dlg".len() + 1) * 2;
        let mut ids = Vec::new();
        for _ in 0..items().len() {
            at = at.div_ceil(4) * 4;
            let id = u16::from_le_bytes([b[at + 16], b[at + 17]]);
            ids.push(id);
            at += 18;
            if b[at] == 0xFF && b[at + 1] == 0xFF {
                at += 4;
            } else {
                while b[at..at + 2] != [0, 0] {
                    at += 2;
                }
                at += 2;
            }
            while b[at..at + 2] != [0, 0] {
                at += 2;
            }
            at += 2 + 2;
        }
        assert_eq!(at, b.len());
        assert_eq!(ids[0], IDC_VERIFY);
    }

    #[test]
    fn control_ids_are_unique() {
        let mut ids: Vec<u16> = items().iter().map(|i| i.id).collect();
        ids.extend([IDC_LINKS, IDC_FOOTER]);
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), items().len() + 2);
    }

    #[test]
    fn controls_stay_inside_the_page() {
        for r in items()
            .iter()
            .map(|i| i.rect)
            .chain([LINKS_RECT, FOOTER_RECT])
        {
            assert!(
                r.0 >= 0 && r.1 >= 0 && r.0 + r.2 <= 252 && r.1 + r.3 <= 218,
                "{r:?}"
            );
        }
    }

    #[test]
    fn result_only_controls_start_hidden() {
        let hidden = [
            IDC_PROGRESS,
            IDC_CANCEL,
            IDC_DETAILS,
            IDC_GLYPH,
            IDC_ICON,
            IDC_COPY_SHA,
            IDC_COPY_SIGNER,
        ];
        for i in items() {
            if hidden.contains(&i.id) {
                assert_eq!(i.style & WS_VISIBLE, 0);
            }
        }
    }
}
