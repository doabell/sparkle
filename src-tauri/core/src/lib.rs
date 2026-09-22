//! Sparkle's library and provider logic, independent of Tauri and audio devices.

pub mod analytics;
pub mod artwork_store;
pub mod audio_identity;
pub mod backup;
pub mod cache;
pub mod db;
pub mod db_writer;
pub mod diagnostics;
pub mod http_client;
pub mod library_scan;
pub mod logging;
pub mod models;
pub mod normalizer;
pub mod providers;
pub mod scanner;
pub mod settings;

#[cfg(test)]
#[path = "tests/support.rs"]
mod test_support;
