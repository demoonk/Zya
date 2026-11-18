// One-file ZЯ app: `zя.exe`.
//
// Running it with no parameters starts the working mode.
// It creates/updates the installed app in the user Roaming folder:
//   C:\Users\USER\AppData\Roaming\zя\zя.exe
// and creates settings files there if they are missing.

use std::io::Write;

const APP: &[u8] = include_bytes!("zya_app.bin");

const DEFAULT_CONFIG: &str = r#"{
  "margin_unknown": 2.5,
  "margin_known": 1.0,
  "word_bonus": 20.0,
  "plausibility": 0.0,
  "min_len": 3,
  "protect_code": true,
  "hotkey_undo": "alt",
  "hotkey_toggle": "ctrl+alt+p",
  "hotkey_last": "break",
  "hotkey_selection": "break",
  "hotkey_upper": "ctrl+break",
  "hotkey_invert": "shift+break",
  "whitelist": [],
  "exclude_processes": [],
  "exclude_dirs": [],
  "auto_enabled": true,
  "autostart": true,
  "log_enabled": true
}
"#;

const RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "zя";

fn app_data_dir() -> std::path::PathBuf {
    let base = match std::env::var("APPDATA") {
        Ok(v) => std::path::PathBuf::from(v),
        Err(_) => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    base.join("zя")
}

fn config_wants_autostart(config: &str) -> bool {
    config.contains("\"autostart\": true") || config.contains("\"autostart\":true")
}

fn run_reg(args: &[&str]) -> bool {
    match std::process::Command::new("reg")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn ensure_autostart(installed_path: &str) -> bool {
    if !run_reg(&["query", RUN_KEY, "/v", RUN_VALUE]) {
        run_reg(&[
            "add",
            RUN_KEY,
            "/v",
            RUN_VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &format!("\"{}\" run headless", installed_path),
            "/f",
        ])
    } else {
        true
    }
}

fn append_startup_log(dir: &std::path::Path, msg: &str) {
    let path = dir.join("startup.log");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let line = format!("[launcher {}s] {}\n", stamp, msg);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = app_data_dir();

    std::fs::create_dir_all(&dir)?;
    append_startup_log(&dir, "zя launcher: запуск");

    let installed_path = dir.join("zя.exe");
    let current_path = std::env::current_exe().unwrap_or_else(|_| installed_path.clone());

    // If this exe was launched from another folder, overwrite the installed
    // one-file app with this copy. If the copy fails, the embedded app is still
    // extracted and run from the cache.
    if current_path != installed_path {
        match std::fs::copy(&current_path, &installed_path) {
            Ok(_) => append_startup_log(&dir, "zя launcher: обновил zя.exe в AppData"),
            Err(e) => append_startup_log(&dir, &format!("zя launcher: не смог обновить zя.exe: {e}")),
        }
    }

    let config_path = dir.join("config.json");
    let config_text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(_) => {
            std::fs::write(&config_path, DEFAULT_CONFIG)?;
            DEFAULT_CONFIG.to_string()
        }
    };

    let diary_path = dir.join("diario.log");
    if !diary_path.exists() {
        std::fs::write(&diary_path, "")?;
    }

    if std::env::var("ZYA_NO_AUTOSTART").is_err() && config_wants_autostart(&config_text) {
        ensure_autostart(&installed_path.display().to_string());
    }

    // Extract the embedded application binary from this single exe and run it.
    let cache_dir = dir.join(".cache");
    std::fs::create_dir_all(&cache_dir)?;
    let app_path = cache_dir.join("zя_app.exe");
    std::fs::write(&app_path, APP)?;

    let mut cmd = std::process::Command::new(&app_path);
    if args.is_empty() {
        cmd.args(["run", "headless"]);
    } else {
        cmd.args(args);
    }

    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}
