//! Autostart at logon. The old mechanism is the registry Run key. The new one is
//! a Task Scheduler task, used when the user wants zя to start as an administrator.

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "zя.exe";
const TASK: &str = "ZyaAutoStart";
const OLD_VALUES: &[&str] = &["z.exe", "zя", "zya", "z", "Abrakadabra"];

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_run(writing: bool) -> Option<HKEY> {
    let name = wide(KEY);
    let flags = if writing { KEY_SET_VALUE } else { KEY_QUERY_VALUE };
    unsafe {
        let mut key = HKEY::default();
        let result = RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(name.as_ptr()), None, flags, &mut key);
        if result.is_err() {
            return None;
        }
        Some(key)
    }
}

fn has_value(name: &str) -> bool {
    let Some(key) = open_run(false) else {
        return false;
    };
    let value = wide(name);
    let ok = unsafe { RegQueryValueExW(key, PCWSTR(value.as_ptr()), None, None, None, None).is_ok() };
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok
}

fn write_value(name: &str, data: &str) -> bool {
    let Some(key) = open_run(true) else {
        return false;
    };
    let value = wide(name);
    let bytes: Vec<u8> = data.encode_utf16().flat_map(|unit| unit.to_le_bytes()).collect();
    let result = unsafe {
        RegSetValueExW(key, PCWSTR(value.as_ptr()), None, REG_SZ, Some(&bytes))
    };
    let ok = result.is_ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok
}

fn delete_value(name: &str) -> bool {
    let Some(key) = open_run(true) else {
        return false;
    };
    let value = wide(name);
    let ok = unsafe { RegDeleteValueW(key, PCWSTR(value.as_ptr())) }.is_ok();
    unsafe {
        let _ = RegCloseKey(key);
    }
    ok
}

fn run_cmd(program: &str, args: &[&str]) -> bool {
    match std::process::Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn remove_old_entries() {
    delete_value(VALUE);
    for value in OLD_VALUES {
        if has_value(value) {
            delete_value(value);
        }
    }
}

fn task_exists() -> bool {
    run_cmd("schtasks", &["/query", "/tn", TASK])
}

fn remove_task() {
    if task_exists() {
        run_cmd("schtasks", &["/delete", "/tn", TASK, "/f"]);
    }
}

fn create_task(path: &str) -> bool {
    let command = format!("\"{path}\" run headless");
    run_cmd(
        "schtasks",
        &[
            "/create", "/tn", TASK, "/tr", &command, "/sc", "ONLOGON", "/rl", "HIGHEST", "/f",
        ],
    )
}

pub fn enabled() -> bool {
    has_value(VALUE) || OLD_VALUES.iter().any(|v| has_value(v)) || task_exists()
}

pub fn admin_enabled() -> bool {
    task_exists()
}

/// Write (or remove) the Run entry. `path` is the executable that Windows will
/// launch; `headless` makes it start hidden in the tray.
pub fn set(on: bool, path: &str) -> bool {
    set_with_admin(on, false, path)
}

pub fn set_with_admin(on: bool, admin: bool, path: &str) -> bool {
    if !on {
        delete_value(VALUE);
        remove_old_entries();
        remove_task();
        true
    } else if admin {
        delete_value(VALUE);
        remove_old_entries();
        create_task(path)
    } else {
        remove_task();
        remove_old_entries();
        write_value(VALUE, &format!("\"{path}\" run headless"))
    }
}

/// Prefer the installed resident binary in %APPDATA%\zя\zя.exe.
pub fn installed_exe() -> PathBuf {
    crate::config::config_dir().join("zя.exe")
}

pub fn current_exe() -> PathBuf {
    let local = installed_exe();
    if local.exists() {
        local
    } else {
        std::env::current_exe().unwrap_or_else(|_| PathBuf::from("zя.exe"))
    }
}

/// Task Scheduler must point to the executable that is actually running now.
pub fn current_process_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| current_exe())
}

pub fn sync_with_config(want: bool, admin: bool) -> bool {
    let path = current_process_exe().display().to_string();

    if !want {
        remove_old_entries();
        remove_task();
        return false;
    }

    if admin {
        // Always refresh the task command, even if the task already exists.
        create_task(&path);
        return task_exists();
    }

    // Normal Run key also points to the currently running executable.
    remove_task();
    remove_old_entries();
    set_with_admin(true, false, &path)
}

/// Tray-menu action: flip the registry entry and report the new state.
pub fn toggle() -> String {
    let next = !enabled();
    let admin = next && admin_enabled();
    set_with_admin(next, admin, &current_process_exe().display().to_string());
    if next {
        "включён".into()
    } else {
        "выключен".into()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn run_key_can_be_read() {
        let key = super::open_run(false);
        assert!(key.is_some(), "HKCU\\...\\Run должна читаться");
        if let Some(key) = key {
            unsafe {
                let _ = windows::Win32::System::Registry::RegCloseKey(key);
            }
        }
    }

    #[test]
    fn wide_helper_terminates() {
        let text = super::wide("zя");
        assert_eq!(text.len(), 3);
        assert_eq!(text[2], 0);
    }

    #[test]
    fn old_names_are_cleaned() {
        assert_eq!(super::OLD_VALUES.len(), 5);
        assert!(!super::OLD_VALUES.contains(&"zя.exe"));
    }
}
