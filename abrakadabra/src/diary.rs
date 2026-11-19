//! The "diary": a log of every automatic conversion.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use crate::config::config_dir;
use crate::engine::Decision;

#[repr(C)]
struct SYSTEMTIME {
    w_year: u16,
    w_month: u16,
    w_day_of_week: u16,
    w_day: u16,
    w_hour: u16,
    w_minute: u16,
    w_seconds: u16,
    w_ms: u16,
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLocalTime(time: *mut SYSTEMTIME);
}

/// Diary timestamps are printed in the machine's local time: the user reads the
/// log next to the clock on the task bar, not in UTC.
fn local_stamp() -> String {
    let mut t = SYSTEMTIME {
        w_year: 0,
        w_month: 0,
        w_day_of_week: 0,
        w_day: 0,
        w_hour: 0,
        w_minute: 0,
        w_seconds: 0,
        w_ms: 0,
    };
    unsafe { GetLocalTime(&mut t) };
    format!("{:02}:{:02}:{:02}", t.w_hour, t.w_minute, t.w_seconds)
}

static LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

#[derive(Debug, Clone)]
pub struct Entry {
    pub time: String,
    pub original: String,
    pub converted: String,
    pub why: String,
}

static RECENT: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();

pub fn recent() -> Vec<Entry> {
    RECENT
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

pub fn clear_recent() {
    let recent = RECENT.get_or_init(|| Mutex::new(Vec::new()));
    recent.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

/// The tail of the diary file. `recent()` only knows about the switches made in
/// the current process, so the settings "Лог" tab would be empty after a fresh
/// start even though the diary has hundreds of lines.
pub fn tail(limit: usize) -> Vec<Entry> {
    let text = match std::fs::read_to_string(log_path()) {
        Ok(text) => text,
        Err(_) => return Vec::new(),
    };
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .filter_map(|line| parse_line(line))
        .collect()
}

fn parse_line(line: &str) -> Option<Entry> {
    // "[09:42:12 UTC] #35 тщеузфв -> notepad | вручную: смена раскладки"
    let close = line.find(']')?;
    let time = line[1..close].trim().to_string();
    let rest = line[close + 1..].trim();
    let rest = rest.strip_prefix('#').unwrap_or(rest);
    let space = rest.find(' ')?;
    let body = rest[space + 1..].trim();

    let arrow = body.find(" -> ")?;
    let original = body[..arrow].trim().to_string();
    let tail = body[arrow + 4..].trim();
    let parts: Vec<&str> = tail.split(" | ").collect();
    let converted = parts[0].trim().to_string();
    let why = parts[1..].join(" | ");
    if original.is_empty() || converted.is_empty() {
        return None;
    }
    Some(Entry {
        time,
        original,
        converted,
        why,
    })
}

pub fn path() -> std::path::PathBuf {
    log_path()
}

fn log_path() -> std::path::PathBuf {
    LOG_PATH
        .get_or_init(|| config_dir().join("diario.log"))
        .clone()
}

pub fn log(d: &Decision, count: u32) {
    let stamp = local_stamp();
    let line = format!(
        "[{stamp}] #{count} {} -> {} | {}\n",
        d.original, d.converted, d.why
    );
    if let Ok(mut fh) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = fh.write_all(line.as_bytes());
    }

    let recent = RECENT.get_or_init(|| Mutex::new(Vec::new()));
    {
        let mut guard = recent.lock().unwrap_or_else(|e| e.into_inner());
        guard.push(Entry {
            time: stamp,
            original: d.original.clone(),
            converted: d.converted.clone(),
            why: d.why.clone(),
        });
        if guard.len() > 200 {
            let extra = guard.len() - 200;
            guard.drain(..extra);
        }
    }

    println!("{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_diary_line() {
        let entry = parse_line("[09:44:07 UTC] #19 тщеузфв -> notepad | вручную: смена раскладки")
            .expect("line should parse");
        assert_eq!(entry.time, "09:44:07 UTC");
        assert_eq!(entry.original, "тщеузфв");
        assert_eq!(entry.converted, "notepad");
        assert_eq!(entry.why, "вручную: смена раскладки");
    }

    #[test]
    fn a_line_without_an_arrow_is_skipped() {
        assert!(parse_line("[09:44:07 UTC] запущен").is_none());
        assert!(parse_line("мусор").is_none());
    }
}
