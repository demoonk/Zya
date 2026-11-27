//! L-32 end to end in a real application: Notepad.
//!
//!   cargo run --release --example notepadgate
//!
//! Types a word, presses Break while the caret is where the hook thinks it is,
//! then clicks inside the document with the mouse — the caret jumps with no key
//! event — and presses Break again. The second Break must refuse instead of
//! eating characters from the middle of the word. Typing one character after
//! that reopens the gate.
//!
//! The resident zя must be running.
use std::thread::sleep;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, KEYBDINPUT, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MOVE, MOUSEINPUT, VK_A, VK_B, VK_C, VK_D, VK_E, VK_F, VK_LCONTROL, VK_PAUSE,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowRect, SetForegroundWindow,
};

unsafe fn send(input: &INPUT) {
    SendInput(&[input.clone()], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn key(vk: VIRTUAL_KEY) {
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
    sleep(Duration::from_millis(50));
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
    sleep(Duration::from_millis(120));
}

unsafe fn type_text(text: &str) {
    for c in text.encode_utf16() {
        send(&INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: c,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        sleep(Duration::from_millis(40));
    }
}

unsafe fn click(x: i32, y: i32) {
    let ax = (x as f64 / 2560.0 * 65535.0) as i32;
    let ay = (y as f64 / 1440.0 * 65535.0) as i32;
    for flags in [
        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTDOWN,
        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTUP,
    ] {
        send(&INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: ax,
                    dy: ay,
                    mouseData: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        sleep(Duration::from_millis(200));
    }
}

unsafe fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn window_title(handle: HWND) -> String {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextLengthW, GetWindowTextW};
    let length = GetWindowTextLengthW(handle).max(0) as usize;
    let mut buffer = vec![0u16; length + 1];
    GetWindowTextW(handle, &mut buffer);
    String::from_utf16_lossy(&buffer[..length])
}

unsafe fn clipboard_text() -> Option<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    if OpenClipboard(None).is_err() {
        return None;
    }
    let out = match GetClipboardData(13) {
        Ok(handle) => {
            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory) as *const u16;
            let mut text = String::new();
            if !ptr.is_null() {
                let mut i = 0;
                while *ptr.add(i) != 0 {
                    if let Some(ch) = char::from_u32(*ptr.add(i) as u32) {
                        text.push(ch);
                    }
                    i += 1;
                }
            }
            GlobalUnlock(memory).ok();
            Some(text)
        }
        Err(_) => None,
    };
    CloseClipboard().ok();
    out
}

unsafe fn snapshot() -> String {
    key(VK_A); // Ctrl is held by the caller, so this is "select all"
    key(VK_C); // and "copy"
    sleep(Duration::from_millis(400));
    clipboard_text().unwrap_or_else(|| "<clipboard empty>".into())
}

fn main() {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            VK_A, VK_B, VK_C, VK_D, VK_E, VK_F, VK_LCONTROL,
        };

        let notepad = match FindWindowW(PCWSTR(utf16("Notepad").as_ptr()), PCWSTR::null()) {
            Ok(handle) => handle,
            Err(_) => {
                println!("Notepad is not open, start it first");
                return;
            }
        };
        SetForegroundWindow(notepad);
        sleep(Duration::from_millis(900));

        let mut rect = RECT::default();
        GetWindowRect(notepad, &mut rect).ok();
        println!("notepad window {:?}", rect);

        // A click inside the text area both focuses the document and moves the
        // caret with no key event at all.
        click(rect.left + 300, rect.top + 120);
        sleep(Duration::from_millis(500));

        for vk in [VK_A, VK_B, VK_C, VK_D, VK_E] {
            key(vk);
        }
        sleep(Duration::from_millis(900));
        hold(VK_LCONTROL);
        println!("1) typed {:?}", snapshot());
        release(VK_LCONTROL);

        key(VK_PAUSE);
        sleep(Duration::from_millis(2200));
        hold(VK_LCONTROL);
        println!("2) Break after typing -> {:?}", snapshot());
        release(VK_LCONTROL);

        click(rect.left + 260, rect.top + 120);
        sleep(Duration::from_millis(600));

        key(VK_PAUSE);
        sleep(Duration::from_millis(2200));
        hold(VK_LCONTROL);
        println!("3) Break after the mouse click -> {:?}", snapshot());
        release(VK_LCONTROL);

        key(VK_F);
        sleep(Duration::from_millis(400));
        key(VK_PAUSE);
        sleep(Duration::from_millis(2200));
        hold(VK_LCONTROL);
        println!("4) Break after typing again -> {:?}", snapshot());
        release(VK_LCONTROL);

        println!("window title: {:?}", window_title(notepad));
    }
}

unsafe fn hold(vk: VIRTUAL_KEY) {
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
    sleep(Duration::from_millis(30));
}

unsafe fn release(vk: VIRTUAL_KEY) {
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
    sleep(Duration::from_millis(30));
}
