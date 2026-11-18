// One-file launcher for ZЯ.
//
// `zya.exe` contains the actual Rust application embedded inside it.
// Running it with no parameters starts the app in working mode and creates
// the user config folder:
//   C:\Users\USER\AppData\Roaming\zя\
//
// The app binary is extracted there as `zya_app.exe`, and config.json is
// created if it does not exist.

const APP: &[u8] = include_bytes!("../release/zya_app.exe");

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

fn app_data_dir() -> std::path::PathBuf {
    let base = match std::env::var("APPDATA") {
        Ok(v) => std::path::PathBuf::from(v),
        Err(_) => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
    };
    base.join("zя")
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = app_data_dir();

    std::fs::create_dir_all(&dir)?;

    let app_path = dir.join("zya_app.exe");
    if std::fs::metadata(&app_path).map(|m| m.len() != APP.len() as u64).unwrap_or(true) {
        std::fs::write(&app_path, APP)?;
    }

    let config_path = dir.join("config.json");
    if !config_path.exists() {
        std::fs::write(&config_path, DEFAULT_CONFIG)?;
    }

    let mut cmd = std::process::Command::new(&app_path);
    if args.is_empty() {
        cmd.args(["run", "headless"]);
    } else {
        cmd.args(args);
    }

    let status = cmd.status()?;
    std::process::exit(status.code().unwrap_or(1));
}
