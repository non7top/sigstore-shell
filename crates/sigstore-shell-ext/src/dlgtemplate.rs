//! The property page layout as an in-memory DLGTEMPLATE, so the DLL needs no resource file.

pub const IDC_CLAIM: u16 = 101;
pub const IDC_HEADLINE: u16 = 102;
pub const IDC_DETAILS: u16 = 103;
pub const IDC_CONSENT: u16 = 104;
pub const IDC_PROGRESS: u16 = 105;
pub const IDC_PROGRESS_TEXT: u16 = 106;
pub const IDC_VERIFY: u16 = 107;
pub const IDC_CANCEL: u16 = 108;

const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_BORDER: u32 = 0x0080_0000;
const WS_VSCROLL: u32 = 0x0020_0000;
const WS_CLIPCHILDREN: u32 = 0x0200_0000;
const DS_3DLOOK: u32 = 0x0004;
const DS_SETFONT: u32 = 0x0040;
const DS_CONTROL: u32 = 0x0400;
const SS_NOPREFIX: u32 = 0x0080;
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
    vec![
        Item {
            class: Class::Static,
            text: "",
            id: IDC_CLAIM,
            style: vis | SS_NOPREFIX,
            rect: (7, 7, 238, 22),
        },
        Item {
            class: Class::Static,
            text: "",
            id: IDC_HEADLINE,
            style: vis | SS_NOPREFIX,
            rect: (7, 33, 238, 24),
        },
        Item {
            class: Class::Edit,
            text: "",
            id: IDC_DETAILS,
            style: vis
                | WS_TABSTOP
                | WS_BORDER
                | WS_VSCROLL
                | ES_MULTILINE
                | ES_AUTOVSCROLL
                | ES_READONLY,
            rect: (7, 60, 238, 78),
        },
        Item {
            class: Class::Static,
            text: "",
            id: IDC_CONSENT,
            style: vis | SS_NOPREFIX,
            rect: (7, 142, 238, 30),
        },
        Item {
            class: Class::Named("msctls_progress32"),
            text: "",
            id: IDC_PROGRESS,
            style: WS_CHILD | PBS_MARQUEE,
            rect: (7, 176, 238, 8),
        },
        Item {
            class: Class::Static,
            text: "",
            id: IDC_PROGRESS_TEXT,
            style: vis | SS_NOPREFIX,
            rect: (7, 186, 238, 10),
        },
        Item {
            class: Class::Button,
            text: "Verify",
            id: IDC_VERIFY,
            style: vis | WS_TABSTOP | BS_DEFPUSHBUTTON,
            rect: (7, 200, 56, 14),
        },
        Item {
            class: Class::Button,
            text: "Cancel",
            id: IDC_CANCEL,
            style: WS_CHILD | WS_TABSTOP,
            rect: (68, 200, 56, 14),
        },
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
        b.extend(0u32.to_le_bytes());
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
        assert_eq!(ids[0], IDC_CLAIM);
        assert_eq!(*ids.last().unwrap(), IDC_CANCEL);
    }

    #[test]
    fn control_ids_are_unique() {
        let mut ids: Vec<u16> = items().iter().map(|i| i.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), items().len());
    }

    #[test]
    fn progress_and_cancel_start_hidden() {
        for i in items() {
            if i.id == IDC_PROGRESS || i.id == IDC_CANCEL {
                assert_eq!(i.style & WS_VISIBLE, 0);
            }
        }
    }
}
