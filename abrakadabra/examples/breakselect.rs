//! Controlled reproduction of the "select a letter, press Break" gesture.
//!
//!   cargo run --release --example breakselect -- "сжатиtм" 5
//!
//! The probe owns its own window with a plain Edit control: it clicks into the
//! field to take focus, selects one letter with EM_SETSEL, presses Break as an
//! untagged keystroke (so the resident hook treats it as a real user gesture),
//! and prints what the control contains afterwards.
//!
//! The resident zя must be running; this example installs no hook of its own.

use std::thread::sleep;
use std::time::Duration;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HGLOBAL, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Controls::{EM_GETSEL, EM_SETSEL};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
    SendInput, INPUT, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, GetFocus, SetFocus,
    VK_C, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_PAUSE, VK_Z, VK_HOME, VK_RIGHT, VK_LEFT,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, BringWindowToTop, DispatchMessageW,
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId,
    MSG, PeekMessageW, PM_REMOVE, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetWindowTextW, SetWindowPos, ShowWindow, SW_SHOW, SW_SHOWMINIMIZED, SW_RESTORE,
    SWP_SHOWWINDOW, HWND_TOP, IsWindowVisible, TranslateMessage, WINDOW_EX_STYLE, WNDCLASSW,
    WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

const CF_UNICODE: u32 = 13;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn send(input: &INPUT) {
    SendInput(&[input.clone()], std::mem::size_of::<INPUT>() as i32);
}

/// The Edit control only reacts to keystrokes while its owning thread pumps
/// messages, so the probe runs a real message loop between the injections.
unsafe fn pump(ms: u64) {
    let deadline = Duration::from_millis(ms);
    let start = std::time::Instant::now();
    while start.elapsed() < deadline {
        let mut message = MSG::default();
        if PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).into() {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        } else {
            sleep(Duration::from_millis(10));
        }
    }
}

unsafe fn key(vk: VIRTUAL_KEY, up: bool) {
    send(&INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_FLAGS },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}

unsafe fn tap(vk: VIRTUAL_KEY) {
    key(vk, false);
    key(vk, true);
}

/// Alt+Shift: the same layout switch the user makes with the keyboard.
unsafe fn alt_shift() {
    key(VK_LMENU, false);
    key(VK_LSHIFT, false);
    key(VK_LSHIFT, true);
    key(VK_LMENU, true);
}

unsafe fn type_physical(text: &str) {
    for ch in text.chars() {
        let vk = match ch {
            'a'..='z' => VIRTUAL_KEY(0x41 + (ch as u16 - 'a' as u16)),
            'A'..='Z' => VIRTUAL_KEY(0x41 + (ch as u16 - 'A' as u16)),
            ';' => VIRTUAL_KEY(0x3B),
            _ => continue,
        };
        key(vk, false);
        key(vk, true);
        pump(60);
    }
}

const KEYBD_FLAGS: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS =
    windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0);

unsafe fn mouse(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS, dx: i32, dy: i32) {
    send(&INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    });
}

unsafe fn click(x: i32, y: i32) {
    let screen_w = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SYSTEM_METRICS_INDEX(0)).max(1);
    let screen_h = GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SYSTEM_METRICS_INDEX(1)).max(1);
    mouse(
        MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE,
        x * 65535 / screen_w,
        y * 65535 / screen_h,
    );
    sleep(Duration::from_millis(80));
    mouse(MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTDOWN, x * 65535 / screen_w, y * 65535 / screen_h);
    sleep(Duration::from_millis(40));
    mouse(MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_LEFTUP, x * 65535 / screen_w, y * 65535 / screen_h);
    sleep(Duration::from_millis(150));
}

unsafe fn clipboard() -> Option<String> {
    if OpenClipboard(None).is_err() {
        return None;
    }
    let out = match GetClipboardData(CF_UNICODE) {
        Ok(handle) => {
            let block = HGLOBAL(handle.0);
            let ptr = GlobalLock(block) as *const u16;
            let mut text = String::new();
            if !ptr.is_null() {
                let mut i = 0;
                while unsafe { *ptr.add(i) } != 0 {
                    if let Some(ch) = char::from_u32(unsafe { *ptr.add(i) } as u32) {
                        text.push(ch);
                    }
                    i += 1;
                }
            }
            GlobalUnlock(block).ok();
            Some(text)
        }
        Err(_) => None,
    };
    CloseClipboard().ok();
    out
}

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(window, message, wparam, lparam)
}

fn text_of(window: HWND) -> String {
    unsafe {
        let mut buffer = [0u16; 512];
        let length = GetWindowTextW(window, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..length])
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let word = args.first().cloned().unwrap_or_else(|| "сжатиtм".to_string());
    let pick = args
        .get(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(word.find('t').unwrap_or(0));

    unsafe {
        let class_name = wide("ZyaBreakProbe");
        let title = wide("Break probe");
        let edit_class = wide("Edit");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            println!("RegisterClassW failed");
            return;
        }

        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            200,
            200,
            640,
            320,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(edit_class.as_ptr()),
            PCWSTR(wide("текст").as_ptr()),
            WS_CHILD | WS_VISIBLE,
            12,
            12,
            600,
            140,
            Some(window),
            None,
            None,
            None,
        )
        .unwrap();

        ShowWindow(window, SW_SHOW);
        SetWindowPos(
            window,
            Some(HWND_TOP),
            1200,
            700,
            640,
            320,
            SWP_SHOWWINDOW,
        )
        .ok();
        for attempt in 0..8 {
            let foreground = GetForegroundWindow();
            if foreground.0 == window.0 {
                break;
            }
            let my_tid = GetCurrentThreadId();
            let fg_tid = GetWindowThreadProcessId(foreground, None);
            let target_tid = GetWindowThreadProcessId(window, None);
            AttachThreadInput(my_tid, fg_tid, true);
            AttachThreadInput(my_tid, target_tid, true);
            ShowWindow(window, SW_SHOW);
            BringWindowToTop(window).ok();
            SetForegroundWindow(window);
            AttachThreadInput(my_tid, target_tid, false);
            AttachThreadInput(my_tid, fg_tid, false);
            pump(150);
            if GetForegroundWindow().0 == window.0 {
                break;
            }
            ShowWindow(window, SW_SHOWMINIMIZED);
            pump(150);
            ShowWindow(window, SW_RESTORE);
            pump(150);
            SetForegroundWindow(window);
            if GetForegroundWindow().0 == window.0 {
                break;
            }
            // A real mouse click on our own window is the interaction the
            // foreground lock is looking for.
            let mut rect = RECT::default();
            GetWindowRect(window, &mut rect).ok();
            click((rect.left + rect.right) / 2, (rect.top + rect.bottom) / 2);
            pump(150);
            println!("focus attempt {attempt}: fg {:#x}", GetForegroundWindow().0 as usize);
        }
        SetFocus(Some(edit)).ok();
        pump(400);

        let mut rect = RECT::default();
        GetWindowRect(window, &mut rect).ok();
        println!(
            "focus: foreground {:#x}, GetFocus {:#x} (window {:#x}, edit {:#x}), visible {}",
            GetForegroundWindow().0 as usize,
            GetFocus().0 as usize,
            window.0 as usize,
            edit.0 as usize,
            IsWindowVisible(window).as_bool()
        );

        SetWindowTextW(edit, PCWSTR(wide(&word).as_ptr()));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(pick)), Some(LPARAM((pick + 1) as isize)));
        let mut start = 0u32;
        let mut end = 0u32;
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut start as *const u32 as usize)),
            Some(LPARAM((&mut end as *const u32 as usize) as isize)),
        );
        println!("probe: word {word:?}, EM_GETSEL = {start}..{end}");
        println!("clipboard before: {:?}", clipboard());

        // A realistic run: type the word with real keystrokes, switching the
        // layout in the middle exactly like a user does, then select the stray
        // letter with the mouse (EM_SETSEL, no key events) and press Break.
        SetWindowTextW(edit, PCWSTR(wide("").as_ptr()));
        pump(200);
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            ActivateKeyboardLayout, GetKeyboardLayoutList, HKL,
        };
        let mut list = [HKL(std::ptr::null_mut()); 16];
        let n = GetKeyboardLayoutList(Some(list.as_mut_slice())) as usize;
        let ru = list
            .iter()
            .take(n)
            .find(|h| ((h.0 as usize) & 0xFFFF) == 0x0419)
            .copied()
            .unwrap_or(list[0]);
        ActivateKeyboardLayout(ru, windows::Win32::UI::Input::KeyboardAndMouse::ACTIVATE_KEYBOARD_LAYOUT_FLAGS(0)).ok();
        pump(200);
        type_physical("c;fnb"); // сжати on the Russian layout
        alt_shift(); // the user switches to English
        pump(300);
        type_physical("t"); // the forgotten letter, typed in English
        alt_shift(); // and back to Russian
        pump(300);
        type_physical("v"); // м
        println!("typed: {:?}", text_of(edit));
        let mut ts = 0u32;
        let mut te = 0u32;

        // Case A: Break with NO selection at all.
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(usize::MAX)), Some(LPARAM(-1isize)));
        key(VK_PAUSE, false);
        pump(150);
        key(VK_PAUSE, true);
        pump(3000);
        println!("A) no selection -> {:?}", text_of(edit));

        // Case B: a mouse click inside the word (caret moves, no key event),
        // then Break.
        let mut rect = RECT::default();
        GetWindowRect(edit, &mut rect).ok();
        click(rect.left + 30, rect.top + 20);
        pump(300);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("B) caret at {ts}..{te}");
        key(VK_PAUSE, false);
        pump(150);
        key(VK_PAUSE, true);
        pump(3000);
        println!("B) click then Break -> {:?}", text_of(edit));

        // Case C: a real keyboard selection of the stray letter.
        key(VK_HOME, false);
        key(VK_HOME, true);
        for _ in 0..pick {
            key(VK_RIGHT, false);
            key(VK_RIGHT, true);
        }
        key(VK_LSHIFT, false);
        key(VK_RIGHT, false);
        key(VK_RIGHT, true);
        key(VK_LSHIFT, true);
        pump(250);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("C) selection {ts}..{te}");
        key(VK_PAUSE, false);
        pump(150);
        key(VK_PAUSE, true);
        pump(3000);
        println!("C) selection then Break -> {:?}", text_of(edit));
        println!("clipboard after: {:?}", clipboard());

        // E) a genuine mouse-like selection of the stray letter, then Break.
        SetWindowTextW(edit, PCWSTR(wide("сжатиtм").as_ptr()));
        pump(200);
        key(VK_HOME, false);
        key(VK_HOME, true);
        pump(150);
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(5usize)), Some(LPARAM(6isize)));
        pump(200);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("E) selection {ts}..{te}");
        key(VK_PAUSE, false);
        pump(150);
        key(VK_PAUSE, true);
        pump(3000);
        println!("E) mouse selection then Break -> {:?}", text_of(edit));
        println!("clipboard after E: {:?}", clipboard());

        // F) control: the probe itself does Ctrl+C on the same selection, without
        // any hook involvement.
        pump(200);
        key(VK_LCONTROL, false);
        key(VK_C, false);
        key(VK_C, true);
        key(VK_LCONTROL, true);
        pump(500);
        println!("F) probe's own Ctrl+C clipboard: {:?}", clipboard());

        // D) the same caret probe the app now performs: select backwards and copy.
        SetWindowTextW(edit, PCWSTR(wide("abcde").as_ptr()));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(5usize)), Some(LPARAM(5isize)));
        pump(200);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("D) caret before: {ts}..{te}");
        key(VK_LEFT, false);
        key(VK_LEFT, true);
        pump(200);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("D) after Left: {ts}..{te}");
        key(VK_LSHIFT, false);
        key(VK_LEFT, false);
        key(VK_LEFT, true);
        pump(300);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("D) shift held selection is {ts}..{te}");
        key(VK_LSHIFT, true);
        pump(200);
        type_physical("X");
        println!("D) text after typing X at caret: {:?}", text_of(edit));
        // G) is the injected Shift state visible to the control at all?
        SetWindowTextW(edit, PCWSTR(wide("abcde").as_ptr()));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(5usize)), Some(LPARAM(5isize)));
        pump(200);
        key(VK_LSHIFT, false);
        key(VK_RIGHT, false);
        key(VK_RIGHT, true);
        key(VK_LSHIFT, true);
        pump(400);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("G) Shift+Right selection {ts}..{te}");
        key(VK_LSHIFT, false);
        key(VK_Z, false);
        key(VK_Z, true);
        key(VK_LSHIFT, true);
        pump(400);
        println!("G) Shift+Z typed: {:?}", text_of(edit));
        // H) can the control select at all? caret at 0, Shift+Right twice.
        SetWindowTextW(edit, PCWSTR(wide("abcde").as_ptr()));
        SendMessageW(edit, EM_SETSEL, Some(WPARAM(0usize)), Some(LPARAM(0isize)));
        pump(200);
        key(VK_LSHIFT, false);
        key(VK_RIGHT, false);
        key(VK_RIGHT, true);
        key(VK_RIGHT, false);
        key(VK_RIGHT, true);
        key(VK_LSHIFT, true);
        pump(400);
        SendMessageW(
            edit,
            EM_GETSEL,
            Some(WPARAM(&mut ts as *const u32 as usize)),
            Some(LPARAM((&mut te as *const u32 as usize) as isize)),
        );
        println!("H) Shift+Right x2 selection {ts}..{te}");
        key(VK_LCONTROL, false);
        key(VK_C, false);
        key(VK_C, true);
        key(VK_LCONTROL, true);
        pump(300);
        println!("D) clipboard now {:?}", clipboard());

        DestroyWindow(window).ok();
    }
}
