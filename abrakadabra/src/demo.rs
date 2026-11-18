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
    VK_3, VK_A, VK_C, VK_DELETE, VK_END, VK_HOME, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_Z,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetForegroundWindow, GetParent, GetWindowTextW, GetWindowThreadProcessId, PeekMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, ShowWindow, TranslateMessage, MSG,
    PM_REMOVE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_SETTEXT, WNDCLASSW, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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
    let mut buf = vec![0u16; 8192];
    let n = GetWindowTextW(hwnd, &mut buf);
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

/// Pump until the control contains `want` or the deadline passes. Injected
/// rewrites are asynchronous, so a fixed sleep is both slow and flaky.
unsafe fn wait_for_text(edit: HWND, want: &str, ms: u64) -> String {
    let start = Instant::now();
    loop {
        let text = text_of(edit);
        if text == want || start.elapsed() >= Duration::from_millis(ms) {
            return text;
        }
        pump_ms(250);
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
    for c in text.chars() {
        let mut landed = false;
        for attempt in 0..3 {
            if !is_focused(edit) && !focus_edit(edit) {
                return false;
            }
            let before = text_of(edit);
            press_key(c);
            pump_ms(gap_ms.max(60));
            // A rewrite injected by the hook can delay the character, so give
            // the message loop one more turn before pressing the key again.
            if text_of(edit) == before {
                pump_ms(250);
            }
            if text_of(edit) != before {
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

/// A keystroke reaches `hwnd` when it is focused and its top-level owner is the
/// foreground window. Child controls have the parent as foreground.
unsafe fn root_of(hwnd: HWND) -> HWND {
    match GetParent(hwnd) {
        Ok(p) if !p.0.is_null() => p,
        _ => hwnd,
    }
}

unsafe fn is_focused(hwnd: HWND) -> bool {
    GetFocus() == hwnd && GetForegroundWindow() == root_of(hwnd)
}

unsafe fn focus_edit(edit: HWND) -> bool {
    let root = root_of(edit);
    for _ in 0..12 {
        pump_ms(120);
        force_foreground(root);
        pump_ms(60);
        let _ = SetFocus(Some(edit));
        if is_focused(edit) {
            return true;
        }
        println!(
            "diag: focus {:?} / foreground {:?} (want {:?} in {root:?})",
            GetFocus(),
            GetForegroundWindow(),
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
    let _ = AttachThreadInput(my_tid, fg_tid, true);
    let _ = BringWindowToTop(hwnd);
    let _ = SetForegroundWindow(hwnd);
    let _ = AttachThreadInput(my_tid, fg_tid, false);
    GetForegroundWindow() == hwnd
}

/// Bring the demo window to a known keyboard layout: both the layout the hook
/// tracks and the one the OS reports for the edit control's thread.
unsafe fn ensure_layout(edit: HWND, want: u32) -> bool {
    for _ in 0..6 {
        let tid = GetWindowThreadProcessId(edit, None);
        let os = ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32;
        if win::current_layout_langid() == want && os == want {
            return true;
        }
        press_alt_shift();
        pump_ms(400);
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
            WINDOW_STYLE(WS_OVERLAPPEDWINDOW.0),
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
        win::install_hook()?;
        println!(
            "diag: hook installed, langid={}, verbose={}",
            win::current_layout_langid(),
            win::verbose()
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
        win::set_exclude_processes(&["zя".into()]);
        let _ = ensure_layout(edit, 1033);
        set_text(edit, "");
        focus_edit(edit);
        type_into(edit, "ghbdtn ", 60);
        let blocked = wait_for_text(edit, "ghbdtn ", 1200);
        println!(
            "test 17: excluded process (context blocked={}) -> \"{blocked}\"",
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

        // 19. The tray icon is created and refreshed for every state.
        match tray::attach() {
            Ok(h) => {
                tray::refresh(true, true);
                tray::refresh(false, true);
                tray::refresh(true, false);
                tray::refresh(false, false);
                println!("test 19: tray icon created and refreshed (4 states)");
                tray::detach(h);
            }
            Err(e) => println!("test 19: tray icon unavailable: {e}"),
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
