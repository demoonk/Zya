//! Small diagnostic: create a classic EDIT window in this very process, type a
//! physical key, inject Alt+Shift, type again, then destroy the window. The
//! title shows which layout is live ("j" = English, "о" = Russian). No external
//! application is ever opened.

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyboardLayout, GetKeyboardLayoutList, SendInput, SetFocus, HKL, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_J, VK_LMENU,
    VK_LSHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetWindowTextW, GetWindowThreadProcessId,
    PeekMessageW, ShowWindow, TranslateMessage, MSG, PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WS_OVERLAPPEDWINDOW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn press_vk(vk: VIRTUAL_KEY) {
    let batch = [key(vk, false), key(vk, true)];
    unsafe {
        SendInput(&batch, std::mem::size_of::<INPUT>() as i32);
    }
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(std::time::Duration::from_millis(ms));
}

fn langid_of(hkl: HKL) -> u32 {
    (hkl.0 as u64 as u32) & 0xFFFF
}

/// The EDIT window only processes WM_CHAR while its messages are dispatched.
fn pump_ms(ms: u64) {
    unsafe {
        let mut msg = MSG::default();
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(ms) {
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                // A bare EDIT window hangs on DispatchMessage for WM_SYSKEYDOWN.
                if msg.message != 260 && msg.message != 261 {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            } else {
                sleep_ms(5);
            }
        }
    }
}

fn text_of(h: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(h, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..n])
    }
}

pub fn run() {
    unsafe {
        let mut list = [HKL(std::ptr::null_mut()); 32];
        let n = GetKeyboardLayoutList(Some(list.as_mut_slice())) as usize;
        for h in list.iter().take(n) {
            println!("installed layout: {:04x}", langid_of(*h));
        }

        let title = wide("ZyaProbe");
        let win = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(wide("EDIT").as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            0,
            0,
            320,
            140,
            None,
            None,
            None,
            None,
        ) {
            Ok(w) => w,
            Err(e) => {
                println!("probe: cannot create window: {e}");
                return;
            }
        };
        let _ = ShowWindow(win, SW_SHOW);
        pump_ms(400);
        for _ in 0..20 {
            let _ = SetFocus(Some(win));
            pump_ms(30);
            if !GetFocus().0.is_null() {
                break;
            }
        }

        let tid = GetWindowThreadProcessId(win as HWND, None);
        println!("window langid: {:04x}", langid_of(GetKeyboardLayout(tid)));

        press_vk(VK_J);
        pump_ms(250);
        println!("pressed J, text={:?}", text_of(win));

        SendInput(&[key(VK_LMENU, false)], std::mem::size_of::<INPUT>() as i32);
        sleep_ms(40);
        SendInput(
            &[key(VK_LSHIFT, false)],
            std::mem::size_of::<INPUT>() as i32,
        );
        sleep_ms(40);
        SendInput(&[key(VK_LSHIFT, true)], std::mem::size_of::<INPUT>() as i32);
        sleep_ms(40);
        SendInput(&[key(VK_LMENU, true)], std::mem::size_of::<INPUT>() as i32);
        pump_ms(600);
        println!(
            "after Alt+Shift, window langid: {:04x}",
            langid_of(GetKeyboardLayout(tid))
        );
        press_vk(VK_J);
        pump_ms(250);
        println!("pressed J again, text={:?}", text_of(win));

        if let Err(e) = DestroyWindow(win) {
            println!("probe: DestroyWindow: {e}");
        }
    }
}
