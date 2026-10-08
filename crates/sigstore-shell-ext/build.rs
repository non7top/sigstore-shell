#[path = "../provenance-core/src/claim_format.rs"]
mod claim_format;

use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_SIZES: [u32; 6] = [16, 20, 24, 32, 48, 64];

/// (SVG file, resource id); the ids match `ICON_*` in dlgtemplate.rs.
const ICONS: [(&str, &str); 4] = [
    ("Kliponious-green-tick.svg", "201"),
    ("failed.svg", "202"),
    ("neutral.svg", "203"),
    ("warning.svg", "204"),
];

fn view_box(svg: &str) -> (f64, f64) {
    let attr = svg
        .split("viewBox=\"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("SVG has a viewBox");
    let n: Vec<f64> = attr
        .split_whitespace()
        .map(|v| v.parse().unwrap())
        .collect();
    (n[2], n[3])
}

/// Renders the SVG centred on a square canvas, so a non-square drawing is not stretched.
fn render_png(svg: &Path, size: u32) -> Vec<u8> {
    let (w, h) = view_box(&std::fs::read_to_string(svg).unwrap());
    let scale = f64::from(size) / w.max(h);
    let (pw, ph) = ((w * scale).round() as u32, (h * scale).round() as u32);
    let out = Command::new("rsvg-convert")
        .args(["-f", "png", "-w", &pw.to_string(), "-h", &ph.to_string()])
        .args([
            "--page-width",
            &size.to_string(),
            "--page-height",
            &size.to_string(),
        ])
        .args(["--left", &((size - pw) / 2).to_string()])
        .args(["--top", &((size - ph) / 2).to_string()])
        .arg(svg)
        .output()
        .expect("run rsvg-convert (apt: librsvg2-bin)");
    assert!(
        out.status.success(),
        "rsvg-convert failed on {}",
        svg.display()
    );
    out.stdout
}

/// An .ico whose entries are PNGs.
fn ico(svg: &Path) -> Vec<u8> {
    let pngs: Vec<(u32, Vec<u8>)> = ICON_SIZES
        .iter()
        .map(|&s| (s, render_png(svg, s)))
        .collect();
    let mut out = vec![0, 0, 1, 0];
    out.extend((pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (size, png) in &pngs {
        out.extend([*size as u8, *size as u8, 0, 0]);
        out.extend(1u16.to_le_bytes());
        out.extend(32u16.to_le_bytes());
        out.extend((png.len() as u32).to_le_bytes());
        out.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in pngs {
        out.extend(png);
    }
    out
}

fn main() {
    println!("cargo:rerun-if-env-changed=PROVENANCE_REPO");
    println!("cargo:rerun-if-env-changed=RELEASE_VERSION");
    println!("cargo:rerun-if-changed=../../resources/icons");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for (file, id) in ICONS {
        let dest = out_dir.join(format!("icon{id}.ico"));
        std::fs::write(&dest, ico(&Path::new("../../resources/icons").join(file))).unwrap();
        res.set_icon_with_id(dest.to_str().unwrap(), id);
    }
    // Only release builds set this, so a local build does not claim to come from the repo's workflow.
    if let Ok(repo) = std::env::var("PROVENANCE_REPO") {
        res.set(claim_format::PE_KEY, &repo);
    }
    // The crate version stays fixed: release-please cannot update Cargo.lock, and --locked builds need it to match.
    if let Ok(version) = std::env::var("RELEASE_VERSION") {
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
    }
    res.compile().expect("compile version resource");
}
