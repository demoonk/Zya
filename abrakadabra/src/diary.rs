//! The "diary": a log of every automatic conversion.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::config_dir;
use crate::engine::Decision;

static LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

pub fn path() -> std::path::PathBuf {
    log_path()
}

fn log_path() -> std::path::PathBuf {
    LOG_PATH
        .get_or_init(|| config_dir().join("diario.log"))
        .clone()
}

pub fn log(d: &Decision, count: u32) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (h, m, s) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let line = format!(
        "[{h:02}:{m:02}:{s:02} UTC] #{count} {} -> {} | {}\n",
        d.original, d.converted, d.why
    );
    if let Ok(mut fh) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path())
    {
        let _ = fh.write_all(line.as_bytes());
    }
    println!("{line}");
}
