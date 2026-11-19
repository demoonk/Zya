//! Tray icon + context menu, so the switcher behaves like a real resident app.
//!
//! The icon changes with the state: the code shows the layout the hook is
//! currently tracking (`RU` = Russian, `EN` = English). Automatic switching is
//! on when the layout code sits on the dark plate alone, off when a red slash
//! crosses it. The letters are white *on a solid dark plate* - white glyphs on
//! a transparent background were invisible on a light Windows taskbar.

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
    ShowWindow, TrackPopupMenu, HICON, IMAGE_FLAGS, MF_CHECKED, MF_STRING, SW_HIDE, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_COMMAND, WM_DESTROY, WM_NULL, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};

// `GetDpiForWindow` lives in `Win32::UI::HiDpi`, which this crate's Cargo.toml
// does not enable, so it is declared against user32 directly (Windows 8.1+).
#[link(name = "user32")]
extern "system" {
    fn GetDpiForWindow(hwnd: HWND) -> u32;
}

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

/// The "tuck the menu a little above the icon" margin. `GetCursorPos` reports
/// physical screen pixels while the 4 px margin is a logical one, so at 150%
/// scaling the raw `- 4` landed the menu visibly off the icon. Convert it with
/// the DPI of the tray window: 96 dpi -> 4 px, 144 dpi -> 6 px, 192 dpi -> 8 px.
fn menu_margin(dpi: u32) -> i32 {
    (4 * dpi as i32) / 96
}

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
    if !Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &nid).as_bool() {
        if crate::win::verbose() {
            extern "system" {
                fn GetLastError() -> u32;
            }
            println!(
                "zя: Shell_NotifyIconW {} failed, last error {}, icon {}",
                if add { "ADD" } else { "MODIFY" },
                unsafe { GetLastError() },
                nid.hIcon.0 as usize
            );
        }
        return false;
    }
    true
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
        let hwnd = HWND(hwnd as *mut std::ffi::c_void);
        // If the shell dropped our icon (it does after a taskbar restart), the
        // MODIFY call fails; re-ADD it instead of leaving the app icon-less.
        if !notify(hwnd, is_ru, auto, false) {
            notify(hwnd, is_ru, auto, true);
        }
    }
}

/// The variant currently shown in the notification area, for tests and probes.
pub fn current_variant() -> u8 {
    LAST_VARIANT.load(Ordering::Relaxed)
}

/// Print which icon the tray is showing right now. Used by the `trayprobe`
/// command to prove the letters follow the layout Windows is really using.
pub fn probe() {
    let is_ru = crate::win::current_layout_is_russian();
    let auto = crate::win::auto_enabled();
    println!(
        "tray: russian layout={} auto={} icon variant {}",
        is_ru,
        auto,
        variant(is_ru, auto)
    );
}

/// Keep the notification-area icon alive. Explorer sometimes discards an icon
/// after a taskbar restart, and on the very first launch of a fresh install the
/// shell may not be ready when NIM_ADD runs. Re-checking every few seconds makes
/// the menu available without a reinstall.
pub fn keep_alive() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(8));
        let Some(&hwnd) = TRAY_HWND.get() else {
            return;
        };
        let is_ru = crate::win::current_layout_is_russian();
        let auto = crate::win::auto_enabled();
        unsafe {
            let h = HWND(hwnd as *mut std::ffi::c_void);
            if !notify(h, is_ru, auto, false) {
                notify(h, is_ru, auto, true);
            }
        }
    });
}

/// Diagnostic: open the tray menu without a click so its geometry can be
/// screenshotted.
pub fn menu_probe() {
    unsafe {
        extern "system" {
            fn FindWindowW(class: *const u16, title: *const u16) -> usize;
            fn PostMessageW(window: usize, msg: u32, w: usize, l: usize) -> i32;
        }
        match attach() {
            Ok(hwnd) => {
                std::thread::spawn(|| {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let cls: Vec<u16> = "#32768".encode_utf16().chain(std::iter::once(0)).collect();
                    let popup = FindWindowW(cls.as_ptr(), std::ptr::null());
                    println!("probe: всплывающее меню {popup}");
                    if popup != 0 {
                        PostMessageW(popup, 0x0018 /* WM_CLOSE */, 0, 0);
                    }
                });
                let mut pt = POINT { x: 0, y: 0 };
                let _ = GetCursorPos(&mut pt);
                println!("probe: курсор {} {}", pt.x, pt.y);
                println!("probe: dpi {}", GetDpiForWindow(hwnd));
                show_menu(hwnd);
                detach(hwnd);
                println!("probe: меню закрыто");
            }
            Err(e) => println!("probe: значок не создан: {e}"),
        }
    }
}

unsafe fn show_menu(hwnd: HWND) {
    // Without a foreground window the menu is drawn but its commands never
    // reach the window procedure - the classic tray-icon trap. On Windows 10/11
    // the same trap shows up as a menu that pops open and closes immediately.
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

    // The icon sits at the right end of the taskbar: anchor the menu at the
    // cursor and lift it by the DPI-scaled margin so it opens above the bar.
    let dpi = GetDpiForWindow(hwnd).max(96) as u32;
    let margin = menu_margin(dpi);
    // TPM_RETURNCMD: TrackPopupMenu returns the picked command on this thread
    // instead of posting WM_COMMAND to a hidden window. That is the reliable
    // way to keep the menu from closing before the command is handled.
    let cmd = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
        pt.x - margin,
        pt.y - margin,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    // Release the foreground grab so the next click on the icon is delivered.
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));

    match cmd.0 as usize {
        CMD_TOGGLE => crate::win::toggle_auto(),
        CMD_AUTOSTART => {
            let exe = crate::autostart::toggle();
            println!("zя: автозапуск {}", exe);
        }
        CMD_SETTINGS => crate::settings::open(),
        CMD_QUIT => {
            detach(hwnd);
            crate::win::remove_hook();
            std::process::exit(0);
        }
        // 0 means the menu was dismissed without a selection.
        _ => {}
    }
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

        // At logon the shell may not be ready yet, and the very first NIM_ADD
        // then fails for good: no icon, so no menu. A fresh install usually
        // starts the app exactly at that moment, which is why the menu was dead
        // on the first launch and worked after reinstalling. Retry for a while
        // instead of giving up.
        let is_ru = crate::win::current_layout_is_russian();
        let auto = crate::win::auto_enabled();
        let mut added = false;
        for attempt in 0..4 {
            if notify(hwnd, is_ru, auto, true) {
                added = true;
                break;
            }
            if attempt < 3 {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
        if !added {
            // The shell was not ready. The window and the message handler exist,
            // so keep_alive() will put the icon back a little later; the menu is
            // still reachable through the window in the meantime.
            LAST_VARIANT.store(255, Ordering::Relaxed);
            if crate::win::verbose() {
                println!("zя: значок ещё не принят shell, повторим позже");
            }
            return Ok(hwnd);
        }
        LAST_VARIANT.store(variant(is_ru, auto) as u8, Ordering::Relaxed);
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

    /// The glyph has to survive a *light* taskbar, so it must sit on an opaque
    /// dark plate instead of a transparent background.
    #[test]
    fn every_icon_has_an_opaque_dark_plate_and_a_white_glyph() {
        for (name, dib) in [
            ("ru_on", DIB_RU_ON),
            ("en_on", DIB_EN_ON),
            ("ru_off", DIB_RU_OFF),
            ("en_off", DIB_EN_OFF),
        ] {
            let width = i32::from_le_bytes([dib[4], dib[5], dib[6], dib[7]]);
            let planes = u16::from_le_bytes([dib[12], dib[13]]);
            let bpp = u16::from_le_bytes([dib[14], dib[15]]);
            assert_eq!((width, planes, bpp), (32, 1, 32), "{name} header");

            let xor = &dib[40..40 + 32 * 32 * 4];
            // Pixels in the middle band of the icon: the plate, fully opaque.
            for y in 8..24 {
                let i = (31 - y) * 128 + 2 * 4;
                let (b, g, r, a) = (xor[i], xor[i + 1], xor[i + 2], xor[i + 3]);
                assert_eq!(a, 255, "{name}: plate pixel at y={y} is transparent");
                assert!(
                    r < 130 && g < 150 && b < 170,
                    "{name}: plate is not dark: r={r} g={g} b={b}"
                );
            }
            // And the letters are really drawn on top of it.
            assert!(
                xor.chunks(4)
                    .any(|p| p[3] == 255 && p[0] > 190 && p[1] > 190 && p[2] > 190),
                "{name}: no white glyph pixels"
            );

            // AND mask: 128 bytes, and the rows covered by the plate are opaque.
            let mask = &dib[40 + 32 * 32 * 4..];
            assert_eq!(mask.len(), 32 * 4, "{name}: mask size");
            assert_eq!(mask[16 * 4], 0xff, "{name}: middle mask row not opaque");
            assert_eq!(mask[16 * 4 + 1], 0xff, "{name}: middle mask row not opaque");
        }
    }

    #[test]
    fn menu_margin_follows_the_display_scale() {
        assert_eq!(menu_margin(96), 4); // 100 %
        assert_eq!(menu_margin(120), 5); // 125 %
        assert_eq!(menu_margin(144), 6); // 150 %
        assert_eq!(menu_margin(192), 8); // 200 %
    }
}
