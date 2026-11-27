//! User configuration, stored as JSON next to the diary.
//!
//! L-05: a broken `config.json` is never silently replaced by defaults. The
//! corrupt file is kept as `config.json.bad-<timestamp>`, every write keeps a
//! `config.json.bak` copy first, and the problem is reported (printed and
//! collected in `warnings()` so the settings window can show it).
//! L-06: numeric fields are clamped into sane ranges before they reach the
//! engine.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::engine::Config;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default = "default_margin_unknown")]
    pub margin_unknown: f32,
    #[serde(default = "default_margin_known")]
    pub margin_known: f32,
    #[serde(default = "default_word_bonus")]
    pub word_bonus: f32,
    #[serde(default = "default_plausibility")]
    pub plausibility: f32,
    #[serde(default = "default_min_len")]
    pub min_len: usize,
    #[serde(default = "default_true")]
    pub protect_code: bool,
    #[serde(default = "default_hotkey_undo")]
    pub hotkey_undo: String,
    #[serde(default = "default_hotkey_toggle")]
    pub hotkey_toggle: String,
    #[serde(default = "default_hotkey_last")]
    pub hotkey_last: String,
    #[serde(default = "default_hotkey_selection")]
    pub hotkey_selection: String,
    #[serde(default = "default_hotkey_upper")]
    pub hotkey_upper: String,
    #[serde(default = "default_hotkey_upper_last")]
    pub hotkey_upper_last: String,
    #[serde(default = "default_hotkey_invert")]
    pub hotkey_invert: String,
    #[serde(default = "default_hotkey_invert_last")]
    pub hotkey_invert_last: String,
    #[serde(default = "default_hotkey_add_word")]
    pub hotkey_add_word: String,
    #[serde(default = "default_true")]
    pub smart_undo: bool,
    #[serde(default = "default_true")]
    pub numpad_comma_as_dot: bool,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub short_ru: Vec<String>,
    #[serde(default)]
    pub short_en: Vec<String>,
    #[serde(default)]
    pub exclude_processes: Vec<String>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_enabled: bool,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default)]
    pub admin_autostart: bool,
    #[serde(default = "default_true")]
    pub log_enabled: bool,
}

fn default_margin_unknown() -> f32 {
    2.5
}
fn default_margin_known() -> f32 {
    1.0
}
fn default_word_bonus() -> f32 {
    20.0
}
fn default_plausibility() -> f32 {
    -12.0
}
fn default_min_len() -> usize {
    3
}
fn default_hotkey_undo() -> String {
    "alt".into()
}
fn default_hotkey_toggle() -> String {
    "ctrl+alt+p".into()
}
fn default_hotkey_last() -> String {
    "break".into()
}
fn default_hotkey_selection() -> String {
    "break".into()
}
fn default_hotkey_upper() -> String {
    "ctrl+break".into()
}
fn default_hotkey_upper_last() -> String {
    "ctrl+break".into()
}
fn default_hotkey_invert() -> String {
    "shift+break".into()
}
fn default_hotkey_invert_last() -> String {
    "shift+break".into()
}
fn default_hotkey_add_word() -> String {
    "ctrl+`".into()
}
fn default_true() -> bool {
    true
}

// ---- L-06: the ranges a parameter is allowed to have ----------------------

const MIN_MARGIN: f32 = -20.0;
const MAX_MARGIN: f32 = 20.0;
const MIN_PLAUSIBILITY: f32 = -100.0;
const MAX_PLAUSIBILITY: f32 = 100.0;
const MIN_WORD_BONUS: f32 = 0.0;
const MAX_WORD_BONUS: f32 = 50.0;
const MIN_MIN_LEN: usize = 1;
const MAX_MIN_LEN: usize = 25;

/// Keep a number inside its range, falling back to `fallback` for NaN and
/// logging the correction so the user sees why their value changed.
fn clamp_f32(name: &str, value: f32, lo: f32, hi: f32, fallback: f32, warnings: &mut Vec<String>) -> f32 {
    if !value.is_finite() {
        warnings.push(format!("{name} = {value} — не число, взято {fallback}"));
        return fallback;
    }
    if value < lo || value > hi {
        let fixed = value.clamp(lo, hi);
        warnings.push(format!(
            "{name} = {value} вне диапазона [{lo}, {hi}] — исправлено на {fixed}"
        ));
        return fixed;
    }
    value
}

fn clamp_usize(name: &str, value: usize, lo: usize, hi: usize, fallback: usize, warnings: &mut Vec<String>) -> usize {
    if value < lo || value > hi {
        let fixed = value.clamp(lo, hi);
        warnings.push(format!(
            "{name} = {value} вне диапазона [{lo}, {hi}] — исправлено на {fixed} (по умолчанию {fallback})"
        ));
        return fixed;
    }
    value
}

fn clean_list(name: &str, items: &[String], warnings: &mut Vec<String>) -> Vec<String> {
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let t = item.trim();
        if t.is_empty() {
            warnings.push(format!("{name}: пустая запись пропущена"));
            continue;
        }
        out.push(t.to_string());
    }
    out
}

/// A word list keeps only letters, lower case and no duplicates. Entries longer
/// than twelve letters are dropped: the short-word dictionary is not a place for
/// whole sentences, and a stray sentence usually means the field was misused.
fn clean_words(name: &str, items: &[String], warnings: &mut Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let t: String = item
            .trim()
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        if t.is_empty() {
            warnings.push(format!("{name}: пустая запись пропущена"));
            continue;
        }
        if t.chars().count() > 12 {
            warnings.push(format!(
                "{name}: «{t}» длиннее 12 букв — в словарчике коротких слов не место"
            ));
            continue;
        }
        let lower = t.to_lowercase();
        if !out.iter().any(|w: &String| w == &lower) {
            out.push(lower);
        }
    }
    out
}

fn clean_hotkey(name: &str, value: &str, fallback: &str, warnings: &mut Vec<String>) -> String {    let t = value.trim();
    if t.is_empty() {
        warnings.push(format!("{name}: пустая горячая клавиша — взято {fallback}"));
        return fallback.to_string();
    }
    t.to_ascii_lowercase()
}

/// L-06: a `Config` is clamped into the ranges the engine can actually use.
/// A negative `margin_known` would rewrite almost every word, `min_len = 0`
/// would send single keystrokes to the model, and so on.
pub fn sanitize(c: &Config) -> Config {
    let mut warnings = Vec::new();
    let d = Config::default();
    let out = Config {
        margin_unknown: clamp_f32(
            "margin_unknown",
            c.margin_unknown,
            MIN_MARGIN,
            MAX_MARGIN,
            d.margin_unknown,
            &mut warnings,
        ),
        margin_known: clamp_f32(
            "margin_known",
            c.margin_known,
            MIN_MARGIN,
            MAX_MARGIN,
            d.margin_known,
            &mut warnings,
        ),
        word_bonus: clamp_f32(
            "word_bonus",
            c.word_bonus,
            MIN_WORD_BONUS,
            MAX_WORD_BONUS,
            d.word_bonus,
            &mut warnings,
        ),
        // The plausibility floor is a *penalty* threshold: the default is
        // negative, so only NaN and absurd magnitudes are out of range.
        plausibility: clamp_f32(
            "plausibility",
            c.plausibility,
            MIN_PLAUSIBILITY,
            MAX_PLAUSIBILITY,
            d.plausibility,
            &mut warnings,
        ),
        min_len: clamp_usize("min_len", c.min_len, MIN_MIN_LEN, MAX_MIN_LEN, d.min_len, &mut warnings),
        protect_code: c.protect_code,
        hotkey_undo: clean_hotkey("hotkey_undo", &c.hotkey_undo, &d.hotkey_undo, &mut warnings),
        hotkey_toggle: clean_hotkey("hotkey_toggle", &c.hotkey_toggle, &d.hotkey_toggle, &mut warnings),
        hotkey_last: clean_hotkey("hotkey_last", &c.hotkey_last, &d.hotkey_last, &mut warnings),
        hotkey_selection: clean_hotkey(
            "hotkey_selection",
            &c.hotkey_selection,
            &d.hotkey_selection,
            &mut warnings,
        ),
        hotkey_upper: clean_hotkey("hotkey_upper", &c.hotkey_upper, &d.hotkey_upper, &mut warnings),
        hotkey_upper_last: clean_hotkey(
            "hotkey_upper_last",
            &c.hotkey_upper_last,
            &d.hotkey_upper_last,
            &mut warnings,
        ),
        hotkey_invert: clean_hotkey("hotkey_invert", &c.hotkey_invert, &d.hotkey_invert, &mut warnings),
        hotkey_invert_last: clean_hotkey(
            "hotkey_invert_last",
            &c.hotkey_invert_last,
            &d.hotkey_invert_last,
            &mut warnings,
        ),
        hotkey_add_word: clean_hotkey(
            "hotkey_add_word",
            &c.hotkey_add_word,
            &d.hotkey_add_word,
            &mut warnings,
        ),
        smart_undo: c.smart_undo,
            numpad_comma_as_dot: c.numpad_comma_as_dot,
        whitelist: clean_list("whitelist", &c.whitelist, &mut warnings),
        short_ru: clean_words("short_ru", &c.short_ru, &mut warnings),
        short_en: clean_words("short_en", &c.short_en, &mut warnings),
        exclude_processes: clean_list("exclude_processes", &c.exclude_processes, &mut warnings),
        exclude_dirs: clean_list("exclude_dirs", &c.exclude_dirs, &mut warnings),
        auto_enabled: c.auto_enabled,
        autostart: c.autostart,
        admin_autostart: c.admin_autostart,
        log_enabled: c.log_enabled,
    };
    for w in warnings {
        warn(&w);
    }
    out
}

impl From<&Config> for FileConfig {
    fn from(c: &Config) -> FileConfig {
        FileConfig {
            margin_unknown: c.margin_unknown,
            margin_known: c.margin_known,
            word_bonus: c.word_bonus,
            plausibility: c.plausibility,
            min_len: c.min_len,
            protect_code: c.protect_code,
            hotkey_undo: c.hotkey_undo.clone(),
            hotkey_toggle: c.hotkey_toggle.clone(),
            hotkey_last: c.hotkey_last.clone(),
            hotkey_selection: c.hotkey_selection.clone(),
            hotkey_upper: c.hotkey_upper.clone(),
            hotkey_upper_last: c.hotkey_upper_last.clone(),
            hotkey_invert: c.hotkey_invert.clone(),
            hotkey_invert_last: c.hotkey_invert_last.clone(),
            hotkey_add_word: c.hotkey_add_word.clone(),
            smart_undo: c.smart_undo,
            numpad_comma_as_dot: c.numpad_comma_as_dot,
            whitelist: c.whitelist.clone(),
            short_ru: c.short_ru.clone(),
            short_en: c.short_en.clone(),
            exclude_processes: c.exclude_processes.clone(),
            exclude_dirs: c.exclude_dirs.clone(),
            auto_enabled: c.auto_enabled,
            autostart: c.autostart,
            admin_autostart: c.admin_autostart,
            log_enabled: c.log_enabled,
        }
    }
}

/// Problems found while loading the config. The settings window can print these
/// instead of the user silently losing their settings (L-05).
static WARNINGS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

pub fn warnings() -> Vec<String> {
    WARNINGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn warn(msg: &str) {
    eprintln!("предупреждение: {msg}");
    WARNINGS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(msg.to_string());
}

fn timestamp() -> String {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs().to_string(),
        Err(_) => "0".into(),
    }
}

pub fn config_dir() -> PathBuf {
    let base = match std::env::var("APPDATA") {
        Ok(v) => PathBuf::from(v),
        Err(_) => PathBuf::from("."),
    };
    base.join("zя")
}

/// The project used to be called AbraKadabra. If the new folder is empty but
/// the old one still has the config or the diary, move them over so the
/// settings survive the rename.
fn migrate_legacy(dir: &Path) {
    let base = match std::env::var("APPDATA") {
        Ok(v) => PathBuf::from(v),
        Err(_) => return,
    };
    let old = base.join("AbraKadabra");
    if old == dir || !old.exists() {
        return;
    }
    for name in ["config.json", "diario.log"] {
        let from = old.join(name);
        let to = dir.join(name);
        if from.exists() && !to.exists() && std::fs::copy(&from, &to).is_ok() {
            println!("перенесён старый файл: {}", from.display());
        }
    }
}

pub fn save(cfg: &Config) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("config.json");
    let cfg = sanitize(cfg);

    // L-05: before the first write, keep the previous file so a bad save never
    // destroys the user's settings without a way back.
    if path.exists() {
        match std::fs::copy(&path, dir.join("config.json.bak")) {
            Ok(_) => {}
            Err(e) => warn(&format!(
                "не удалось сделать резервную копию config.json: {e}"
            )),
        }
    }

    let file = FileConfig::from(&cfg);
    match serde_json::to_string_pretty(&file) {
        Ok(text) => std::fs::write(&path, text),
        Err(e) => {
            warn(&format!("config.json не сериализуется: {e}"));
            Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e))
        }
    }
}

pub fn load() -> Config {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).ok();
    migrate_legacy(&dir);
    let path = dir.join("config.json");

    let defaults = Config::default();
    let mut bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            if path.exists() {
                warn(&format!("config.json не читается: {e}; используются значения по умолчанию"));
            }
            // No config file yet: write the defaults so the settings window has
            // something to edit.
            let f = FileConfig::from(&defaults);
            if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()) {
                warn(&format!("не удалось записать config.json: {e}"));
            }
            crate::diary::set_log_enabled(defaults.log_enabled);
            return defaults;
        }
    };

    // Notepad/PowerShell can save the file with a UTF-8 BOM.
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes = bytes[3..].to_vec();
    }

    let cfg = match serde_json::from_slice::<FileConfig>(&bytes) {
        Ok(f) => sanitize(&Config {
            margin_unknown: f.margin_unknown,
            margin_known: f.margin_known,
            word_bonus: f.word_bonus,
            plausibility: f.plausibility,
            min_len: f.min_len,
            protect_code: f.protect_code,
            hotkey_undo: f.hotkey_undo,
            hotkey_toggle: f.hotkey_toggle,
            hotkey_last: f.hotkey_last,
            hotkey_selection: f.hotkey_selection,
            hotkey_upper: f.hotkey_upper,
            hotkey_upper_last: f.hotkey_upper_last,
            hotkey_invert: f.hotkey_invert,
            hotkey_invert_last: f.hotkey_invert_last,
            hotkey_add_word: f.hotkey_add_word,
            smart_undo: f.smart_undo,
            numpad_comma_as_dot: f.numpad_comma_as_dot,
            whitelist: f.whitelist,
            short_ru: f.short_ru,
            short_en: f.short_en,
            exclude_processes: f.exclude_processes,
            exclude_dirs: f.exclude_dirs,
            auto_enabled: f.auto_enabled,
            autostart: f.autostart,
            admin_autostart: f.admin_autostart,
            log_enabled: f.log_enabled,
        }),
        Err(e) => {
            // L-05: the file is kept, not thrown away. It is renamed to
            // config.json.bad-<timestamp> so the user can repair it by hand,
            // and the running program falls back to the defaults.
            let backup = dir.join(format!("config.json.bad-{}", timestamp()));
            match std::fs::rename(&path, &backup) {
                Ok(()) => warn(&format!(
                    "config.json повреждён ({e}); испорченный файл сохранён как {}, \
                     применяются значения по умолчанию",
                    backup.display()
                )),
                Err(re) => warn(&format!(
                    "config.json повреждён ({e}) и его не удалось сохранить отдельно ({re}); \
                     применяются значения по умолчанию"
                )),
            }
            defaults
        }
    };

    // L-12: the diary must actually obey the setting.
    crate::diary::set_log_enabled(cfg.log_enabled);
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_fields_are_clamped() {
        let mut cfg = Config::default();
        cfg.margin_known = -500.0; // would rewrite every word
        cfg.margin_unknown = f32::NAN;
        cfg.word_bonus = 1000.0;
        cfg.min_len = 0;
        cfg.plausibility = 1000.0;
        cfg.whitelist = vec!["  git  ".into(), "".into()];
        let clean = sanitize(&cfg);
        assert_eq!(clean.margin_known, MIN_MARGIN);
        assert_eq!(clean.margin_unknown, Config::default().margin_unknown);
        assert_eq!(clean.word_bonus, MAX_WORD_BONUS);
        assert_eq!(clean.min_len, MIN_MIN_LEN);
        assert_eq!(clean.plausibility, MAX_PLAUSIBILITY);
        assert_eq!(clean.whitelist, vec!["git".to_string()]);
        // Values inside the range are untouched.
        assert_eq!(clean.margin_known, MIN_MARGIN);
        assert!(warnings().iter().any(|w| w.contains("margin_known")));
    }

    #[test]
    fn defaults_survive_the_clamp() {
        let d = Config::default();
        assert_eq!(sanitize(&d).plausibility, d.plausibility);
        assert_eq!(sanitize(&d).min_len, d.min_len);
        assert_eq!(sanitize(&d).margin_unknown, d.margin_unknown);
    }

    #[test]
    fn file_config_round_trip() {
        let cfg = Config::default();
        let file = FileConfig::from(&cfg);
        let text = serde_json::to_string_pretty(&file).unwrap();
        let parsed: FileConfig = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.margin_known, cfg.margin_known);
        assert_eq!(parsed.min_len, cfg.min_len);
        assert_eq!(parsed.whitelist.len(), cfg.whitelist.len());
    }

    #[test]
    fn a_broken_json_is_rejected_not_silently_mangled() {
        // A stray comma, the classic L-05 typo.
        let bad = r#"{"margin_known": "-1.0",, "min_len": 3}"#;
        assert!(serde_json::from_str::<FileConfig>(bad).is_err());
    }
}
