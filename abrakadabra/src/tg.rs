//! Telegram Desktop probe. Telegram is a Qt application, so it is the most
//! realistic test of the hook: no classic Win32 EDIT control, its own text
//! widget. We only ever type into "Saved Messages" (Избранное) and never send
//! anything to another chat.

use anyhow::{Context, Result};
use std::io::Write;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HDC, SRCCOPY,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyboardLayout, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, VIRTUAL_KEY, VK_A, VK_BACK, VK_C, VK_ESCAPE, VK_LCONTROL,
    VK_LSHIFT, VK_RETURN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsWindow, SetForegroundWindow,
};

pub struct WinInfo {
    pub hwnd: HWND,
    pub title: String,
    pub class: String,
    pub process: String,
    pub langid: u32,
}

unsafe extern "system" fn enum_windows_proc(hwnd: HWND, arg: LPARAM) -> BOOL {
    let list = arg.0 as *mut Vec<WinInfo>;
    if list.is_null() {
        return BOOL(1);
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(hwnd, &mut cls);
    if n == 0 {
        return BOOL(1);
    }
    let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);

    let mut buf = [0u16; 256];
    let t = GetWindowTextW(hwnd, &mut buf);
    let title = String::from_utf16_lossy(&buf[..t.max(0) as usize]);

    let mut pid = 0u32;
    let tid = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let lang = ((GetKeyboardLayout(tid).0 as u64 as u32) & 0xFFFF) as u32;

    (*list).push(WinInfo {
        hwnd,
        title,
        class,
        process: exe_name(pid),
        langid: lang,
    });
    BOOL(1)
}

fn exe_name(pid: u32) -> String {
    unsafe {
        let h = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        let mut buf = vec![0u16; 512];
        let mut len = 512u32;
        let full = match QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        ) {
            Ok(()) => String::from_utf16_lossy(&buf[..len as usize]),
            Err(_) => String::new(),
        };
        let _ = CloseHandle(h);
        full.rsplit('\\').next().unwrap_or("").to_string()
    }
}

pub fn telegram_windows() -> Result<Vec<WinInfo>> {
    unsafe {
        let mut list: Vec<WinInfo> = Vec::new();
        EnumWindows(
            Some(enum_windows_proc),
            LPARAM((&mut list as *const Vec<WinInfo>) as isize),
        )
        .context("EnumWindows failed")?;
        Ok(list
            .into_iter()
            .filter(|w| {
                IsWindow(Some(w.hwnd)) == BOOL(1)
                    && (w.process.eq_ignore_ascii_case("telegram.exe")
                        || w.title.contains("Telegram")
                        || w.title.contains("Избранное"))
            })
            .collect())
    }
}

/// PrintWindow the whole Telegram window into a BMP file.
pub fn capture_window(hwnd: HWND, path: &str) -> Result<()> {
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).context("GetWindowRect")?;
        let w = rect.right - rect.left;
        let h = rect.bottom - rect.top;
        if w <= 0 || h <= 0 {
            return Err(anyhow::anyhow!("window has no size"));
        }
        let screen = GetDC(None);
        let dc: HDC = CreateCompatibleDC(Some(screen));
        let bmp: HBITMAP = CreateCompatibleBitmap(screen, w, h);
        let _old = SelectObject(dc, bmp.into());
        let ok = PrintWindow(hwnd, dc, PRINT_WINDOW_FLAGS(2)); // PW_RENDERFULLCONTENT
        if ok != BOOL(1) {
            let _ = DeleteObject(bmp.into());
            let _ = DeleteDC(dc);
            return Err(anyhow::anyhow!("PrintWindow failed"));
        }
        write_bmp(path, &grab_bits(dc, bmp, w, h), w, h)
    }
}

/// Grab a screen region with BitBlt (shows exactly what the user sees).
pub fn capture_rect(left: i32, top: i32, right: i32, bottom: i32, path: &str) -> Result<()> {
    unsafe {
        let w = right - left;
        let h = bottom - top;
        if w <= 0 || h <= 0 {
            return Err(anyhow::anyhow!("empty region"));
        }
        let screen = GetDC(None);
        let dc: HDC = CreateCompatibleDC(Some(screen));
        let bmp: HBITMAP = CreateCompatibleBitmap(screen, w, h);
        let _old = SelectObject(dc, bmp.into());
        let _ = BitBlt(dc, 0, 0, w, h, Some(screen), left, top, SRCCOPY);
        write_bmp(path, &grab_bits(dc, bmp, w, h), w, h)
    }
}

unsafe fn grab_bits(dc: HDC, bmp: HBITMAP, w: i32, h: i32) -> Vec<u8> {
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: h,
            biPlanes: 1,
            biBitCount: 32,
            biSizeImage: (w * h * 4) as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = vec![0u8; (w * h * 4) as usize];
    let _ = GetDIBits(
        dc,
        bmp,
        0,
        h as u32,
        Some(bits.as_mut_ptr() as *mut std::ffi::c_void),
        &mut info,
        DIB_RGB_COLORS,
    );
    bits
}

fn write_bmp(path: &str, bits: &[u8], w: i32, h: i32) -> Result<()> {
    let stride = ((w * 4 + 3) & !3) as usize;
    let mut file = std::fs::File::create(path)?;
    let size = 54 + stride * (h as usize);
    file.write_all(b"BM")?;
    file.write_all(&(size as u32).to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&54u32.to_le_bytes())?;
    file.write_all(&40u32.to_le_bytes())?;
    file.write_all(&w.to_le_bytes())?;
    file.write_all(&h.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&((w * h * 4) as u32).to_le_bytes())?;
    file.write_all(&2835u32.to_le_bytes())?;
    file.write_all(&2835u32.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(&0u32.to_le_bytes())?;
    file.write_all(bits)?;
    Ok(())
}

unsafe fn press_scancode(sc: u16) {
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
    let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    std::thread::sleep(std::time::Duration::from_millis(70));
}

unsafe fn press_vk(vk: VIRTUAL_KEY) {
    let mk = |up: bool| INPUT {
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
    let _ = SendInput(&[mk(false), mk(true)], std::mem::size_of::<INPUT>() as i32);
    std::thread::sleep(std::time::Duration::from_millis(70));
}

unsafe fn press_combo2(a: VIRTUAL_KEY, b: VIRTUAL_KEY) {
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
    std::thread::sleep(std::time::Duration::from_millis(200));
}

unsafe fn press_alt_shift() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LSHIFT, VK_MENU};
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
        &[
            mk(VK_MENU, false),
            mk(VK_LSHIFT, false),
            mk(VK_LSHIFT, true),
            mk(VK_MENU, true),
        ],
        std::mem::size_of::<INPUT>() as i32,
    );
    std::thread::sleep(std::time::Duration::from_millis(200));
}

unsafe fn clipboard_text() -> String {
    crate::win::clipboard_snapshot().unwrap_or_default()
}

/// Contents of the Telegram composer, read through select-all + copy.
unsafe fn composer_text() -> String {
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_RIGHT;
    // The probe borrows the clipboard the same way a selection gesture does, so it
    // takes the same guard: otherwise a gesture running on the side thread could
    // write into the clipboard between our snapshot and our blanking.
    let Some(guard) = crate::win::ClipboardGesture::try_acquire() else {
        return String::new();
    };
    // Blank the clipboard first: an empty composer would otherwise leave the
    // previous copy in it and fake a result. The user's own contents are saved
    // and handed back afterwards, so the Telegram probe never leaves a copy of
    // the composer message on the clipboard.
    let saved = crate::win::clipboard_snapshot();
    crate::win::clear_clipboard_text();

    press_combo2(VK_LCONTROL, VK_A);
    pump_ms(200);
    press_combo2(VK_LCONTROL, VK_C);
    pump_ms(300);
    // Collapse the selection to the end, otherwise the next keystroke replaces
    // the whole composer text.
    press_vk(VK_RIGHT);
    pump_ms(150);
    let text = clipboard_text();
    match &saved {
        Some(previous) => {
            crate::win::set_clipboard_text(previous);
        }
        None => {
            crate::win::clear_clipboard_text();
        }
    }
    drop(guard);
    text
}

unsafe fn focus_composer(hwnd: HWND) {
    for attempt in 0..4 {
        if GetForegroundWindow() == hwnd {
            return;
        }
        let _ = bring_to_front(hwnd);
        pump_ms(600);
        if attempt == 3 {
            println!("warning: Telegram is not the foreground window");
        }
    }
}

/// Never send keystrokes to a window that is not Telegram.
unsafe fn telegram_is_foreground(hwnd: HWND) -> bool {
    focus_composer(hwnd);
    GetForegroundWindow() == hwnd
}

/// Make sure the chat currently open in Telegram is "Saved Messages".
unsafe fn open_saved_messages(hwnd: HWND) -> bool {
    let title = window_title(hwnd);
    if title.contains("Избранное") || title.contains("Saved") {
        return true;
    }
    println!("current chat: {title:?} - switching to Saved Messages");
    press_vk(VK_ESCAPE);
    pump_ms(500);
    // The "Saved Messages" row in the chat list: click its centre.
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).ok();
    let x = rect.left + 300;
    let y = rect.top + 245;
    click_at(x, y);
    pump_ms(2500);
    let now = window_title(hwnd);
    println!("chat after click: {now:?}");
    now.contains("Избранное") || now.contains("Saved")
}

unsafe fn click_at(x: i32, y: i32) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
        MOUSEEVENTF_MOVE, MOUSEINPUT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    let sw = GetSystemMetrics(SM_CXSCREEN) as f64;
    let sh = GetSystemMetrics(SM_CYSCREEN) as f64;
    let nx = ((x as f64) / sw * 65535.0) as i32;
    let ny = ((y as f64) / sh * 65535.0) as i32;
    let move_ = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: nx,
                dy: ny,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let mk = |up: bool| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: nx,
                dy: ny,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE
                    | MOUSEEVENTF_ABSOLUTE
                    | if up {
                        MOUSEEVENTF_LEFTUP
                    } else {
                        MOUSEEVENTF_LEFTDOWN
                    },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let _ = SendInput(
        &[move_, mk(false), mk(true)],
        std::mem::size_of::<INPUT>() as i32,
    );
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let mut buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut buf) as usize;
        String::from_utf16(&buf[..n]).unwrap_or_default()
    }
}

unsafe fn clear_composer(hwnd: HWND) {
    focus_composer(hwnd);
    if !telegram_is_foreground(hwnd) {
        println!("warning: Telegram is not in the foreground, skipping clear");
        return;
    }
    // Ctrl+A selects the whole composer; one Backspace removes the selection.
    for attempt in 0..3 {
        press_combo2(VK_LCONTROL, VK_A);
        pump_ms(250);
        press_vk(VK_BACK);
        pump_ms(300);
        if composer_text().is_empty() {
            break;
        }
        println!("clear_composer attempt {attempt} left a residue, retrying");
    }
}

unsafe fn pump_ms(ms: u64) {
    use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, MSG, PM_REMOVE};
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        let mut msg = MSG::default();
        if PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).into() {
            let _ = DispatchMessageW(&msg);
        } else {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

unsafe fn bring_to_front(hwnd: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, IsWindowVisible, ShowWindow, SW_RESTORE,
    };
    if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        pump_ms(500);
    }
    let fg = GetForegroundWindow();
    let tid = GetWindowThreadProcessId(hwnd, None);
    let _ = AttachThreadInput(GetCurrentThreadId(), tid, true);
    let _ = AttachThreadInput(
        GetCurrentThreadId(),
        GetWindowThreadProcessId(fg, None),
        true,
    );
    let _ = BringWindowToTop(hwnd);
    let ok: bool = SetForegroundWindow(hwnd).into();
    let _ = AttachThreadInput(GetCurrentThreadId(), tid, false);
    ok
}

const SCAN: [(char, u16); 39] = [
    ('a', 0x1E),
    ('b', 0x30),
    ('c', 0x2E),
    ('d', 0x20),
    ('e', 0x12),
    ('f', 0x21),
    ('g', 0x22),
    ('h', 0x23),
    ('i', 0x17),
    ('j', 0x24),
    ('k', 0x25),
    ('l', 0x26),
    ('m', 0x32),
    ('n', 0x31),
    ('o', 0x18),
    ('p', 0x19),
    ('q', 0x10),
    ('r', 0x13),
    ('s', 0x1F),
    ('t', 0x14),
    ('u', 0x16),
    ('v', 0x2F),
    ('w', 0x11),
    ('x', 0x2D),
    ('y', 0x15),
    ('z', 0x2C),
    (' ', 0x39),
    (',', 0x33),
    ('.', 0x34),
    ('1', 0x02),
    ('2', 0x03),
    ('3', 0x04),
    ('4', 0x05),
    ('5', 0x06),
    ('6', 0x07),
    ('7', 0x08),
    ('8', 0x09),
    ('9', 0x0A),
    ('0', 0x0B),
];

fn sc_of(c: char) -> u16 {
    SCAN.iter()
        .find(|(k, _)| *k == c.to_ascii_lowercase())
        .map(|(_, s)| *s)
        .unwrap_or(0)
}

/// Type a string into the Telegram composer from a *separate* process, so the
/// resident hook (started with `zя run`) is the one doing the work.
pub fn type_command(text: &str) -> Result<()> {
    let win = main_window()?;
    unsafe {
        focus_composer(win.hwnd);
        ensure_layout(win.hwnd, 1033);
        type_text(text.trim_end_matches('\n'));
        if text.ends_with('\n') {
            press_vk(VK_RETURN);
        }
        pump_ms(900);
    }
    println!("typed into telegram composer: {text:?}");
    Ok(())
}

pub fn read_command() -> Result<()> {
    let win = main_window()?;
    unsafe {
        focus_composer(win.hwnd);
        println!("composer: {:?}", composer_text());
    }
    Ok(())
}

pub fn key_command(which: &str) -> Result<()> {
    let win = main_window()?;
    let h = match which {
        "undo" => crate::win::configured_undo(),
        "toggle" => crate::win::configured_toggle(),
        _ => return Err(anyhow::anyhow!("unknown key: {which}")),
    };
    unsafe {
        focus_composer(win.hwnd);
        press_hotkey(h);
    }
    println!("pressed {which} hotkey");
    Ok(())
}

pub fn probe() -> Result<()> {
    let wins = telegram_windows()?;
    if wins.is_empty() {
        println!("Telegram Desktop не найден среди окон.");
        return Ok(());
    }
    for w in &wins {
        println!(
            "hwnd={} class=\"{}\" title=\"{}\" langid={}",
            w.hwnd.0 as usize, w.class, w.title, w.langid
        );
    }
    unsafe {
        println!("foreground = {}", GetForegroundWindow().0 as usize);
    }
    Ok(())
}

pub fn main_window() -> Result<WinInfo> {
    let wins = telegram_windows()?;
    wins.into_iter()
        .filter(|w| w.process.eq_ignore_ascii_case("telegram.exe"))
        .filter(|w| !w.title.is_empty() && !w.class.contains("Observer"))
        .max_by_key(|w| w.title.len())
        .context("no Telegram window")
}

/// Restore the Telegram window if it is minimized, so keystrokes reach it.
pub fn restore_window() -> Result<WinInfo> {
    let w = main_window()?;
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{IsIconic, ShowWindow, SW_RESTORE};
        if IsIconic(w.hwnd).as_bool() {
            let _ = ShowWindow(w.hwnd, SW_RESTORE);
            pump_ms(800);
        }
        bring_to_front(w.hwnd);
        pump_ms(400);
        let mut rect = RECT::default();
        GetWindowRect(w.hwnd, &mut rect).ok();
        println!(
            "telegram window {}x{} at ({},{})",
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top
        );
    }
    Ok(w)
}

pub fn open_saved_command() -> Result<()> {
    let w = restore_window()?;
    unsafe {
        if open_saved_messages(w.hwnd) {
            println!("Saved Messages открыт");
        } else {
            println!("не удалось открыть Saved Messages");
        }
    }
    Ok(())
}

pub fn snapshot(path: &str) -> Result<()> {
    let w = main_window()?;
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(w.hwnd, &mut rect).ok();
        println!(
            "hwnd={} title=\"{}\" langid={} rect={}x{} at ({},{}) foreground={}",
            w.hwnd.0 as usize,
            w.title,
            w.langid,
            rect.right - rect.left,
            rect.bottom - rect.top,
            rect.left,
            rect.top,
            GetForegroundWindow().0 as usize
        );
        capture_window(w.hwnd, path)?;
    }
    println!("snapshot -> {path}");
    Ok(())
}

unsafe fn press_shifted(c: char) {
    let vk = VIRTUAL_KEY(c.to_ascii_uppercase() as u16);
    let scan = sc_of(c);
    let mk = |vk: VIRTUAL_KEY, scan: u16, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
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
    // wScan is filled in so the hook's ToUnicodeEx sees the real key position.
    let _ = SendInput(
        &[
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LSHIFT,
                        wScan: 0x2A,
                        dwFlags: KEYBD_EVENT_FLAGS(0),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            mk(vk, scan, false),
            mk(vk, scan, true),
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VK_LSHIFT,
                        wScan: 0x2A,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ],
        std::mem::size_of::<INPUT>() as i32,
    );
    std::thread::sleep(std::time::Duration::from_millis(70));
}

unsafe fn type_text(text: &str) {
    for c in text.chars() {
        match c {
            ' ' | ',' | '.' => press_scancode(sc_of(c)),
            c if c.is_ascii_uppercase() => press_shifted(c),
            c if c.is_ascii_digit() => press_vk(VIRTUAL_KEY(c as u16)),
            c if c.is_ascii_alphabetic() => press_scancode(sc_of(c)),
            '_' => press_combo2(
                VK_LSHIFT,
                windows::Win32::UI::Input::KeyboardAndMouse::VK_OEM_MINUS,
            ),
            '-' => press_vk(windows::Win32::UI::Input::KeyboardAndMouse::VK_OEM_MINUS),
            _ => continue,
        }
        pump_ms(140);
    }
}

unsafe fn press_hotkey(h: crate::win::Hotkey) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LWIN, VK_MENU};
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
    }
    // The bare-Alt undo is exactly "Alt down, Alt up"; adding a second press
    // of the same key would only be Alt+Alt.
    if !(h.alt && h.vk == VK_MENU.0) {
        let vk = VIRTUAL_KEY(h.vk);
        let _ = SendInput(
            &[mk(vk, false), mk(vk, true)],
            std::mem::size_of::<INPUT>() as i32,
        );
    }
    for m in mods.iter().rev() {
        let _ = SendInput(&[mk(*m, true)], std::mem::size_of::<INPUT>() as i32);
    }
    pump_ms(300);
}

/// Interactive probe: can we clear the composer and read it back?
pub fn clear_probe() -> Result<()> {
    let win = main_window()?;
    let hwnd = win.hwnd;
    unsafe {
        bring_to_front(hwnd);
        pump_ms(400);
        println!("composer before: {:?}", composer_text());
        clear_composer(hwnd);
        println!("after 80 backspaces: {:?}", composer_text());
        type_text("xyz ");
        pump_ms(500);
        println!("after typing xyz: {:?}", composer_text());
        clear_composer(hwnd);
        println!("cleared again: {:?}", composer_text());
    }
    Ok(())
}

unsafe fn real_layout(hwnd: HWND) -> u32 {
    let tid = GetWindowThreadProcessId(hwnd, None);
    let hkl = GetKeyboardLayout(tid);
    ((hkl.0 as usize) & 0xFFFF) as u32
}

unsafe fn ensure_layout(hwnd: HWND, langid: u32) {
    for _ in 0..4 {
        if real_layout(hwnd) == langid {
            break;
        }
        press_alt_shift();
        pump_ms(300);
    }
}

/// Capture the composer strip of the Telegram window straight from the screen,
/// because PrintWindow on Qt clips the input box.
pub fn composer_shot(path: &str) -> Result<()> {
    let w = main_window()?;
    unsafe {
        let mut rect = RECT::default();
        GetWindowRect(w.hwnd, &mut rect).ok();
        let bottom = rect.bottom;
        capture_rect(rect.left, bottom - 260, rect.right, bottom, path)?;
    }
    println!("composer strip -> {path}");
    Ok(())
}

pub fn composer_test() -> Result<()> {
    let win = main_window()?;
    let hwnd = win.hwnd;
    println!(
        "telegram: hwnd={} class=\"{}\" title=\"{}\" os_langid={}",
        hwnd.0 as usize, win.class, win.title, win.langid
    );

    crate::win::install_hook()?;
    let mut passed = 0;
    let mut total = 0;
    unsafe {
        let ok = bring_to_front(hwnd);
        println!(
            "SetForegroundWindow -> {ok}, foreground = {}",
            GetForegroundWindow().0 as usize
        );
        pump_ms(600);
        if !open_saved_messages(hwnd) {
            return Err(anyhow::anyhow!("could not open Saved Messages in Telegram"));
        }

        let mut check = |name: &str, got: &str, want: &str| {
            total += 1;
            if got.contains(want) {
                passed += 1;
                println!("OK  {name}: {got:?}");
            } else {
                println!("FAIL {name}: {got:?}  (ожидается {want:?})");
            }
        };

        // 1. EN layout, mistyped Russian word -> hook fixes it and switches to RU.
        clear_composer(hwnd);
        ensure_layout(hwnd, 1033);
        println!(
            "layout before typing: {}",
            crate::win::current_layout_langid()
        );
        type_text("ghbdtn ");
        pump_ms(1200);
        check("wrong-layout word", &composer_text(), "привет");

        // 2. The layout really switched: physical keys now produce Russian.
        type_text("vbh ");
        pump_ms(1000);
        check("layout switched in Qt", &composer_text(), "привет мир");

        // 3. The configured undo hotkey (default: Alt) restores the mistyped word.
        clear_composer(hwnd);
        ensure_layout(hwnd, 1033);
        type_text("ghbdtn ");
        pump_ms(1000);
        press_hotkey(crate::win::configured_undo());
        pump_ms(900);
        check("undo hotkey", &composer_text(), "ghbdtn");

        // 4. Code/URL/identifier tokens are left alone.
        clear_composer(hwnd);
        ensure_layout(hwnd, 1033);
        type_text("ghbdtn.com ghbdtn123 my_file ghbdTn ");
        pump_ms(1200);
        check(
            "code protection",
            &composer_text(),
            "ghbdtn.com ghbdtn123 my_file ghbdTn",
        );

        // 5. Correct Russian text typed in the Russian layout is not touched.
        //    (In the RU layout the physical keys ghbdtn ghjdthrf = привет проверка.)
        clear_composer(hwnd);
        ensure_layout(hwnd, 1049);
        type_text("ghbdtn ghjdthrf ");
        pump_ms(1200);
        check("Russian untouched", &composer_text(), "привет проверка");

        // 6. Reverse direction: physical "hello" typed while the layout is
        //    Russian prints "руддщ" and must come back as "hello".
        clear_composer(hwnd);
        ensure_layout(hwnd, 1049);
        type_text("hello ");
        pump_ms(1200);
        check("RU->EN direction", &composer_text(), "hello ");

        // 7. The configured toggle hotkey turns the corrector off, then back on.
        clear_composer(hwnd);
        ensure_layout(hwnd, 1033);
        press_hotkey(crate::win::configured_toggle());
        check(
            "toggle hotkey -> off",
            &format!("{}", crate::win::auto_enabled()),
            "false",
        );
        type_text("ghbdtn ");
        pump_ms(1000);
        check("off means no rewrite", &composer_text(), "ghbdtn ");
        press_hotkey(crate::win::configured_toggle());
        check(
            "toggle hotkey -> on",
            &format!("{}", crate::win::auto_enabled()),
            "true",
        );

        // 8. Technical terms with digits typed in the Russian layout
        //    (physical "ddr4 nvme oled" prints "ввк4 тмью щдув") must be repaired.
        clear_composer(hwnd);
        ensure_layout(hwnd, 1049);
        type_text("ddr4 nvme oled ");
        pump_ms(1400);
        check("technical terms", &composer_text(), "ddr4 nvme oled");

        clear_composer(hwnd);
        total += 1;
        let left = composer_text();
        if left.is_empty() {
            passed += 1;
            println!("OK  composer cleared (nothing was sent)");
        } else {
            println!("FAIL composer still contains {left:?}");
        }
        capture_window(hwnd, "tg_final.bmp")?;
        println!(
            "telegram hook tests: {passed}/{total}, switches={}, layout={}",
            crate::win::switch_count(),
            crate::win::current_layout_langid()
        );
    }
    crate::win::remove_hook();
    if passed == total {
        Ok(())
    } else {
        Err(anyhow::anyhow!("telegram test: {passed}/{total} passed"))
    }
}
