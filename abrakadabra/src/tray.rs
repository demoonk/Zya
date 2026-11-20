//! Tray icon + context menu, so the switcher behaves like a real resident app.
//!
//! The icon changes with the state: the code shows the layout the hook is
//! currently tracking (`RU` = Russian, `EN` = English). Automatic switching is
//! on when the layout code is clean, off when a red slash crosses it.
//! The letters are drawn directly into a transparent icon resource:
//! white text in dark theme, black text in light theme, without a colored plate.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen, CreateSolidBrush, DeleteObject,
    DrawTextW, FillRect, GetDC, GetTextExtentPoint32W, LineTo, MoveToEx, ReleaseDC, SelectObject,
    SetBkColor, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, CLIP_DEFAULT_PRECIS,
    DEFAULT_CHARSET, DEFAULT_QUALITY, DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
    FF_MODERN, HGDIOBJ, OPAQUE, OUT_DEFAULT_PRECIS, PS_SOLID,
};
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

const ICON_SIZE: i32 = 32;
const MASK_BYTES: usize = ICON_SIZE as usize * 4;
const BITS_BYTES: usize = ICON_SIZE as usize * ICON_SIZE as usize * 4;

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(((b as u32) << 16) | ((g as u32) << 8) | r as u32)
}

/// Draw a 32x32 transparent icon: layout letters only, no solid background plate.
/// Dark theme uses white letters; light theme uses black letters. Auto-off gets a
/// red slash, but the background stays transparent.
unsafe fn tray_icon_resource(is_ru: bool, auto: bool, dark: bool) -> Vec<u8> {
    let label = if is_ru { "RU" } else { "EN" };

    let screen_dc = GetDC(None);
    let mem_dc = CreateCompatibleDC(Some(screen_dc));

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: 40,
            biWidth: ICON_SIZE,
            biHeight: -ICON_SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: BITS_BYTES as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let Ok(bitmap) = CreateDIBSection(Some(screen_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
    else {
        ReleaseDC(None, screen_dc);
        return Vec::new();
    };
    if bitmap.0.is_null() || bits.is_null() {
        ReleaseDC(None, screen_dc);
        return Vec::new();
    }

    let _ = SelectObject(mem_dc, HGDIOBJ(bitmap.0));

    // Pick the biggest Segoe UI glyph that still fits the 32x32 icon: the letters
    // are as wide as the tray icon allows, leaving a one-pixel margin on each side.
    let text = wide(label);
    let mut glyph_font = HGDIOBJ(std::ptr::null_mut());
    for height in (10..=ICON_SIZE).rev() {
        let candidate = CreateFontW(
            height,
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            FF_MODERN.0 as u32,
            PCWSTR(wide("Segoe UI").as_ptr()),
        );
        let _ = SelectObject(mem_dc, HGDIOBJ(candidate.0));
        let mut extent = SIZE { cx: 0, cy: 0 };
        let _ = GetTextExtentPoint32W(mem_dc, &text[..text.len() - 1], &mut extent);
        if extent.cx <= ICON_SIZE - 2 && extent.cy <= ICON_SIZE - 2 {
            glyph_font = HGDIOBJ(candidate.0);
            break;
        }
        let _ = DeleteObject(HGDIOBJ(candidate.0));
    }
    if glyph_font.0.is_null() {
        ReleaseDC(None, screen_dc);
        return Vec::new();
    }
    let _ = SelectObject(mem_dc, glyph_font);

    // The plate is painted white only so that every non-white pixel can be read
    // back as "glyph or slash" when the mask is built. The final icon has no
    // background: the mask makes the plate fully transparent.
    let background = RECT {
        left: 0,
        top: 0,
        right: ICON_SIZE,
        bottom: ICON_SIZE,
    };
    let _ = FillRect(mem_dc, &background, CreateSolidBrush(rgb(255, 255, 255)));
    let _ = SetBkColor(mem_dc, rgb(255, 255, 255));
    let _ = SetTextColor(mem_dc, rgb(0, 0, 0));
    let _ = SetBkMode(mem_dc, OPAQUE);

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: ICON_SIZE,
        bottom: ICON_SIZE,
    };
    let mut glyphs = text.clone();
    DrawTextW(
        mem_dc,
        glyphs.as_mut_slice(),
        &mut rect,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );

    if !auto {
        let pen = CreatePen(PS_SOLID, 3, rgb(220, 0, 0));
        let _ = SelectObject(mem_dc, HGDIOBJ(pen.0));
        let _ = MoveToEx(mem_dc, 5, 27, None);
        let _ = LineTo(mem_dc, 27, 5);
    }

    let pixels = bits as *const u32;
    let mut data = Vec::new();
    data.extend(&40u32.to_le_bytes());
    data.extend(&(ICON_SIZE as u32).to_le_bytes());
    data.extend(&(ICON_SIZE as u32).to_le_bytes());
    data.extend(&1u16.to_le_bytes());
    data.extend(&32u16.to_le_bytes());
    data.extend(&0u32.to_le_bytes());
    data.extend(&(BITS_BYTES as u32).to_le_bytes());
    data.extend(&0i32.to_le_bytes());
    data.extend(&0i32.to_le_bytes());
    data.extend(&0u32.to_le_bytes());
    data.extend(&0u32.to_le_bytes());
    data.resize(40 + BITS_BYTES + MASK_BYTES, 0);

    for y in 0..ICON_SIZE {
        let bottom_row = (ICON_SIZE - 1 - y) as usize;
        for x in 0..ICON_SIZE {
            let px = unsafe { *pixels.add((y * ICON_SIZE + x) as usize) };
            let r = ((px >> 16) & 0xff) as u8;
            let g = ((px >> 8) & 0xff) as u8;
            let b = (px & 0xff) as u8;

            // Anything that is not white-ish belongs to the glyph or slash.
            let opaque = !(r >= 220 && g >= 220 && b >= 220);
            if !opaque {
                continue;
            }

            let (cr, cg, cb) = if r > 150 && g < 90 && b < 90 {
                // red slash
                (220, 0, 0)
            } else if dark {
                (255, 255, 255)
            } else {
                (0, 0, 0)
            };

            let off = 40 + bottom_row * (ICON_SIZE as usize * 4) + x as usize * 4;
            data[off] = cb;
            data[off + 1] = cg;
            data[off + 2] = cr;
            data[off + 3] = 255;

            let mask_byte = 40 + BITS_BYTES + bottom_row * 4 + (x as usize / 8);
            data[mask_byte] |= 1 << (7 - (x as usize % 8));
        }
    }

    ReleaseDC(None, screen_dc);
    data
}

unsafe fn create_tray_icon(is_ru: bool, auto: bool, dark: bool) -> HICON {
    let data = tray_icon_resource(is_ru, auto, dark);
    if data.is_empty() {
        return HICON(std::ptr::null_mut());
    }
    CreateIconFromResourceEx(
        &data,
        true,
        0x0003_0000,
        ICON_SIZE,
        ICON_SIZE,
        IMAGE_FLAGS(0),
    )
    .unwrap_or(HICON(std::ptr::null_mut()))
}

fn icon_for(is_ru: bool, auto: bool) -> HICON {
    unsafe { create_tray_icon(is_ru, auto, crate::theme::is_dark()) }
}

/// (russian layout, auto on) -> index into the icon table.
fn variant(is_ru: bool, auto: bool) -> usize {
    match (is_ru, auto) {
        (true, true) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (false, false) => 3,
    }
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
        hIcon: icon_for(is_ru, auto),
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
        CMD_SETTINGS => {
            let _ = crate::settings::open();
        }
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
                    let _ = crate::settings::open();
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
    fn tray_icons_are_transparent_and_use_theme_text_color() {
        for dark in [true, false] {
            let data = unsafe { tray_icon_resource(true, true, dark) };
            assert_eq!(data.len(), 40 + BITS_BYTES + MASK_BYTES);

            let xor = &data[40..40 + BITS_BYTES];
            let mask = &data[40 + BITS_BYTES..];

            // The top-left corner must remain transparent.
            assert_eq!(xor[31 * 128 + 3], 0, "tray background is not transparent");

            // Some glyph pixels are opaque, and their color follows the theme.
            let expected = if dark { (255, 255, 255) } else { (0, 0, 0) };
            let has_expected_glyph = (0..xor.len() / 4).any(|i| {
                let pixel = &xor[i * 4..i * 4 + 4];
                let row = i / 32;
                let col = i % 32;
                let opaque = mask[row * 4 + col / 8] & (1 << (7 - col % 8)) != 0;
                opaque
                    && pixel[3] == 255
                    && pixel[0] == expected.2
                    && pixel[1] == expected.1
                    && pixel[2] == expected.0
            });
            assert!(
                has_expected_glyph,
                "tray glyph is not {expected:?} in {dark} theme"
            );
        }
    }

    #[test]
    fn tray_previews_can_be_written_for_inspection() {
        if std::env::var("ZYA_TRAY_PREVIEW").is_err() {
            return;
        }
        for (name, ru, auto, dark) in [
            ("tray_ru_on_dark", true, true, true),
            ("tray_ru_on_light", true, true, false),
            ("tray_en_on_dark", false, true, true),
            ("tray_en_on_light", false, true, false),
        ] {
            let data = unsafe { tray_icon_resource(ru, auto, dark) };
            let xor = &data[40..40 + BITS_BYTES];
            let mask = &data[40 + BITS_BYTES..];
            let mut bmp = Vec::new();
            bmp.extend(&40u32.to_le_bytes());
            bmp.extend(&(ICON_SIZE as u32).to_le_bytes());
            bmp.extend(&(ICON_SIZE as u32).to_le_bytes());
            bmp.extend(&1u16.to_le_bytes());
            bmp.extend(&24u16.to_le_bytes());
            bmp.extend(&0u32.to_le_bytes());
            bmp.extend(&((ICON_SIZE * ICON_SIZE * 3) as u32).to_le_bytes());
            bmp.extend(&0i32.to_le_bytes());
            bmp.extend(&0i32.to_le_bytes());
            bmp.extend(&0u32.to_le_bytes());
            bmp.extend(&0u32.to_le_bytes());
            // Both the icon bits and the BMP rows are stored bottom-up.
            for row in 0..ICON_SIZE {
                for col in 0..ICON_SIZE {
                    let index = (row as usize) * ICON_SIZE as usize + col as usize;
                    let opaque = mask[row as usize * 4 + col as usize / 8] & (1 << (7 - col as usize % 8)) != 0;
                    if opaque {
                        bmp.extend(&[xor[index * 4 + 2], xor[index * 4 + 1], xor[index * 4]]);
                    } else if dark {
                        bmp.extend(&[0x19, 0x15, 0x12]);
                    } else {
                        bmp.extend(&[0xF7, 0xF5, 0xF3]);
                    }
                }
            }
            std::fs::write(format!("data/check/{name}.bmp"), bmp)
                .expect("preview written");
        }
    }

    #[test]
    fn tray_glyph_spans_the_icon_width() {
        for label_ru in [true, false] {
            let data = unsafe { tray_icon_resource(label_ru, true, true) };
            let mask = &data[40 + BITS_BYTES..];
            let mut min_col = ICON_SIZE;
            let mut max_col = -1;
            for row in 0..ICON_SIZE {
                for col in 0..ICON_SIZE {
                    if mask[row as usize * 4 + (col as usize / 8)] & (1 << (7 - col as usize % 8)) != 0 {
                        if col < min_col {
                            min_col = col;
                        }
                        if col > max_col {
                            max_col = col;
                        }
                    }
                }
            }
            let width = max_col - min_col + 1;
            assert!(width >= 22, "glyph only {width} px wide in a 32 px icon");
        }
    }

    #[test]
    fn variant_indexes_are_distinct() {        assert_eq!(variant(true, true), 0);
        assert_eq!(variant(false, true), 1);
        assert_eq!(variant(true, false), 2);
        assert_eq!(variant(false, false), 3);
    }

    #[test]
    fn menu_margin_follows_the_display_scale() {
        assert_eq!(menu_margin(96), 4); // 100 %
        assert_eq!(menu_margin(120), 5); // 125 %
        assert_eq!(menu_margin(144), 6); // 150 %
        assert_eq!(menu_margin(192), 8); // 200 %
    }
}
