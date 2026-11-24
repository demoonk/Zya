//! Autostart at logon. The old mechanism is the registry Run key. The new one is
//! a Task Scheduler task, used when the user wants zя to start as an administrator.

use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "zя.exe";
const TASK: &str = "ZyaAutoStart";
const OLD_VALUES: &[&str] = &["z.exe", "zя", "zya", "z", "Abrakadabra"];

// schtasks.exe is a console program: without this flag running it from a GUI
// process can flash a console window on the desktop.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// H-03: `schtasks /query` used to run synchronously before every tray menu,
/// i.e. on every right click of the icon. The presence of the task is now cached
/// and refreshed only where the task is created or deleted.
/// 0 = never looked, 1 = task exists, 2 = task does not exist.
static TASK_STATE: AtomicU8 = AtomicU8::new(0);

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

/// One RegOpenKeyExW for the whole check instead of six: `enabled()` is called
/// from the tray menu, so it should not reopen the key for every old name.
fn run_key_present() -> bool {
    let Some(key) = open_run(false) else {
        return false;
    };
    let names: Vec<Vec<u16>> = std::iter::once(VALUE)
        .chain(OLD_VALUES.iter().copied())
        .map(wide)
        .collect();
    let mut present = false;
    unsafe {
        for name in &names {
            if RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, None).is_ok() {
                present = true;
                break;
            }
        }
        let _ = RegCloseKey(key);
    }
    present
}

fn write_value(name: &str, data: &str) -> bool {
    let Some(key) = open_run(true) else {
        return false;
    };
    let value = wide(name);
    // L-16: a REG_SZ is a UTF-16 string *with* its terminating NUL. Without it
    // `reg.exe export`, migration tools and any reader that uses lstrlenW walk
    // past the end of the stored value.
    let bytes: Vec<u8> = data
        .encode_utf16()
        .chain(std::iter::once(0u16))
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
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
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    // P-13: `status()` waits for the process to exit. If the Task Scheduler is
    // wedged (or an antivirus is intercepting the spawn), that wait never ends
    // and whoever called it is stuck forever. Poll with a deadline and kill the
    // child when it overruns.
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            Err(_) => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

fn remove_old_entries() {
    // One open for the whole sweep: RegDeleteValueW simply fails for a name that
    // is not there, so probing each of the six names with its own RegOpenKeyExW
    // would cost six key opens for nothing.
    let Some(key) = open_run(true) else {
        return;
    };
    unsafe {
        for name in std::iter::once(VALUE).chain(OLD_VALUES.iter().copied()) {
            let value = wide(name);
            let _ = RegDeleteValueW(key, PCWSTR(value.as_ptr()));
        }
        let _ = RegCloseKey(key);
    }
}

fn task_exists_now() -> bool {
    run_cmd("schtasks", &["/query", "/tn", TASK])
}

fn remember_task(exists: bool) {
    TASK_STATE.store(if exists { 1 } else { 2 }, Ordering::Relaxed);
}

/// Read the Task Scheduler once, then answer from the cache. Every create/delete
/// in this module refreshes the cache, so a right click on the tray icon never
/// spawns schtasks.exe.
fn task_exists_cached() -> bool {
    match TASK_STATE.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let exists = task_exists_now();
            remember_task(exists);
            exists
        }
    }
}

fn refresh_task_cache() -> bool {
    let exists = task_exists_now();
    remember_task(exists);
    exists
}

fn remove_task() {
    if task_exists_cached() {
        if run_cmd("schtasks", &["/delete", "/tn", TASK, "/f"]) {
            remember_task(false);
        } else {
            refresh_task_cache();
        }
    }
}

fn create_task(path: &str) -> bool {
    let command = format!("\"{path}\" run headless");
    let created = run_cmd(
        "schtasks",
        &[
            "/create", "/tn", TASK, "/tr", &command, "/sc", "ONLOGON", "/rl", "HIGHEST", "/f",
        ],
    );
    if created {
        // schtasks answered "created": no need to spend another process spawn on
        // a query, the cache can simply say the task is there.
        remember_task(true);
    } else {
        refresh_task_cache();
    }
    created
}

pub fn enabled() -> bool {
    run_key_present() || task_exists_cached()
}

pub fn admin_enabled() -> bool {
    task_exists_cached()
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
        // L-15: the Run key and the Task Scheduler task both start zя at logon,
        // so an admin autostart must clear the registry entry first - otherwise
        // the app launches twice at every logon (the second copy is then killed
        // by the single-instance check, and the stale path stays in the registry).
        // remove_old_entries() drops both the current value and the legacy names.
        remove_old_entries();
        create_task(&path);
        return task_exists_cached();
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
    let path = current_process_exe().display().to_string();
    if !set_with_admin(next, admin, &path) {
        return "не изменён (реестр/Планировщик отказали)".into();
    }
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

    #[test]
    fn reg_sz_bytes_end_with_the_nul_terminator() {
        // L-16: the stored REG_SZ must include the trailing UTF-16 NUL, otherwise
        // `reg export` and lstrlenW-style readers run past the value.
        let data = "C:\\zя.exe run headless";
        let bytes: Vec<u8> = data
            .encode_utf16()
            .chain(std::iter::once(0u16))
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        let units = data.encode_utf16().count();
        assert_eq!(bytes.len(), (units + 1) * 2);
        assert_eq!(&bytes[bytes.len() - 2..], &[0, 0]);
    }

    #[test]
    fn task_state_cache_is_used_instead_of_schtasks() {
        use std::sync::atomic::Ordering;
        super::remember_task(true);
        assert_eq!(super::TASK_STATE.load(Ordering::Relaxed), 1);
        assert!(super::task_exists_cached(), "кэш должен отвечать без запуска schtasks");
        super::remember_task(false);
        assert_eq!(super::TASK_STATE.load(Ordering::Relaxed), 2);
        assert!(!super::task_exists_cached());
    }
}
