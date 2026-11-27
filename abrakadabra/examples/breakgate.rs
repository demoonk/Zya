//! Decisive test for L-32.
//!
//!   cargo run --release --example breakgate
//!
//! Types a word, presses Break (the caret is where the engine thinks it is),
//! then clicks inside the same field with the mouse — the caret jumps with no
//! key event — and presses Break again. The second Break must refuse instead of
//! deleting characters from the middle of the word.
//!
//! The resident zя must be running.
use std::thread::sleep;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::UI::Controls::{EM_GETSEL, EM_SETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYBDINPUT, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE,
    MOUSEINPUT, VK_PAUSE, VK_A, VK_B, VK_C, VK_D, VK_E,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, MSG, PeekMessageW, PM_REMOVE, RegisterClassW, SendMessageW, SetForegroundWindow,
    ShowWindow, SW_SHOW, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn window_proc(handle: HWND, message: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(handle, message, w, l)
}

unsafe fn send(input: &INPUT) {
    SendInput(&[input.clone()], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn pump(ms: u64) {
    let limit = Duration::from_millis(ms);
    let start = std::time::Instant::now();
    while start.elapsed() < limit {
        let mut message = MSG::default();
        if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        } else {
            sleep(Duration::from_millis(10));
        }
    }
}

unsafe fn tap(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) {
    send(&INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(80);
    send(&INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(220);
}

unsafe fn click(x: i32, y: i32) {
    let ax = (x as f64 / 2560.0) * 65535.0;
    let ay = (y as f64 / 1440.0) * 65535.0;
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(200);
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTDOWN,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(150);
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(400);
}

unsafe fn absolute_move(x: i32, y: i32) {
    let ax = (x as f64 / 2560.0) * 65535.0;
    let ay = (y as f64 / 1440.0) * 65535.0;
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(150);
}

unsafe fn drag(y: i32, x1: i32, x2: i32) {
    let ax = (x2 as f64 / 2560.0) * 65535.0;
    let ay = (y as f64 / 1440.0) * 65535.0;
    absolute_move(x1, y);
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: (x1 as f64 / 2560.0 * 65535.0) as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTDOWN,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(120);
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(120);
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: ax as i32,
                dy: ay as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTUP,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
    pump(400);
}

unsafe fn selection(edit: HWND) -> (u32, u32) {
    let mut start = 0u32;
    let mut end = 0u32;
    SendMessageW(
        edit,
        EM_GETSEL,
        Some(WPARAM(&mut start as *const u32 as usize)),
        Some(LPARAM((&mut end as *const u32 as usize) as isize)),
    );
    (start, end)
}

unsafe fn text_of(edit: HWND) -> String {
    let length = GetWindowTextLengthW(edit).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    GetWindowTextW(edit, &mut buffer);
    String::from_utf16_lossy(&buffer[..length])
}

fn main() {
    unsafe {
        let class_name = wide("GateClass2");
        let title = wide("Caret gate test");
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
            900,
            900,
            420,
            220,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(wide("EDIT").as_ptr()),
            PCWSTR(wide("").as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            12,
            12,
            390,
            180,
            Some(window),
            None,
            None,
            None,
        )
        .unwrap();
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        pump(500);

        for key in [VK_A, VK_B, VK_C, VK_D, VK_E] {
            tap(key);
        }
        println!("1) typed {:?} caret {:?}", text_of(edit), selection(edit));

        tap(VK_PAUSE);
        pump(2500);
        println!("2) Break with the caret at the end -> {:?}", text_of(edit));

        let mut rect = RECT::default();
        GetWindowRect(edit, &mut rect).ok();
        click(rect.left + 25, rect.top + 20);
        println!("3) after the mouse click caret {:?}", selection(edit));

        tap(VK_PAUSE);
        pump(2500);
        println!("4) Break after the click -> {:?}", text_of(edit));

        // A fresh typed character makes the caret known again.
        tap(VK_A);
        pump(200);
        tap(VK_PAUSE);
        pump(2500);
        println!("5) type then Break again -> {:?}", text_of(edit));

        // 6) the case the user reports: select with the mouse, then Break.
        let mut rect = RECT::default();
        GetWindowRect(edit, &mut rect).ok();
        let y = rect.top + 20;
        for key in [VK_B, VK_C, VK_D, VK_E] {
            tap(key);
        }
        let before = text_of(edit);
        drag(y, rect.left + 40, rect.left + 90);
        println!("6) mouse drag selection caret {:?}", selection(edit));
        tap(VK_PAUSE);
        pump(2500);
        println!("6) Break on the selection {:?} -> {:?}", before, text_of(edit));

        // 7) a genuine selection made after a click must still work: the
        // clipboard tells us exactly what is selected, the caret is irrelevant.
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(1usize)), Some(LPARAM(4isize)));
        pump(200);
        let before = text_of(edit);
        println!("7a) selection {:?} caret {:?}", before, selection(edit));
        tap(VK_PAUSE);
        pump(2500);
        println!("7) Break with a real selection {:?} -> {:?}", before, text_of(edit));
        println!("7b) caret after {:?}", selection(edit));

        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0usize)), Some(LPARAM(0isize)));
        pump(100);
        DestroyWindowSafe(edit);
    }
}

unsafe fn DestroyWindowSafe(edit: HWND) {
    windows::Win32::UI::WindowsAndMessaging::DestroyWindow(edit).ok();
}
