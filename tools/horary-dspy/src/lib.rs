#![forbid(unsafe_code)]
pub mod anchors;
pub mod book;
pub mod catalogue;
pub mod codex;
pub mod demo;
pub mod functions;
pub mod google;
pub mod journey;
#[cfg(test)]
mod journey_tests;
// These files belong to the application crate's formatting edition.
#[rustfmt::skip]
#[path = "../../../src-tauri/src/reading_method.rs"]
pub mod native_evidence;
#[rustfmt::skip]
#[path = "../../../src-tauri/src/geocode.rs"]
pub mod native_geocode;
pub mod native_export;
pub mod optimize;
pub mod preflight;
pub mod queue;
pub mod runner;
pub mod signatures;
pub mod teachers;
