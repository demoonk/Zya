//! Tray icon + context menu, so the switcher behaves like a real resident app.
//!
//! The icon changes with the state: the code shows the layout the hook is
//! currently tracking (`RU` = Russian, `EN` = English). Automatic switching is
//! on for the filled white text and off for the hollow white outline.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyMenu, DestroyWindow, GetCursorPos, PostMessageW, RegisterClassW, SetForegroundWindow,
    ShowWindow, TrackPopupMenu, HICON, IMAGE_FLAGS, MF_CHECKED, MF_STRING,
    SW_HIDE, TPM_LEFTALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_COMMAND, WM_DESTROY, WM_NULL,
    WNDCLASSW, WS_OVERLAPPEDWINDOW,
};

const WM_TRAYICON: u32 = 0x1111;
const CMD_TOGGLE: usize = 1;
const CMD_AUTOSTART: usize = 2;
const CMD_SETTINGS: usize = 3;
const CMD_QUIT: usize = 4;

// Complete ICONREF blocks: BITMAPINFOHEADER + bottom-up 32bpp BGRA + 1bpp AND
// mask. Handing CreateIcon a naked BGRA buffer produced a blue, vertically
// mirrored glyph in the notification area, so we ship the real icon resources.
const DIB_RU_ON: &[u8] = include_bytes!("../data/tray_ru_on.dib");
const DIB_EN_ON: &[u8] = include_bytes!("../data/tray_en_on.dib");
const DIB_RU_OFF: &[u8] = include_bytes!("../data/tray_ru_off.dib");
const DIB_EN_OFF: &[u8] = include_bytes!("../data/tray_en_off.dib");

/// (russian layout, auto on) -> index into the icon table.
fn variant(is_ru: bool, auto: bool) -> usize {
    match (is_ru, auto) {
        (true, true) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (false, false) => 3,
    }
}

fn icons() -> [HICON; 4] {
    static CACHE: OnceLock<[usize; 4]> = OnceLock::new();
    let table = CACHE.get_or_init(|| {
        let mut out = [0usize; 4];
        for (slot, dib) in out
            .iter_mut()
            .zip([DIB_RU_ON, DIB_EN_ON, DIB_RU_OFF, DIB_EN_OFF])
        {
            *slot = unsafe {
                CreateIconFromResourceEx(dib, true, 0x0003_0000, 32, 32, IMAGE_FLAGS(0))
                    .map(|h| h.0 as usize)
                    .unwrap_or(0)
            };
        }
        out
    });
    table.map(|v| HICON(v as *mut std::ffi::c_void))
}

static TRAY_HWND: OnceLock<usize> = OnceLock::new();
static LAST_VARIANT: AtomicU8 = AtomicU8::new(255);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn tip(is_ru: bool, auto: bool) -> String {
    let layout = if is_ru { "RU" } else { "EN" };
    if auto {
        format!("zя — автоисправление включено ({layout})")
    } else {
        format!("zя — автоисправление выключено ({layout})")
    }
}

unsafe fn notify(hwnd: HWND, is_ru: bool, auto: bool, add: bool) -> bool {
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAYICON,
        hIcon: icons()[variant(is_ru, auto)],
        ..Default::default()
    };
    let t = tip(is_ru, auto);
    for (i, c) in t.encode_utf16().enumerate().take(127) {
        nid.szTip[i] = c;
    }
    Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &nid).as_bool()
}

/// Called from the hook whenever the tracked layout or the auto flag changes.
pub fn refresh(is_ru: bool, auto: bool) {
    let Some(&hwnd) = TRAY_HWND.get() else {
        return;
    };
    let v = variant(is_ru, auto) as u8;
    if LAST_VARIANT.load(Ordering::Relaxed) == v {
        return;
    }
    LAST_VARIANT.store(v, Ordering::Relaxed);
    unsafe {
        notify(HWND(hwnd as *mut std::ffi::c_void), is_ru, auto, false);
    }
}

unsafe fn show_menu(hwnd: HWND) {
    // Without a foreground window the menu is drawn but its commands never
    // reach the window procedure - the classic tray-icon trap.
    let _ = SetForegroundWindow(hwnd);
    let auto = crate::win::auto_enabled();
    let autostart = crate::autostart::enabled();
    let mut pt = POINT { x: 0, y: 0 };
    if GetCursorPos(&mut pt).is_err() {
        return;
    }
    // CreatePopupMenu, not CreateMenu: the latter builds a window menu bar,
    // and TrackPopupMenu on it produced a 9-pixel-wide sliver with no text.
    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    // AppendMenuW, not InsertMenuW with MF_BYCOMMAND: with InsertMenuW the
    // second argument is the *existing command id* to insert before, and the
    // ids 0..4 matched nothing, so the items came out in reverse order and the
    // popup collapsed to a nine-pixel sliver.
    let _ = AppendMenuW(
        menu,
        if auto { MF_CHECKED } else { MF_STRING },
        CMD_TOGGLE,
        PCWSTR(wide("Автосправление раскладки").as_ptr()),
    );
    let _ = AppendMenuW(
        menu,
        if autostart { MF_CHECKED } else { MF_STRING },
        CMD_AUTOSTART,
        PCWSTR(wide("Автозапуск при входе").as_ptr()),
    );
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        CMD_SETTINGS,
        PCWSTR(wide("Настройки").as_ptr()),
    );
    let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, PCWSTR(wide("Выход").as_ptr()));
    // The icon sits at the right end of the taskbar: open the menu above the
    // bar, aligned to the cursor.
    let _ = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RIGHTBUTTON,
        pt.x,
        pt.y - 4,
        None,
        hwnd,
        None,
    );
    // The menu must be dismissed explicitly, otherwise the foreground window
    // stays captured by our hidden window and the next click does nothing.
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            let cmd = wparam.0 & 0xFFFF;
            match cmd {
                CMD_TOGGLE => {
                    crate::win::toggle_auto();
                }
                CMD_AUTOSTART => {
                    let exe = crate::autostart::toggle();
                    println!("zя: автозапуск {}", exe);
                }
                CMD_SETTINGS => {
                    crate::settings::open();
                }
                CMD_QUIT => {
                    detach(hwnd);
                    crate::win::remove_hook();
                    std::process::exit(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            // Windows 10/11 deliver the real mouse messages in lparam:
            // WM_LBUTTONUP = 0x0202, WM_RBUTTONUP = 0x0205. The legacy values
            // 1..4 (WM_LBUTTONDOWN..WM_RBUTTONUP) still appear in some shells.
            // Only the release of the button acts, otherwise one click would
            // toggle twice.
            let click = (lparam.0 & 0xFFFF) as u16;
            if matches!(click, 0x0205 | 4) {
                show_menu(hwnd);
            } else if matches!(click, 0x0202 | 2) {
                crate::win::toggle_auto();
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

pub fn attach() -> anyhow::Result<HWND> {
    unsafe {
        let name = wide("ZyaTray");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err(anyhow::anyhow!("RegisterClassW failed"));
        }
        let title = wide("zя");
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            1,
            1,
            None,
            None,
            None,
            None,
        )?;
        let _ = ShowWindow(hwnd, SW_HIDE);
        TRAY_HWND.set(hwnd.0 as usize).ok();
        if !notify(
            hwnd,
            crate::win::current_layout_is_russian(),
            crate::win::auto_enabled(),
            true,
        ) {
            return Err(anyhow::anyhow!("Shell_NotifyIconW failed"));
        }
        Ok(hwnd)
    }
}

pub fn detach(hwnd: HWND) {
    unsafe {
        let nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            ..Default::default()
        };
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        let _ = DestroyWindow(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_indexes_are_distinct() {
        assert_eq!(variant(true, true), 0);
        assert_eq!(variant(false, true), 1);
        assert_eq!(variant(true, false), 2);
        assert_eq!(variant(false, false), 3);
    }

    #[test]
    fn icon_data_is_32x32_bgra() {
        for dib in [DIB_RU_ON, DIB_EN_ON, DIB_RU_OFF, DIB_EN_OFF] {
            // 40-byte BITMAPINFOHEADER + 32*32*4 XOR bitmap + 32*4 AND mask.
            assert_eq!(dib.len(), 40 + 32 * 32 * 4 + 32 * 4);
            let height = i32::from_le_bytes([dib[8], dib[9], dib[10], dib[11]]);
            assert_eq!(
                height, 64,
                "biHeight must cover the XOR bitmap and the mask"
            );
        }
    }

    /// A null HICON makes Shell_NotifyIcon drop the icon from the notification
    /// area, which is exactly what the "the tray icon blinks" report looked like.
    #[test]
    fn every_variant_icon_is_created() {
        for (i, h) in icons().iter().enumerate() {
            assert!(!h.0.is_null(), "icon variant {i} was not created");
        }
    }
}
