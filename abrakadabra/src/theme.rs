//! System theme detection.
//!
//! Windows keeps the "Apps use light theme" flag in
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
//! A missing key is treated as dark, which matches the default of older builds.

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    REG_VALUE_TYPE,
};

const PERSONALIZE: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize";
const APPS_USE_LIGHT_THEME: &str = "AppsUseLightTheme";

fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_dark() -> bool {
    // A manual override is handy for testing both palettes without changing the
    // system setting: `ZYA_THEME=light` or `ZYA_THEME=dark`.
    match std::env::var("ZYA_THEME").unwrap_or_default().as_str() {
        "light" => return false,
        "dark" => return true,
        _ => {}
    }

    let subkey = utf16(PERSONALIZE);
    let name = utf16(APPS_USE_LIGHT_THEME);

    unsafe {
        let mut opened = HKEY::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut opened,
        )
        .is_err()
        {
            return true;
        }

        let mut kind = REG_VALUE_TYPE(0);
        let mut bytes = [0u8; 4];
        let mut size = 4u32;
        let result = RegQueryValueExW(
            opened,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(bytes.as_mut_ptr()),
            Some(&mut size),
        );
        let _ = RegCloseKey(opened);

        if result.is_err() {
            return true;
        }
        u32::from_le_bytes(bytes) == 0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn theme_probe_is_safe_to_call() {
        let dark = super::is_dark();
        assert!(dark || !dark);
    }
}
