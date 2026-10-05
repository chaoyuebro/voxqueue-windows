#![cfg(windows)]
// Compile the adapter in a focused harness: legacy Unix-only lib tests cannot build on Windows.
pub use easy_codex_host::{codex_catalog, store, windows_paths};
#[path = "../src/desktop_runner.rs"]
mod desktop_runner;
