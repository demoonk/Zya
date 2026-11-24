//! Two levels of self-test:
//!   simulate  - runs the decision pipeline head-less;
//!   window    - creates a real Win32 window with an Edit control, installs the
//!               hook, injects keystrokes and reads the result back.

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyboardLayout, LoadKeyboardLayoutW, SendInput, SetFocus, INPUT, INPUT_0,
    INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, KLF_ACTIVATE, VIRTUAL_KEY,
    VK_3, VK_A, VK_C, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME, VK_LCONTROL, VK_LMENU, VK_LSHIFT,
    VK_OEM_3, VK_Q, VK_Z, KEYBD_EVENT_FLAGS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    FindWindowW, GetClassNameW, GetForegroundWindow, GetParent, GetWindow, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, MoveWindow, PeekMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, GW_CHILD,
    GW_HWNDNEXT, MSG, PM_REMOVE, SW_SHOW, SW_SHOWMINIMIZED, SW_RESTORE, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_GETTEXT, WM_SETTEXT,
    WNDCLASSW, WS_CHILD, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

use crate::engine::Engine;
use crate::layout;
use crate::model::Model;
use crate::tray;
use crate::win;

const EN: &str = "00000409";
const RU: &str = "00000419";

/// Head-less simulation of the hook's buffer + decision logic.
pub fn simulate(engine: &Engine, typed: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let close = |word: &String, out: &mut String| {
        if word.is_empty() {
            return;
        }
        match engine.decide(word) {
            Some(d) => out.push_str(&d.converted),
            None => out.push_str(word),
        }
    };
    for c in typed.chars() {
        if c.is_alphabetic() || layout::is_ambiguous(c) {
            word.push(c);
        } else {
            close(&word, &mut out);
            word.clear();
            out.push(c);
        }
    }
    close(&word, &mut out);
    out
}

pub fn run() -> anyhow::Result<()> {
    let engine = Engine::new(Model::load()?, crate::config::load());
    let cases = [
        ("ghbdtn ", "привет "),
        ("hf,jnf ", "работа "),
        ("руддщ world", "hello world"),
        ("привет мир", "привет мир"),
        ("hello, world", "hello, world"),
        ("cdj,jlf ", "свобода "),
    ];
    let mut ok = 0;
    for (typed, want) in cases {
        let got = simulate(&engine, typed);
        if got == want {
            ok += 1;
            println!("OK  : \"{typed}\" -> \"{got}\"");
        } else {
            println!("FAIL: \"{typed}\" -> \"{got}\" (expected \"{want}\")");
        }
    }
    println!("{ok}/{} simulation cases passed", cases.len());
    Ok(())
}

fn scan(c: char) -> u16 {
    match c.to_ascii_lowercase() {
        'q' => 0x10,
        'w' => 0x11,
        'e' => 0x12,
        'r' => 0x13,
        't' => 0x14,
        'y' => 0x15,
        'u' => 0x16,
        'i' => 0x17,
        'o' => 0x18,
        'p' => 0x19,
        'a' => 0x1E,
        's' => 0x1F,
        'd' => 0x20,
        'f' => 0x21,
        'g' => 0x22,
        'h' => 0x23,
        'j' => 0x24,
        'k' => 0x25,
        'l' => 0x26,
        'z' => 0x2C,
        'x' => 0x2D,
        'c' => 0x2E,
        'v' => 0x2F,
        'b' => 0x30,
        'n' => 0x31,
        'm' => 0x32,
        ' ' => 0x39,
        ',' => 0x33,
        '.' => 0x34,
        '-' => 0x0C,
        // OEM keys need a real scan code: TranslateMessage cannot turn a bare
        // virtual key into a character without it.
        '1' => 0x02,
        '2' => 0x03,
        '3' => 0x04,
        '4' => 0x05,
        '5' => 0x06,
        '6' => 0x07,
        '7' => 0x08,
        '8' => 0x09,
        '9' => 0x0A,
        '0' => 0x0B,
        ';' => 0x27,
        '\'' => 0x28,
        '[' => 0x1A,
        ']' => 0x1B,
        '=' => 0x0E,
        '/' => 0x35,
        _ => 0,
    }
}

unsafe extern "system" fn demo_wndproc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    DefWindowProcW(h, m, w, l)
}

unsafe fn press_physical(c: char) {
    let sc = scan(c);
    if sc == 0 {
        return;
    }
    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: sc,
                    dwFlags: KEYEVENTF_SCANCODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: sc,
                    dwFlags: KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];
    let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    if sent != 2 {
        println!("diag: physical SendInput({c:?}) -> {sent:?}");
    }
    sleep(Duration::from_millis(40));
}

unsafe fn press_vk(vk: VIRTUAL_KEY) {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let inputs = [
        INPUT {
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
        },
        INPUT {
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
        },
    ];
    let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(40));
}

/// Press a hotkey exactly as configured in config.json.
unsafe fn press_hotkey(h: win::Hotkey) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYBD_EVENT_FLAGS, VK_LWIN, VK_MENU};
    let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
    };
    let mut mods = Vec::new();
    if h.ctrl {
        mods.push(VK_LCONTROL);
    }
    if h.alt {
        mods.push(VK_MENU);
    }
    if h.shift {
        mods.push(VK_LSHIFT);
    }
    if h.win {
        mods.push(VK_LWIN);
    }
    for m in &mods {
        let _ = SendInput(&[mk(*m, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(20));
    }
    if !(h.alt && h.vk == VK_MENU.0) {
        let vk = VIRTUAL_KEY(h.vk);
        let _ = SendInput(
            &[mk(vk, false), mk(vk, true)],
            std::mem::size_of::<INPUT>() as i32,
        );
        sleep(Duration::from_millis(20));
    }
    for m in mods.iter().rev() {
        let _ = SendInput(&[mk(*m, true)], std::mem::size_of::<INPUT>() as i32);
    }
    sleep(Duration::from_millis(250));
}

/// Press a hotkey with the demo edit control guaranteed to be in the foreground:
/// the rewrite the hook injects goes to the focused window, so a stolen focus
/// would silently send the corrected text somewhere else.
unsafe fn press_hotkey_in(edit: HWND, h: win::Hotkey) {
    focus_edit(edit);
    pump_ms(200);
    press_hotkey(h);
}

/// Alt down, Shift down, Shift up, Alt up — with real dwExtraInfo, so the hook
/// sees it as a genuine user layout switch.
unsafe fn press_alt_shift() {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
    };
    let _ = SendInput(&[mk(VK_LMENU, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LSHIFT, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LSHIFT, true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LMENU, true)], std::mem::size_of::<INPUT>() as i32);
}

/// Alt+Shift held, `key` tapped in between, then released — the shape of a real
/// application shortcut such as Alt+Shift+3.
unsafe fn press_alt_shift_key(key: VIRTUAL_KEY) {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
    };
    let _ = SendInput(&[mk(VK_LMENU, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LSHIFT, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(key, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(key, true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LSHIFT, true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LMENU, true)], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn press_alt_key(key: VIRTUAL_KEY) {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
    };
    let _ = SendInput(&[mk(VK_LMENU, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(key, false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(key, true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(VK_LMENU, true)], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn press_alt() {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_LMENU,
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
    };
    let _ = SendInput(&[mk(false)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(30));
    let _ = SendInput(&[mk(true)], std::mem::size_of::<INPUT>() as i32);
}

unsafe fn pump_ms(ms: u64) {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(ms) {
        let mut msg = MSG::default();
        if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            // System key messages for Alt deadlock inside DefWindowProc of a
            // bare EDIT window, so they are not dispatched here.
            if msg.message != 260 && msg.message != 261 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        } else {
            sleep(Duration::from_millis(5));
        }
    }
}

unsafe fn text_of(hwnd: HWND) -> String {
    // GetWindowText does not return the contents of a control owned by another
    // process, so the test asks the control directly with WM_GETTEXT. Works for
    // local windows too.
    let buf = vec![0u16; 8192];
    let n = SendMessageW(
        hwnd,
        WM_GETTEXT,
        Some(WPARAM(buf.len() - 1)),
        Some(LPARAM(buf.as_ptr() as isize)),
    );
    if n.0 <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n.0 as usize])
}

/// Pump until the control contains `want` or the deadline passes. Injected
/// rewrites are asynchronous, so a fixed sleep is both slow and flaky.
unsafe fn wait_for_text(edit: HWND, want: &str, ms: u64) -> String {
    wait_for(&|| text_of(edit), want, ms)
}

unsafe fn wait_for(get: &dyn Fn() -> String, want: &str, ms: u64) -> String {
    let start = Instant::now();
    loop {
        let text = get();
        if text == want || start.elapsed() >= Duration::from_millis(ms) {
            return text;
        }
        pump_ms(200);
    }
}

/// EM_GETSEL: the caret/selection range inside an EDIT control.
unsafe fn caret_of(hwnd: HWND) -> (usize, usize) {
    let mut start: i32 = 0;
    let mut end: i32 = 0;
    let _ = SendMessageW(
        hwnd,
        0x0400 + 11,
        Some(WPARAM(&mut start as *mut i32 as usize)),
        Some(LPARAM(&mut end as *mut i32 as isize)),
    );
    (start.max(0) as usize, end.max(0) as usize)
}

/// Select the whole contents of a RichEdit control with Ctrl+A. The classic
/// EDIT control does not implement that accelerator.
unsafe fn select_all_rich(edit: HWND) {
    focus_edit(edit);
    pump_ms(200);
    press_combo2(VK_LCONTROL, VK_A);
    pump_ms(400);
}

/// Send keystrokes to `edit`, re-asserting the foreground on every character.
/// Without this a competing window (a browser, a notification) can steal the
/// keystrokes mid-test and make the result non-deterministic.
unsafe fn window_title(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 256];
    let n = GetWindowTextW(hwnd, &mut buf);
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

unsafe fn type_into(edit: HWND, text: &str, gap_ms: u64) -> bool {
    type_into_dyn(edit, &|| text_of(edit), text, gap_ms)
}

/// Clear a window for a new test round. WM_SETTEXT wipes the control, but the
/// hook still holds the word it was building, so a real boundary keystroke is
/// needed to flush it.
unsafe fn reset_window(edit: HWND, get: &dyn Fn() -> String) {
    set_text(edit, "");
    focus_edit(edit);
    pump_ms(200);
    press_key(' ');
    pump_ms(400);
    set_text(edit, "");
    pump_ms(200);
    let t = get();
    if t != "" {
        println!("diag: reset of {edit:?} left {t:?}");
    }
}

unsafe fn key_is_down(vk: u16) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    GetAsyncKeyState(vk as i32) & -32768 != 0
}

unsafe fn release_stuck_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_MENU, VK_SHIFT};
    for vk in [VK_MENU, VK_LMENU, VK_SHIFT, VK_LSHIFT, VK_LCONTROL] {
        let mk = INPUT {
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
        };
        let _ = SendInput(&[mk], std::mem::size_of::<INPUT>() as i32);
    }
    pump_ms(50);
}

unsafe fn type_into_dyn(edit: HWND, get: &dyn Fn() -> String, text: &str, gap_ms: u64) -> bool {
    for c in text.chars() {
        let mut landed = false;
        for attempt in 0..3 {
            if !is_focused(edit) && !focus_edit(edit) {
                return false;
            }
            if key_is_down(VK_LMENU.0) {
                println!("diag: Alt is logically held, releasing it before {c:?}");
                release_stuck_modifiers();
            }
            let before = get();
            press_key(c);
            pump_ms(gap_ms.max(60));
            // A rewrite injected by the hook can delay the character, so give
            // the message loop one more turn before pressing the key again.
            if get() == before {
                pump_ms(250);
            }
            if get() != before {
                landed = true;
                break;
            }
            println!(
                "diag: char {c:?} did not reach the edit (attempt {attempt}, foreground {:?} \"{}\")",
                GetForegroundWindow(),
                window_title(GetForegroundWindow())
            );
        }
        if !landed {
            return false;
        }
    }
    true
}

/// ASCII characters that need Shift on a US/RTF keyboard.
fn shifted_key(c: char) -> Option<VIRTUAL_KEY> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        VK_0, VK_1, VK_2, VK_3, VK_4, VK_5, VK_6, VK_7, VK_8, VK_9, VK_OEM_1, VK_OEM_2, VK_OEM_3,
        VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7, VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD,
        VK_OEM_PLUS,
    };
    match c {
        '!' => Some(VK_1),
        '@' => Some(VK_2),
        '#' => Some(VK_3),
        '$' => Some(VK_4),
        '%' => Some(VK_5),
        '^' => Some(VK_6),
        '&' => Some(VK_7),
        '*' => Some(VK_8),
        '(' => Some(VK_9),
        ')' => Some(VK_0),
        '+' => Some(VK_OEM_PLUS),
        '_' => Some(VK_OEM_MINUS),
        '{' => Some(VK_OEM_4),
        '}' => Some(VK_OEM_6),
        '[' => Some(VK_OEM_4),
        ']' => Some(VK_OEM_6),
        ':' => Some(VK_OEM_1),
        '"' => Some(VK_OEM_7),
        '<' => Some(VK_OEM_COMMA),
        '>' => Some(VK_OEM_PERIOD),
        '?' => Some(VK_OEM_2),
        '|' => Some(VK_OEM_5),
        '~' => Some(VK_OEM_3),
        _ => None,
    }
}

/// Letters are pressed by scancode (so the layout really matters); everything
/// else, such as digits, goes through the virtual key.
unsafe fn press_key(c: char) {
    if c.is_ascii_uppercase() {
        use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
        let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
        };
        let _ = SendInput(&[mk(VK_LSHIFT, false)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(15));
        let _ = SendInput(
            &[
                mk(VIRTUAL_KEY(c as u16), false),
                mk(VIRTUAL_KEY(c as u16), true),
            ],
            std::mem::size_of::<INPUT>() as i32,
        );
        sleep(Duration::from_millis(15));
        let _ = SendInput(&[mk(VK_LSHIFT, true)], std::mem::size_of::<INPUT>() as i32);
    } else if c == '_' {
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_OEM_MINUS;
        press_combo2(VK_LSHIFT, VK_OEM_MINUS);
    } else if let Some(vk) = shifted_key(c) {
        press_combo2(VK_LSHIFT, vk);
    } else if !c.is_ascii() {
        // Dash, quotes and other typographic characters have no physical key in
        // the test harness: inject them as Unicode. The hook still sees them as a
        // real boundary keystroke.
        use windows::Win32::UI::Input::KeyboardAndMouse::KEYEVENTF_UNICODE;
        let mk = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0),
                    wScan: c as u16,
                    dwFlags: if up {
                        KEYEVENTF_UNICODE | KEYEVENTF_KEYUP
                    } else {
                        KEYEVENTF_UNICODE
                    },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let _ = SendInput(&[mk(false), mk(true)], std::mem::size_of::<INPUT>() as i32);
        sleep(Duration::from_millis(20));
    } else if scan(c) != 0 {
        press_physical(c);
    } else {
        press_vk(VIRTUAL_KEY(c as u16));
    }
}

unsafe fn press_combo2(a: VIRTUAL_KEY, b: VIRTUAL_KEY) {
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
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
    };
    let _ = SendInput(
        &[mk(a, false), mk(b, false), mk(b, true), mk(a, true)],
        std::mem::size_of::<INPUT>() as i32,
    );
    sleep(Duration::from_millis(40));
}

unsafe fn set_text(hwnd: HWND, text: &str) {
    let t: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _ = SendMessageW(hwnd, WM_SETTEXT, None, Some(LPARAM(t.as_ptr() as isize)));
}

/// Text of an edit control. `GetWindowText` only reports the real contents of a
/// control that belongs to *this* process; across processes WM_GETTEXT/
/// GetWindowText fall back to the window title. So a window owned by another
/// application must report its own contents (the focus test does that through a
/// log file).
unsafe fn local_text(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 8192];
    let n = GetWindowTextW(hwnd, &mut buf);
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// A keystroke reaches `hwnd` when it is focused and its top-level owner is the
/// foreground window. Child controls have the parent as foreground.
unsafe fn root_of(hwnd: HWND) -> HWND {
    match GetParent(hwnd) {
        Ok(p) if !p.0.is_null() => p,
        _ => hwnd,
    }
}

unsafe fn is_focused(hwnd: HWND) -> bool {
    let root = root_of(hwnd);
    if GetForegroundWindow() != root {
        return false;
    }
    if GetFocus() == hwnd {
        return true;
    }
    // GetFocus only reports windows of the calling thread, so a window belonging
    // to another process is accepted when it is the foreground window: that is
    // exactly where the keystrokes go.
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    pid != std::process::id()
}

/// Windows refuses `SetForegroundWindow` for a process that has not interacted
/// with the desktop yet. A real click on our own demo window is exactly the
/// interaction the foreground lock is looking for.
unsafe fn click_to_foreground(hwnd: HWND) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LMENU, KEYEVENTF_KEYUP, KEYBD_EVENT_FLAGS};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassNameW, GetWindow, GW_HWNDPREV, SetWindowPos, HWND_TOP, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE,
    };
    let mut rc = RECT::default();
    if GetWindowRect(hwnd, &mut rc).is_err() {
        return;
    }
    // The click must land on *our* window, so it has to be above everything
    // else at those coordinates first.
    let top = SetWindowPos(
        hwnd,
        Some(HWND_TOP),
        0,
        0,
        0,
        0,
        SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
    );
    if win::verbose() {
        let prev = GetWindow(hwnd, GW_HWNDPREV).unwrap_or(HWND(std::ptr::null_mut()));
        let mut pc = [0u16; 64];
        let pn = if prev.0.is_null() {
            0
        } else {
            GetClassNameW(prev, &mut pc).max(0) as usize
        };
        println!(
            "diag: SetWindowPos top -> {top:?}, rect {:?}, window above {:?}",
            rc,
            (prev, String::from_utf16_lossy(&pc[..pn]))
        );
    }
    // The classic foreground-lock workaround: a real Alt keystroke releases the
    // "no process other than the foreground one may take the foreground" rule,
    // after which SetForegroundWindow is honoured again.
    let mk = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_LMENU,
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
    };
    let _ = SendInput(&[mk(false), mk(true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(40));
    let x = (rc.left + rc.right) / 2;
    let y = (rc.top + rc.bottom) / 2;
    if win::verbose() {
        println!(
            "diag: click at ({x},{y}) to activate {hwnd:?}, visible {:?}",
            IsWindowVisible(hwnd)
        );
    }
    click_abs(x, y);
    sleep(Duration::from_millis(120));
    let _ = SetForegroundWindow(hwnd);
    if win::verbose() {
        let mut pos = windows::Win32::Foundation::POINT::default();
        let cp = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut pos);
        println!(
            "diag: after click foreground = {:?}, cursor {:?} ({cp:?})",
            GetForegroundWindow(),
            pos
        );
    }
}

unsafe fn focus_edit(edit: HWND) -> bool {
    let root = root_of(edit);
    for i in 0..12 {
        pump_ms(120);
        force_foreground(root);
        pump_ms(60);
        let _ = SetFocus(Some(edit));
        if is_focused(edit) {
            return true;
        }
        if i >= 4 {
            click_to_foreground(root);
            pump_ms(120);
            let _ = SetFocus(Some(edit));
            if is_focused(edit) {
                return true;
            }
        }
        let fg = GetForegroundWindow();
        let mut cls = [0u16; 64];
        let n = GetClassNameW(fg, &mut cls).max(0) as usize;
        let mut title = [0u16; 128];
        let t = GetWindowTextW(fg, &mut title).max(0) as usize;
        println!(
            "diag: focus {:?} / foreground {:?} class {:?} title {:?} (want {:?} in {root:?})",
            GetFocus(),
            fg,
            String::from_utf16_lossy(&cls[..n]),
            String::from_utf16_lossy(&title[..t]),
            edit
        );
    }
    false
}

unsafe fn force_foreground(hwnd: HWND) -> bool {
    if GetForegroundWindow() == hwnd {
        return true;
    }
    let my_tid = GetCurrentThreadId();
    let fg_tid = GetWindowThreadProcessId(GetForegroundWindow(), None);
    let target_tid = GetWindowThreadProcessId(hwnd, None);
    let a1 = AttachThreadInput(my_tid, fg_tid, true);
    let a2 = AttachThreadInput(my_tid, target_tid, true);
    if win::verbose() {
        println!("diag: AttachThreadInput fg={a1:?} target={a2:?} (fg tid {fg_tid}, my tid {my_tid})");
    }
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = BringWindowToTop(hwnd);
    let ok = SetForegroundWindow(hwnd);
    let _ = AttachThreadInput(my_tid, target_tid, false);
    let _ = AttachThreadInput(my_tid, fg_tid, false);
    if win::verbose() {
        println!(
            "diag: SetForegroundWindow -> {ok:?} (last error {:?})",
            std::io::Error::last_os_error().raw_os_error()
        );
    }
    if GetForegroundWindow() == hwnd {
        return true;
    }
    // A window restored from the minimized state is handed the foreground by the
    // shell itself, which is the only route left when the foreground lock refuses
    // a plain SetForegroundWindow.
    let _ = ShowWindow(hwnd, SW_SHOWMINIMIZED);
    sleep(Duration::from_millis(120));
    let _ = ShowWindow(hwnd, SW_RESTORE);
    sleep(Duration::from_millis(120));
    let _ = SetForegroundWindow(hwnd);
    if GetForegroundWindow() == hwnd {
        return true;
    }
    // Windows keeps a "foreground lock": a process that has not produced input
    // recently is not allowed to steal the foreground window. An injected,
    // otherwise harmless modifier press releases that lock, which is what makes
    // the test runnable from a shell instead of only from an interactive click.
    use windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS;
    let mk = |up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VK_LCONTROL,
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
    };
    let _ = SendInput(&[mk(false), mk(true)], std::mem::size_of::<INPUT>() as i32);
    sleep(Duration::from_millis(40));
    let _ = AttachThreadInput(my_tid, fg_tid, true);
    let _ = SetForegroundWindow(hwnd);
    let _ = AttachThreadInput(my_tid, fg_tid, false);
    GetForegroundWindow() == hwnd
}

/// Bring the demo window to a known keyboard layout: both the layout the hook
/// tracks and the one the OS reports for the edit control's thread.
unsafe fn ensure_layout(edit: HWND, want: u32) -> bool {
    for i in 0..8 {
        let tid = GetWindowThreadProcessId(edit, None);
        // The layout belongs to the thread of the focused window, so Alt+Shift
        // has to be pressed while this very window is in front.
        focus_edit(edit);
        pump_ms(150);
        let os = ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32;
        if os == want {
            return true;
        }
        println!("diag: layout attempt {i}: thread {tid} os {os}");
        press_alt_shift();
        pump_ms(300);
    }
    false
}

/// Type `text` into a private EDIT window **without installing a hook**, so a
/// resident `zя run` process in another PID can be observed fixing it.
pub fn peer_test(text: &str) -> anyhow::Result<()> {
    std::thread::spawn(|| {
        sleep(Duration::from_secs(20));
        println!("peer test timed out");
        std::process::exit(3);
    });
    unsafe {
        let cls: Vec<u16> = "ZyaPeer".encode_utf16().chain(std::iter::once(0)).collect();
        let class = WNDCLASSW {
            lpfnWndProc: Some(demo_wndproc),
            lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "EDIT"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "zя peer"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            10,
            10,
            420,
            200,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(edit, SW_SHOW);
        pump_ms(400);
        if !focus_edit(edit) {
            let _ = DestroyWindow(edit);
            return Err(anyhow::anyhow!("could not focus the peer window"));
        }
        set_text(edit, "");
        if !type_into(edit, text, 90) {
            let _ = DestroyWindow(edit);
            return Err(anyhow::anyhow!("lost the peer window while typing"));
        }
        pump_ms(900);
        if std::env::var("ZYA_PEER_UNDO").is_ok() {
            println!("peer: pressing Alt (undo)");
            press_alt();
            pump_ms(800);
        }
        println!("peer result: {:?}", text_of(edit));
        let _ = DestroyWindow(edit);
        Ok(())
    }
}

/// A probe that installs no hook of its own: it just types into a fresh EDIT
/// window and reports what landed. If the text was rewritten, some *other*
/// keyboard hook (a second zя instance, Punto Switcher, ...) is resident.
pub fn passive_probe() -> anyhow::Result<()> {
    unsafe {
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "EDIT"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "zя passive probe"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0),
            40,
            40,
            420,
            200,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(edit, SW_SHOW);
        pump_ms(500);
        focus_edit(edit);
        set_text(edit, "");
        let tid = GetWindowThreadProcessId(edit, None);
        println!(
            "passive: layout of the probe window = {:#06x}",
            ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32
        );
        type_into(edit, "ghbdtn ", 60);
        pump_ms(1500);
        println!("passive: text landed as {:?}", text_of(edit));
        DestroyWindow(edit).ok();
    }
    Ok(())
}

pub fn window_test() -> anyhow::Result<()> {
    // Watchdog: the window test must never hang forever.
    std::thread::spawn(|| {
        sleep(Duration::from_secs(180));
        println!("window test timed out");
        std::process::exit(3);
    });
    unsafe {
        let cls: Vec<u16> = "ZyaDemo".encode_utf16().chain(std::iter::once(0)).collect();
        let class = WNDCLASSW {
            lpfnWndProc: Some(demo_wndproc),
            lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }
        // An EDIT control can be a top-level window; that keeps focus simple.
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "EDIT"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "zя window test"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            // ES_MULTILINE so Enter really inserts a line break: without it the
            // L-02 test cannot see whether the correction landed on the new line.
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | 0x0004 | 0x0040),
            0,
            0,
            420,
            260,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(edit, SW_SHOW);
        pump_ms(400);
        if !focus_edit(edit) {
            return Err(anyhow::anyhow!("could not focus the edit control"));
        }
        set_text(edit, "");
        let fg = GetFocus();
        println!("diag: edit={edit:?} focus={fg:?} text={:?}", text_of(edit));

        let en_name: Vec<u16> = EN.encode_utf16().collect();
        let ru_name: Vec<u16> = RU.encode_utf16().collect();
        // Before installing our own hook, probe the machine: if a keyboard hook
        // belonging to another copy of zя (or another switcher) is already
        // resident, it will rewrite this word and the whole test becomes a race
        // between two hooks.
        focus_edit(edit);
        set_text(edit, "");
        type_into(edit, "ghbdtn ", 60);
        pump_ms(900);
        let probe = text_of(edit);
        set_text(edit, "");
        if probe.trim() != "ghbdtn" {
            println!(
                "diag: другой перехватчик клавиатуры уже работает (он превратил \"ghbdtn\" в {:?}). \
                Два экземпляра переключают одно и то же слово дважды. Завершите предыдущий: \
                значок в трее → «Выход», затем повторите тест.",
                probe
            );
            return Err(anyhow::anyhow!("another keyboard hook is resident"));
        }

        win::install_hook()?;
        println!(
            "diag: hook installed, langid={}, verbose={}, pid={}, other_instance={}",
            win::current_layout_langid(),
            win::verbose(),
            std::process::id(),
            win::another_instance_running()
        );
        let _en = LoadKeyboardLayoutW(windows::core::PCWSTR(en_name.as_ptr()), KLF_ACTIVATE)?;

        if !type_into(edit, "ghbdtn ", 60) {
            return Err(anyhow::anyhow!("lost the demo window during test 1"));
        }
        pump_ms(300);
        let got = wait_for_text(edit, "привет ", 4000);
        println!(
            "test 1: physical \"ghbdtn\" (EN layout) -> \"{got}\"  langid={}",
            win::current_layout_langid()
        );
        assert_eq!(got, "привет ", "wrong-layout word was not fixed");

        // The hook switched the thread layout to Russian; the same physical keys
        // now produce the correct Russian word and must be left alone.
        focus_edit(edit);
        if !type_into(edit, "ghbdtn ", 60) {
            return Err(anyhow::anyhow!("lost the demo window during test 2"));
        }
        pump_ms(300);
        let got2 = wait_for_text(edit, "привет привет ", 4000);
        println!("test 2: same keys after the switch -> \"{got2}\"");
        assert_eq!(got2, "привет привет ", "correct word was rewritten");

        // Force the Russian layout and press the physical keys h,e,l,l,o which
        // now produce "руддщ" - the engine must turn them back into "hello".
        let _ru = LoadKeyboardLayoutW(windows::core::PCWSTR(ru_name.as_ptr()), KLF_ACTIVATE);
        pump_ms(200);
        focus_edit(edit);
        if !type_into(edit, "hello ", 60) {
            return Err(anyhow::anyhow!("lost the demo window during test 3"));
        }
        pump_ms(300);
        let got3 = wait_for_text(edit, "привет привет hello ", 4000);
        println!("test 3: physical \"hello\" with RU layout -> \"{got3}\"");
        assert!(got3.ends_with("hello "), "RU->EN fix failed: {got3}");

        // The configured undo hotkey (default: bare Alt) must undo the last
        // conversion and switch the layout back.
        focus_edit(edit);
        pump_ms(200);
        println!(
            "diag 4: before undo focus={:?} foreground={:?} edit={:?} text=\"{}\"",
            GetFocus(),
            GetForegroundWindow(),
            edit,
            text_of(edit)
        );
        press_hotkey_in(edit, win::configured_undo());
        let got4 = wait_for_text(edit, "привет привет руддщ ", 4000);
        println!(
            "test 4: undo hotkey after the fix -> \"{got4}\"  langid={}",
            win::current_layout_langid()
        );
        assert!(got4.ends_with("руддщ "), "undo failed: {got4}");

        // A genuine user Alt+Shift must be tracked: the hook's idea of the layout
        // has to match what the window's thread actually uses.
        focus_edit(edit);
        press_alt_shift();
        pump_ms(600);
        let tid = GetWindowThreadProcessId(edit, None);
        let os_lang = ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as i32;
        let tracked = win::current_layout_langid() as i32;
        println!("test 5: user Alt+Shift -> tracked {tracked}, OS {os_lang}");
        assert_eq!(tracked, os_lang, "layout tracking desynchronised");
        set_text(edit, "");
        if !type_into(edit, "hello ", 60) {
            return Err(anyhow::anyhow!("lost the demo window during test 5"));
        }
        pump_ms(300);
        let got5 = wait_for_text(edit, "hello ", 3000);
        println!("test 5 typed \"hello\" -> \"{got5}\"");
        assert_eq!(got5, "hello ", "typing after a manual switch was broken");

        // Stress: type fast, alternating layouts, no artificial pumping between
        // keys (a real user does not wait for the message loop).
        set_text(edit, "");
        focus_edit(edit);
        if !type_into(edit, "ghbdtn hf,jnf bqk rudдщ the ", 60) {
            return Err(anyhow::anyhow!("lost the demo window during test 6"));
        }
        pump_ms(1200);
        let got6 = text_of(edit);
        println!("test 6: fast mixed typing -> \"{got6}\"");
        assert!(
            got6.contains("привет") && got6.contains("работа") && got6.contains("the"),
            "fast typing corrupted: {got6}"
        );

        println!("hook events seen: {}", win::event_count());
        println!("switches performed: {}", win::switch_count());

        // 7. A login/password form must never be rewritten. A real form is a
        // captioned parent window with an EDIT child; ES_PASSWORD (0x0010) is the
        // same bit as WS_CHILD, so the style cannot be used to detect password
        // boxes — only the window title/class heuristics are reliable.
        let form = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "ZyaDemo"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "Sign in — password"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            60,
            60,
            340,
            180,
            None,
            None,
            None,
            None,
        )?;
        let pw = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "EDIT"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "password"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            0,
            0,
            330,
            170,
            Some(form),
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(form, SW_SHOW);
        pump_ms(400);
        if focus_edit(pw) {
            // The login control is a child window, so keystrokes occasionally
            // land elsewhere; retry a few times and then check the invariant:
            // the hook must not turn anything in a login field Cyrillic.
            let mut got7 = String::new();
            for _ in 0..3 {
                set_text(pw, "");
                focus_edit(pw);
                if !type_into(pw, "ghbdtn ", 80) {
                    break;
                }
                pump_ms(700);
                got7 = text_of(pw);
                if got7 == "ghbdtn " {
                    break;
                }
            }
            println!("test 7: login form -> \"{got7}\" (must stay as typed)");
            assert!(
                !got7.chars().any(crate::layout::is_cyrillic),
                "the hook rewrote a login form: {got7:?}"
            );
        } else {
            println!("test 7: skipped (could not focus the login form control)");
        }
        if let Err(e) = DestroyWindow(pw) {
            println!("diag: destroy login control: {e}");
        }
        if let Err(e) = DestroyWindow(form) {
            println!("diag: destroy login form: {e}");
        }

        // 8. A modifier-chord shortcut (Alt+Shift+3) must not desynchronise the
        // hook's idea of the layout from the window thread's real layout.
        focus_edit(edit);
        pump_ms(300);
        let tid8 = GetWindowThreadProcessId(edit, None);
        let os_before = ((GetKeyboardLayout(tid8).0 as u64 as u32) & 0xFFFF) as i32;
        press_alt_shift_key(VK_3);
        pump_ms(500);
        let os_after = ((GetKeyboardLayout(tid8).0 as u64 as u32) & 0xFFFF) as i32;
        let tracked = win::current_layout_langid() as i32;
        println!(
            "test 8: Alt+Shift+3 -> tracked {tracked}, OS before {os_before}, after {os_after}"
        );
        assert_eq!(
            tracked, os_after,
            "layout tracking desynchronised by a shortcut"
        );

        // 9. Alt+<key> is an application shortcut and must not undo; a bare Alt
        // press-and-release still undoes the last correction.
        focus_edit(edit);
        pump_ms(300);
        set_text(edit, "");
        if !focus_edit(edit) {
            println!("test 9: skipped (focus lost)");
        } else {
            // Work in whichever layout the window thread is in right now; only
            // ASCII key names can be pressed through `scan()`.
            let lang = win::current_layout_langid() as i32;
            let (typed, want, undone) = if lang == 1049 {
                // Russian layout: the keys h,e,l,l,o print "руддщ" -> fix to "hello".
                ("hello ", "hello ", "руддщ ")
            } else {
                // English layout: the keys g,h,b,d,t,n print "ghbdtn" -> "привет".
                ("ghbdtn ", "привет ", "ghbdtn ")
            };
            if !type_into(edit, typed, 80) {
                println!("test 9: skipped (focus lost while typing)");
            } else {
                pump_ms(900);
                let after_fix = text_of(edit);
                press_alt_key(VK_3);
                pump_ms(800);
                let after_shortcut = text_of(edit);
                println!(
                    "diag 9: before undo fg={:?} edit={:?} caret={:?}",
                    GetForegroundWindow(),
                    edit,
                    caret_of(edit)
                );
                press_hotkey_in(edit, win::configured_undo());
                let after_alt = wait_for_text(edit, undone, 4000);
                println!("diag 9: after undo caret={:?}", caret_of(edit));
                println!(
                    "test 9: fix \"{after_fix}\", Alt+3 kept \"{after_shortcut}\", undo hotkey -> \"{after_alt}\""
                );
                assert_eq!(after_fix, want, "test 9 did not correct the word");
                assert_eq!(
                    after_shortcut, want,
                    "Alt+3 shortcut wrongly undid the correction"
                );
                assert_eq!(
                    after_alt, undone,
                    "the configured undo hotkey did not undo the correction"
                );
            }
        }

        // 10. The Pause/Break hotkey switches automatic mode off and on again.
        focus_edit(edit);
        pump_ms(300);
        set_text(edit, "");
        if !focus_edit(edit) {
            println!("test 10: skipped (focus lost)");
        } else {
            press_hotkey(win::configured_toggle());
            pump_ms(300);
            let off = win::auto_enabled();
            let lang = win::current_layout_langid() as i32;
            let typed = if lang == 1049 { "hello " } else { "ghbdtn " };
            type_into(edit, typed, 80);
            pump_ms(900);
            let with_auto_off = text_of(edit);
            press_hotkey(win::configured_toggle());
            pump_ms(300);
            let on = win::auto_enabled();
            set_text(edit, "");
            focus_edit(edit);
            type_into(edit, typed, 80);
            pump_ms(900);
            let with_auto_on = text_of(edit);
            println!(
                "test 10: toggle off={off} typed \"{with_auto_off}\", toggle on={on} -> \"{with_auto_on}\""
            );
            assert!(
                !off,
                "the toggle hotkey did not disable automatic switching"
            );
            assert_eq!(
                with_auto_off, typed,
                "words were switched while auto was off"
            );
            assert!(
                on,
                "the toggle hotkey did not re-enable automatic switching"
            );
            assert_ne!(
                with_auto_on, typed,
                "words were not switched after re-enabling"
            );
        }

        // 11. File names and identifiers (digits, extensions) are left alone.
        // Make sure we are in the English layout before typing the code tokens.
        focus_edit(edit);
        pump_ms(300);
        let ready_en = ensure_layout(edit, 1033);
        println!(
            "diag: test 11 layout tracked={} os={} ready={}",
            win::current_layout_langid(),
            {
                let t = GetWindowThreadProcessId(edit, None);
                ((GetKeyboardLayout(t).0 as u64 as u32) & 0xFFFF) as i32
            },
            ready_en
        );
        set_text(edit, "");
        if !ready_en || !focus_edit(edit) {
            println!("test 11: skipped (focus or layout not ready)");
        } else {
            let code = "ghbdtn.txt ghbdtn123 ghbdtn.com my_file ghbdTn ";
            if !type_into(edit, code, 80) {
                println!("test 11: skipped (focus lost while typing)");
            } else {
                pump_ms(1200);
                let kept = text_of(edit);
                println!("test 11: code-like tokens kept -> \"{kept}\"");
                assert_eq!(kept, code, "the engine rewrote a code-like token");
            }
        }

        // 12. Technical terms with digits: "ddr4" typed while the Russian layout
        // is active prints "ввк4" and must still be repaired.
        focus_edit(edit);
        pump_ms(300);
        let ready_ru = ensure_layout(edit, 1049);
        set_text(edit, "");
        if !ready_ru || !focus_edit(edit) {
            println!("test 12: skipped (focus or layout not ready)");
        } else {
            if !type_into(edit, "ddr4 nvme oled rtx 5060 ", 80) {
                println!("test 12: skipped (focus lost while typing)");
            } else {
                pump_ms(1200);
                let kept = text_of(edit);
                println!("test 12: technical terms typed in RU layout -> \"{kept}\"");
                assert!(
                    kept.contains("ddr4") && kept.contains("nvme") && kept.contains("oled"),
                    "technical terms were not repaired: {kept}"
                );
            }
        }

        // 13. Break switches the layout of the last word when nothing is
        // selected. A hyphen belongs to the word, so "wi-fi" is one token.
        focus_edit(edit);
        pump_ms(300);
        let ready13 = ensure_layout(edit, 1033);
        win::set_auto(false);
        set_text(edit, "");
        focus_edit(edit);
        if !ready13 || !type_into(edit, "ghbdtn ", 60) {
            println!("test 13: skipped (focus or layout not ready)");
        } else {
            pump_ms(600);
            let before = text_of(edit);
            press_hotkey_in(edit, win::configured_last());
            let after = wait_for_text(edit, "привет ", 4000);
            println!("test 13: Break on the last word: \"{before}\" -> \"{after}\"");
            assert_eq!(
                after, "привет ",
                "Break did not switch the layout of the last word"
            );

            // The layout is Russian now: the same physical keys print "цш-аш".
            focus_edit(edit);
            if !type_into(edit, "wi-fi ", 60) {
                println!("test 13b: skipped (focus lost)");
            } else {
                pump_ms(600);
                let hyphen = text_of(edit);
                press_hotkey_in(edit, win::configured_last());
                let fixed = wait_for_text(edit, "привет wi-fi ", 4000);
                println!("test 13b: hyphenated token \"{hyphen}\" -> \"{fixed}\"");
                assert!(
                    fixed.ends_with("wi-fi "),
                    "the hyphen was treated as a separator: {fixed}"
                );
            }
        }
        win::set_auto(true);

        // 14. Break with a selection switches the layout of the selected text.
        // The classic EDIT control does not support Ctrl+A or keyboard
        // selection reliably, so the selection gestures are tested on a
        // RichEdit control, which behaves like the editors users really use.
        use windows::Win32::System::LibraryLoader::LoadLibraryW;
        let msftedit: Vec<u16> = "msftedit.dll"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = LoadLibraryW(windows::core::PCWSTR(msftedit.as_ptr()));
        let rich_class: Vec<u16> = "RichEdit50W"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let host = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(cls.as_ptr()),
            windows::core::PCWSTR(
                "zя selection test"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            60,
            60,
            520,
            300,
            None,
            None,
            None,
            None,
        )?;
        let rich = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(rich_class.as_ptr()),
            windows::core::PCWSTR(
                "".encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            10,
            10,
            480,
            240,
            Some(host),
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(host, SW_SHOW);
        pump_ms(500);
        if !focus_edit(rich) {
            println!("test 14-16: skipped, the rich edit control could not be focused");
        } else {
            set_text(rich, "привет мир");
            pump_ms(300);
            select_all_rich(rich);
            press_hotkey_in(rich, win::configured_selection());
            let selected = wait_for_text(rich, "ghbdtn vbh", 4000);
            println!("test 14: Break on the selection -> \"{selected}\"");
            assert_eq!(
                selected, "ghbdtn vbh",
                "Break did not switch the layout of the selection"
            );

            // 15. Ctrl+Break upper-cases the selection.
            focus_edit(rich);
            press_combo2(VK_LCONTROL, VK_A);
            pump_ms(400);
            press_hotkey_in(rich, win::configured_upper());
            let upper = wait_for_text(rich, "GHBDTN VBH", 4000);
            println!("test 15: Ctrl+Break on the selection -> \"{upper}\"");
            assert_eq!(
                upper, "GHBDTN VBH",
                "Ctrl+Break did not upper-case the selection"
            );

            // 16. Shift+Break swaps the case of the selection.
            focus_edit(rich);
            press_combo2(VK_LCONTROL, VK_A);
            pump_ms(400);
            press_hotkey_in(rich, win::configured_invert());
            let inverted = wait_for_text(rich, "ghbdtn vbh", 4000);
            println!("test 16: Shift+Break on the selection -> \"{inverted}\"");
            assert_eq!(
                inverted, "ghbdtn vbh",
                "Shift+Break did not invert the selection case"
            );
            let _ = DestroyWindow(rich);
        }
        let _ = DestroyWindow(host);
        focus_edit(edit);

        // 17. A process listed in exclude_processes is never corrected.
        win::set_auto(true);
        let own = std::env::current_exe()
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
            .unwrap_or_else(|| "zя.exe".to_string());
        win::set_exclude_processes(&[own.clone()]);
        let _ = ensure_layout(edit, 1033);
        set_text(edit, "");
        focus_edit(edit);
        type_into(edit, "ghbdtn ", 60);
        let blocked = wait_for_text(edit, "ghbdtn ", 1200);
        println!(
            "test 17: excluded process {own:?} (context blocked={}) -> \"{blocked}\"",
            win::context_blocked_now()
        );
        assert_eq!(
            blocked, "ghbdtn ",
            "exclude_processes did not suppress the correction"
        );
        win::set_exclude_processes(&[]);
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        let allowed = wait_for_text(edit, "привет ", 3000);
        println!("test 17b: no exclusions -> \"{allowed}\"");
        assert_eq!(
            allowed, "привет ",
            "the engine stopped working after clearing exclusions"
        );

        // 18. A folder in exclude_dirs suppresses the correction for every
        // executable that lives inside it (this test process included).
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().unwrap().to_string_lossy().to_lowercase();
        win::set_exclude_dirs(&[dir]);
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        pump_ms(900);
        let in_dir = text_of(edit);
        println!("test 18: process from an excluded folder -> \"{in_dir}\"");
        assert_eq!(
            in_dir, "ghbdtn ",
            "exclude_dirs did not suppress the correction"
        );
        win::set_exclude_dirs(&[]);
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        pump_ms(900);
        let outside = text_of(edit);
        println!("test 18b: exclusions cleared -> \"{outside}\"");
        assert_eq!(outside, "привет ", "exclude_dirs were not cleared");

        // 20. Ping-pong guard: a manual Break reversal must not be undone by the
        // automatic layer a second later (the "double trigger" complaint).
        win::set_auto(true);
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        let fixed = wait_for_text(edit, "привет ", 3000);
        press_hotkey_in(edit, win::configured_last());
        pump_ms(900);
        let back = text_of(edit);
        println!("test 20a: auto {fixed:?} then Break -> {back:?}");
        assert_eq!(back, "ghbdtn ", "Break did not revert the automatic fix");
        // Type the very same token again: the guard must not flip it back.
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        pump_ms(1200);
        let guarded = text_of(edit);
        println!("test 20b: same token within 5 s -> {guarded:?}");
        assert_eq!(
            guarded, "ghbdtn ",
            "the ping-pong guard let the just-reverted word flip back"
        );
        // After the guard window closes the word is corrected again.
        pump_ms(5200);
        set_text(edit, "");
        focus_edit(edit);
        let _ = ensure_layout(edit, 1033);
        type_into(edit, "ghbdtn ", 60);
        let again = wait_for_text(edit, "привет ", 3000);
        println!("test 20c: after the guard window -> {again:?}");
        assert_eq!(
            again, "привет ",
            "the engine stopped working after the guard window"
        );

        // 21. The Notepad/Teams case: the layout the hook uses must come from the
        // thread that owns the focused control. Typing the physical keys of
        // "render" while that layout is Russian puts "кутвук" in the document;
        // the hook has to recognise the Cyrillic gibberish and write "render" back.
        win::set_auto(true);
        set_text(edit, "");
        focus_edit(edit);
        if ensure_layout(edit, 0x0419) {
            type_into(edit, "render ", 60);
            let ru = wait_for_text(edit, "render ", 3000);
            println!("test 21: physical \"render\" typed with the RU layout -> {ru:?}");
            assert_eq!(
                ru, "render ",
                "the hook used the root window layout instead of the focused control's"
            );
        } else {
            println!("test 21: skipped, could not select the RU layout for the demo window");
        }

        // 22. The exact Notepad shape: the top-level window and the text control
        // belong to *different threads*, and only the control's thread carries the
        // real layout. The root window is left on English while the child EDIT is
        // switched to Russian.
        let parent_name: Vec<u16> = "zя parent window"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(cls.as_ptr()),
            windows::core::PCWSTR(parent_name.as_ptr()),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            10,
            10,
            460,
            300,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(parent, SW_SHOW);
        pump_ms(400);
        // Put the parent's thread back to English; the child thread will be Russian.
        let parent_en: Vec<u16> = EN.encode_utf16().chain(std::iter::once(0)).collect();
        let _ = LoadKeyboardLayoutW(windows::core::PCWSTR(parent_en.as_ptr()), KLF_ACTIVATE);

        let (tx, rx) = std::sync::mpsc::channel::<(u32, u32, String)>();
        let parent_handle = parent.0 as usize;
        std::thread::spawn(move || {
            let ru: Vec<u16> = RU.encode_utf16().chain(std::iter::once(0)).collect();
            let _ = LoadKeyboardLayoutW(windows::core::PCWSTR(ru.as_ptr()), KLF_ACTIVATE);
            let root = HWND(parent_handle as *mut std::ffi::c_void);
            let child = match CreateWindowExW(
                WINDOW_EX_STYLE(0),
                windows::core::PCWSTR(
                    "EDIT"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<u16>>()
                        .as_ptr(),
                ),
                windows::core::PCWSTR(
                    "child control"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect::<Vec<u16>>()
                        .as_ptr(),
                ),
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
                10,
                10,
                440,
                260,
                Some(root),
                None,
                None,
                None,
            ) {
                Ok(c) => c,
                Err(e) => {
                    println!("test 22: the child control could not be created: {e}");
                    return;
                }
            };
            let _ = SetFocus(Some(child));
            // The edit control only reacts once this thread pumps its own queue.
            for _ in 0..20 {
                let mut msg = MSG::default();
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                sleep(Duration::from_millis(40));
            }
            let child_tid = GetWindowThreadProcessId(child, None);
            let root_tid = GetWindowThreadProcessId(root, None);
            let root_layout = ((GetKeyboardLayout(root_tid).0 as u64 as u32) & 0xFFFF) as u32;
            let site_layout = ((GetKeyboardLayout(child_tid).0 as u64 as u32) & 0xFFFF) as u32;
            set_text(child, "");
            let result = if type_into(child, "render ", 60) {
                wait_for_text(child, "render ", 3000)
            } else {
                String::new()
            };
            tx.send((root_layout, site_layout, result)).ok();
            sleep(Duration::from_millis(300));
            let _ = DestroyWindow(child);
        });
        // The main thread must keep pumping messages: the keyboard hook is
        // installed here, and a blocked pump would deadlock the child thread.
        let deadline = Instant::now() + Duration::from_secs(20);
        let outcome = loop {
            pump_ms(150);
            if let Ok(value) = rx.try_recv() {
                break Some(value);
            }
            if Instant::now() > deadline {
                break None;
            }
        };
        match outcome {
            Some((root_layout, site_layout, result)) => {
                println!(
                    "test 22: root thread layout {root_layout:#06x}, focused control layout {site_layout:#06x}"
                );
                if root_layout == 0x0409 && site_layout == 0x0419 {
                    println!(
                        "test 22: physical \"render\" in a foreign-thread RU control -> {result:?}"
                    );
                    assert_eq!(
                        result, "render ",
                        "the hook trusted the root window thread instead of the focused control"
                    );
                } else {
                    println!("test 22: skipped, the two-thread layout split did not materialise");
                }
            }
            None => println!("test 22: skipped, the child control thread never answered"),
        }
        pump_ms(200);
        let _ = DestroyWindow(parent);
        focus_edit(edit);

        // 19. The tray icon is created and refreshed for every state.
        match tray::attach() {
            Ok(h) => {
                tray::refresh(true, true);
                tray::refresh(false, true);
                tray::refresh(true, false);
                tray::refresh(false, false);
                println!("test 19: tray icon created and refreshed (4 states)");

                // 23. The letters in the icon must follow the layout the OS is
                // really using for the focused control, not the one we last set.
                focus_edit(edit);
                win::set_auto(true);
                if ensure_layout(edit, 0x0419) {
                    win::sync_layout_now();
                    let ru = tray::current_variant();
                    if ensure_layout(edit, 0x0409) {
                        win::sync_layout_now();
                        let en = tray::current_variant();
                        println!("test 23: icon variant with RU layout {ru}, with EN layout {en}");
                        assert_eq!(
                            ru, 0,
                            "the icon did not show RU while the layout was Russian"
                        );
                        assert_eq!(
                            en, 1,
                            "the icon did not show EN while the layout was English"
                        );
                    } else {
                        println!("test 23: skipped, could not return to the English layout");
                    }
                } else {
                    println!("test 23: skipped, could not select the RU layout");
                }
                tray::detach(h);
            }
            Err(e) => println!("test 19: tray icon unavailable: {e}"),
        }

        // 24. L-02: a boundary key (Enter, Tab) right after a wrong-layout word
        // must not send the backspaces onto the new line.
        focus_edit(edit);
        if ensure_layout(edit, 0x0409) {
            win::set_auto(true);
            set_text(edit, "");
            type_into(edit, "ghbdtn", 60);
            pump_ms(250);
            press_vk(windows::Win32::UI::Input::KeyboardAndMouse::VK_RETURN);
            pump_ms(700);
            let with_enter = text_of(edit).replace("\r\n", "\n");
            println!("test 24: \"ghbdtn\" + Enter -> {with_enter:?}");
            assert!(
                with_enter.starts_with("привет\n") && !with_enter.contains("ghbdtn"),
                "the Enter swallowed the correction or moved it after the newline"
            );

            set_text(edit, "");
            type_into(edit, "ghbdtn", 60);
            pump_ms(250);
            press_vk(windows::Win32::UI::Input::KeyboardAndMouse::VK_TAB);
            pump_ms(700);
            let with_tab = text_of(edit);
            println!("test 24b: \"ghbdtn\" + Tab -> {with_tab:?}");
            assert!(
                with_tab.starts_with("привет") && !with_tab.contains("ghbdtn"),
                "the Tab swallowed the correction"
            );
        } else {
            println!("test 24: skipped, the English layout could not be selected");
        }

        // 25. Break is a toggle. Pressing it a second time by mistake must give
        // back the text exactly as it was before the first press — not the token
        // run through the mapping again.
        win::set_auto(true);
        set_text(edit, "");
        focus_edit(edit);
        if ensure_layout(edit, 0x0409) {
            type_into(edit, "ghbdtn ", 60);
            let fixed = wait_for_text(edit, "привет ", 3000);
            press_hotkey_in(edit, win::configured_last());
            pump_ms(800);
            let mirrored = text_of(edit);
            press_hotkey_in(edit, win::configured_last());
            pump_ms(800);
            let restored = text_of(edit);
            println!("test 25: {fixed:?} -> Break {mirrored:?} -> Break {restored:?}");
            assert_eq!(mirrored, "ghbdtn ", "the first Break did not mirror the word");
            assert_eq!(
                restored, "привет ",
                "the second Break did not restore the text before it"
            );
        } else {
            println!("test 25: skipped, the English layout could not be selected");
        }

        win::remove_hook();
        let _ = DestroyWindow(edit);
        println!("window test ok");
        Ok(())
    }
}

/// Length of the longest common subsequence of two character sequences.
fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for &x in a {
        for j in 1..=b.len() {
            cur[j] = if x == b[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(cur[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn data_path(name: &str) -> anyhow::Result<std::path::PathBuf> {
    for dir in ["data", "../data"] {
        let p = std::path::Path::new(dir).join(name);
        if p.exists() {
            return Ok(p);
        }
    }
    Err(anyhow::anyhow!("data/{name} not found"))
}

/// The real-world test: type a 780-character multilingual 3dnews article, typed
/// with the wrong layout, through the live hook and compare the result with the
/// original text.
pub fn article_test() -> anyhow::Result<()> {
    std::thread::spawn(|| {
        sleep(Duration::from_secs(300));
        println!("article test timed out");
        std::process::exit(3);
    });

    let typed = std::fs::read_to_string(data_path("article_typed.txt")?)?;
    let expected = std::fs::read_to_string(data_path("article_expected.txt")?)?;
    let typed = typed.trim_end();
    let expected = expected.trim_end();
    println!(
        "article test: {} символов, {} латинских токенов",
        expected.chars().count(),
        typed
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|t| t.chars().all(|c| c.is_ascii_alphabetic()))
            .count()
    );

    unsafe {
        let cls: Vec<u16> = "ZyaArticle"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let class = WNDCLASSW {
            lpfnWndProc: Some(demo_wndproc),
            lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                "EDIT"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "zя article test"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            // ES_MULTILINE (0x0004) | ES_AUTOVSCROLL (0x0040): the classic single
            // line EDIT control stops accepting characters once the caret reaches
            // the window edge, which is far before the end of a 781-char article.
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0 | 0x0004 | 0x0040),
            40,
            40,
            900,
            600,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(edit, SW_SHOW);
        pump_ms(500);
        if !focus_edit(edit) {
            return Err(anyhow::anyhow!("could not focus the article window"));
        }
        win::install_hook()?;
        let en_name: Vec<u16> = EN.encode_utf16().collect();
        let _ = LoadKeyboardLayoutW(windows::core::PCWSTR(en_name.as_ptr()), KLF_ACTIVATE)?;
        set_text(edit, "");
        focus_edit(edit);

        if !type_into(edit, typed, 25) {
            win::remove_hook();
            let _ = DestroyWindow(edit);
            return Err(anyhow::anyhow!("lost the article window while typing"));
        }
        pump_ms(3000);
        let got = text_of(edit);
        println!(
            "переключений сделано: {}, событий: {}",
            win::switch_count(),
            win::event_count()
        );

        let want: Vec<char> = expected.chars().collect();
        let have: Vec<char> = got.chars().collect();
        let mut same = 0usize;
        let mut diffs = Vec::new();
        for (i, &a) in want.iter().enumerate() {
            let b = have.get(i).copied();
            if b == Some(a) {
                same += 1;
            } else if diffs.len() < 12 {
                diffs.push((i, a, b.unwrap_or('\0')));
            }
        }
        for (i, a, b) in &diffs {
            println!("  позиция {i}: ожидалось {a:?}, получилось {b:?}");
        }
        // A positional match is fragile: one dropped character shifts the whole
        // text and makes every later position look wrong. The longest common
        // subsequence measures how much of the article survived the hook.
        let lcs = lcs_len(&want, &have);
        let acc = lcs as f64 / want.len() as f64;
        println!(
            "восстановление текста: {}/{} = {:.1} % (по позициям {}/{})",
            lcs,
            want.len(),
            acc * 100.0,
            same,
            want.len()
        );
        println!(
            "первая строка: {}",
            got.chars().take(80).collect::<String>()
        );
        if win::verbose() {
            let path = std::path::Path::new("data").join("article_got.txt");
            {
                if let Err(e) = std::fs::write(&path, got.as_bytes()) {
                    println!("  (не удалось сохранить текст: {e})");
                } else {
                    println!("  итоговый текст сохранён в data/article_got.txt");
                }
            }
        }
        win::remove_hook();
        let _ = DestroyWindow(edit);
        if acc < 0.95 {
            return Err(anyhow::anyhow!("article reconstruction accuracy too low"));
        }
        println!("article test ok");
        Ok(())
    }
}

/// Small diagnostic that shows how a selection is made in a classic EDIT or a
/// RichEdit control and whether Ctrl+C really puts it on the clipboard.
pub fn selection_probe(wnd_class: &str) -> anyhow::Result<()> {
    unsafe {
        use windows::Win32::System::LibraryLoader::LoadLibraryW;
        if wnd_class == "RichEdit50W" {
            let _ = LoadLibraryW(windows::core::PCWSTR(
                "msftedit.dll"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ));
        }
        let cls: Vec<u16> = "ZyaProbe"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let wclass = WNDCLASSW {
            lpfnWndProc: Some(demo_wndproc),
            lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&wclass) == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }
        let parent = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(cls.as_ptr()),
            windows::core::PCWSTR(
                "probe host"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
            0,
            0,
            520,
            320,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(parent, SW_SHOW);
        pump_ms(400);
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            windows::core::PCWSTR(
                wnd_class
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            windows::core::PCWSTR(
                "selection probe"
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect::<Vec<u16>>()
                    .as_ptr(),
            ),
            if wnd_class == "EDIT" || wnd_class == "RichEdit50W" {
                WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0)
            } else {
                WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0)
            },
            0,
            0,
            400,
            200,
            if wnd_class == "EDIT" || wnd_class == "RichEdit50W" {
                Some(parent)
            } else {
                None
            },
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(edit, SW_SHOW);
        pump_ms(500);
        println!("probe: focus after SetFocus {:?}", focus_edit(edit));
        println!(
            "probe: focus={:?} foreground={:?} window={:?}",
            GetFocus(),
            GetForegroundWindow(),
            edit
        );
        set_text(edit, "привет мир");
        pump_ms(200);
        println!(
            "probe: text \"{}\" caret {:?}",
            text_of(edit),
            caret_of(edit)
        );
        // 1. caret test: Home, then type Z
        press_vk(VK_HOME);
        pump_ms(250);
        press_vk(VK_Z);
        pump_ms(300);
        println!("probe: after Home+Z text \"{}\"", text_of(edit));
        // 2. keyboard selection: End then Shift+Home, then Delete
        press_vk(VK_END);
        pump_ms(250);
        press_combo2(VK_LSHIFT, VK_HOME);
        pump_ms(300);
        press_vk(VK_DELETE);
        pump_ms(400);
        println!(
            "probe: after Shift+Home + Delete text \"{}\"",
            text_of(edit)
        );
        // 3. Ctrl+A + Delete (RichEdit supports it)
        set_text(edit, "привет мир");
        pump_ms(200);
        press_combo2(VK_LCONTROL, VK_A);
        pump_ms(300);
        press_vk(VK_DELETE);
        pump_ms(400);
        println!("probe: after Ctrl+A + Delete text \"{}\"", text_of(edit));
        // 4. Ctrl+A then Ctrl+C
        set_text(edit, "привет мир");
        pump_ms(200);
        press_combo2(VK_LCONTROL, VK_A);
        pump_ms(300);
        press_combo2(VK_LCONTROL, VK_C);
        pump_ms(600);
        println!(
            "probe: clipboard after Ctrl+A Ctrl+C {:?}",
            win::clipboard_snapshot()
        );
        let _ = DestroyWindow(edit);
        let _ = DestroyWindow(parent);
        Ok(())
    }
}

unsafe fn make_edit_window(class: &str, title: &str, x: i32, y: i32) -> anyhow::Result<HWND> {
    let cls: Vec<u16> = class.encode_utf16().chain(std::iter::once(0)).collect();
    let wclass = WNDCLASSW {
        lpfnWndProc: Some(demo_wndproc),
        lpszClassName: windows::core::PCWSTR(cls.as_ptr()),
        ..Default::default()
    };
    if RegisterClassW(&wclass) == 0 {
        return Err(anyhow::anyhow!("RegisterClassW failed for {class}"));
    }
    let name: Vec<u16> = "EDIT".encode_utf16().chain(std::iter::once(0)).collect();
    let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let edit = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        windows::core::PCWSTR(name.as_ptr()),
        windows::core::PCWSTR(t.as_ptr()),
        WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
        x,
        y,
        420,
        200,
        None,
        None,
        None,
        None,
    )?;
    let _ = ShowWindow(edit, SW_SHOW);
    pump_ms(200);
    Ok(edit)
}

unsafe fn find_window_by_title(title: &str) -> Option<HWND> {
    let t: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    match FindWindowW(
        windows::core::PCWSTR::null(),
        windows::core::PCWSTR(t.as_ptr()),
    ) {
        Ok(h) if !h.0.is_null() => Some(h),
        _ => None,
    }
}

unsafe fn pid_of(hwnd: HWND) -> u32 {
    let mut pid: u32 = 0;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    pid
}

unsafe fn thread_layout(hwnd: HWND) -> u32 {
    let tid = GetWindowThreadProcessId(hwnd, None);
    ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32
}

/// A second process that only owns an EDIT window. The focus test uses it so the
/// two windows really belong to two different applications (and two different
/// keyboard-layout threads). Its contents are reported through a log file:
/// Windows does not let another process read a control's text (WM_GETTEXT on a
/// foreign window returns the window title).
pub fn host_window(title: &str, secs: u64) -> anyhow::Result<()> {
    unsafe {
        let edit = make_edit_window("ZyaHost", title, 60, 60)?;
        // The window belongs to this thread, so the focus has to be set here:
        // SetFocus from another process does not reach it.
        let _ = SetFocus(Some(edit));
        let log = std::env::var("ZYA_HOST_LOG").unwrap_or_else(|_| "focus_host.log".to_string());
        let _ = std::fs::write(&log, format!("host {title} hwnd {:?}\n", edit));
        let deadline = Instant::now() + Duration::from_secs(secs);
        let mut last = Instant::now();
        let mut chars: Vec<char> = Vec::new();
        let mut codes: Vec<u32> = Vec::new();
        while Instant::now() < deadline {
            let mut msg = MSG::default();
            if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                codes.push(msg.message);
                if msg.message == 258 {
                    chars.push(char::from_u32(msg.wParam.0 as u32).unwrap_or('?'));
                }
                if msg.message != 260 && msg.message != 261 {
                    let _ = TranslateMessage(&msg);
                    let _ = DispatchMessageW(&msg);
                }
            } else {
                sleep(Duration::from_millis(10));
            }
            if last.elapsed() >= Duration::from_millis(80) {
                last = Instant::now();
                if GetForegroundWindow() == edit && GetFocus() != edit {
                    let _ = SetFocus(Some(edit));
                }
                let tail: Vec<u32> = codes.iter().rev().take(12).rev().cloned().collect();
                let _ = std::fs::write(
                    &log,
                    format!(
                        "host {title} hwnd {:?} focus {:?} foreground {:?} chars {} codes {tail:?} text={}\n",
                        edit,
                        GetFocus(),
                        GetForegroundWindow(),
                        chars.iter().collect::<String>(),
                        local_text(edit)
                    ),
                );
            }
        }
        DestroyWindow(edit).ok();
    }
    Ok(())
}

/// Read what the host process last reported about its own edit control.
fn host_report(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(s) => s
            .lines()
            .last()
            .unwrap_or("")
            .split("text=")
            .nth(1)
            .unwrap_or("")
            .to_string(),
        Err(_) => String::new(),
    }
}

/// Focus control across two applications. Window A belongs to a second process,
/// window B to this one. Every check verifies that a correction lands in the
/// window that actually holds the keyboard focus, never in the other.
pub fn focus_test() -> anyhow::Result<()> {
    std::thread::spawn(|| {
        sleep(Duration::from_secs(150));
        println!("focus test timed out");
        std::process::exit(3);
    });

    unsafe {
        let exe = std::env::current_exe()?;
        let title_a = format!("zя focus A {}", std::process::id());
        let host_log = format!("focus_host_{}.log", std::process::id());
        let mut host = std::process::Command::new(&exe)
            .args(["hostedit", &title_a, "150"])
            .env("ZYA_HOST_LOG", &host_log)
            .spawn()?;

        let mut a = HWND(std::ptr::null_mut());
        for _ in 0..30 {
            if let Some(h) = find_window_by_title(&title_a) {
                a = h;
                break;
            }
            sleep(Duration::from_millis(200));
        }
        if a.0.is_null() {
            return Err(anyhow::anyhow!(
                "the second application window never appeared"
            ));
        }
        // The host reports its own control text: a foreign process cannot read
        // another process's edit control with WM_GETTEXT (it gets the title).
        let a_text = || host_report(&host_log);
        let b = make_edit_window("ZyaFocusB", "zя focus B", 520, 200)?;

        println!(
            "focus test: A {a:?} pid {} (host process), B {b:?} pid {} (this process)",
            pid_of(a),
            pid_of(b)
        );

        if std::env::var("ZYA_NO_HOOK").is_ok() {
            println!("diag: hook not installed (ZYA_NO_HOOK) — checking raw focus only");
        } else {
            win::install_hook()?;
            win::start_focus_guard();
        }

        // 0. Sanity: keystrokes reach a focused window of another process.
        focus_edit(a);
        pump_ms(600);
        set_text(a, "");
        pump_ms(300);
        if !type_into_dyn(a, &a_text, "abc", 80) {
            return Err(anyhow::anyhow!("keystrokes never reached A"));
        }
        println!("test 0: A raw typing -> {:?}", a_text());

        // 1. A word typed in A is corrected in A only.
        if !focus_edit(a) {
            return Err(anyhow::anyhow!("cannot give focus to A"));
        }
        ensure_layout(a, 1033);
        reset_window(a, &a_text);
        reset_window(b, &|| text_of(b));
        if !type_into_dyn(a, &a_text, "ghbdtn ", 70) {
            return Err(anyhow::anyhow!("keystrokes never reached A"));
        }
        let a1 = wait_for(&a_text, "привет ", 6000);
        let b1 = text_of(b);
        println!("test 1: A -> {a1:?}, B -> {b1:?}");
        assert_eq!(a1, "привет ", "the word typed in A was not corrected in A");
        assert_eq!(b1, "", "the correction leaked into the unfocused window B");

        // 2. The same keys typed in B are corrected in B, A stays untouched.
        println!("diag: switching focus to B");
        if !focus_edit(b) {
            println!(
                "diag: focus B failed, foreground {:?} focus {:?}",
                GetForegroundWindow(),
                GetFocus()
            );
            return Err(anyhow::anyhow!("cannot give focus to B"));
        }
        set_text(b, "");
        ensure_layout(b, 1033);
        if !type_into(b, "ghbdtn ", 70) {
            return Err(anyhow::anyhow!("keystrokes never reached B"));
        }
        let b2 = wait_for_text(b, "привет ", 6000);
        let a2 = a_text();
        println!("test 2: B -> {b2:?}, A -> {a2:?}");
        assert_eq!(b2, "привет ", "the word typed in B was not corrected in B");
        assert_eq!(a2, "привет ", "typing in B changed the text of A");

        // 3. Focus changes in the middle of a word: the pending word of A must
        // not be rewritten once the focus has moved to B.
        reset_window(a, &a_text);
        reset_window(b, &|| text_of(b));
        focus_edit(a);
        pump_ms(300);
        ensure_layout(a, 1033);
        reset_window(a, &a_text);
        pump_ms(300);
        println!(
            "diag: test 3 A layout {} tracked {}",
            thread_layout(a),
            win::current_layout_langid()
        );
        type_into_dyn(a, &a_text, "ghbdtn", 70);
        let pending = a_text();
        println!("diag: focus A -> B, pending text {pending:?}");
        focus_edit(b);
        pump_ms(400);
        press_key(' ');
        pump_ms(900);
        let a3 = a_text();
        let b3 = text_of(b);
        println!(
            "test 3: A holds {pending:?} without a boundary; focus -> B, space typed \
            -> A {a3:?}, B {b3:?}"
        );
        assert_eq!(
            a3, "ghbdtn",
            "the word left pending in A was rewritten after the focus moved to B"
        );
        assert_eq!(b3, " ", "the space typed in B was swallowed by the hook");

        // 4. The same switch with no pause at all: the polling focus guard has not
        // run yet, only the per-keystroke window check can save the situation.
        reset_window(a, &a_text);
        reset_window(b, &|| text_of(b));
        focus_edit(a);
        pump_ms(300);
        ensure_layout(a, 1033);
        reset_window(a, &a_text);
        type_into_dyn(a, &a_text, "ghbdtn", 70);
        focus_edit(b);
        press_key(' ');
        pump_ms(900);
        let a4 = a_text();
        let b4 = text_of(b);
        println!("test 4: immediate focus switch -> A {a4:?}, B {b4:?}");
        assert_eq!(
            a4, "ghbdtn",
            "an instant focus switch still rewrote the word of the previous window"
        );
        assert_eq!(b4, " ", "the space in B was swallowed during the switch");

        // 5. The layout the hook tracks must be the layout of the focused window.
        focus_edit(a);
        pump_ms(400);
        let layout_a = thread_layout(a);
        focus_edit(b);
        pump_ms(400);
        press_key('a');
        pump_ms(300);
        let layout_b = thread_layout(b);
        let tracked = win::current_layout_langid();
        println!(
            "test 5: focus is on B: tracked {tracked}, thread of A {layout_a}, thread of B {layout_b}"
        );
        assert_eq!(
            tracked, layout_b,
            "the hook kept the layout of the window that lost the focus"
        );

        // 6. The manual gesture acts on the focused window only.
        reset_window(b, &|| text_of(b));
        focus_edit(b);
        ensure_layout(b, 1033);
        reset_window(b, &|| text_of(b));
        type_into(b, "ghbdtn ", 70);
        let b5 = wait_for_text(b, "привет ", 4000);
        press_hotkey(win::configured_last());
        pump_ms(900);
        let b6 = text_of(b);
        let a5 = a_text();
        println!("test 6: manual switch in B {b5:?} -> {b6:?}, A stays {a5:?}");
        assert_ne!(
            b5, b6,
            "the manual gesture did not act on the focused window"
        );

        // 7. Ping-pong guard: the token just reverted by Break must not be
        // "corrected" again by the automatic layer, while the manual gesture
        // still works, and nothing leaks to the other window.
        reset_window(b, &|| text_of(b));
        focus_edit(b);
        let _ = ensure_layout(b, 1033);
        type_into(b, "ghbdtn ", 70);
        pump_ms(900);
        let b7 = text_of(b);
        press_hotkey(win::configured_last());
        pump_ms(900);
        let b8 = text_of(b);
        let a_before = a_text();
        focus_edit(a);
        pump_ms(400);
        press_key(' ');
        pump_ms(900);
        let a7 = a_text();
        println!(
            "test 7: guarded typing in B {b7:?}, Break -> {b8:?}; A {a_before:?} + space -> {a7:?}"
        );
        assert_eq!(
            b7, "ghbdtn ",
            "the automatic layer re-flipped a token Break had just reverted"
        );
        assert_eq!(
            b8, "привет ",
            "Break stopped working after the guard kicked in"
        );
        assert_eq!(
            a7,
            format!("{a_before} "),
            "the Break gesture rewrote the window that lost the focus"
        );

        // 8. Focus really moved: the hook's own view of the foreground window.
        focus_edit(b);
        pump_ms(400);
        press_key('x');
        pump_ms(300);
        let tracked_fg = win::foreground_window_matches(b);
        println!("test 8: the hook sees window B as foreground: {tracked_fg}");
        assert!(tracked_fg, "the hook tracked a different foreground window");

        if win::foreign_instance_seen() {
            println!(
                "diag: одновременно работал другой перехватчик клавиатуры — результаты могут быть искажены"
            );
        }
        win::remove_hook();
        DestroyWindow(b).ok();
        host.kill().ok();
        println!("focus test ok");
        Ok(())
    }
}

unsafe fn class_of(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 128];
    let n = GetClassNameW(hwnd, &mut buf);
    if n > 0 {
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    } else {
        String::new()
    }
}

/// The deepest visible descendant of a window: the control that really receives
/// the keystrokes (NotepadTextBox/RichEditD2DPT, Chrome_RenderWidgetHostHWND).
unsafe fn deepest_visible_child(root: HWND) -> HWND {
    let null = HWND(std::ptr::null_mut());
    let mut site = root;
    let mut queue = vec![root];
    let mut head = 0;
    while head < queue.len() {
        let w = queue[head];
        head += 1;
        let mut child = GetWindow(w, GW_CHILD).unwrap_or(null);
        while !child.0.is_null() {
            if IsWindowVisible(child).as_bool() {
                queue.push(child);
                site = child;
            }
            child = GetWindow(child, GW_HWNDNEXT).unwrap_or(null);
        }
    }
    site
}

unsafe fn find_notepad() -> Option<HWND> {
    let cls: Vec<u16> = "Notepad".encode_utf16().chain(std::iter::once(0)).collect();
    match FindWindowW(
        windows::core::PCWSTR(cls.as_ptr()),
        windows::core::PCWSTR::null(),
    ) {
        Ok(h) if !h.0.is_null() => Some(h),
        _ => None,
    }
}

/// Reuse the Notepad window that is already open. Only one window is ever
/// created: a second `notepadtest` run attaches to the same one.
unsafe fn notepad_window() -> anyhow::Result<HWND> {
    if let Some(h) = find_notepad() {
        return Ok(h);
    }
    std::process::Command::new("notepad.exe").spawn()?;
    for _ in 0..40 {
        pump_ms(250);
        if let Some(h) = find_notepad() {
            return Ok(h);
        }
    }
    Err(anyhow::anyhow!("Notepad window never appeared"))
}

unsafe fn click_abs(x: i32, y: i32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS,
    };
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SetCursorPos,
    };
    let mk = |flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SetCursorPos takes physical screen pixels, which is exactly what
    // GetWindowRect reports. The normalised MOUSEEVENTF_ABSOLUTE coordinates of
    // SendInput are relative to the primary monitor only, so on a scaled or
    // multi-monitor desktop they put the pointer somewhere else; use them just
    // as a fallback when SetCursorPos is refused.
    if SetCursorPos(x, y).is_err() {
        let nx = (x as f64 * 65535.0 / GetSystemMetrics(SM_CXSCREEN) as f64) as i32;
        let ny = (y as f64 * 65535.0 / GetSystemMetrics(SM_CYSCREEN) as f64) as i32;
        let _ = SendInput(
            &[mk(MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE, nx, ny)],
            std::mem::size_of::<INPUT>() as i32,
        );
    }
    let mut here = POINT { x: 0, y: 0 };
    let _ = GetCursorPos(&mut here);
    if win::verbose() {
        println!(
            "diag: pointer at ({},{}), wanted ({x},{y})",
            here.x, here.y
        );
    }
    pump_ms(120);
    let _ = SendInput(
        &[mk(MOUSEEVENTF_LEFTDOWN, 0, 0)],
        std::mem::size_of::<INPUT>() as i32,
    );
    pump_ms(80);
    let _ = SendInput(
        &[mk(MOUSEEVENTF_LEFTUP, 0, 0)],
        std::mem::size_of::<INPUT>() as i32,
    );
    pump_ms(300);
}

/// The Notepad editor belongs to another process: a mouse click inside the
/// document is the dependable way to put the caret there (SetFocus from our
/// thread does not reach it).
unsafe fn focus_site(root: HWND, site: HWND) -> bool {
    force_foreground(root);
    pump_ms(150);
    let mut rc = windows::Win32::Foundation::RECT::default();
    if GetWindowRect(site, &mut rc).is_ok() && rc.right - rc.left > 120 && rc.bottom - rc.top > 240
    {
        click_abs(rc.left + 60, rc.top + 180);
        return true;
    }
    false
}

unsafe fn type_foreign(root: HWND, site: HWND, text: &str, gap_ms: u64) {
    focus_site(root, site);
    for c in text.chars() {
        release_stuck_modifiers();
        press_key(c);
        pump_ms(gap_ms);
    }
}

unsafe fn clear_notepad(root: HWND, site: HWND) {
    focus_site(root, site);
    press_combo2(VK_LCONTROL, VK_A);
    pump_ms(250);
    press_vk(VK_DELETE);
    pump_ms(400);
}

/// Notepad's editor text cannot be read from another process (WM_GETTEXT returns
/// the title), so the test reads it back through the clipboard.
unsafe fn clipboard_of(root: HWND, site: HWND) -> String {
    focus_site(root, site);
    press_combo2(VK_LCONTROL, VK_A);
    pump_ms(300);
    press_combo2(VK_LCONTROL, VK_C);
    pump_ms(500);
    win::clipboard_text().unwrap_or_default()
}

unsafe fn press_alt_shift_slow() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS};
    unsafe {
        keybd_event(0x12, 0, KEYBD_EVENT_FLAGS(0), 0);
        pump_ms(60);
        keybd_event(0x10, 0, KEYBD_EVENT_FLAGS(0), 0);
        pump_ms(60);
        keybd_event(0x10, 0, KEYBD_EVENT_FLAGS(2), 0);
        pump_ms(60);
        keybd_event(0x12, 0, KEYBD_EVENT_FLAGS(2), 0);
    }
}

unsafe fn press_win_space() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS};
    unsafe {
        keybd_event(0xA0, 0, KEYBD_EVENT_FLAGS(0), 0);
        pump_ms(60);
        keybd_event(0x20, 0, KEYBD_EVENT_FLAGS(0), 0);
        pump_ms(60);
        keybd_event(0x20, 0, KEYBD_EVENT_FLAGS(2), 0);
        pump_ms(60);
        keybd_event(0xA0, 0, KEYBD_EVENT_FLAGS(2), 0);
    }
}

/// Put the editor thread into layout `want`. Windows cycles layouts with
/// Alt+Shift, but when the second input method is an IME the language bar hotkey
/// is sometimes ignored, so Win+Space (the input-method switch) is tried too.
unsafe fn ensure_layout_foreign(root: HWND, site: HWND, want: u32) -> bool {
    for i in 0..10 {
        focus_site(root, site);
        let tid = GetWindowThreadProcessId(site, None);
        let os = ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32;
        if os == want {
            return true;
        }
        println!("diag: layout attempt {i}: site thread {tid} os {os}");
        if i % 2 == 0 {
            press_alt_shift_slow();
        } else {
            press_win_space();
        }
        pump_ms(400);
    }
    false
}

/// Minimal probe: click into the Notepad editor and type a string, so the result
/// can be checked with a screenshot.
pub fn notepad_type(text: &str) -> anyhow::Result<()> {
    unsafe {
        let notepad = notepad_window()?;
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _ = ShowWindow(notepad, SW_SHOW).ok();
        let site = deepest_visible_child(notepad);
        press_vk(VK_ESCAPE);
        pump_ms(200);
        focus_site(notepad, site);
        println!(
            "np: fg {} site {} layout {:#06x}",
            GetForegroundWindow().0 as usize,
            class_of(site),
            thread_layout(site)
        );
        for c in text.chars() {
            press_key(c);
            pump_ms(60);
        }
        pump_ms(400);
        println!("np: typed {text:?} layout {:#06x}", thread_layout(site));
        for i in 0..4 {
            press_alt_shift_slow();
            pump_ms(400);
            println!("np: alt+shift {i} -> {:#06x}", thread_layout(site));
        }
    }
    Ok(())
}

/// Probe: type a physical key sequence into Notepad and read the document back,
/// so we can see which layout the editor really uses.
pub fn notepad_layout_probe() -> anyhow::Result<()> {
    unsafe {
        let notepad = notepad_window()?;
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _ = ShowWindow(notepad, SW_SHOW).ok();
        let site = deepest_visible_child(notepad);
        let tid = GetWindowThreadProcessId(site, None);
        let rid = GetWindowThreadProcessId(notepad, None);
        let lay = |t: u32| ((GetKeyboardLayout(t).0 as u64 as u32) & 0xFFFF) as u32;
        force_foreground(notepad);
        pump_ms(300);
        press_vk(VK_ESCAPE);
        pump_ms(200);
        println!(
            "probe: site {tid} {:#06x} root {rid} {:#06x} fg {:#x}",
            lay(tid),
            lay(rid),
            GetForegroundWindow().0 as usize
        );
        for round in 0..4 {
            clear_notepad(notepad, site);
            for c in "ghbdtn".chars() {
                press_key(c);
                pump_ms(50);
            }
            pump_ms(300);
            println!(
                "probe round {round}: layout {:#06x} typed ghbdtn -> {:?}",
                lay(tid),
                clipboard_of(notepad, site)
            );
            press_alt_shift();
            pump_ms(800);
        }
    }
    Ok(())
}

/// Type a physical key sequence into the Notepad document without installing a
/// hook, so a resident hook in another process can see it as genuine typing.
pub fn notepad_keys(text: &str) -> anyhow::Result<()> {
    unsafe {
        let notepad = notepad_window()?;
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _site = deepest_visible_child(notepad);
        for _i in 0..6 {
            force_foreground(notepad);
            pump_ms(250);
            if GetForegroundWindow() == notepad {
                break;
            }
        }
        let (root_lid, site_lid, cls) = win::layout_probe();
        println!(
            "npkeys: fg {:#x} root {root_lid:#06x} site {cls} {site_lid:#06x}",
            GetForegroundWindow().0 as usize
        );
        for c in text.chars() {
            press_physical(c);
            pump_ms(100);
        }
        pump_ms(1500);
    }
    Ok(())
}

/// Does the low-level hook actually fire inside this process?
/// Live event counter: install the hook, pump messages, print the event count
/// once a second. Lets a test confirm the hook is actually firing.
pub fn hook_watch(secs: u64) -> anyhow::Result<()> {
    unsafe {
        win::install_hook()?;
        win::start_focus_guard();
        let until = Instant::now() + Duration::from_secs(secs);
        let mut tick = 0;
        while Instant::now() < until {
            let mut msg = MSG::default();
            if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                sleep(Duration::from_millis(50));
            } else if msg.message != 260 && msg.message != 261 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            tick += 1;
            if tick % 20 == 0 {
                println!(
                    "watch t={} events {} layout {:#06x}",
                    tick / 20,
                    win::event_count(),
                    win::active_layout_langid()
                );
            }
        }
        println!("watch done: events {}", win::event_count());
        win::remove_hook();
    }
    Ok(())
}

/// A key press without our tag: the hook treats it as a physical key, which is
/// what a real user's Ctrl+` looks like.
fn raw_key(vk: VIRTUAL_KEY, up: bool) -> u32 {
    unsafe {
        let input = INPUT {
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
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32)
    }
}

/// `zя inject <n>` — send <n> plain 'q' key presses, nothing else. Used to
/// check that a hook installed in another process sees the injected keys.
pub fn inject_keys(count: u32) -> anyhow::Result<()> {
    for _ in 0..count {
        raw_key(VK_Q, false);
        raw_key(VK_Q, true);
        sleep(Duration::from_millis(150));
    }
    println!("inject: sent {count} q keys");
    Ok(())
}

/// `zя addwordtest` — the Ctrl+` gesture against a window of this process: a
/// phrase is put into an edit control, selected with the keyboard, copied, and
/// Ctrl+` must put the first word of the selection into the dictionary.
pub fn addword_test() -> anyhow::Result<()> {
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_END, VK_HOME, VK_LSHIFT};
        win::install_hook()?;
        let edit = make_edit_window("EDIT", "ZyaAddWord", 200, 200)?;
        focus_edit(edit);
        set_text(edit, "эксперимент проверка слов");
        pump_ms(400);
        println!(
            "addwordtest: focus {:?} foreground {:?} text {:?}",
            GetFocus(),
            window_title(GetForegroundWindow()),
            text_of(edit)
        );
        press_vk(VK_HOME);
        pump_ms(150);
        press_combo2(VK_LSHIFT, VK_END);
        pump_ms(200);
        press_combo2(VK_LCONTROL, VK_C);
        pump_ms(400);
        println!("addwordtest: clipboard {:?}", win::clipboard_snapshot());
        press_combo2(VK_LCONTROL, VK_OEM_3);
        pump_ms(2500);
        println!("addwordtest: hook events {}", win::event_count());
        let fresh = crate::config::load();
        println!("addwordtest: ru list {}", fresh.short_ru.len());
        for word in &fresh.short_ru {
            print!("{word} ");
        }
        println!();
        println!("addwordtest: en list {}", fresh.short_en.len());
        for word in &fresh.short_en {
            print!("{word} ");
        }
        println!();
        win::remove_hook();
    }
    Ok(())
}

pub fn hook_smoke() -> anyhow::Result<()> {
    unsafe {
        match win::install_hook() {
            Ok(()) => println!("smoke: hook installed ok"),
            Err(e) => println!("smoke: install failed {e}"),
        }
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_secs(1));
            use windows::Win32::UI::Input::KeyboardAndMouse::{keybd_event, KEYBD_EVENT_FLAGS};
            for _ in 0..5 {
                let r = keybd_event(0x41, 0, KEYBD_EVENT_FLAGS(0), 0);
                println!("smoke: keybd_event -> {r:?}");
                std::thread::sleep(Duration::from_millis(80));
                keybd_event(0x41, 0, KEYBD_EVENT_FLAGS(2), 0);
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            let mut msg = MSG::default();
            if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                sleep(Duration::from_millis(20));
                continue;
            }
            if msg.message != 260 && msg.message != 261 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            println!("smoke: events so far {}", win::event_count());
        }
        println!("smoke: events = {}", win::event_count());
        win::remove_hook();
    }
    Ok(())
}

/// The exact scenario the user reports: the Notepad editor is in the Russian
/// layout, the physical keys spell an English word, the document ends up with
/// Russian gibberish. One process: the hook and the typing are both ours.
pub fn notepad_hook_test() -> anyhow::Result<()> {
    unsafe {
        let notepad = notepad_window()?;
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _ = ShowWindow(notepad, SW_SHOW).ok();
        pump_ms(300);
        let site = deepest_visible_child(notepad);
        let (root_lid, site_lid, site_class) = win::layout_probe();
        println!(
            "notepad: root {root_lid:#06x} site {site_class} {site_lid:#06x} (deepest {site:?})"
        );

        win::install_hook()?;
        win::start_focus_guard();
        win::set_auto(true);

        clear_notepad(notepad, site);
        // Physical keys r,e,n,d,e,r: with the Russian layout they produce
        // "кутвук". The hook must turn it back into "render".
        for c in "render ".chars() {
            press_physical(c);
            pump_ms(90);
        }
        pump_ms(1200);
        let doc = clipboard_of(notepad, site);
        println!("notepad hook test: document now {doc:?}");
        println!("hook events seen: {}", win::event_count());
        win::remove_hook();
        if doc == "render " {
            println!("ok: the hook corrected the Russian gibberish");
        } else {
            println!("FAIL: expected \"render \"");
        }
    }
    Ok(())
}

/// End-to-end: the hook lives in this process (with a real message loop), a
/// helper thread types physical keys into the Notepad document.
pub fn notepad_hook_loop_test() -> anyhow::Result<()> {
    unsafe {
        let notepad = notepad_window()?;
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _ = ShowWindow(notepad, SW_SHOW).ok();
        pump_ms(400);
        let (root_lid, site_lid, cls) = win::layout_probe();
        println!("scene: root {root_lid:#06x} site {cls} {site_lid:#06x}");

        win::install_hook()?;
        win::start_focus_guard();
        win::set_auto(true);

        let np = notepad.0 as usize;
        std::thread::spawn(move || {
            {
                let notepad = HWND(np as *mut std::ffi::c_void);
                pump_ms(1500);
                clear_notepad(notepad, deepest_visible_child(notepad));
                // Physical keys "render" typed while the layout is Russian give
                // "кутвук"; the hook must turn it back into "render".
                for c in "render ".chars() {
                    press_physical(c);
                    std::thread::sleep(Duration::from_millis(120));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });

        let until = Instant::now() + Duration::from_secs(12);
        while Instant::now() < until {
            let mut msg = MSG::default();
            if !PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                sleep(Duration::from_millis(20));
                continue;
            }
            if msg.message != 260 && msg.message != 261 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        println!("loop test: events {} switches {}", win::event_count(), 0);
        let doc = clipboard_of(notepad, deepest_visible_child(notepad));
        println!("loop test: document {doc:?}");
        win::remove_hook();
    }
    Ok(())
}

/// End-to-end test against the real Windows 11 Notepad: one window, reused.
pub fn notepad_test() -> anyhow::Result<()> {
    std::thread::spawn(|| {
        sleep(Duration::from_secs(150));
        println!("notepad test timed out");
        std::process::exit(3);
    });
    unsafe {
        let notepad = notepad_window()?;
        // Put the window somewhere visible: a window spawned by the harness can
        // start parked off-screen.
        MoveWindow(notepad, 300, 300, 800, 500, true).ok();
        let _ = ShowWindow(notepad, SW_SHOW).ok();
        pump_ms(400);
        press_vk(VK_ESCAPE); // close any Find box left open by an earlier run
        pump_ms(300);
        let site = deepest_visible_child(notepad);
        let root_lid = thread_layout(notepad);
        let site_lid = thread_layout(site);
        println!(
            "notepad: root {notepad:?} class {} layout {root_lid:#06x}, editor {site:?} class {} layout {site_lid:#06x}",
            class_of(notepad),
            class_of(site)
        );

        win::install_hook()?;
        win::start_focus_guard();
        win::set_auto(true);

        // 0. Sanity: the keystrokes really land in the Notepad document, and the
        // hook leaves a plausible English word alone in the English layout.
        if !ensure_layout_foreign(notepad, site, 1033) {
            println!("diag: could not reach the English layout in Notepad");
        }
        clear_notepad(notepad, site);
        type_foreign(notepad, site, "hello ", 60);
        pump_ms(800);
        let t0 = clipboard_of(notepad, site);
        println!("notepad test 0: typed \"hello \" -> {t0:?}");
        if t0 != "hello " {
            // Injected input into a window belonging to another process is dropped
            // by some shells. If the keystrokes never land, there is nothing for the
            // hook to correct, so the live checks cannot be run.
            println!("notepad test: keystrokes did not land, skipping the live checks");
            return Ok(());
        }

        // 1. Russian gibberish typed in the Russian layout must become English.
        if !ensure_layout_foreign(notepad, site, 1049) {
            println!("diag: could not reach the Russian layout in Notepad");
        }
        clear_notepad(notepad, site);
        println!("diag: layout before test 1 = {:#06x}", thread_layout(site));
        type_foreign(notepad, site, "render ", 70);
        pump_ms(1500);
        let t1 = clipboard_of(notepad, site);
        println!("notepad test 1: physical \"render \" in RU layout -> {t1:?}");
        assert_eq!(t1, "render ", "Notepad text was not corrected");

        // 2. English-looking keys in the English layout must become Russian.
        if !ensure_layout_foreign(notepad, site, 1033) {
            println!("diag: could not reach the English layout in Notepad");
        }
        clear_notepad(notepad, site);
        type_foreign(notepad, site, "ghbdtn ", 70);
        pump_ms(1500);
        let t2 = clipboard_of(notepad, site);
        println!("notepad test 2: physical \"ghbdtn \" in EN layout -> {t2:?}");
        assert_eq!(t2, "привет ", "Notepad text was not switched to Russian");

        // 3. The manual Break gesture works in Notepad too.
        win::set_auto(false);
        if !ensure_layout_foreign(notepad, site, 1049) {
            println!("diag: could not reach the Russian layout for the manual test");
        }
        clear_notepad(notepad, site);
        type_foreign(notepad, site, "ghbdtn ", 70);
        pump_ms(900);
        let t3a = clipboard_of(notepad, site);
        press_hotkey(win::configured_last());
        pump_ms(1200);
        let t3b = clipboard_of(notepad, site);
        println!("notepad test 3: auto off {t3a:?} + Break -> {t3b:?}");
        assert_ne!(t3a, t3b, "the manual gesture did nothing in Notepad");
        win::set_auto(true);

        if win::foreign_instance_seen() {
            println!("diag: another keyboard hook is active — results may be skewed");
        }
        win::remove_hook();
        println!("notepad test ok");
        Ok(())
    }
}
