//! Does the "Добавить активное приложение" button really show a menu of open
//! windows? The probe sends the same WM_COMMAND the button sends, then looks for
//! the popup (class #32768) among all top-level windows — FindWindow does not
//! always see menu windows — and closes it so nothing stays on the screen.
//! Run while the settings window is open: zя.exe settings.
//!
//!   cargo run --release --example windowmenu
use windows::core::{BOOL, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClassNameW, PostMessageW, WM_CLOSE, WM_COMMAND,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn look(hwnd: HWND, param: LPARAM) -> BOOL {
    let hits = unsafe { &mut *(param.0 as *mut Vec<HWND>) };
    let mut name = [0u16; 64];
    let length = GetClassNameW(hwnd, &mut name).max(0) as usize;
    let class = String::from_utf16_lossy(&name[..length]);
    if class == "#32768" {
        hits.push(hwnd);
    }
    BOOL(1)
}

fn main() {
    unsafe {
        let class = wide("ZyaSettingsWindow");
        let window = FindWindowW(PCWSTR(class.as_ptr()), None)
            .unwrap_or(HWND(std::ptr::null_mut()));
        if window.0.is_null() {
            println!("окно настроек не открыто: запустите zя.exe settings");
            return;
        }
        // BTN_ADD_PROCESSES = 130: the "Добавить активное приложение" button.
        PostMessageW(Some(window), WM_COMMAND, WPARAM(130), LPARAM(0)).ok();
        for _ in 0..25 {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let mut menus: Vec<HWND> = Vec::new();
            let pointer = &mut menus as *mut Vec<HWND> as *mut std::ffi::c_void;
            windows::Win32::UI::WindowsAndMessaging::EnumWindows(Some(look), LPARAM(pointer as isize)).ok();
            if !menus.is_empty() {
                println!("меню активных окон открыто: {menus:?}");
                for menu in menus {
                    PostMessageW(Some(menu), WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
                }
                std::thread::sleep(std::time::Duration::from_millis(400));
                let mut left: Vec<HWND> = Vec::new();
                let pointer = &mut left as *mut Vec<HWND> as *mut std::ffi::c_void;
                windows::Win32::UI::WindowsAndMessaging::EnumWindows(Some(look), LPARAM(pointer as isize)).ok();
                println!("меню закрыто: {}", left.is_empty());
                return;
            }
        }
        println!("меню не появилось — кнопка не открыла список окон");
    }
}
