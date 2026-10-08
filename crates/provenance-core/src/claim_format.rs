//! Names of the embedded claim; project.md ("Embedded claim") is the normative spec.
//! Shared with the build scripts through `#[path]`, so keep it dependency-free.
#![allow(dead_code)]

/// PE version-resource string name.
pub const PE_KEY: &str = "ProvenanceRepo";
/// ELF section holding the note.
pub const ELF_SECTION: &str = ".note.provenance";
/// ELF note name (without the terminating NUL).
pub const ELF_NOTE_NAME: &str = "ProvenanceRepo";
/// ELF note type under `ELF_NOTE_NAME`.
pub const ELF_NOTE_TYPE: u32 = 1;
