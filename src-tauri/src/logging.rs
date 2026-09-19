use log::{Level, LevelFilter};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::LazyLock;

pub const FILE_STEM: &str = if cfg!(debug_assertions) {
    "sparkle-dev"
} else {
    "sparkle"
};

const MAX_FILE_BYTES: u128 = 2 * 1024 * 1024;
const ARCHIVED_FILES: usize = 2;
static LOG_LEVEL: AtomicU8 = AtomicU8::new(LogLevel::Info as u8);

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum LogLevel {
    Error = 1,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    fn level(self) -> Level {
        match self {
            Self::Error => Level::Error,
            Self::Warn => Level::Warn,
            Self::Info => Level::Info,
            Self::Debug => Level::Debug,
            Self::Trace => Level::Trace,
        }
    }
}

pub fn deserialize_setting<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<LogLevel, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum SavedLevel {
        Level(LogLevel),
        Verbose(bool),
    }
    // Settings embedded in older backups use the verbose boolean.
    Ok(match SavedLevel::deserialize(deserializer)? {
        SavedLevel::Level(level) => level,
        SavedLevel::Verbose(true) => LogLevel::Debug,
        SavedLevel::Verbose(false) => LogLevel::Info,
    })
}

pub fn set_level(level: LogLevel) {
    let previous = LOG_LEVEL.swap(level as u8, Ordering::Relaxed);
    // Skip disabled log arguments at the call site, including locks and formatting.
    log::set_max_level(level.level().to_level_filter());
    if previous != level as u8 {
        log::info!(target: "sparkle::logging", "event=level_changed level={}", level.level());
    }
}

fn should_emit(level: Level, target: &str, configured: u8) -> bool {
    let application = target == "sparkle"
        || target.starts_with("sparkle::")
        || target == "sparkle_lib"
        || target.starts_with("sparkle_lib::");
    level as u8 <= configured && (application || level <= Level::Warn)
}

// Errors from HTTP clients can contain signed URLs or credentials. Keep every
// record on one bounded line; callers must still avoid logging secrets/payloads.
fn sanitize(message: &str) -> String {
    static URL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"(?i)https?://[^\s<>\"']+"#).unwrap());
    let redacted = URL.replace_all(message, "[url]");
    let mut output = String::new();
    for (index, character) in redacted.chars().enumerate() {
        if index == 4096 {
            output.push_str("…[truncated]");
            break;
        }
        match character {
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            c if c.is_control() => output.push(' '),
            c => output.push(c),
        }
    }
    output
}

pub fn plugin<R: tauri::Runtime>() -> tauri::plugin::TauriPlugin<R> {
    use tauri_plugin_log::{RotationStrategy, Target, TargetKind};
    let target = |kind| {
        Target::new(kind).format(|out, message, _| {
            // The parent formatter supplies UTC time, target, and severity.
            out.finish(format_args!("{}", sanitize(&message.to_string())));
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
        .filter(|metadata| {
            should_emit(
                metadata.level(),
                metadata.target(),
                LOG_LEVEL.load(Ordering::Relaxed),
            )
        })
        .max_file_size(MAX_FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(ARCHIVED_FILES))
        .build()
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 64
        && label
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

#[tauri::command]
pub fn log_frontend(
    level: LogLevel,
    scope: String,
    event: String,
    message: Option<String>,
) -> Result<(), String> {
    if !valid_label(&scope)
        || !valid_label(&event)
        || message.as_ref().is_some_and(|s| s.len() > 8192)
    {
        return Err("Invalid frontend log record".into());
    }
    log::log!(
        target: "sparkle::frontend",
        level.level(),
        "event={event} scope={scope}{}",
        message.map(|m| format!(" error={m}")).unwrap_or_default()
    );
    Ok(())
}

#[cfg(test)]
#[path = "tests/logging.rs"]
mod tests;
