//! Drop a tray icon from a saved `.icon` dump into the real notification area for a
//! few seconds, so the glyph can be measured on the live taskbar.
//!
//!   cargo run --release --example tray_probe -- data/check/tray_ru_on_dark.icon 8
//!
//! Run the binary from outside the workspace: the sandbox blocks Shell_NotifyIconW
//! for processes whose image lives inside it.

use std::thread;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW,
    ShowWindow, IMAGE_FLAGS, SW_HIDE, WM_NULL, WNDCLASSW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
};

const WM_TRAYICON: u32 = 0x1111;

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_NULL {
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[link(name = "kernel32")]
extern "system" {
    fn GetLastError() -> u32;
    fn GetConsoleWindow() -> HWND;
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args
        .first()
        .cloned()
        .unwrap_or_else(|| "data/check/tray_ru_on_dark.icon".into());
    let seconds: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8);

    let data = std::fs::read(&path).expect("icon dump readable");

    unsafe {
        let name = wide("ZyaProbe");
        let class = WNDCLASSW {
            lpfnWndProc: Some(proc),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            println!("probe: RegisterClassW failed");
            return;
        }
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(name.as_ptr()),
            PCWSTR(wide("zя probe").as_ptr()),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            1,
            1,
            None,
            None,
            None,
            None,
        ) else {
            println!("probe: window creation failed");
            return;
        };
        let _ = ShowWindow(hwnd, SW_HIDE);

        // The notification area rejects an icon whose owner window is not a real,
        // console-attached one, so the console window is the icon's owner when there
        // is one.
        let console = GetConsoleWindow();
        let owner = if console.0.is_null() { hwnd } else { console };

        let Ok(icon) = CreateIconFromResourceEx(&data, true, 0x0003_0000, 32, 32, IMAGE_FLAGS(0))
        else {
            println!("probe: CreateIconFromResourceEx failed");
            return;
        };

        let tip = wide("zя probe");
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: owner,
            uID: 7,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: icon,
            ..Default::default()
        };
        for (i, c) in tip.iter().take(127).enumerate() {
            nid.szTip[i] = *c;
        }
        if !Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
            println!("probe: NIM_ADD failed, last error {}", GetLastError());
            return;
        }
        println!(
            "probe: icon added from {path} for {seconds}s (hwnd {})",
            owner.0 as usize
        );
        thread::sleep(Duration::from_secs(seconds));
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = DestroyWindow(hwnd);
        println!("probe: icon removed");
    }
}
