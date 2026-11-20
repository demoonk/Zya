// One-file ZЯ installer/launcher: `z.exe`.
//
// Running it as administrator:
//   - creates C:\Users\USER\AppData\Roaming\zя\
//   - writes the real local app as zя.exe
//   - creates config.json and diario.log if missing
//   - removes old autostart entries and stops old switchers/old z.exe copies
//   - starts zя.exe run headless
//   - shows a success/error window
//
// The GitHub release asset must remain the ASCII file `z.exe`.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, TerminateProcess, PROCESS_TERMINATE,
};
use windows::Win32::UI::Shell::IsUserAnAdmin;

#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(
        hwnd: *mut std::ffi::c_void,
        text: *const u16,
        caption: *const u16,
        flags: u32,
    ) -> i32;
}

const MB_ICONERROR: u32 = 0x0010;
const MB_ICONINFORMATION: u32 = 0x0040;

const APP: &[u8] = include_bytes!("z.bin");

const DEFAULT_CONFIG: &str = r#"{
  "margin_unknown": 2.5,
  "margin_known": 1.0,
  "word_bonus": 20.0,
  "plausibility": -12.0,
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
  "admin_autostart": false,
  "log_enabled": true
}
"#;

const RUN_KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE: &str = "zя.exe";
const OLD_RUN_VALUES: &[&str] = &["z.exe", "zя", "zya", "z", "Abrakadabra", "abracadabra"];
const OLD_PROCESS_NAMES: &[&str] = &[
    "z.exe",
    "zя.exe",
    "z_app.exe",
    "abrakadabra.exe",
    "abracadabra.exe",
    "puntoswitcher.exe",
    "punto_switcher.exe",
    "keyswitcher.exe",
    "layoutswitcher.exe",
];

fn app_data_dir() -> PathBuf {
    let base = match std::env::var("APPDATA") {
        Ok(v) => PathBuf::from(v),
        Err(_) => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    base.join("zя")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn config_wants_autostart(config: &str) -> bool {
    config.contains("\"autostart\": true") || config.contains("\"autostart\":true")
}

fn run_reg(args: &[&str]) -> bool {
    match Command::new("reg")
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn remove_old_run_entries() {
    for value in OLD_RUN_VALUES {
        if run_reg(&["query", RUN_KEY, "/v", value]) {
            run_reg(&["delete", RUN_KEY, "/v", value, "/f"]);
        }
    }
}

fn ensure_autostart(installed_path: &str) -> bool {
    remove_old_run_entries();
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

fn process_entry_name(entry: &PROCESSENTRY32W) -> String {
    String::from_utf16_lossy(&entry.szExeFile)
        .split('\0')
        .next()
        .unwrap_or_default()
        .to_string()
}

fn is_old_process(name: &str, pid: u32) -> bool {
    if pid == unsafe { GetCurrentProcessId() } {
        return false;
    }
    let lower = name.to_lowercase();
    OLD_PROCESS_NAMES
        .iter()
        .any(|candidate| lower == candidate.to_lowercase())
}

fn kill_old_processes() {
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return,
        };

        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let pid = entry.th32ProcessID;
                let name = process_entry_name(&entry);
                if is_old_process(&name, pid) {
                    if let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                        let _ = TerminateProcess(handle, 0);
                        let _ = CloseHandle(handle);
                    }
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
    }
}

fn append_startup_log(dir: &Path, msg: &str) {
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

fn message(title: &str, text: &str, flags: u32) {
    let t = wide(title);
    let m = wide(text);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), flags);
    }
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // zя does not need administrator rights: the keyboard hook and the tray icon
    // work for the current user. Requiring them made the first launch fail on a
    // fresh machine (the UAC prompt was dismissed), so the menu never appeared.
    if !unsafe { IsUserAnAdmin() }.as_bool() {
        eprintln!("z.exe: работаем без прав администратора (они не нужны)");
    }

    let dir = app_data_dir();

    if let Err(e) = std::fs::create_dir_all(&dir) {
        message(
            "Ошибка установки zя",
            &format!("Не удалось создать папку {} : {}", dir.display(), e),
            MB_ICONERROR,
        );
        eprintln!(
            "z.exe launcher error: cannot create {} : {}",
            dir.display(),
            e
        );
        return Err(e);
    }

    kill_old_processes();
    remove_old_run_entries();

    let installed_path = dir.join("zя.exe");

    let config_path = dir.join("config.json");
    let config_text = match std::fs::read_to_string(&config_path) {
        Ok(text) => text,
        Err(read_err) => match std::fs::write(&config_path, DEFAULT_CONFIG) {
            Ok(_) => DEFAULT_CONFIG.to_string(),
            Err(e) => {
                message(
                    "Ошибка установки zя",
                    &format!("Не удалось создать config.json: {e}\nОшибка чтения: {read_err}"),
                    MB_ICONERROR,
                );
                return Err(e);
            }
        },
    };

    let diary_path = dir.join("diario.log");
    if !diary_path.exists() {
        if let Err(e) = std::fs::write(&diary_path, "") {
            message(
                "Ошибка установки zя",
                &format!("Не удалось создать diario.log: {e}"),
                MB_ICONERROR,
            );
            return Err(e);
        }
    }

    if let Err(e) = std::fs::write(&installed_path, APP) {
        message(
            "Ошибка установки zя",
            &format!("Не удалось записать {}: {e}", installed_path.display()),
            MB_ICONERROR,
        );
        return Err(e);
    }

    append_startup_log(&dir, "z.exe launcher: installed zя.exe");

    if std::env::var("ZYA_NO_AUTOSTART").is_err() && config_wants_autostart(&config_text) {
        if !ensure_autostart(&installed_path.display().to_string()) {
            append_startup_log(
                &dir,
                "z.exe launcher: не удалось зарегистрировать автозапуск",
            );
        }
    }

    let mut cmd = Command::new(&installed_path);
    if args.is_empty() {
        cmd.args(["run", "headless"]);
    } else {
        cmd.args(args);
    }

    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::sleep(Duration::from_secs(1));
            match child.try_wait()? {
                Some(status) if status.success() => {
                    message(
                        "Установлено",
                        &format!(
                            "zя установлен и запущен.\nФайл: {}\nПапка: {}",
                            installed_path.display(),
                            dir.display()
                        ),
                        MB_ICONINFORMATION,
                    );
                }
                Some(status) => {
                    message(
                        "Ошибка запуска zя",
                        &format!(
                            "zя.exe завершился с кодом {}\nПроверьте папку: {}",
                            status.code().unwrap_or(1),
                            dir.display()
                        ),
                        MB_ICONERROR,
                    );
                    std::process::exit(status.code().unwrap_or(1));
                }
                None => {
                    message(
                        "Установлено",
                        &format!(
                            "zя установлен и работает в фоне.\nФайл: {}\nПапка: {}",
                            installed_path.display(),
                            dir.display()
                        ),
                        MB_ICONINFORMATION,
                    );
                }
            }
        }
        Err(e) => {
            message(
                "Ошибка запуска zя",
                &format!("Не удалось запустить {}: {e}", installed_path.display()),
                MB_ICONERROR,
            );
            return Err(e);
        }
    }

    Ok(())
}
