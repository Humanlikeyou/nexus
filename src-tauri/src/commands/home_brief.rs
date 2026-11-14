//! The home screen's greeting and weather, read from `~/.hermes/amara/home_brief.json`, which a
//! silent Hermes cron job (`scripts/home_brief.py`) rewrites every 30 minutes. Read-only, and a
//! missing or damaged file just means the home screen falls back to its local greeting.
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

const MAX_BYTES: u64 = 8_192;
const MAX_TEXT: usize = 80;
const MAX_WEATHER_AGE_HOURS: i64 = 3;
const MAX_GREETING_AGE_HOURS: i64 = 3;

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeWeather {
    pub place: Option<String>,
    pub temp_c: Option<f64>,
    pub summary: Option<String>,
    pub high_c: Option<f64>,
    pub low_c: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct StoredBrief {
    greeting: Option<String>,
    weather: Option<HomeWeather>,
    weather_at: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HomeBrief {
    pub greeting: Option<String>,
    pub weather: Option<HomeWeather>,
    pub updated_at: Option<String>,
}

fn clip(text: Option<String>) -> Option<String> {
    let clean: String = text?
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect();
    let clean = clean.trim().to_string();
    (!clean.is_empty()).then_some(clean)
}

fn fresh(stamp: Option<&str>, now: DateTime<Utc>, hours: i64) -> bool {
    stamp
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .is_some_and(|t| now.signed_duration_since(t) <= Duration::hours(hours))
}

/// Stale or unusable values come back as None, so the screen hides them.
fn parse_brief(text: &str, now: DateTime<Utc>) -> HomeBrief {
    let Ok(stored) = serde_json::from_str::<StoredBrief>(text) else {
        return HomeBrief::default();
    };
    let greeting = fresh(stored.updated_at.as_deref(), now, MAX_GREETING_AGE_HOURS)
        .then(|| clip(stored.greeting))
        .flatten();
    let weather = fresh(stored.weather_at.as_deref(), now, MAX_WEATHER_AGE_HOURS)
        .then_some(stored.weather)
        .flatten()
        .filter(|w| w.temp_c.is_some_and(f64::is_finite))
        .map(|w| HomeWeather {
            place: clip(w.place),
            summary: clip(w.summary),
            ..w
        });
    HomeBrief {
        greeting,
        weather,
        updated_at: clip(stored.updated_at),
    }
}

fn brief_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".hermes/amara/home_brief.json"))
}

#[tauri::command]
#[specta::specta]
pub fn home_brief() -> HomeBrief {
    let read = || {
        // O_NOFOLLOW and limits on the open handle, so the file can't change between check and read.
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(brief_path()?)
            .ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        let mut text = String::new();
        file.take(MAX_BYTES).read_to_string(&mut text).ok()?;
        Some(text)
    };
    read().map_or_else(HomeBrief::default, |text| parse_brief(&text, Utc::now()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(hour: u32) -> DateTime<Utc> {
        format!("2026-11-05T{hour:02}:00:00Z").parse().unwrap()
    }

    const GOOD: &str = r#"{"greeting":"Good morning, Taisun.","updatedAt":"2026-11-05T09:00:00+09:00",
        "weatherAt":"2026-11-05T09:00:00+09:00",
        "weather":{"place":"Utsunomiya","tempC":14,"summary":"rain","highC":18,"lowC":11}}"#;

    #[test]
    fn a_fresh_file_comes_through() {
        let brief = parse_brief(GOOD, at(1));
        assert_eq!(brief.greeting.as_deref(), Some("Good morning, Taisun."));
        assert_eq!(brief.weather.unwrap().temp_c, Some(14.0));
    }

    #[test]
    fn stale_values_are_hidden() {
        let brief = parse_brief(GOOD, at(5));
        assert_eq!(brief.greeting, None);
        assert_eq!(brief.weather, None);
    }

    #[test]
    fn damaged_or_empty_files_fall_back_quietly() {
        assert_eq!(parse_brief("{broken", at(1)), HomeBrief::default());
        assert_eq!(parse_brief("{}", at(1)).weather, None);
        let no_temp = r#"{"weatherAt":"2026-11-05T09:00:00+09:00","weather":{"summary":"rain"}}"#;
        assert_eq!(parse_brief(no_temp, at(1)).weather, None);
    }

    #[test]
    fn text_is_cleaned_and_capped() {
        let long = format!(
            r#"{{"greeting":"Hi\n{}","updatedAt":"2026-11-05T09:00:00+09:00"}}"#,
            "x".repeat(200)
        );
        let greeting = parse_brief(&long, at(1)).greeting.unwrap();
        assert_eq!(greeting.chars().count(), MAX_TEXT);
        assert!(!greeting.contains('\n'));
    }
}
