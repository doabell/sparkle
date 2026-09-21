use log::LevelFilter;
pub use sparkle_core::logging::{set_level, LogLevel, FILE_STEM};

const MAX_FILE_BYTES: u128 = 2 * 1024 * 1024;
const ARCHIVED_FILES: usize = 2;

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    let target = |kind| {
        Target::new(kind).format(|out, message, _| {
            // The parent formatter supplies UTC time, target, and severity.
            out.finish(format_args!(
                "{}",
                sparkle_core::logging::sanitize(&message.to_string())
            ));
        })
    };
    tauri_plugin_log::Builder::new()
        .targets([
            target(TargetKind::Stdout),
            target(TargetKind::LogDir {
                file_name: Some(FILE_STEM.into()),
            }),
        ])
        .level(LevelFilter::Trace)
        .filter(sparkle_core::logging::enabled)
        .max_file_size(MAX_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(ARCHIVED_FILES))
        .build()
}

#[tauri::command]
pub fn log_frontend(
    level: LogLevel,
    scope: String,
    event: String,
    message: Option<String>,
) -> Result<(), String> {
    sparkle_core::logging::log_frontend(level, scope, event, message)
}
