//! User configuration, stored as JSON next to the diary.

use std::path::{Path, PathBuf};

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
    #[serde(default)]
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
    #[serde(default = "default_hotkey_invert")]
    pub hotkey_invert: String,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub exclude_processes: Vec<String>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    #[serde(default = "default_true")]
    pub auto_enabled: bool,
    #[serde(default)]
    pub autostart: bool,
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
fn default_hotkey_invert() -> String {
    "shift+break".into()
}
fn default_true() -> bool {
    true
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
            hotkey_invert: c.hotkey_invert.clone(),
            whitelist: c.whitelist.clone(),
            exclude_processes: c.exclude_processes.clone(),
            exclude_dirs: c.exclude_dirs.clone(),
            auto_enabled: c.auto_enabled,
            autostart: c.autostart,
            log_enabled: c.log_enabled,
        }
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

pub fn load() -> Config {
    let dir = config_dir();
    std::fs::create_dir_all(&dir).ok();
    migrate_legacy(&dir);
    let path = dir.join("config.json");

    let defaults = Config::default();
    match std::fs::read(&path) {
        Ok(mut bytes) => {
            // Notepad/PowerShell can save the file with a UTF-8 BOM.
            if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                bytes = bytes[3..].to_vec();
            }
            match serde_json::from_slice::<FileConfig>(&bytes) {
                Ok(f) => Config {
                    margin_unknown: f.margin_unknown,
                    margin_known: f.margin_known,
                    word_bonus: f.word_bonus,
                    plausibility: f.plausibility,
                    min_len: if f.min_len > 0 {
                        f.min_len
                    } else {
                        defaults.min_len
                    },
                    protect_code: f.protect_code,
                    hotkey_undo: f.hotkey_undo,
                    hotkey_toggle: f.hotkey_toggle,
                    hotkey_last: f.hotkey_last,
                    hotkey_selection: f.hotkey_selection,
                    hotkey_upper: f.hotkey_upper,
                    hotkey_invert: f.hotkey_invert,
                    whitelist: f.whitelist,
                    exclude_processes: f.exclude_processes,
                    exclude_dirs: f.exclude_dirs,
                    auto_enabled: f.auto_enabled,
                    autostart: f.autostart,
                    log_enabled: f.log_enabled,
                },
                Err(e) => {
                    eprintln!("config.json parse error: {e}; using defaults");
                    defaults
                }
            }
        }
        Err(_) => {
            let f = FileConfig::from(&defaults);
            if let Err(e) = std::fs::write(&path, serde_json::to_string_pretty(&f).unwrap()) {
                eprintln!("could not write config: {e}");
            }
            defaults
        }
    }
}
