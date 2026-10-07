//! Explorer property page that shows an exe's Sigstore provenance.
//!
//! Everything except the `windows`-only modules is plain Rust and unit-tested off Windows.

pub mod cache;
pub mod dlgtemplate;
pub mod job;
pub mod model;
pub mod registration;
pub mod settings;

#[cfg(windows)]
mod com;
#[cfg(windows)]
mod dll;
#[cfg(windows)]
mod page;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod worker;
