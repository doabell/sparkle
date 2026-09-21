//! Line-level LRC semantics, also exercised by the frontend against the shared
//! `test/fixtures/lrc.json` contract. Blank cues are deliberately ignored: the
//! current sentence stays visible through instrumental gaps.

use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

const MAX_TIME: i64 = 9_007_199_254_740_991; // JavaScript's largest safe integer.
static TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[(\d+):(\d+(?:\.\d+)?)\]").unwrap());
static WORD_TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(\d+):(\d+(?:\.\d+)?)>").unwrap());
static OFFSET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?im)^\s*\[offset:\s*([+-]?\d+)\s*\]\s*$").unwrap());
static METADATA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^\[(?:ar|al|ti|au|by|offset|re|ve|length|id):[^\]]*\]$").unwrap()
});

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcLine {
    pub time_ms: i64,
    pub text: String,
}

fn timestamp_ms(captures: &Captures<'_>) -> Option<i64> {
    let minutes = captures.get(1)?.as_str().parse::<f64>().ok()?;
    let seconds = captures.get(2)?.as_str().parse::<f64>().ok()?;
    let value = ((minutes * 60.0 + seconds) * 1000.0).round();
    (value.is_finite() && value >= 0.0 && value <= MAX_TIME as f64).then_some(value as i64)
}

pub fn file_offset_ms(text: &str) -> i64 {
    OFFSET
        .captures_iter(&normalized_newlines(text))
        .filter_map(|c| {
            c[1].parse::<i64>()
                .ok()
                .filter(|v| v.unsigned_abs() <= MAX_TIME as u64)
        })
        .last()
        .unwrap_or(0)
}

fn normalized_newlines(text: &str) -> String {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn clean_text(text: &str) -> String {
    WORD_TIMESTAMP.replace_all(text, "").trim().to_string()
}

pub fn parse_lrc(text: &str) -> Vec<LrcLine> {
    let offset = file_offset_ms(text);
    let mut lines = Vec::new();
    for raw in normalized_newlines(text).lines() {
        let raw = raw.trim();
        if !raw.starts_with('[') || METADATA.is_match(raw) {
            continue;
        }
        let times: Vec<i64> = TIMESTAMP
            .captures_iter(raw)
            .filter_map(|c| timestamp_ms(&c))
            .collect();
        let lyric = clean_text(&TIMESTAMP.replace_all(raw, ""));
        if lyric.is_empty() {
            continue;
        }
        for time in times {
            lines.push(LrcLine {
                time_ms: time.saturating_sub(offset).clamp(0, MAX_TIME),
                text: lyric.clone(),
            });
        }
    }
    lines.sort_by_key(|line| line.time_ms);
    lines
}

pub fn strip_lrc_timestamps(text: &str) -> String {
    let lines = parse_lrc(text);
    if !lines.is_empty() {
        return lines
            .into_iter()
            .map(|l| l.text)
            .collect::<Vec<_>>()
            .join("\n");
    }
    normalized_newlines(text)
        .lines()
        .map(str::trim)
        .filter(|line| !METADATA.is_match(line))
        .map(|line| clean_text(&TIMESTAMP.replace_all(line, "")))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn first_synced_line(text: &str) -> Option<String> {
    parse_lrc(text).into_iter().next().map(|line| line.text)
}

pub fn format_timestamp(time_ms: i64) -> String {
    let time_ms = time_ms.max(0);
    format!(
        "{:02}:{:02}.{:03}",
        time_ms / 60_000,
        time_ms / 1000 % 60,
        time_ms % 1000
    )
}

/// Bake both the file header's offset and Sparkle's per-track delay into the
/// timestamps. LRC positive offsets advance lyrics; Sparkle positive offsets
/// delay them. Keep metadata, blank cues and word cues intact for export.
pub fn apply_lrc_offset(text: &str, delay_ms: i64) -> String {
    let offset = file_offset_ms(text);
    let shifted_time = |time: i64| {
        time.saturating_sub(offset)
            .clamp(0, MAX_TIME)
            .saturating_add(delay_ms)
            .clamp(0, MAX_TIME)
    };
    normalized_newlines(text)
        .split('\n')
        .map(|line| {
            if OFFSET.is_match(line) {
                return "[offset:0]".to_string();
            }
            let shifted = TIMESTAMP.replace_all(line, |c: &Captures<'_>| {
                timestamp_ms(c)
                    .map(|time| format!("[{}]", format_timestamp(shifted_time(time))))
                    .unwrap_or_else(|| c[0].to_string())
            });
            WORD_TIMESTAMP
                .replace_all(&shifted, |c: &Captures<'_>| {
                    timestamp_ms(c)
                        .map(|time| format!("<{}>", format_timestamp(shifted_time(time))))
                        .unwrap_or_else(|| c[0].to_string())
                })
                .into_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn inject_translation(original: &str, translation: &str) -> String {
    let translated: std::collections::HashMap<i64, String> = parse_lrc(translation)
        .into_iter()
        .map(|line| (line.time_ms, line.text))
        .collect();
    parse_lrc(original)
        .into_iter()
        .map(|line| {
            let text = match translated.get(&line.time_ms) {
                Some(translation) if translation != &line.text => {
                    format!("{}/{translation}", line.text)
                }
                _ => line.text,
            };
            format!("[{}]{text}", format_timestamp(line.time_ms))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
#[path = "tests/document.rs"]
mod tests;
