use rusqlite::{Connection, Result};
pub use sparkle_core::db::{open_connection, recover_interrupted_listens};
use std::fs;
use tauri::{AppHandle, Manager};

/// Returns the profile-specific application data directory.
///
/// Release builds keep the existing Tauri directory so installed users do
/// not lose their library. Debug builds use a sibling directory instead,
/// preventing `tauri dev` from opening or modifying production data.
pub fn data_dir(app: &AppHandle) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    if !cfg!(debug_assertions) {
        return base;
    }

    let Some(name) = base.file_name() else {
        return base.join("dev");
    };
    let mut dev_name = name.to_os_string();
    dev_name.push("-dev");
    base.with_file_name(dev_name)
}

pub fn db_path(app: &AppHandle) -> std::path::PathBuf {
    let dir = data_dir(app);
    fs::create_dir_all(&dir).expect("failed to create app data dir");
    dir.join("sparkle.db")
}

pub fn init_db(app: &AppHandle) -> Result<(Connection, bool)> {
    sparkle_core::db::init_db(&db_path(app))
}

#[cfg(test)]
#[path = "tests/db.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::test_connection;
