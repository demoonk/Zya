//! The "diary": a log of every automatic conversion.
//!
//! Two practical constraints shape this module:
//!  * P-05 — the file is read from the *end* in chunks instead of being slurped
//!    whole, and it is rotated once it grows past 2 MB.
//!  * L-12 — writing is optional: `log_entry` takes the flag explicitly and
//!    `log` consults the global flag that `config::load` sets from
//!    `Config::log_enabled`.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::config::config_dir;
use crate::engine::Decision;

/// Rotate the diary once it passes this size, keeping the newest slice in the
/// live file and moving the rest to `diario.log.1`.
pub const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
/// How much of the recent history stays in `diario.log` after a rotation.
pub const KEEP_LOG_BYTES: u64 = 1024 * 1024;
/// Size of the backwards read used by `tail`.
const READ_CHUNK: u64 = 32 * 1024;

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
        w_day: 1,
        w_hour: 0,
        w_minute: 0,
        w_seconds: 0,
        w_ms: 0,
    };
    unsafe { GetLocalTime(&mut t) };
    format!("{:02}:{:02}:{:02}", t.w_hour, t.w_minute, t.w_seconds)
}

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// L-12: `Config::log_enabled` has to reach the writer. `config::load` calls
/// `set_log_enabled`, and `log` consults it, so a disabled diary never touches
/// the disk. Callers that already know the flag can use `log_entry` directly.
static LOG_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(true);

#[derive(Debug, Clone)]
pub struct Entry {
    pub time: String,
    pub original: String,
    pub converted: String,
    pub why: String,
}

static RECENT: OnceLock<Mutex<Vec<Entry>>> = OnceLock::new();

#[allow(dead_code)]
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

pub fn set_log_enabled(enabled: bool) {
    LOG_ENABLED.store(enabled, std::sync::atomic::Ordering::Relaxed);
}

pub fn log_enabled() -> bool {
    LOG_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Write one decision to the diary. `enabled` is the user's "log switches"
/// switch: when it is false nothing is written to disk (the in-memory ring
/// still records the switch, so the UI can show what happened this session).
pub fn log_entry(d: &Decision, count: u32, enabled: bool) {
    log_entry_to(&log_path(), d, count, enabled)
}

/// Same as `log_entry`, but with an explicit destination. Tests use it so they
/// never touch the real `%APPDATA%\zя\diario.log`.
pub fn log_entry_to(path: &Path, d: &Decision, count: u32, enabled: bool) {
    let stamp = local_stamp();
    let line = format!(
        "[{stamp}] #{count} {} -> {} | {}\n",
        d.original, d.converted, d.why
    );

    if enabled {
        if path == log_path() && rotate_if_needed() {
            println!(
                "журнал переполнен: старые записи перенесены в {}",
                archive_path().display()
            );
        }
        if let Ok(mut fh) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = fh.write_all(line.as_bytes());
        }
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

/// Backwards-compatible wrapper used by the hook: it honours the global
/// `log_enabled` flag (L-12).
pub fn log(d: &Decision, count: u32) {
    log_entry(d, count, log_enabled());
}

/// The tail of the diary file. `recent()` only knows about the switches made in
/// the current process, so the settings "Лог" tab would be empty after a fresh
/// start even though the diary has hundreds of lines.
///
/// P-05: the file is read backwards in `READ_CHUNK` blocks until enough lines
/// are in hand, so a 50 MB diary does not cost a 50 MB read (and a second
/// copy) just to show 500 rows.
pub fn tail(limit: usize) -> Vec<Entry> {
    tail_from(&log_path(), limit)
}

fn tail_from(path: &Path, limit: usize) -> Vec<Entry> {
    if limit == 0 {
        return Vec::new();
    }
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    let size = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return Vec::new(),
    };
    if size == 0 {
        return Vec::new();
    }

    let mut window: Vec<u8> = Vec::new();
    let mut pos = size;
    while pos > 0 {
        let take = READ_CHUNK.min(pos);
        let start = pos - take;
        let mut buf = vec![0u8; take as usize];
        if file.seek(std::io::SeekFrom::Start(start)).is_err() {
            break;
        }
        if file.read_exact(&mut buf).is_err() {
            break;
        }
        // Prepend: the chunks are read from the end towards the beginning.
        window.splice(0..0, buf);
        pos = start;
        // One extra line guarantees the first line of the window is complete
        // even if the chunk boundary cut it in half.
        if count_newlines(&window) >= limit + 1 {
            break;
        }
    }

    let mut text = String::from_utf8_lossy(&window).to_string();
    // Notepad writes a BOM; without stripping it the "[..]" index below lands
    // inside the three-byte marker and panics.
    if text.starts_with('\u{feff}') {
        text = text.trim_start_matches('\u{feff}').to_string();
    }
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(limit);
    lines[start..]
        .iter()
        .filter_map(|line| parse_line(line))
        .collect()
}

fn count_newlines(buf: &[u8]) -> usize {
    buf.iter().filter(|byte| **byte == b'\n').count()
}

fn parse_line(line: &str) -> Option<Entry> {
    // "[09:42:12 UTC] #35 тщеузфв -> notepad | вручную: смена раскладки"
    if !line.starts_with('[') {
        return None;
    }
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

pub fn path() -> PathBuf {
    log_path()
}

fn log_path() -> PathBuf {
    LOG_PATH
        .get_or_init(|| config_dir().join("diario.log"))
        .clone()
}

fn archive_path() -> PathBuf {
    config_dir().join("diario.log.1")
}

/// P-05: keep the live diary bounded. Everything older than the last
/// `KEEP_LOG_BYTES` is moved to `diario.log.1`, so the recent entries stay
/// readable and the history is not simply thrown away.
fn rotate_if_needed() -> bool {
    rotate(&log_path(), &archive_path())
}

fn rotate(path: &Path, archive: &Path) -> bool {
    let size = match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(_) => return false,
    };
    if size <= MAX_LOG_BYTES {
        return false;
    }
    let keep_from = size.saturating_sub(KEEP_LOG_BYTES);
    let mut file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.seek(std::io::SeekFrom::Start(keep_from)).is_err() {
        return false;
    }
    let mut tail = Vec::new();
    if file.read_to_end(&mut tail).is_err() {
        return false;
    }
    // Start the fresh file at a line boundary, never in the middle of a record.
    match tail.iter().position(|&b| b == b'\n') {
        Some(nl) => tail = tail[nl + 1..].to_vec(),
        None => tail.clear(),
    }
    if std::fs::rename(path, archive).is_err() {
        return false;
    }
    std::fs::write(path, &tail).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Cargo gives every test a writable scratch directory under `target/`.
        // `temp_dir()` is not always usable in a restricted environment, and
        // writing test files into the project's `data/` directory (the old habit)
        // pollutes the real diary.
        let base = std::env::var("CARGO_TARGET_TMP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        let dir = base.join(format!("zya-diary-{tag}-{secs}"));
        match std::fs::create_dir_all(&dir) {
            Ok(()) => dir,
            Err(_) => {
                let fallback = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join("tmp")
                    .join(format!("zya-diary-{tag}-{secs}"));
                std::fs::create_dir_all(&fallback).expect("temp dir");
                fallback
            }
        }
    }

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
    fn a_line_without_a_bracket_is_skipped() {
        assert!(parse_line("ghbdtn -> привет").is_none());
    }

    #[test]
    fn a_line_without_an_arrow_is_skipped() {
        assert!(parse_line("[09:44:07 UTC] запущен").is_none());
        assert!(parse_line("мусор").is_none());
    }

    /// P-05: `tail` must give the last `limit` records without reading the
    /// whole file, and it still works when the file is a few megabytes.
    #[test]
    fn tail_reads_the_last_records_from_a_big_file() {
        let dir = temp_dir("tail");
        let path = dir.join("diario.log");
        let total = 20_000usize;
        let mut body = String::with_capacity(total * 45);
        for i in 0..total {
            body.push_str(&format!("[12:00:00] #{i} слов{i} -> word{i} | тест\n"));
        }
        std::fs::write(&path, body).expect("write diary");

        let got = tail_from(&path, 5);
        assert_eq!(got.len(), 5);
        assert_eq!(got[0].original, "слов19995");
        assert_eq!(got[4].original, "слов19999");
        assert_eq!(got[4].converted, "word19999");
        assert_eq!(got[4].why, "тест");

        // A larger window still ends at the same record.
        let more = tail_from(&path, 100);
        assert_eq!(more.len(), 100);
        assert_eq!(more[99].original, "слов19999");
        assert!(tail_from(&path, 0).is_empty());
    }

    #[test]
    fn rotation_moves_the_old_entries_to_diario_log_1() {
        let dir = temp_dir("rotate");
        let path = dir.join("diario.log");
        let archive = dir.join("diario.log.1");
        let total = 60_000usize;
        let mut body = String::with_capacity(total * 45);
        for i in 0..total {
            body.push_str(&format!("[12:00:00] #{i} слов{i} -> word{i} | тест\n"));
        }
        std::fs::write(&path, body).expect("write diary");
        let before = std::fs::metadata(&path).unwrap().len();
        assert!(before > MAX_LOG_BYTES, "test file must exceed the limit");

        assert!(rotate(&path, &archive), "rotation should happen");

        let after = std::fs::metadata(&path).unwrap().len();
        assert!(after <= KEEP_LOG_BYTES, "live diary shrank to {after} bytes");
        assert!(archive.exists(), "the old entries were archived");
        assert_eq!(std::fs::metadata(&archive).unwrap().len(), before);

        // The recent records are still readable after the rotation.
        let tail = tail_from(&path, 3);
        assert_eq!(tail.len(), 3);
        assert_eq!(tail[2].original, format!("слов{}", total - 1));

        // Below the limit nothing is rotated.
        assert!(!rotate(&path, &archive), "no rotation while under the limit");
    }

    #[test]
    fn empty_diary_yields_no_entries() {
        let dir = temp_dir("empty");
        let path = dir.join("diario.log");
        std::fs::write(&path, "").unwrap();
        assert!(tail_from(&path, 10).is_empty());
        assert!(tail_from(&dir.join("missing.log"), 10).is_empty());
    }

    /// L-12: the flag really controls disk writes.
    #[test]
    fn log_entry_respects_the_enabled_flag() {
        let dir = temp_dir("flag");
        let path = dir.join("diario.log");
        let d = Decision {
            original: "ghbdtn".into(),
            converted: "привет".into(),
            target: crate::engine::TargetLayout::Russian,
            why: "тест".into(),
            flip: true,
        };
        LOG_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        log_entry_to(&path, &d, 1, false);
        log_entry_to(&path, &d, 2, log_enabled());
        assert!(
            !path.exists(),
            "a disabled diary must not create {}",
            path.display()
        );
        LOG_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
        log_entry_to(&path, &d, 3, log_enabled());
        let rows = tail_from(&path, 10);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].original, "ghbdtn");
        LOG_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
