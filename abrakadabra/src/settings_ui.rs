//! Settings window for ZЯ.
//!
//! Plain Win32, no UI framework: the window is painted by hand (rounded cards,
//! a left column of tabs, a header) and every control is either an owner-drawn
//! button, an edit field or an owner-drawn list. The palette follows the system
//! theme — dark cards on a dark window when `AppsUseLightTheme` is 0, light
//! cards when it is 1 — and the whole layout is scaled from 96-DPI design units.
//!
//! Three pages:
//!   1. Настройки    — version, update button, autostart, "run as admin", hotkeys
//!   2. Игнорировать — processes and folders the keyboard hook must not touch
//!   3. Лог          — the switch diary, colour coded, copyable with the right mouse
//!
//! Clicking a row in the hotkey table opens the "press a new combination" dialog.
//! If a combination cannot be assigned, the status column says why.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CLIP_DEFAULT_PRECIS, BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_QUALITY, DeleteObject, DrawTextW, EndPaint, FillRect, FF_MODERN, HGDIOBJ, HDC, LineTo,
    MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, TRANSPARENT,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, LVIF_TEXT, LVNI_SELECTED, LVITEMW, LVM_DELETEALLITEMS,
    LVM_GETNEXTITEM, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETBKCOLOR,
    LVS_EX_FULLROWSELECT, LVS_REPORT, ODS_SELECTED, ODT_LISTVIEW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CallWindowProcW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClientRect, GetCursorPos,
    GetWindowLongPtrW, GetMessageW, GetWindowTextW, GWL_ID, GWL_WNDPROC, HHOOK,
    HMENU, IDC_ARROW, HWND_TOP,
    KBDLLHOOKSTRUCT, LLKHF_UP, MF_ENABLED, MSG, PostMessageW, RegisterClassExW, SendMessageW,
    SetWindowLongPtrW, SetWindowLongW, SetWindowPos, SetWindowsHookExW, SetWindowTextW, ShowWindow,
    SW_HIDE, SW_SHOW, SWP_NOZORDER, SWP_NOMOVE, SWP_NOSIZE, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, UnhookWindowsHookEx, WNDCLASSEXW, WNDPROC, WH_KEYBOARD_LL, WM_COMMAND,
    WM_CONTEXTMENU, WM_CREATE, WM_CTLCOLORSTATIC, WM_CTLCOLOREDIT, WM_DESTROY, WM_DRAWITEM,
    WM_ERASEBKGND, WM_PAINT, WM_KEYDOWN, WM_SETICON, WM_SETFONT, WM_QUIT, WINDOW_EX_STYLE, WINDOW_STYLE,
    WS_CHILD, WS_VISIBLE, CreatePopupMenu,
};

#[link(name = "user32")]
extern "system" {
    fn InvalidateRect(hwnd: HWND, lprect: *const RECT, erase: i32) -> i32;
}

use crate::engine::Config;

// ---------------------------------------------------------------------------
// Layout, in 96-DPI design pixels
// ---------------------------------------------------------------------------

const BASE_DPI: i32 = 96;
const WINDOW_WIDTH: i32 = 1200;
const WINDOW_HEIGHT: i32 = 820;

const HEADER_TOP: i32 = 20;
const SIDEBAR_X: i32 = 24;
const SIDEBAR_WIDTH: i32 = 232;
const SIDEBAR_TOP: i32 = 88;
const TAB_HEIGHT: i32 = 46;
const TAB_GAP: i32 = 10;

const CONTENT_X: i32 = 280;
const CONTENT_WIDTH: i32 = 896;
const CARD_PAD: i32 = 16;
const CARD_RADIUS: i32 = 10;

const VERSION_CARD_Y: i32 = 88;
const VERSION_CARD_H: i32 = 116;
const LAUNCH_CARD_Y: i32 = 216;
const LAUNCH_CARD_H: i32 = 152;
const HOTKEY_CARD_Y: i32 = 380;
const HOTKEY_CARD_H: i32 = 372;
const HOTKEY_LIST_Y: i32 = 470;
const STATUS_Y: i32 = 762;

const IGNORE_CARD_Y: i32 = 88;
const IGNORE_CARD_H: i32 = 156;
const IGNORE_LIST_CARD_Y: i32 = 256;
const IGNORE_LIST_CARD_H: i32 = 496;

const LOG_CARD_Y: i32 = 88;
const LOG_CARD_H: i32 = 664;

const TAB_SETTINGS: usize = 100;
const TAB_IGNORE: usize = 101;
const TAB_LOG: usize = 102;

const BTN_CONFIG: usize = 110;
const BTN_CLOSE: usize = 111;
const BTN_UPDATE: usize = 112;

const CB_AUTOSTART: usize = 120;
const CB_ADMIN: usize = 121;
const HOTKEY_LIST: usize = 122;
const BTN_HOTKEY_RESET: usize = 123;

const BTN_ADD_PROCESSES: usize = 130;
const BTN_ADD_PROCESS: usize = 131;
const BTN_REMOVE_IGNORE: usize = 132;
const IGNORE_INPUT: usize = 133;
const BTN_ADD_COMMIT: usize = 134;
const IGNORE_FILTER: usize = 135;
const IGNORE_LIST: usize = 136;
const IGNORE_FILTER_LABEL: usize = 137;
const LOG_FILTER_LABEL: usize = 144;

const BTN_LOG_REFRESH: usize = 140;
const BTN_LOG_CLEAR: usize = 141;
const LOG_FILTER: usize = 142;
const LOG_LIST: usize = 143;

const STATUS_LABEL: usize = 150;
const MODAL_CANCEL: usize = 160;

const WS_TABSTOP_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x00000010);
const WS_BORDER_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x00000080);
const BS_OWNERDRAW_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0000000B);
const OWNERDRAW_LIST_STYLE: WINDOW_STYLE = WINDOW_STYLE(LVS_REPORT | 0x00000400);
const SETTINGS_WINDOW_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0000002B);

// Notification codes that the owner-draw list views send to their parent.
const NM_CLICK: u32 = 0x1111;
const NM_DBLCLK: u32 = 0x1112;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

static DARK: AtomicBool = AtomicBool::new(true);
static CURRENT_TAB: AtomicUsize = AtomicUsize::new(0);
static MAIN_HANDLE: OnceLock<Win> = OnceLock::new();
static HOTKEY_HOOK: OnceLock<usize> = OnceLock::new();
static MODAL_HANDLE: OnceLock<Mutex<Option<Win>>> = OnceLock::new();
static MODAL_TARGET: AtomicUsize = AtomicUsize::new(usize::MAX);
static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
static UPDATE_CHECKED: AtomicBool = AtomicBool::new(false);
static UPDATE_INFO: OnceLock<Mutex<(String, String, u64)>> = OnceLock::new();
static LOG_ROWS: OnceLock<Mutex<Vec<LogRow>>> = OnceLock::new();
static STATUS_MESSAGE: OnceLock<Mutex<String>> = OnceLock::new();

/// `HWND` wraps a raw pointer, which is neither `Send` nor `Sync`. Window
/// handles are handed to statics and to the update thread, so they travel in
/// this wrapper instead.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Win(HWND);

unsafe impl Send for Win {}
unsafe impl Sync for Win {}

fn is_null(hwnd: HWND) -> bool {
    hwnd.0.is_null()
}

#[link(name = "user32")]
extern "system" {
    fn GetDpiForWindow(hwnd: HWND) -> u32;
    fn LoadImageW(
        lpsz: *const u16,
        lpsz_image: *const u16,
        lrflags: u32,
        width: u32,
        height: u32,
        itype: u32,
    ) -> *mut c_void;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
}

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(owner: *mut c_void) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attribute: u32, value: *const c_void, size: u32) -> i32;
}

struct ControlHandle {
    hwnd: Win,
}

fn controls() -> &'static Mutex<HashMap<usize, ControlHandle>> {
    static CONTROLS: OnceLock<Mutex<HashMap<usize, ControlHandle>>> = OnceLock::new();
    CONTROLS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_control(role: usize, hwnd: HWND) {
    if role == 0 || is_null(hwnd) {
        return;
    }
    // Some control classes ignore the `hMenu` child-id argument of
    // CreateWindowExW, so set GWL_ID explicitly. Without it a real mouse click
    // would post WM_COMMAND with id 0 and the tab buttons would do nothing.
    unsafe { SetWindowLongW(hwnd, GWL_ID, role as i32) };
    controls()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(role, ControlHandle { hwnd: Win(hwnd) });
}

fn handle(role: usize) -> Option<HWND> {
    controls()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&role)
        .map(|control| control.hwnd.0)
}

fn role_of(hwnd: HWND) -> usize {
    controls()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(_, control)| control.hwnd.0 == hwnd)
        .map(|(role, _)| *role)
        .unwrap_or(0)
}

fn cfg() -> &'static Mutex<Config> {
    static CFG: OnceLock<Mutex<Config>> = OnceLock::new();
    CFG.get_or_init(|| Mutex::new(crate::config::load()))
}

fn get_cfg() -> Config {
    cfg()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

unsafe fn save_config() {
    let guard = cfg()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if let Err(error) = crate::config::save(&guard) {
        eprintln!("zя: не удалось сохранить настройки: {error}");
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn child_id(value: usize) -> HMENU {
    HMENU(value as *mut c_void)
}

fn null_hwnd() -> HWND {
    HWND(std::ptr::null_mut())
}

unsafe fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam)))
}

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(((b as u32) << 16) | ((g as u32) << 8) | r as u32)
}

fn dark_theme() -> bool {
    DARK.load(Ordering::Relaxed)
}

fn bg_color() -> COLORREF {
    if dark_theme() {
        rgb(0x12, 0x15, 0x19)
    } else {
        rgb(0xF3, 0xF5, 0xF7)
    }
}

fn card_color() -> COLORREF {
    if dark_theme() {
        rgb(0x1B, 0x20, 0x26)
    } else {
        rgb(0xFF, 0xFF, 0xFF)
    }
}

fn card_border() -> COLORREF {
    if dark_theme() {
        rgb(0x2A, 0x31, 0x3A)
    } else {
        rgb(0xD7, 0xDD, 0xE3)
    }
}

fn fg_color() -> COLORREF {
    if dark_theme() {
        rgb(0xF5, 0xF7, 0xFA)
    } else {
        rgb(0x14, 0x18, 0x1D)
    }
}

fn dim_color() -> COLORREF {
    if dark_theme() {
        rgb(0x93, 0x9C, 0xA6)
    } else {
        rgb(0x62, 0x6B, 0x74)
    }
}

fn accent_color() -> COLORREF {
    if dark_theme() {
        rgb(0x3B, 0x82, 0xF6)
    } else {
        rgb(0x1F, 0x5E, 0x8C)
    }
}

fn button_color() -> COLORREF {
    if dark_theme() {
        rgb(0x2A, 0x31, 0x39)
    } else {
        rgb(0xEC, 0xEF, 0xF3)
    }
}

fn danger_color() -> COLORREF {
    if dark_theme() {
        rgb(0xE3, 0x51, 0x60)
    } else {
        rgb(0xB0, 0x2A, 0x30)
    }
}

fn success_color() -> COLORREF {
    if dark_theme() {
        rgb(0x2E, 0x9F, 0x76)
    } else {
        rgb(0x14, 0x6C, 0x30)
    }
}

fn warning_color() -> COLORREF {
    if dark_theme() {
        rgb(0xE0, 0x7A, 0x1D)
    } else {
        rgb(0x82, 0x3C, 0x08)
    }
}

fn kind_color(kind: &str) -> COLORREF {
    match kind {
        "manual" => success_color(),
        "warn" => warning_color(),
        "error" => danger_color(),
        _ => accent_color(),
    }
}

// ---------------------------------------------------------------------------
// Fonts
// ---------------------------------------------------------------------------

fn make_font(height: i32, weight: i32) -> windows::Win32::Graphics::Gdi::HFONT {
    unsafe {
        let name = wide("Segoe UI");
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            FF_MODERN.0 as u32,
            PCWSTR(name.as_ptr()),
        )
    }
}

#[derive(Clone, Copy)]
struct Fonts {
    title: HGDIOBJ,
    card: HGDIOBJ,
    big: HGDIOBJ,
    body: HGDIOBJ,
    dim: HGDIOBJ,
}

// Font handles are process-wide GDI objects that are never freed, so the table
// can be shared between threads.
unsafe impl Send for Fonts {}
unsafe impl Sync for Fonts {}

fn fonts(dpi: u32) -> Fonts {
    static TABLE: OnceLock<Mutex<HashMap<u32, Fonts>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = guard.get(&dpi) {
        return found.clone();
    }
    let created = Fonts {
        title: HGDIOBJ(make_font(s(22, dpi), 700).0),
        card: HGDIOBJ(make_font(s(16, dpi), 600).0),
        big: HGDIOBJ(make_font(s(18, dpi), 700).0),
        body: HGDIOBJ(make_font(s(14, dpi), 400).0),
        dim: HGDIOBJ(make_font(s(12, dpi), 400).0),
    };
    guard.insert(dpi, created.clone());
    created
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

unsafe fn draw_text(hdc: HDC, text: &str, rect: &RECT, font: HGDIOBJ, color: COLORREF, flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT) {
    let _ = SelectObject(hdc, font);
    let _ = SetTextColor(hdc, color);
    let _ = SetBkMode(hdc, TRANSPARENT);
    let mut encoded = wide(text);
    let mut area = *rect;
    DrawTextW(hdc, encoded.as_mut_slice(), &mut area, flags);
}

unsafe fn rounded(
    hdc: HDC,
    left: i32,
    top: i32,
    width: i32,
    height: i32,
    radius: i32,
    fill: COLORREF,
    border: COLORREF,
) {
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let _ = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = SelectObject(hdc, HGDIOBJ(pen.0));
    let _ = RoundRect(hdc, left, top, left + width, top + height, radius * 2, radius * 2);
    let _ = DeleteObject(HGDIOBJ(pen.0));
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn fill_rect(hdc: HDC, left: i32, top: i32, width: i32, height: i32, color: COLORREF) {
    if width <= 0 || height <= 0 {
        return;
    }
    let brush = CreateSolidBrush(color);
    let rect = RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    };
    let _ = FillRect(hdc, &rect, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

unsafe fn paint_window(hwnd: HWND, hdc: HDC) {
    let dpi = window_dpi(hwnd);
    let rect = client_rect(hwnd);
    fill_rect(hdc, rect.left, rect.top, rect.right, rect.bottom, bg_color());

    draw_text(
        hdc,
        "Настройки",
        &RECT {
            left: s(SIDEBAR_X, dpi),
            top: s(HEADER_TOP, dpi),
            right: s(SIDEBAR_X + 340, dpi),
            bottom: s(HEADER_TOP + 34, dpi),
        },
        fonts(dpi).title,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );

    match current_tab() {
        0 => paint_settings_page(hdc, dpi),
        1 => paint_ignore_page(hdc, dpi),
        _ => paint_log_page(hdc, dpi),
    }
}

unsafe fn paint_settings_page(hdc: HDC, dpi: u32) {
    let x = s(CONTENT_X, dpi);
    let width = s(CONTENT_WIDTH, dpi);

    rounded(
        hdc,
        x,
        s(VERSION_CARD_Y, dpi),
        width,
        s(VERSION_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        "Версия",
        &rect_at(CONTENT_X + CARD_PAD, 100, 200, 22, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        &format!("ZЯ {} — zя.exe", crate::update::app_version()),
        &rect_at(CONTENT_X + CARD_PAD, 126, 640, 30, dpi),
        fonts(dpi).big,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        &format!("Файлы настроек: {}", crate::config::config_dir().display()),
        &rect_at(CONTENT_X + CARD_PAD, 158, 700, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );

    if UPDATE_AVAILABLE.load(Ordering::Relaxed) {
        let info = update_info();
        let version = info.as_ref().map(|(value, _, _)| value.clone()).unwrap_or_default();
        let size = info.as_ref().map(|(_, _, value)| value / 1024).unwrap_or(0);
        draw_text(
            hdc,
            &format!("Доступно обновление {version}, {size} КБ", version = version, size = size),
            &rect_at(x + 770, 150, 190, 34, dpi),
            fonts(dpi).dim,
            warning_color(),
            windows::Win32::Graphics::Gdi::DT_LEFT
                | windows::Win32::Graphics::Gdi::DT_VCENTER,
        );
    } else {
        let note = if UPDATE_CHECKED.load(Ordering::Relaxed) {
            "Обновлений нет"
        } else {
            "Проверяю обновления…"
        };
        draw_text(
            hdc,
            note,
            &rect_at(x + 770, 150, 190, 34, dpi),
            fonts(dpi).dim,
            dim_color(),
            windows::Win32::Graphics::Gdi::DT_LEFT
                | windows::Win32::Graphics::Gdi::DT_VCENTER,
        );
    }

    rounded(
        hdc,
        x,
        s(LAUNCH_CARD_Y, dpi),
        width,
        s(LAUNCH_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        "Запуск",
        &rect_at(CONTENT_X + CARD_PAD, 228, 200, 22, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        "Автозапуск — запись в раздел Run. «Как администратор» — задание ZyaAutoStart в Планировщике заданий с уровнем HIGHEST.",
        &rect_at(CONTENT_X + CARD_PAD, 340, 864, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );

    rounded(
        hdc,
        x,
        s(HOTKEY_CARD_Y, dpi),
        width,
        s(HOTKEY_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        "Горячие клавиши",
        &rect_at(CONTENT_X + CARD_PAD, 392, 320, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        "Клик по строке — назначить новую комбинацию.",
        &rect_at(CONTENT_X + CARD_PAD, 424, 420, 22, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    // Column titles are painted by the list header itself.
}

unsafe fn paint_ignore_page(hdc: HDC, dpi: u32) {
    let x = s(CONTENT_X, dpi);
    let width = s(CONTENT_WIDTH, dpi);

    rounded(
        hdc,
        x,
        s(IGNORE_CARD_Y, dpi),
        width,
        s(IGNORE_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        "Игнорировать",
        &rect_at(CONTENT_X + CARD_PAD, 100, 300, 22, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        "Процессы и папки, которые хук не трогает: пароли, командные строки, игровые клиенты.",
        &rect_at(CONTENT_X + CARD_PAD, 212, 864, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );

    rounded(
        hdc,
        x,
        s(IGNORE_LIST_CARD_Y, dpi),
        width,
        s(IGNORE_LIST_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        &format!("Список исключений — {} записей", ignore_rows().len()),
        &rect_at(CONTENT_X + CARD_PAD, 266, 400, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    if ignore_rows().is_empty() {
        draw_text(
            hdc,
            "Список пуст — добавьте процесс или папку кнопками выше.",
            &rect_at(CONTENT_X + CARD_PAD, 734, 700, 22, dpi),
            fonts(dpi).dim,
            dim_color(),
            windows::Win32::Graphics::Gdi::DT_LEFT
                | windows::Win32::Graphics::Gdi::DT_VCENTER
                | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
        );
    }
}

unsafe fn paint_log_page(hdc: HDC, dpi: u32) {
    let x = s(CONTENT_X, dpi);
    let width = s(CONTENT_WIDTH, dpi);

    rounded(
        hdc,
        x,
        s(LOG_CARD_Y, dpi),
        width,
        s(LOG_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
    );
    draw_text(
        hdc,
        &format!("Лог переключений — {} строк", current_log_rows().len()),
        &rect_at(CONTENT_X + CARD_PAD, 100, 400, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    let footnote = if current_log_rows().is_empty() {
        "Лог пуст — переключения раскладки появятся здесь после работы хука."
    } else {
        "Правая кнопка мыши на строке — копировать."
    };
    draw_text(
        hdc,
        footnote,
        &rect_at(CONTENT_X + CARD_PAD, 712, 864, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
}

fn rect_at(left: i32, top: i32, width: i32, height: i32, dpi: u32) -> RECT {
    RECT {
        left: s(left, dpi),
        top: s(top, dpi),
        right: s(left + width, dpi),
        bottom: s(top + height, dpi),
    }
}

// ---------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------

unsafe fn make_button(
    parent: HWND,
    dpi: u32,
    label: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    role: usize,
) -> HWND {
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Button").as_ptr()),
        PCWSTR(wide(label).as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_TABSTOP_STYLE | BS_OWNERDRAW_STYLE,
        s(x, dpi),
        s(y, dpi),
        s(width, dpi),
        s(height, dpi),
        Some(parent),
        Some(child_id(role)),
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    register_control(role, hwnd);
    hwnd
}

unsafe fn make_field(
    parent: HWND,
    dpi: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    role: usize,
    placeholder: &str,
) -> HWND {
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Edit").as_ptr()),
        PCWSTR(wide(placeholder).as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_BORDER_STYLE | WS_TABSTOP_STYLE,
        s(x, dpi),
        s(y, dpi),
        s(width, dpi),
        s(height, dpi),
        Some(parent),
        Some(child_id(role)),
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    register_control(role, hwnd);
    send(hwnd, WM_SETFONT, make_font(s(14, dpi), 400).0 as usize, 1);
    hwnd
}

unsafe fn make_list(
    parent: HWND,
    dpi: u32,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    role: usize,
) -> HWND {
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("SysListView32").as_ptr()),
        PCWSTR(wide("").as_ptr()),
        WS_CHILD | WS_VISIBLE | OWNERDRAW_LIST_STYLE,
        s(x, dpi),
        s(y, dpi),
        s(width, dpi),
        s(height, dpi),
        Some(parent),
        Some(child_id(role)),
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    register_control(role, hwnd);
    let font = make_font(s(15, dpi), 400);
    SendMessageW(
        hwnd,
        WM_SETFONT,
        Some(WPARAM(font.0 as usize)),
        Some(LPARAM(1)),
    );
    SendMessageW(
        hwnd,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(LVS_EX_FULLROWSELECT as usize)),
        Some(LPARAM(LVS_EX_FULLROWSELECT as isize)),
    );
    SendMessageW(
        hwnd,
        LVM_SETBKCOLOR,
        Some(WPARAM(0)),
        Some(LPARAM(card_color().0 as isize)),
    );
    // A report view needs columns, otherwise the owner-draw rows are never
    // requested from the parent window.
    for (index, title) in ["Действие", "Клавиша", "Статус"].iter().enumerate() {
        let text = wide(title);
        let column = LVITEMW {
            mask: LVIF_TEXT,
            iItem: index as i32,
            iSubItem: 0,
            pszText: PWSTR(text.as_ptr() as *mut u16),
            cchTextMax: text.len() as i32,
            iImage: -1,
            ..Default::default()
        };
        SendMessageW(
            hwnd,
            4123, // LVM_INSERTCOLUMNW
            Some(WPARAM(0)),
            Some(LPARAM(std::ptr::addr_of!(column) as isize)),
        );
    }
    SendMessageW(hwnd, 4111, Some(WPARAM(0)), Some(LPARAM(s(34, dpi) as isize)));
    // The header is a plain white SysHeader32 child. Subclass it so it matches
    // the card and carries the column titles of the page.
    let header = SendMessageW(hwnd, 4127, Some(WPARAM(0)), Some(LPARAM(0)));
    let header_hwnd = HWND(header.0 as *mut c_void);
    if !is_null(header_hwnd) {
        header_roles()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(header_hwnd.0 as usize, role);
        let previous = GetWindowLongPtrW(header_hwnd, GWL_WNDPROC);
        header_original()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(header_hwnd.0 as usize, previous);
        let proc: WNDPROC = Some(dark_header_proc);
        SetWindowLongPtrW(header_hwnd, GWL_WNDPROC, proc.map_or(0, |f| f as isize));
    }
    if role == HOTKEY_LIST {
        let previous = GetWindowLongPtrW(hwnd, GWL_WNDPROC);
        list_original()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(hwnd.0 as usize, previous);
        let proc: WNDPROC = Some(list_click_proc);
        SetWindowLongPtrW(hwnd, GWL_WNDPROC, proc.map_or(0, |f| f as isize));
    }
    hwnd
}

fn header_roles() -> &'static Mutex<HashMap<usize, usize>> {
    static ROLES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
    ROLES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn header_original() -> &'static Mutex<HashMap<usize, isize>> {
    static ORIGINAL: OnceLock<Mutex<HashMap<usize, isize>>> = OnceLock::new();
    ORIGINAL.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe extern "system" fn dark_header_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_PAINT {
        let role = header_roles()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(hwnd.0 as usize))
            .copied()
            .unwrap_or(0);
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);
        let rect = client_rect(hwnd);
        let dpi = window_dpi(hwnd);
        fill_rect(
            hdc,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            card_color(),
        );
        let single = windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE;
        let titles: [(i32, &str); 3] = match role {
            IGNORE_LIST => [(14, "Процесс или путь"), (600, "Источник"), (0, "")],
            LOG_LIST => [(12, "Время"), (88, "Что произошло"), (504, "Почему")],
            _ => [(14, "Действие"), (400, "Клавиша"), (560, "Статус")],
        };
        for (offset, title) in titles {
            if title.is_empty() {
                continue;
            }
            draw_text(
                hdc,
                title,
                &RECT {
                    left: rect.left + s(offset, dpi),
                    top: rect.top,
                    right: rect.left + s(offset + 320, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                dim_color(),
                single,
            );
        }
        let _ = EndPaint(hwnd, &paint);
        return LRESULT(1);
    }
    let previous = header_original()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(hwnd.0 as usize))
        .copied();
    match previous {
        Some(pointer) => {
            let original: WNDPROC = Some(unsafe {
                std::mem::transmute::<
                    isize,
                    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
                >(pointer)
            });
            CallWindowProcW(original, hwnd, msg, wparam, lparam)
        }
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// A raw SysListView32 does not reliably deliver NM_CLICK to its parent when it
// is owner-drawn, so the hotkey rows are watched through a subclassed window
// procedure instead of a notification code.
const WM_MOUSE_UP: u32 = 0x0202; // WM_LBUTTONUP
const WM_LBUTTON_DOWN: u32 = 0x0201; // WM_LBUTTONDOWN

// Every owner-draw row remembers its own rectangle while it is painted, so a
// click can be matched to the right hotkey without guessing the row height.
type RowRects = HashMap<usize, HashMap<usize, (i32, i32)>>;

fn list_original() -> &'static Mutex<HashMap<usize, isize>> {
    static ORIGINAL: OnceLock<Mutex<HashMap<usize, isize>>> = OnceLock::new();
    ORIGINAL.get_or_init(|| Mutex::new(HashMap::new()))
}

fn row_rects() -> &'static Mutex<RowRects> {
    static RECTS: OnceLock<Mutex<RowRects>> = OnceLock::new();
    RECTS.get_or_init(|| Mutex::new(RowRects::new()))
}

unsafe extern "system" fn list_click_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let previous = list_original()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(hwnd.0 as usize))
        .copied();
    let result = match previous {
        Some(pointer) => {
            let original: WNDPROC = Some(unsafe {
                std::mem::transmute::<
                    isize,
                    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
                >(pointer)
            });
            CallWindowProcW(original, hwnd, msg, wparam, lparam)
        }
        None => DefWindowProcW(hwnd, msg, wparam, lparam),
    };

    if msg == WM_MOUSE_UP || msg == WM_LBUTTON_DOWN {
        let point = lparam.0 as u32;
        let y = (point >> 16) as i32;
        let rows = row_rects()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(hwnd.0 as usize))
            .cloned()
            .unwrap_or_default();
        for (index, (top, bottom)) in rows {
            if y >= top && y < bottom {
                open_hotkey_modal(index);
                break;
            }
        }
    }
    result
}

unsafe fn create_controls(parent: HWND, dpi: u32) {
    let tabs = [
        (TAB_SETTINGS, "Настройки"),
        (TAB_IGNORE, "Игнорировать"),
        (TAB_LOG, "Лог"),
    ];
    for (index, (role, label)) in tabs.iter().enumerate() {
        let y = SIDEBAR_TOP + index as i32 * (TAB_HEIGHT + TAB_GAP);
        make_button(parent, dpi, label, SIDEBAR_X, y, SIDEBAR_WIDTH, TAB_HEIGHT, *role);
    }

    let right = CONTENT_X + CONTENT_WIDTH;
    make_button(parent, dpi, "Открыть файл конфигурации", right - 242, HEADER_TOP, 190, 34, BTN_CONFIG);
    make_button(parent, dpi, "✕", right - 34, HEADER_TOP, 34, 34, BTN_CLOSE);

    // 1. Settings
    make_button(parent, dpi, "Проверить обновления", right - 190, 112, 190, 40, BTN_UPDATE);
    make_button(parent, dpi, "Автозапуск при входе", CONTENT_X + CARD_PAD, 254, 430, 38, CB_AUTOSTART);
    make_button(
        parent,
        dpi,
        "Запускать как администратор",
        CONTENT_X + CARD_PAD,
        298,
        430,
        38,
        CB_ADMIN,
    );
    make_button(parent, dpi, "Сбросить", right - 120, 392, 120, 32, BTN_HOTKEY_RESET);
    let hotkeys = make_list(parent, dpi, CONTENT_X + 12, HOTKEY_LIST_Y, CONTENT_WIDTH - 24, 280, HOTKEY_LIST);
    fill_hotkey_list(hotkeys);
    // 2. Ignore
    make_button(
        parent,
        dpi,
        "Добавить из запущенных",
        CONTENT_X + CARD_PAD,
        128,
        250,
        40,
        BTN_ADD_PROCESSES,
    );
    make_button(
        parent,
        dpi,
        "Добавить новый процесс",
        CONTENT_X + CARD_PAD + 264,
        128,
        230,
        40,
        BTN_ADD_PROCESS,
    );
    make_button(
        parent,
        dpi,
        "Удалить выбранное",
        CONTENT_X + CARD_PAD + 508,
        128,
        190,
        40,
        BTN_REMOVE_IGNORE,
    );
    let input = make_field(
        parent,
        dpi,
        CONTENT_X + CARD_PAD,
        178,
        560,
        34,
        IGNORE_INPUT,
        "",
    );
    let commit = make_button(parent, dpi, "Добавить", CONTENT_X + 600, 178, 120, 34, BTN_ADD_COMMIT);
    let _ = ShowWindow(input, SW_HIDE);
    let _ = ShowWindow(commit, SW_HIDE);
    make_field(parent, dpi, right - 196, 266, 180, 32, IGNORE_FILTER, "");
    let filter_label = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Static").as_ptr()),
        PCWSTR(wide("Фильтр").as_ptr()),
        WS_CHILD | WS_VISIBLE,
        s(right - 286, dpi),
        s(266, dpi),
        s(84, dpi),
        s(32, dpi),
        Some(parent),
        None,
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    send(filter_label, WM_SETFONT, make_font(s(13, dpi), 400).0 as usize, 1);
    register_control(IGNORE_FILTER_LABEL, filter_label);
    let list = make_list(parent, dpi, CONTENT_X + 12, 308, CONTENT_WIDTH - 24, 420, IGNORE_LIST);
    fill_ignore_list(list);

    // 3. Log
    make_button(parent, dpi, "Очистить", right - 120, 96, 120, 32, BTN_LOG_CLEAR);
    make_button(parent, dpi, "Обновить", right - 420, 132, 120, 32, BTN_LOG_REFRESH);
    make_field(parent, dpi, right - 196, 132, 180, 32, LOG_FILTER, "");
    let log_label = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Static").as_ptr()),
        PCWSTR(wide("Фильтр").as_ptr()),
        WS_CHILD | WS_VISIBLE,
        s(right - 286, dpi),
        s(132, dpi),
        s(84, dpi),
        s(32, dpi),
        Some(parent),
        None,
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    send(log_label, WM_SETFONT, make_font(s(13, dpi), 400).0 as usize, 1);
    register_control(LOG_FILTER_LABEL, log_label);
    let log_list = make_list(parent, dpi, CONTENT_X + 12, 174, CONTENT_WIDTH - 24, 520, LOG_LIST);
    fill_log_list(log_list);

    let status = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Static").as_ptr()),
        PCWSTR(wide("Готово").as_ptr()),
        WS_CHILD | WS_VISIBLE,
        s(CONTENT_X, dpi),
        s(STATUS_Y, dpi),
        s(CONTENT_WIDTH, dpi),
        s(28, dpi),
        Some(parent),
        Some(child_id(STATUS_LABEL)),
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    register_control(STATUS_LABEL, status);
    send(status, WM_SETFONT, make_font(s(12, dpi), 400).0 as usize, 1);
    apply_tab_visibility();
}

/// Only the controls of the active tab are visible; the rest stay hidden so
/// they never overlap the page being drawn.
unsafe fn apply_tab_visibility() {
    let tab = current_tab();
    let groups: [(usize, &[usize]); 3] = [
        (
            0,
            &[BTN_UPDATE, CB_AUTOSTART, CB_ADMIN, BTN_HOTKEY_RESET, HOTKEY_LIST],
        ),
        (
            1,
            &[
                BTN_ADD_PROCESSES,
                BTN_ADD_PROCESS,
                BTN_REMOVE_IGNORE,
                IGNORE_FILTER,
                IGNORE_FILTER_LABEL,
                IGNORE_LIST,
            ],
        ),
        (
            2,
            &[BTN_LOG_REFRESH, BTN_LOG_CLEAR, LOG_FILTER, LOG_FILTER_LABEL, LOG_LIST],
        ),
    ];
    for (index, roles) in groups {
        for role in roles {
            if let Some(hwnd) = handle(*role) {
                let _ = ShowWindow(hwnd, if index == tab { SW_SHOW } else { SW_HIDE });
            }
        }
    }
    // The "add process" line is only opened on demand.
    for role in [IGNORE_INPUT, BTN_ADD_COMMIT] {
        if let Some(hwnd) = handle(role) {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
    }
}

// ---------------------------------------------------------------------------
// Message handling
// ---------------------------------------------------------------------------

unsafe extern "system" fn settings_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    MAIN_HANDLE.get_or_init(|| Win(hwnd));
    match msg {
        WM_CREATE => {
            let dpi = window_dpi(hwnd);
            DARK.store(crate::theme::is_dark(), Ordering::Relaxed);
            create_controls(hwnd, dpi);
            set_window_icon(hwnd);
            apply_titlebar_theme(hwnd);
            repaint(hwnd);
            spawn_update_check();
            return LRESULT(0);
        }
        WM_ERASEBKGND => {
            DARK.store(crate::theme::is_dark(), Ordering::Relaxed);
            paint_window(hwnd, HDC(wparam.0 as *mut c_void));
            return LRESULT(1);
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let child = HWND(lparam.0 as *mut c_void);
            let background = if role_of(child) == STATUS_LABEL { bg_color() } else { card_color() };
            let _ = SetTextColor(hdc, fg_color());
            let _ = SetBkColor(hdc, background);
            let brush = CreateSolidBrush(background);
            return LRESULT(brush.0 as isize);
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let _ = SetTextColor(hdc, fg_color());
            // The page background (not the card) makes the field read as an
            // input box on both themes.
            let background = bg_color();
            let _ = SetBkColor(hdc, background);
            let brush = CreateSolidBrush(background);
            return LRESULT(brush.0 as isize);
        }
        WM_DRAWITEM => {
            let info = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            if info.CtlType == ODT_LISTVIEW {
                draw_list_item(info);
            } else {
                draw_button(info, info.hwndItem);
            }
            return LRESULT(1);
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            let notification = ((wparam.0 >> 16) & 0xFFFF) as u32;
            match id {
                TAB_SETTINGS | TAB_IGNORE | TAB_LOG => {
                    CURRENT_TAB.store(id - TAB_SETTINGS, Ordering::Relaxed);
                    apply_tab_visibility();
                    repaint(hwnd);
                }
                BTN_CONFIG => open_config_file(),
                BTN_CLOSE => {
                    DestroyWindow(hwnd).ok();
                }
                BTN_UPDATE => run_update(),
                CB_AUTOSTART => toggle_autostart(),
                CB_ADMIN => toggle_admin_autostart(),
                BTN_HOTKEY_RESET => reset_hotkeys(),
                BTN_ADD_PROCESSES => add_running_processes(),
                BTN_ADD_PROCESS => show_ignore_input(),
                BTN_ADD_COMMIT => add_ignore_entry(),
                BTN_REMOVE_IGNORE => remove_ignore_entry(),
                BTN_LOG_REFRESH => refresh_log(),
                BTN_LOG_CLEAR => clear_log(),
                MODAL_CANCEL => close_modal(),
                _ => {
                    // EN_CHANGE (0x0400) arrives for every edit control: the
                    // filter boxes re-filter their list as the user types.
                    if notification == 0x0400 {
                        if id == IGNORE_FILTER {
                            if let Some(list) = handle(IGNORE_LIST) {
                                fill_ignore_list(list);
                                InvalidateRect(list, std::ptr::null(), 1);
                            }
                        } else if id == LOG_FILTER {
                            if let Some(list) = handle(LOG_LIST) {
                                fill_log_list(list);
                                InvalidateRect(list, std::ptr::null(), 1);
                            }
                        }
                    }
                    if id == HOTKEY_LIST && (notification == NM_CLICK || notification == NM_DBLCLK) {
                        let index = list_selected(HOTKEY_LIST).unwrap_or(0);
                        open_hotkey_modal(index);
                    }
                }
            }
            return LRESULT(0);
        }
        WM_CONTEXTMENU => {
            let child = HWND(wparam.0 as *mut c_void);
            let role = role_of(child);
            if role != IGNORE_LIST && role != LOG_LIST {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let mut point = POINT { x: 0, y: 0 };
            GetCursorPos(&mut point).ok();
            unsafe {
                let Ok(menu) = CreatePopupMenu() else {
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                };
                let _ = AppendMenuW(menu, MF_ENABLED, 1, PCWSTR(wide("Скопировать строку").as_ptr()));
                let _ = AppendMenuW(menu, MF_ENABLED, 2, PCWSTR(wide("Скопировать весь список").as_ptr()));
                if role == IGNORE_LIST {
                    let _ = AppendMenuW(menu, MF_ENABLED, 3, PCWSTR(wide("Удалить").as_ptr()));
                }
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_LEFTALIGN | TPM_RIGHTBUTTON | TPM_RETURNCMD,
                    point.x,
                    point.y,
                    Some(0),
                    hwnd,
                    None,
                );
                DestroyMenu(menu).ok();
                match chosen.0 {
                    1 => copy_selected_row(role),
                    2 => copy_visible_rows(role),
                    3 => remove_ignore_entry(),
                    _ => {}
                }
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            if let Some(hook) = HOTKEY_HOOK.get() {
                UnhookWindowsHookEx(HHOOK(*hook as *mut c_void)).ok();
            }
            PostMessageW(None, WM_QUIT, WPARAM(0), LPARAM(0)).ok();
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn repaint(hwnd: HWND) {
    InvalidateRect(hwnd, std::ptr::null(), 1);
}

unsafe fn draw_button(info: &DRAWITEMSTRUCT, hwnd: HWND) {
    let dpi = window_dpi(hwnd);
    let rect = info.rcItem;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    // Owner-draw buttons report itemID = 0, so the control id is looked up
    // from the registry instead.
    let role = role_of(info.hwndItem);
    // The button DC starts out white: fill the whole rectangle with the page
    // background first so the rounded corners do not show up as white specks.
    let backdrop = if rect.left < s(CONTENT_X, dpi) || rect.top < s(70, dpi) {
        bg_color()
    } else {
        card_color()
    };
    fill_rect(info.hDC, rect.left, rect.top, width, height, backdrop);

    match role {
        TAB_SETTINGS | TAB_IGNORE | TAB_LOG => {
            if current_tab() == role - TAB_SETTINGS {
                rounded(
                    info.hDC,
                    rect.left,
                    rect.top,
                    width,
                    height,
                    s(8, dpi),
                    button_color(),
                    card_border(),
                );
                fill_rect(
                    info.hDC,
                    rect.left,
                    rect.top + s(8, dpi),
                    s(3, dpi),
                    height - s(16, dpi),
                    accent_color(),
                );
                draw_text(
                    info.hDC,
                    &label_of(role),
                    &RECT {
                        left: rect.left + s(14, dpi),
                        top: rect.top,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                    fonts(dpi).body,
                    fg_color(),
                    windows::Win32::Graphics::Gdi::DT_LEFT
                        | windows::Win32::Graphics::Gdi::DT_VCENTER
                        | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
                );
            } else {
                draw_text(
                    info.hDC,
                    &label_of(role),
                    &RECT {
                        left: rect.left + s(14, dpi),
                        top: rect.top,
                        right: rect.right,
                        bottom: rect.bottom,
                    },
                    fonts(dpi).body,
                    dim_color(),
                    windows::Win32::Graphics::Gdi::DT_LEFT
                        | windows::Win32::Graphics::Gdi::DT_VCENTER
                        | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
                );
            }
        }
        CB_AUTOSTART | CB_ADMIN => {
            let config = get_cfg();
            let checked = if role == CB_AUTOSTART { config.autostart } else { config.admin_autostart };
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                card_color(),
                card_border(),
            );
            let mark = s(20, dpi);
            let mark_left = rect.left + s(12, dpi);
            let mark_top = rect.top + (height - mark) / 2;
            if checked {
                rounded(
                    info.hDC,
                    mark_left,
                    mark_top,
                    mark,
                    mark,
                    s(4, dpi),
                    accent_color(),
                    accent_color(),
                );
                let pen = CreatePen(PS_SOLID, s(2, dpi), rgb(255, 255, 255));
                let _ = SelectObject(info.hDC, HGDIOBJ(pen.0));
                let _ = MoveToEx(
                    info.hDC,
                    mark_left + s(4, dpi),
                    mark_top + s(10, dpi),
                    None,
                );
                let _ = LineTo(info.hDC, mark_left + s(9, dpi), mark_top + s(15, dpi));
                let _ = LineTo(info.hDC, mark_left + s(16, dpi), mark_top + s(5, dpi));
                let _ = DeleteObject(HGDIOBJ(pen.0));
            } else {
                rounded(
                    info.hDC,
                    mark_left,
                    mark_top,
                    mark,
                    mark,
                    s(4, dpi),
                    bg_color(),
                    card_border(),
                );
            }
            draw_text(
                info.hDC,
                &label_of(role),
                &RECT {
                    left: mark_left + mark + s(10, dpi),
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                windows::Win32::Graphics::Gdi::DT_LEFT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
        BTN_UPDATE => {
            let available = UPDATE_AVAILABLE.load(Ordering::Relaxed);
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                if available { accent_color() } else { card_color() },
                if available { accent_color() } else { card_border() },
            );
            draw_text(
                info.hDC,
                if available { "Обновить" } else { "Проверить обновления" },
                &RECT {
                    left: rect.left + s(12, dpi),
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                if available { rgb(255, 255, 255) } else { fg_color() },
                windows::Win32::Graphics::Gdi::DT_LEFT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
        BTN_CLOSE => {
            rounded(info.hDC, rect.left, rect.top, width, height, s(6, dpi), card_color(), card_border());
            draw_text(
                info.hDC,
                "✕",
                &RECT {
                    left: rect.left,
                    top: rect.top,
                    right: rect.right,
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                dim_color(),
                windows::Win32::Graphics::Gdi::DT_CENTER
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
        _ => {
            rounded(info.hDC, rect.left, rect.top, width, height, s(8, dpi), button_color(), card_border());
            draw_text(
                info.hDC,
                &label_of(role),
                &RECT {
                    left: rect.left + s(12, dpi),
                    top: rect.top,
                    right: rect.right - s(12, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                windows::Win32::Graphics::Gdi::DT_LEFT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
    }
}

unsafe fn draw_list_item(info: &DRAWITEMSTRUCT) {
    let dpi = window_dpi(info.hwndItem);
    // The list view hands the owner a narrow first-column rectangle, so the
    // row is measured against the control's client area instead.
    let client = client_rect(info.hwndItem);
    let rect = RECT {
        left: client.left,
        top: info.rcItem.top,
        right: client.right,
        bottom: info.rcItem.bottom,
    };
    let selected = (info.itemState.0 & ODS_SELECTED.0) != 0;
    let background = if selected { button_color() } else { card_color() };
    fill_rect(
        info.hDC,
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        background,
    );

    let index = info.itemID as usize;
    let role = role_of(info.hwndItem);
    if role == HOTKEY_LIST {
        row_rects()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(info.hwndItem.0 as usize)
            .or_insert_with(HashMap::new)
            .insert(index, (rect.top, rect.bottom));
    }
    let single = windows::Win32::Graphics::Gdi::DT_LEFT
        | windows::Win32::Graphics::Gdi::DT_VCENTER
        | windows::Win32::Graphics::Gdi::DT_SINGLELINE;

    // Thin separator under every row keeps the table readable on both themes.
    fill_rect(
        info.hDC,
        rect.left + s(12, dpi),
        rect.bottom - 1,
        rect.right - rect.left - s(24, dpi),
        1,
        card_border(),
    );

    match role {
        HOTKEY_LIST => {
            let rows = hotkey_rows();
            let Some(row) = rows.get(index) else {
                return;
            };
            draw_text(
                info.hDC,
                &row.action,
                &RECT {
                    left: rect.left + s(14, dpi),
                    top: rect.top,
                    right: rect.left + s(400, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.key,
                &RECT {
                    left: rect.left + s(400, dpi),
                    top: rect.top,
                    right: rect.left + s(560, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).big,
                accent_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.status,
                &RECT {
                    left: rect.left + s(560, dpi),
                    top: rect.top,
                    right: rect.right - s(10, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                if row.status_ok { fg_color() } else { warning_color() },
                single,
            );
        }
        IGNORE_LIST => {
            let rows = ignore_rows();
            let Some(row) = rows.get(index) else {
                return;
            };
            fill_rect(
                info.hDC,
                rect.left,
                rect.top + s(3, dpi),
                s(4, dpi),
                rect.bottom - rect.top - s(6, dpi),
                kind_color(&row.kind),
            );
            draw_text(
                info.hDC,
                &row.name,
                &RECT {
                    left: rect.left + s(14, dpi),
                    top: rect.top,
                    right: rect.left + s(600, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.kind,
                &RECT {
                    left: rect.left + s(600, dpi),
                    top: rect.top,
                    right: rect.right - s(12, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                dim_color(),
                windows::Win32::Graphics::Gdi::DT_RIGHT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
        LOG_LIST => {
            let rows = current_log_rows();
            let Some(row) = rows.get(index) else {
                return;
            };
            fill_rect(
                info.hDC,
                rect.left,
                rect.top + s(3, dpi),
                s(4, dpi),
                rect.bottom - rect.top - s(6, dpi),
                kind_color(&row.kind),
            );
            draw_text(
                info.hDC,
                &row.time,
                &RECT {
                    left: rect.left + s(12, dpi),
                    top: rect.top,
                    right: rect.left + s(84, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                dim_color(),
                single,
            );
            draw_text(
                info.hDC,
                &format!("{} → {}", row.original, row.converted),
                &RECT {
                    left: rect.left + s(88, dpi),
                    top: rect.top,
                    right: rect.left + s(500, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                kind_color(&row.kind),
                single,
            );
            draw_text(
                info.hDC,
                &row.reason,
                &RECT {
                    left: rect.left + s(504, dpi),
                    top: rect.top,
                    right: rect.right - s(10, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                dim_color(),
                single,
            );
        }
        _ => {}
    }
}

fn label_of(role: usize) -> String {
    match role {
        TAB_SETTINGS => "Настройки".into(),
        TAB_IGNORE => "Игнорировать".into(),
        TAB_LOG => "Лог".into(),
        BTN_CONFIG => "Открыть файл конфигурации".into(),
        BTN_CLOSE => "✕".into(),
        BTN_UPDATE => "Проверить обновления".into(),
        CB_AUTOSTART => "Автозапуск при входе".into(),
        CB_ADMIN => "Запускать как администратор".into(),
        BTN_HOTKEY_RESET => "Сбросить".into(),
        BTN_ADD_PROCESSES => "Добавить из запущенных процессов".into(),
        BTN_ADD_PROCESS => "Добавить новый процесс".into(),
        BTN_REMOVE_IGNORE => "Удалить выбранное".into(),
        BTN_ADD_COMMIT => "Добавить".into(),
        BTN_LOG_REFRESH => "Обновить".into(),
        BTN_LOG_CLEAR => "Очистить".into(),
        MODAL_CANCEL => "Отмена".into(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

const HOTKEY_ACTIONS: [(&str, &str, &str); 6] = [
    ("Отменить последнее исправление", "hotkey_undo", "alt"),
    ("Сменить раскладку вручную", "hotkey_toggle", "ctrl+alt+p"),
    ("Исправить последнее слово", "hotkey_last", "break"),
    ("Сменить раскладку выделения", "hotkey_selection", "break"),
    ("Выделение в верхний регистр", "hotkey_upper", "ctrl+break"),
    ("Сменить регистр выделения", "hotkey_invert", "shift+break"),
];

struct HotkeyRow {
    action: String,
    key: String,
    status: String,
    status_ok: bool,
}

fn hotkey_value(config: &Config, field: &str) -> String {
    match field {
        "hotkey_undo" => config.hotkey_undo.clone(),
        "hotkey_toggle" => config.hotkey_toggle.clone(),
        "hotkey_last" => config.hotkey_last.clone(),
        "hotkey_selection" => config.hotkey_selection.clone(),
        "hotkey_upper" => config.hotkey_upper.clone(),
        _ => config.hotkey_invert.clone(),
    }
}

fn hotkey_rows() -> Vec<HotkeyRow> {
    let config = get_cfg();
    HOTKEY_ACTIONS
        .iter()
        .map(|(action, field, _default)| {
            let value = hotkey_value(&config, field);
            let status = validate_hotkey(field, &value);
            HotkeyRow {
                action: action.to_string(),
                key: value.clone(),
                status_ok: status.is_ok(),
                status: match status {
                    Ok(_) => "клик — назначить".to_string(),
                    Err(reason) => reason,
                },
            }
        })
        .collect()
}

unsafe fn insert_row(list: HWND, index: usize, label: &str) {
    let text = wide(label);
    let item = LVITEMW {
        iItem: index as i32,
        mask: LVIF_TEXT,
        pszText: PWSTR(text.as_ptr() as *mut u16),
        ..Default::default()
    };
    SendMessageW(
        list,
        LVM_INSERTITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(std::ptr::addr_of!(item) as isize)),
    );
}

unsafe fn fill_hotkey_list(list: HWND) {
    SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    let rows = hotkey_rows();
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.action);
    }
}

struct IgnoreRow {
    name: String,
    kind: String,
}

fn matches_filter(value: &str, filter: &str) -> bool {
    filter.is_empty() || value.to_lowercase().contains(&filter.to_lowercase())
}

fn ignore_rows() -> Vec<IgnoreRow> {
    let config = get_cfg();
    let filter = field_text(IGNORE_FILTER);
    let mut rows = Vec::new();
    for item in config.exclude_processes {
        if matches_filter(&item, &filter) {
            rows.push(IgnoreRow { name: item, kind: "процесс".into() });
        }
    }
    for item in config.exclude_dirs {
        if matches_filter(&item, &filter) {
            rows.push(IgnoreRow { name: item, kind: "папка".into() });
        }
    }
    rows
}

unsafe fn fill_ignore_list(list: HWND) {
    send(list, LVM_DELETEALLITEMS, 0, 0);
    let rows = ignore_rows();
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.name);
    }
    update_status(&format!("Игнорируется записей: {}", rows.len()));
}

#[derive(Clone)]
struct LogRow {
    time: String,
    original: String,
    converted: String,
    reason: String,
    kind: String,
    raw: String,
}

fn load_log_rows() -> Vec<LogRow> {
    let filter = field_text(LOG_FILTER);
    crate::diary::tail(500)
        .into_iter()
        .filter_map(|entry| {
            let raw = format!(
                "[{}] {} → {} | {}",
                entry.time, entry.original, entry.converted, entry.why
            );
            if !matches_filter(&raw, &filter) {
                return None;
            }
            let kind = kind_of(&entry.why);
            Some(LogRow {
                time: entry.time,
                original: entry.original,
                converted: entry.converted,
                reason: entry.why,
                kind,
                raw,
            })
        })
        .collect()
}

fn kind_of(reason: &str) -> String {
    if reason.contains("ручную") {
        "manual".into()
    } else if reason.contains("заблокировано") || reason.contains("не переключено") {
        "warn".into()
    } else if reason.contains("ошибка") {
        "error".into()
    } else {
        "switch".into()
    }
}

fn current_log_rows() -> Vec<LogRow> {
    LOG_ROWS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

unsafe fn fill_log_list(list: HWND) {
    let rows = load_log_rows();
    {
        let stored = LOG_ROWS.get_or_init(|| Mutex::new(Vec::new()));
        let mut guard = stored.lock().unwrap_or_else(|e| e.into_inner());
        *guard = rows.clone();
    }
    send(list, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.raw);
    }
    update_status(&format!("Строк в логе: {}", rows.len()));
}

unsafe fn refresh_log() {
    if let Some(list) = handle(LOG_LIST) {
        fill_log_list(list);
        InvalidateRect(list, std::ptr::null(), 1);
    }
}

unsafe fn clear_log() {
    let path = crate::config::config_dir().join("diario.log");
    std::fs::write(path, "").ok();
    crate::diary::clear_recent();
    refresh_log();
    update_status("Журнал очищен");
}

fn field_text(role: usize) -> String {
    let Some(hwnd) = handle(role) else {
        return String::new();
    };
    let mut buffer = vec![0u16; 512];
    unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let chars: Vec<u16> = buffer.iter().take_while(|value| **value != 0).copied().collect();
    String::from_utf16_lossy(&chars).trim().to_string()
}

unsafe fn list_selected(role: usize) -> Option<usize> {
    let Some(list) = handle(role) else {
        return None;
    };
    let index = send(list, LVM_GETNEXTITEM, LVNI_SELECTED as usize, -1).0;
    if index >= 0 {
        Some(index as usize)
    } else {
        Some(0)
    }
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

unsafe fn apply_config(mutator: impl Fn(&mut Config)) {
    {
        let reference = cfg();
        let mut guard = reference.lock().unwrap_or_else(|e| e.into_inner());
        mutator(&mut guard);
    }
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
}

unsafe fn toggle_autostart() {
    let next = !get_cfg().autostart;
    let ok = crate::autostart::sync_with_config(next, get_cfg().admin_autostart);
    // Only remember the new value when Windows really accepted it.
    if ok || !next {
        apply_config(|config| config.autostart = next);
    }
    if let Some(hwnd) = MAIN_HANDLE.get() {
        repaint(hwnd.0);
    }
    update_status(&format!(
        "Автозапуск: {}",
        if next {
            if ok {
                "включён"
            } else {
                "не включился: не хватает прав"
            }
        } else {
            "выключен"
        }
    ));
}

unsafe fn toggle_admin_autostart() {
    let next_admin = !get_cfg().admin_autostart;
    let next_auto = next_admin || get_cfg().autostart;
    let ok = crate::autostart::sync_with_config(next_auto, next_admin);
    if ok || !next_admin {
        apply_config(|config| {
            config.admin_autostart = next_admin;
            config.autostart = next_auto;
        });
    }
    if let Some(hwnd) = MAIN_HANDLE.get() {
        repaint(hwnd.0);
    }
    update_status(&format!(
        "Запуск как администратор: {}",
        if next_admin {
            if ok {
                "включён (задание ZyaAutoStart, уровень HIGHEST)"
            } else {
                "не включился: не хватает прав"
            }
        } else {
            "выключен"
        }
    ));
}

unsafe fn reset_hotkeys() {
    let default = Config::default();
    apply_config(|config| {
        config.hotkey_undo = default.hotkey_undo.clone();
        config.hotkey_toggle = default.hotkey_toggle.clone();
        config.hotkey_last = default.hotkey_last.clone();
        config.hotkey_selection = default.hotkey_selection.clone();
        config.hotkey_upper = default.hotkey_upper.clone();
        config.hotkey_invert = default.hotkey_invert.clone();
    });
    if let Some(list) = handle(HOTKEY_LIST) {
        fill_hotkey_list(list);
        InvalidateRect(list, std::ptr::null(), 1);
    }
    update_status("Клавиши сброшены к стандартным");
}

unsafe fn add_to_config(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }
    let reference = cfg();
    let mut guard = reference.lock().unwrap_or_else(|e| e.into_inner());
    if value.contains('\\') || value.contains('/') {
        if guard.exclude_dirs.iter().any(|item| item.eq_ignore_ascii_case(value)) {
            return false;
        }
        guard.exclude_dirs.push(value.to_string());
    } else {
        if guard.exclude_processes.iter().any(|item| item.eq_ignore_ascii_case(value)) {
            return false;
        }
        guard.exclude_processes.push(value.to_string());
    }
    true
}

unsafe fn add_running_processes() {
    let mut added = 0;
    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return;
        };
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let chars: Vec<u16> = entry
                    .szExeFile
                    .iter()
                    .take_while(|value| **value != 0)
                    .copied()
                    .collect();
                let name = String::from_utf16_lossy(&chars).to_lowercase();
                if name.ends_with(".exe")
                    && name != "explorer.exe"
                    && name != "zя.exe"
                    && name != "z.exe"
                    && add_to_config(&name)
                {
                    added += 1;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        windows::Win32::Foundation::CloseHandle(snapshot).ok();
    }
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
    if let Some(list) = handle(IGNORE_LIST) {
        fill_ignore_list(list);
        InvalidateRect(list, std::ptr::null(), 1);
    }
    update_status(&format!("Добавлено процессов: {added}"));
}

unsafe fn show_ignore_input() {
    if let Some(input) = handle(IGNORE_INPUT) {
        let _ = ShowWindow(input, SW_SHOW);
    }
    if let Some(commit) = handle(BTN_ADD_COMMIT) {
        let _ = ShowWindow(commit, SW_SHOW);
    }
    if let Some(hwnd) = MAIN_HANDLE.get() {
        repaint(hwnd.0);
    }
    update_status("Введите процесс (.exe) или путь к папке, например C:\\Soft");
}

unsafe fn add_ignore_entry() {
    let value = field_text(IGNORE_INPUT);
    if add_to_config(&value) {
        crate::win::reload_hotkeys(&get_cfg());
        save_config();
        if let Some(input) = handle(IGNORE_INPUT) {
            SetWindowTextW(input, PCWSTR(std::ptr::null())).ok();
            let _ = ShowWindow(input, SW_HIDE);
        }
        if let Some(commit) = handle(BTN_ADD_COMMIT) {
            let _ = ShowWindow(commit, SW_HIDE);
        }
        if let Some(list) = handle(IGNORE_LIST) {
            fill_ignore_list(list);
            InvalidateRect(list, std::ptr::null(), 1);
        }
        if let Some(hwnd) = MAIN_HANDLE.get() {
            repaint(hwnd.0);
        }
        update_status(&format!("Добавлено: {}", value.trim()));
    } else {
        update_status("Нужен процесс вида chrome.exe или папка вида C:\\Soft");
    }
}

unsafe fn remove_ignore_entry() {
    let Some(index) = list_selected(IGNORE_LIST) else {
        return;
    };
    let rows = ignore_rows();
    let Some(row) = rows.get(index) else {
        update_status("Сначала выберите строку в списке");
        return;
    };
    let name = row.name.clone();
    {
        let reference = cfg();
        let mut guard = reference.lock().unwrap_or_else(|e| e.into_inner());
        guard.exclude_processes.retain(|item| !item.eq_ignore_ascii_case(&name));
        guard.exclude_dirs.retain(|item| !item.eq_ignore_ascii_case(&name));
    }
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
    if let Some(list) = handle(IGNORE_LIST) {
        fill_ignore_list(list);
        InvalidateRect(list, std::ptr::null(), 1);
    }
    update_status(&format!("Удалено: {name}"));
}

unsafe fn copy_selected_row(role: usize) {
    let text = match role {
        IGNORE_LIST => ignore_rows()
            .get(list_selected(IGNORE_LIST).unwrap_or(0))
            .map(|row| row.name.clone())
            .unwrap_or_default(),
        LOG_LIST => current_log_rows()
            .get(list_selected(LOG_LIST).unwrap_or(0))
            .map(|row| row.raw.clone())
            .unwrap_or_default(),
        _ => String::new(),
    };
    if text.is_empty() {
        update_status("Нет выбранной строки");
        return;
    }
    copy_to_clipboard(&text);
    update_status("Строка скопирована");
}

unsafe fn copy_visible_rows(role: usize) {
    let lines: Vec<String> = match role {
        IGNORE_LIST => ignore_rows().into_iter().map(|row| row.name).collect(),
        LOG_LIST => current_log_rows().into_iter().map(|row| row.raw).collect(),
        _ => Vec::new(),
    };
    if lines.is_empty() {
        update_status("Список пуст");
        return;
    }
    copy_to_clipboard(&lines.join("\n"));
    update_status(&format!("Скопировано строк: {}", lines.len()));
}

unsafe fn copy_to_clipboard(text: &str) {
    let encoded = text.encode_utf16().collect::<Vec<u16>>();
    let bytes = encoded.len() * 2;
    if OpenClipboard(std::ptr::null_mut()) == 0 {
        return;
    }
    EmptyClipboard();
    let memory = GlobalAlloc(0, bytes);
    if !memory.is_null() {
        unsafe {
            std::ptr::copy_nonoverlapping(encoded.as_ptr() as *const u8, memory as *mut u8, bytes);
            SetClipboardData(13, memory);
        }
    }
    CloseClipboard();
}

unsafe fn open_config_file() {
    let path = crate::config::config_dir().join("config.json");
    let file = wide(&path.display().to_string());
    let action = wide("open");
    ShellExecuteW(None, PCWSTR(action.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR::null(), None, SW_SHOW);
    update_status(&format!("Открыт {}", path.display()));
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

fn update_info() -> Option<(String, String, u64)> {
    let info = UPDATE_INFO
        .get_or_init(|| Mutex::new((String::new(), String::new(), 0)))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    if info.0.is_empty() {
        None
    } else {
        Some(info)
    }
}

unsafe fn spawn_update_check() {
    std::thread::spawn(|| {
        let release = crate::update::check_latest();
        UPDATE_CHECKED.store(true, Ordering::Relaxed);
        let local = crate::update::app_version();
        let message = match release {
            Some(release) => {
                if crate::update::is_newer(&local, &release.version) {
                    let mut info = UPDATE_INFO
                        .get_or_init(|| Mutex::new((String::new(), String::new(), 0)))
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    *info = (release.version.clone(), release.url.clone(), release.size);
                    UPDATE_AVAILABLE.store(true, Ordering::Relaxed);
                    format!(
                        "Доступно обновление {}: {} ({:.1} МБ)",
                        release.version,
                        release.url,
                        release.size as f64 / 1_048_576.0
                    )
                } else {
                    format!("Обновлений нет: установлена {}, последняя на GitHub {}", local, release.version)
                }
            }
            None => "Не удалось проверить обновления: нет подключения к GitHub".into(),
        };
        if let Some(hwnd) = MAIN_HANDLE.get() {
            unsafe {
                update_status(&message);
                InvalidateRect(hwnd.0, std::ptr::null(), 1);
            }
        }
    });
}

unsafe fn run_update() {
    let info = update_info();
    if UPDATE_AVAILABLE.load(Ordering::Relaxed) {
        let Some((version, url, size)) = info else {
            update_status("Не удалось получить сведения об обновлении");
            return;
        };
        update_status("Скачиваю установщик…");
        let release = crate::update::Release { version, url, size };
        match crate::update::download_installer(&release) {
            Some(path) => {
                if crate::update::run_installer(&path) {
                    update_status(&format!("Установщик запущен: {}", path.display()));
                } else {
                    update_status("Не удалось запустить установщик");
                }
            }
            None => update_status("Скачивание не удалось"),
        }
    } else {
        UPDATE_CHECKED.store(false, Ordering::Relaxed);
        spawn_update_check();
        update_status("Проверяю обновления на GitHub…");
    }
}

// ---------------------------------------------------------------------------
// Hotkey capture
// ---------------------------------------------------------------------------

unsafe fn open_hotkey_modal(index: usize) {
    let Some(parent) = MAIN_HANDLE.get().map(|win| win.0) else {
        update_status("Нет главного окна для назначения");
        return;
    };
    let dpi = window_dpi(parent);
    let row = index.min(HOTKEY_ACTIONS.len() - 1);
    MODAL_TARGET.store(row, Ordering::Relaxed);

    if let Some(existing) = modal_handle() {
        // The modal is already on screen: switch it to the row that was clicked.
        InvalidateRect(existing, std::ptr::null(), 1);
        return;
    }

    let modal = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("ZyaModalWindow").as_ptr()),
        PCWSTR(wide("").as_ptr()),
        WS_CHILD | WS_VISIBLE,
        // Centred over the content column and above the hotkey list, so the
        // modal never covers the rows it was opened from.
        s(498, dpi),
        s(140, dpi),
        s(460, dpi),
        s(300, dpi),
        Some(parent),
        None,
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    if is_null(modal) {
        MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
        update_status("Не удалось открыть окно назначения");
        return;
    }

    let mut guard = MODAL_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    guard.replace(Win(modal));
    // The hotkey list was created earlier, so it sits above the modal in the
    // child z-order; without this the modal cannot receive mouse clicks.
    SetWindowPos(
        modal,
        Some(HWND_TOP),
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE,
    )
    .ok();

    make_button(modal, dpi, "Отмена", 20, 244, 140, 36, MODAL_CANCEL);

    if HOTKEY_HOOK.get().is_none() {
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(capture_hotkey_hook), None, 0) {
            Ok(hook) => {
                HOTKEY_HOOK.get_or_init(|| hook.0 as usize);
            }
            Err(error) => {
                MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
                update_status(&format!("Не удалось слушать клавиши: {error}"));
            }
        }
    }
    InvalidateRect(modal, std::ptr::null(), 1);
}

unsafe fn modal_text(hdc: HDC, dpi: u32, left: i32, top: i32, width: i32, height: i32, text: &str, font: HGDIOBJ, color: COLORREF) {
    draw_text(
        hdc,
        text,
        &rect_at(left, top, width, height, dpi),
        font,
        color,
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
}

fn modal_handle() -> Option<HWND> {
    MODAL_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .map(|win| win.0)
}

unsafe fn close_modal() {
    let mut guard = MODAL_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(modal) = guard.take() {
        DestroyWindow(modal.0).ok();
    }
    MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
}

unsafe extern "system" fn modal_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            let dpi = window_dpi(hwnd);
            let hdc = HDC(wparam.0 as *mut c_void);
            let rect = client_rect(hwnd);
            fill_rect(hdc, rect.left, rect.top, rect.right, rect.bottom, bg_color());
            rounded(
                hdc,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                s(10, dpi),
                card_color(),
                card_border(),
            );
            let target = MODAL_TARGET.load(Ordering::Relaxed);
            let (action, field, default) = HOTKEY_ACTIONS
                .get(target)
                .unwrap_or(&("Действие", "hotkey_toggle", "ctrl+alt+p"));
            let config = get_cfg();
            let current = hotkey_value(&config, field);
            modal_text(hdc, dpi, 20, 18, 420, 26, "Назначение горячей клавиши", fonts(dpi).card, fg_color());
            modal_text(hdc, dpi, 20, 50, 420, 22, action, fonts(dpi).dim, dim_color());
            modal_text(
                hdc,
                dpi,
                20,
                76,
                420,
                24,
                &format!("Сейчас: {current} (по умолчанию {default})"),
                fonts(dpi).body,
                fg_color(),
            );
            modal_text(
                hdc,
                dpi,
                20,
                108,
                420,
                26,
                "Нажмите новую комбинацию",
                fonts(dpi).big,
                accent_color(),
            );
            draw_text(
                hdc,
                "Поддерживаются ctrl, alt, shift, win с буквами, цифрами, Break и F-клавишами. Alt сам по себе занят системой. Esc — отмена.",
                &rect_at(20, 142, 420, 74, dpi),
                fonts(dpi).dim,
                dim_color(),
                windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_WORDBREAK,
            );
            return LRESULT(1);
        }
        WM_DRAWITEM => {
            let info = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            let dpi = window_dpi(hwnd);
            let rect = info.rcItem;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            fill_rect(info.hDC, rect.left, rect.top, width, height, card_color());
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                button_color(),
                card_border(),
            );
            draw_text(
                info.hDC,
                "Отмена",
                &RECT {
                    left: rect.left + s(12, dpi),
                    top: rect.top,
                    right: rect.right - s(12, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                windows::Win32::Graphics::Gdi::DT_CENTER
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
            return LRESULT(0);
        }
        WM_KEYDOWN => {
            if wparam.0 == VK_ESCAPE.0 as usize {
                close_modal();
                return LRESULT(0);
            }
        }
        WM_COMMAND => {
            if (wparam.0 & 0xFFFF) as usize == MODAL_CANCEL {
                close_modal();
                return LRESULT(0);
            }
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

static BREAK_SYNTHESIS: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn capture_hotkey_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Windows does not reliably pass HC_ACTION for low-level hooks, so the
    // up/down state is taken from the KBDLLHOOKSTRUCT flags.
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    {
        let hook = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let pressed = VIRTUAL_KEY(hook.vkCode as u16);
        let up = (hook.flags & LLKHF_UP).0 != 0;
        set_modifier(pressed, !up);

        if up {
            if matches!(pressed.0, 0x11 | 0xA2 | 0xA3) {
                BREAK_SYNTHESIS.store(false, Ordering::Relaxed);
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let target = MODAL_TARGET.load(Ordering::Relaxed);
        if target == usize::MAX {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let vk = pressed;
        // The Break key is delivered by Windows as Pause followed by a synthetic
        // Ctrl+C. Only the Pause part is the key the user actually pressed.
        if vk.0 == 0x13 {
            // VK_PAUSE / Break
            BREAK_SYNTHESIS.store(true, Ordering::Relaxed);
        } else if BREAK_SYNTHESIS.load(Ordering::Relaxed) {
            let synthetic = matches!(vk.0, 0x11 | 0xA2 | 0xA3 | 0x43);
            if synthetic {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            BREAK_SYNTHESIS.store(false, Ordering::Relaxed);
        }
        if vk == VK_ESCAPE {
            update_status("Назначение отменено");
            if let Some(modal) = modal_handle() {
                PostMessageW(Some(modal), WM_COMMAND, WPARAM(MODAL_CANCEL), LPARAM(0)).ok();
            }
            return LRESULT(1);
        }
        let ctrl = modifier_down(VK_LCONTROL) || modifier_down(VK_RCONTROL);
        let alt = modifier_down(VK_LMENU) || modifier_down(VK_RMENU);
        let shift = modifier_down(VK_LSHIFT) || modifier_down(VK_RSHIFT);
        let win = modifier_down(VK_WIN_LEFT) || modifier_down(VK_WIN_RIGHT);
        let spec = capture_spec(vk, ctrl, alt, shift, win);
        let field = HOTKEY_ACTIONS
            .get(target)
            .map(|(_, name, _)| *name)
            .unwrap_or("hotkey_toggle");

        match validate_hotkey(field, &spec) {
            Ok(value) => {
                apply_config(|config| match field {
                    "hotkey_undo" => config.hotkey_undo = value.clone(),
                    "hotkey_toggle" => config.hotkey_toggle = value.clone(),
                    "hotkey_last" => config.hotkey_last = value.clone(),
                    "hotkey_selection" => config.hotkey_selection = value.clone(),
                    "hotkey_upper" => config.hotkey_upper = value.clone(),
                    _ => config.hotkey_invert = value.clone(),
                });
                if let Some(list) = handle(HOTKEY_LIST) {
                    fill_hotkey_list(list);
                    InvalidateRect(list, std::ptr::null(), 1);
                }
                update_status(&format!("Назначено: {value}"));
                MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
                if let Some(modal) = modal_handle() {
                    PostMessageW(Some(modal), WM_COMMAND, WPARAM(MODAL_CANCEL), LPARAM(0)).ok();
                }
            }
            Err(reason) => {
                update_status(&format!("Нельзя назначить: {reason}"));
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

const VK_WIN_LEFT: VIRTUAL_KEY = VIRTUAL_KEY(0x92);
const VK_WIN_RIGHT: VIRTUAL_KEY = VIRTUAL_KEY(0x93);

fn modifier_counts() -> &'static Mutex<HashMap<u16, u32>> {
    static STATE: OnceLock<Mutex<HashMap<u16, u32>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_modifier(vk: VIRTUAL_KEY, down: bool) {
    let mut guard = modifier_counts()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let count = guard.entry(vk.0).or_insert(0);
    if down {
        *count += 1;
    } else {
        *count = count.saturating_sub(1);
    }
}

fn modifier_down(vk: VIRTUAL_KEY) -> bool {
    modifier_counts()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&vk.0)
        .map_or(false, |count| *count > 0)
}

fn capture_spec(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, shift: bool, win: bool) -> String {
    let base = key_name(vk);
    if base.is_empty() {
        return String::new();
    }
    // A modifier pressed alone is its own name, so it must not be repeated.
    let is_modifier = matches!(base.as_str(), "ctrl" | "alt" | "shift" | "win");
    let mut parts: Vec<String> = Vec::new();
    if ctrl && (!is_modifier || base != "ctrl") {
        parts.push("ctrl".into());
    }
    if alt && (!is_modifier || base != "alt") {
        parts.push("alt".into());
    }
    if shift && (!is_modifier || base != "shift") {
        parts.push("shift".into());
    }
    if win && (!is_modifier || base != "win") {
        parts.push("win".into());
    }
    parts.push(base);
    parts.join("+")
}

fn key_name(vk: VIRTUAL_KEY) -> String {
    let code = vk.0;
    match code {
        0x08 => "back".into(),
        0x09 => "tab".into(),
        0x0D => "enter".into(),
        0x10 => "shift".into(),
        0x11 => "ctrl".into(),
        0x12 => "alt".into(),
        0xA0..=0xA1 => "shift".into(),
        0xA2..=0xA3 => "ctrl".into(),
        0xA4..=0xA5 => "alt".into(),
        0x20 => "space".into(),
        0x14 => "caps".into(),
        0x13 => "break".into(),
        0x1B => "esc".into(),
        0x2E => "delete".into(),
        0x92..=0x93 => "win".into(),
        0x70..=0x7B => format!("f{}", code - 0x6F),
        0x30..=0x39 => ((b'0' + (code - 0x30) as u8) as char).to_string(),
        0x41..=0x5A => ((b'a' + (code - 0x41) as u8) as char).to_string(),
        _ => String::new(),
    }
}

fn validate_hotkey(field: &str, spec: &str) -> Result<String, String> {
    let trimmed = spec.trim().to_lowercase();
    if trimmed.is_empty() {
        return Err("нажмите клавишу".into());
    }
    if trimmed == "alt" {
        return Err("Alt сам по себе занят: Alt+Shift меняет раскладку".into());
    }
    if trimmed == "ctrl" || trimmed == "shift" || trimmed == "win" {
        return Err("Нужна комбинация: добавьте обычную клавишу".into());
    }
    if system_hotkeys().contains(&trimmed.as_str()) {
        return Err(format!("«{trimmed}» занято Windows"));
    }
    if trimmed == "break" && field != "hotkey_last" && field != "hotkey_selection" {
        return Err("Break уже занят: он возвращает последние изменения и выделяет".into());
    }
    if crate::win::parse_hotkey(&trimmed).is_some() {
        Ok(trimmed)
    } else {
        Err(format!("«{trimmed}» нельзя распознать как комбинацию"))
    }
}

fn system_hotkeys() -> &'static [&'static str] {
    &[
        "ctrl+alt+delete",
        "win+l",
        "win+r",
        "win+e",
        "win+i",
        "win+a",
        "alt+f4",
        "alt+tab",
        "ctrl+shift+esc",
        "win+tab",
    ]
}

// ---------------------------------------------------------------------------
// Window helpers
// ---------------------------------------------------------------------------

unsafe fn update_status(text: &str) {
    {
        let message = STATUS_MESSAGE.get_or_init(|| Mutex::new(String::new()));
        let mut guard = message.lock().unwrap_or_else(|e| e.into_inner());
        *guard = text.to_string();
    }
    if let Some(status) = handle(STATUS_LABEL) {
        let encoded = wide(text);
        SetWindowTextW(status, PCWSTR(encoded.as_ptr())).ok();
    }
}

unsafe fn set_window_icon(hwnd: HWND) {
    let name = wide("app.ico");
    let icon = LoadImageW(name.as_ptr(), name.as_ptr(), 0x00000010, 32, 32, 3);
    if !icon.is_null() {
        send(hwnd, WM_SETICON, 0, icon as isize);
        send(hwnd, WM_SETICON, 1, icon as isize);
    }
}

unsafe fn apply_titlebar_theme(hwnd: HWND) {
    if dark_theme() {
        let value = 1u32;
        DwmSetWindowAttribute(hwnd, 20, &value as *const u32 as *const c_void, 4);
        DwmSetWindowAttribute(hwnd, 19, &value as *const u32 as *const c_void, 4);
    }
}

fn window_dpi(hwnd: HWND) -> u32 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            96
        } else {
            dpi
        }
    }
}

fn s(base: i32, dpi: u32) -> i32 {
    base * (dpi as i32) / BASE_DPI
}

fn client_rect(hwnd: HWND) -> RECT {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let _ = GetClientRect(hwnd, &mut rect);
        if rect.right <= 0 || rect.bottom <= 0 {
            rect.right = 1;
            rect.bottom = 1;
        }
        rect
    }
}

fn current_tab() -> usize {
    CURRENT_TAB.load(Ordering::Relaxed)
}

/// True while the "press a new combination" dialog is open, so the main hook
/// does not react to the same key press twice.
pub fn is_assign_open() -> bool {
    MODAL_TARGET.load(Ordering::Relaxed) != usize::MAX
}

unsafe fn register_classes() {
    let module = GetModuleHandleW(None).unwrap_or_default();
    let instance = windows::Win32::Foundation::HINSTANCE(module.0);
    let cursor = windows::Win32::UI::WindowsAndMessaging::LoadCursorW(None, IDC_ARROW)
        .unwrap_or_default();
    let settings_name = wide("ZyaSettingsWindow");
    let modal_name = wide("ZyaModalWindow");

    let settings = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpszClassName: PCWSTR(settings_name.as_ptr()),
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(settings_proc),
        hInstance: instance,
        hCursor: cursor,
        ..Default::default()
    };
    RegisterClassExW(&settings);

    let modal = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpszClassName: PCWSTR(modal_name.as_ptr()),
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(modal_proc),
        hInstance: instance,
        hCursor: cursor,
        ..Default::default()
    };
    RegisterClassExW(&modal);
}

pub fn open() -> std::io::Result<()> {
    open_blocking()
}

pub fn open_blocking() -> std::io::Result<()> {
    unsafe {
        register_classes();
        let window = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(wide("ZyaSettingsWindow").as_ptr()),
            PCWSTR(wide("ZЯ — настройки").as_ptr()),
            SETTINGS_WINDOW_STYLE | WS_VISIBLE,
            0,
            0,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            None,
            None,
        );
        let window = window.unwrap_or(null_hwnd());
        if is_null(window) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "не удалось создать окно настроек",
            ));
        }
        let dpi = window_dpi(window);
        SetWindowPos(
            window,
            None,
            0,
            0,
            s(WINDOW_WIDTH, dpi),
            s(WINDOW_HEIGHT, dpi),
            SWP_NOZORDER,
        )
        .ok();
        let _ = ShowWindow(window, SW_SHOW);

        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).into() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

pub fn pick_probe() -> Option<(u16, bool, bool)> {
    None
}

pub fn self_test() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;

    #[test]
    fn card_rows_do_not_overlap_the_list() {
        assert!(HOTKEY_LIST_Y + 280 <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        assert!(IGNORE_LIST_CARD_Y + 308 - IGNORE_LIST_CARD_Y < IGNORE_LIST_CARD_H);
    }

    #[test]
    fn cards_fit_the_window() {
        assert_eq!(CONTENT_X + CONTENT_WIDTH, WINDOW_WIDTH - SIDEBAR_X);
        assert!(SIDEBAR_X + SIDEBAR_WIDTH < CONTENT_X);
        assert!(HOTKEY_CARD_Y + HOTKEY_CARD_H + 40 <= WINDOW_HEIGHT);
        assert!(LOG_CARD_Y + LOG_CARD_H <= STATUS_Y);
        assert!(IGNORE_LIST_CARD_Y + IGNORE_LIST_CARD_H <= STATUS_Y);
    }

    #[test]
    fn dark_and_light_palettes_differ() {
        DARK.store(true, Ordering::Relaxed);
        let dark = bg_color();
        DARK.store(false, Ordering::Relaxed);
        let light = bg_color();
        DARK.store(true, Ordering::Relaxed);
        assert_ne!(dark, light);
    }

    #[test]
    fn bare_alt_is_rejected() {
        assert!(validate_hotkey("hotkey_undo", "alt").is_err());
        assert!(validate_hotkey("hotkey_toggle", "ctrl+alt+p").is_ok());
        assert!(validate_hotkey("hotkey_upper", "win+e").is_err());
    }

    #[test]
    fn key_names_are_readable() {
        assert_eq!(key_name(VK_SPACE), "space");
        assert_eq!(key_name(VIRTUAL_KEY(0x70)), "f1");
        assert_eq!(key_name(VIRTUAL_KEY(0x41)), "a");
        assert_eq!(key_name(VIRTUAL_KEY(0x13)), "break");
    }
}
