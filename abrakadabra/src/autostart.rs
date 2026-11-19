//! Autostart at logon: the `Run` key in the registry, mirrored in config.json so
//! the tray menu and the CLI stay in sync.

use std::path::PathBuf;

const KEY: &str = "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE: &str = "zя.exe";
const OLD_VALUES: &[&str] = &["z.exe", "zя", "zya", "z", "Abrakadabra"];

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

fn remove_old_entries() {
    for value in OLD_VALUES {
        if run_reg(&["query", KEY, "/v", value]) {
            run_reg(&["delete", KEY, "/v", value, "/f"]);
        }
    }
}

pub fn enabled() -> bool {
    run_reg(&["query", KEY, "/v", VALUE])
        || OLD_VALUES.iter().any(|v| run_reg(&["query", KEY, "/v", v]))
}

/// Write (or remove) the Run entry. `path` is the executable that Windows will
/// launch; `headless` makes it start hidden in the tray.
pub fn set(on: bool, path: &str) -> bool {
    if on {
        remove_old_entries();
        run_reg(&[
            "add",
            KEY,
            "/v",
            VALUE,
            "/t",
            "REG_SZ",
            "/d",
            &format!("\"{path}\" run headless"),
            "/f",
        ])
    } else {
        run_reg(&["delete", KEY, "/v", VALUE, "/f"]);
        remove_old_entries();
        true
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

pub fn sync_with_config(want: bool) -> bool {
    let have = enabled();
    if have == want {
        return have;
    }
    set(want, &current_exe().display().to_string())
}

/// Tray-menu action: flip the registry entry and report the new state.
pub fn toggle() -> String {
    let next = !enabled();
    set(next, &current_exe().display().to_string());
    if next {
        "включён".into()
    } else {
        "выключен".into()
    }
}
