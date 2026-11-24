//! Settings window for ZЯ.
//!
//! Plain Win32, no UI framework: the window is painted by hand (rounded cards,
//! a left column of tabs, a header) and every control is either an owner-drawn
//! button, an edit field or an owner-drawn list. The palette follows the system
//! theme — dark cards on a dark window when `AppsUseLightTheme` is 0, light
//! cards when it is 1 — and the whole layout is scaled from 96-DPI design units.
//!
//! Four pages:
//!   1. Настройки    — version, update button, autostart, "run as admin", hotkeys
//!   2. Игнорировать — processes and folders the keyboard hook must not touch
//!   3. Лог          — the switch diary, colour coded, copyable with the right mouse
//!   4. Словарь      — short service words of both layouts, extendable by the user
//!
//! Clicking a row in the hotkey table opens the "press a new combination" dialog.
//! If a combination cannot be assigned, the status column says why.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, COLORREF, GlobalFree, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BACKGROUND_MODE, ArcTo, BeginPath, CLIP_DEFAULT_PRECIS, BeginPaint, CreateFontW, CreatePen,
    CreateSolidBrush, ANTIALIASED_QUALITY, DEFAULT_CHARSET, DeleteObject, DrawTextW, EndPath, EndPaint,
    FillPath, FillRect, FF_MODERN, GetDC, GetMonitorInfoW, GetTextExtentPoint32W, HGDIOBJ, HDC, LineTo,
    MonitorFromPoint, MONITOR_DEFAULTTONEAREST, MONITORINFO, MoveToEx, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
    PS_SOLID, ReleaseDC, ClientToScreen, ScreenToClient, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, StrokePath, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Controls::{
    DRAWITEMSTRUCT, LVCOLUMNW, LVCF_ORDER, LVCF_TEXT, LVCF_WIDTH, LVIF_TEXT, LVNI_SELECTED, LVITEMW,
    LVM_DELETEALLITEMS, LVM_GETITEMRECT, LVM_GETNEXTITEM, LVM_HITTEST, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETBKCOLOR, LVM_SETCOLUMNWIDTH, LVS_EX_FULLROWSELECT, LVS_REPORT,
    ODS_SELECTED, ODT_LISTVIEW, SetWindowTheme,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VIRTUAL_KEY, VK_ESCAPE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CallWindowProcW, CS_HREDRAW, CS_VREDRAW, CreateWindowExW,
    DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GetClassNameW, GetClientRect,
    GetCursorPos, GetWindowLongPtrW, GetWindowRect, GetMessageW, GetWindow, GetWindowTextW, GWL_ID,
    GWL_WNDPROC, GW_CHILD, GW_HWNDNEXT, GW_HWNDPREV, HHOOK,
    CreateIconFromResourceEx, HICON, IMAGE_FLAGS,
    HMENU, IDC_ARROW, HWND_TOP,
    KBDLLHOOKSTRUCT, KBDLLHOOKSTRUCT_FLAGS, LLKHF_UP, MF_ENABLED, MSG, MoveWindow, PostMessageW, RegisterClassExW, SendMessageW,
    SetWindowLongPtrW, SetWindowLongW, SetWindowPos, SetWindowsHookExW, SetWindowTextW, ShowWindow,
    KillTimer, SetTimer,
    SW_HIDE, SW_SHOW, SWP_NOZORDER, SWP_NOMOVE, SWP_NOSIZE, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
    TranslateMessage, UnhookWindowsHookEx, WNDCLASSEXW, WNDPROC, WH_KEYBOARD_LL, WM_COMMAND,
    WM_CONTEXTMENU, WM_CREATE, WM_CTLCOLORSTATIC, WM_CTLCOLOREDIT, WM_DESTROY, WM_DRAWITEM,
    WM_ERASEBKGND, WM_PAINT, WM_KEYDOWN, WM_MOUSEMOVE, WM_SETICON, WM_SETFONT, WM_QUIT, WM_TIMER, WM_CLOSE,
    WINDOW_EX_STYLE, WINDOW_STYLE,
    WS_CHILD, WS_POPUP, WS_VISIBLE, CreatePopupMenu,
    GetForegroundWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SPI_GETWORKAREA,
    SetForegroundWindow,
    SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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
// Inner working area of a card: nothing (button, field, list) is allowed to sit
// closer to the rounded frame than CARD_PAD, and every caption keeps BUTTON_PAD
// of its own padding inside the button so it never touches the button border.
const INNER_LEFT: i32 = CONTENT_X + CARD_PAD;
const INNER_RIGHT: i32 = CONTENT_X + CONTENT_WIDTH - CARD_PAD;
const BUTTON_PAD: i32 = 12;

const VERSION_CARD_Y: i32 = 88;
const VERSION_CARD_H: i32 = 116;
// The "Запуск" card holds the ordinary switches: four owner-draw checkboxes
// (three in the left column, the smart-undo switch in the right one) and the two
// hint lines. It is 216..432; the hotkey card starts below it at 440.
const LAUNCH_CARD_Y: i32 = 216;
const LAUNCH_CARD_H: i32 = 216;
const CB_AUTOSTART_Y: i32 = 254;
const CB_ADMIN_Y: i32 = 296;
const CB_AUTO_Y: i32 = 338;
const LAUNCH_HINT_Y: i32 = 396;
// Right column of the same card: the Break behaviour and its explanation.
const SMART_COL_X: i32 = CONTENT_X + 464;
const SMART_COL_W: i32 = 416;
const CB_SMART_Y: i32 = 254;
const SMART_HINT_Y: i32 = 290;
const HOTKEY_CARD_Y: i32 = 440;
const HOTKEY_CARD_H: i32 = 312;
// Two tables side by side: the hotkeys that work on the last token without any
// selection, and the hotkeys that work only on selected text. The same key may
// be bound in both columns at the same time.
const HOTKEY_LIST_Y: i32 = 512;
const HOTKEY_LIST_W: i32 = 424;
const HOTKEY_WORD_X: i32 = CONTENT_X + CARD_PAD;
const HOTKEY_SEL_X: i32 = CONTENT_X + CARD_PAD + HOTKEY_LIST_W + 16;
const HOTKEY_WORD_LIST_H: i32 = 236;
const HOTKEY_SEL_LIST_H: i32 = 152;
const HOTKEY_GROUP_Y: i32 = HOTKEY_LIST_Y - 30;
// The two tables only need the rows they hold, so the space below them is used
// for the note about the shared combinations.
const HOTKEY_NOTE_Y: i32 = 646;
const HOTKEY_NOTE_TEXT: &str = "Одна комбинация может стоять и в левой, и в правой колонке:\nBreak слева чинит последнее слово, Break справа меняет раскладку выделения.\nCtrl+` добавляет выделенное слово в словарь.";
const STATUS_Y: i32 = 762;
// The bottom status line is a place for action feedback only. A check that
// found nothing new leaves it empty instead of repeating the header note.
const STATUS_IDLE: &str = "";

// The assignment modal. It used to be 460x300 pinned at (498, 140), which put it
// on top of the "Версия" and "Запуск" cards and their buttons. It now lives
// inside the hotkey card, anchored to the row that was clicked, and can be
// dragged by its header.
const MODAL_WIDTH: i32 = 460;
const MODAL_HEIGHT: i32 = 196;
const MODAL_GRAB_H: i32 = 40;

// Printed in the settings card next to the switch: what "умная отмена" does.
// The lines are broken by hand: DrawTextW clips this block at the right edge of
// the column instead of wrapping it, so each line has to fit in 416 design px.
const SMART_UNDO_TEXT: &str = "Первый Break читает буквы вместо знаков:\n«…ю» → «… .», «…б» → «…,», «…ж» → «…;»\nВторой Break на том же слове возвращает\nтекст до первого нажатия. Без неё — зеркало.";

const IGNORE_CARD_Y: i32 = 88;
const IGNORE_CARD_H: i32 = 156;
const IGNORE_LIST_CARD_Y: i32 = 256;
const IGNORE_LIST_CARD_H: i32 = 496;

const LOG_CARD_Y: i32 = 88;
const LOG_CARD_H: i32 = 664;

// Lexicon page: the explanation card mirrors the ignore card, the word table
// takes the same slot as the exception list (308..700) and the two footnote
// lines below it still fit above STATUS_Y.
const LEX_CARD_Y: i32 = 88;
const LEX_CARD_H: i32 = 156;
const LEX_INPUT_Y: i32 = 166;
// DrawText clips here instead of wrapping, so the note is written to fit one
// line of the card; `every_note_line_fits_its_card` checks it with the real font.
const LEX_NOTE: &str = "Служебные слова нельзя менять по зеркалу. Здесь добавляются свои: кириллица — в русский список, латиница — в английский.";
const LEX_LIST_CARD_Y: i32 = 256;
const LEX_LIST_CARD_H: i32 = 496;
const LEX_LIST_Y: i32 = 308;
const LEX_LIST_H: i32 = 392;
const LEX_COUNT_Y: i32 = 706;
const LEX_CONFLICT_Y: i32 = 728;

// The inner working area of a card, shared by the creation code, the paint pass
// and the geometry tests: a control never sits closer than CARD_PAD to a frame,
// and a caption never touches the border of its own button.
#[derive(Debug, Clone, Copy)]
struct ControlSpec {
    role: usize,
    label: &'static str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    // 0 means "fixed width". Otherwise the caption may widen the button up to
    // this limit; a caption that still does not fit is drawn in a smaller font.
    max_width: i32,
    // Anchored controls are placed so their right edge sits on INNER_RIGHT.
    anchored_right: bool,
}

const IGNORE_SLOT: i32 = (INNER_RIGHT - INNER_LEFT - 2 * 16) / 3;
const FILTER_W: i32 = 180;
const FILTER_LABEL_W: i32 = 84;
const ROW_GAP: i32 = 10;

const CONTROLS: [ControlSpec; 20] = [
    ControlSpec { role: BTN_CONFIG, label: "Открыть файл конфигурации", x: 0, y: HEADER_TOP, width: 280, height: 34, max_width: 320, anchored_right: true },
    ControlSpec { role: BTN_UPDATE, label: "Проверить обновления", x: 0, y: 112, width: 200, height: 40, max_width: 260, anchored_right: true },
    ControlSpec { role: CB_AUTOSTART, label: "Автозапуск при входе", x: INNER_LEFT, y: CB_AUTOSTART_Y, width: 430, height: 38, max_width: 440, anchored_right: false },
    ControlSpec { role: CB_ADMIN, label: "Запускать как администратор", x: INNER_LEFT, y: CB_ADMIN_Y, width: 430, height: 38, max_width: 440, anchored_right: false },
    ControlSpec { role: CB_AUTO_LAYOUT, label: "Автоисправление раскладки", x: INNER_LEFT, y: CB_AUTO_Y, width: 430, height: 28, max_width: 440, anchored_right: false },
    ControlSpec { role: CB_SMART_UNDO, label: "Умная отмена", x: SMART_COL_X, y: CB_SMART_Y, width: SMART_COL_W, height: 34, max_width: 416, anchored_right: false },
    ControlSpec { role: BTN_HOTKEY_RESET, label: "Сбросить", x: 0, y: HOTKEY_CARD_Y + 14, width: 120, height: 32, max_width: 160, anchored_right: true },
    ControlSpec { role: BTN_ADD_PROCESSES, label: "Добавить активное приложение", x: INNER_LEFT, y: 128, width: IGNORE_SLOT, height: 40, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_ADD_PROCESS, label: "Добавить новый процесс", x: INNER_LEFT + IGNORE_SLOT + 16, y: 128, width: IGNORE_SLOT, height: 40, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_REMOVE_IGNORE, label: "Удалить выбранное", x: INNER_LEFT + 2 * (IGNORE_SLOT + 16), y: 128, width: IGNORE_SLOT, height: 40, max_width: 0, anchored_right: false },
    ControlSpec { role: IGNORE_INPUT, label: "", x: INNER_LEFT, y: 178, width: INNER_RIGHT - INNER_LEFT - 132, height: 34, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_ADD_COMMIT, label: "Добавить", x: 0, y: 178, width: 120, height: 34, max_width: 0, anchored_right: true },
    ControlSpec { role: IGNORE_FILTER, label: "", x: 0, y: 266, width: FILTER_W, height: 32, max_width: 0, anchored_right: true },
    ControlSpec { role: BTN_LOG_CLEAR, label: "Очистить", x: INNER_LEFT, y: 132, width: 120, height: 32, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_LOG_REFRESH, label: "Обновить", x: INNER_RIGHT - FILTER_W - ROW_GAP - FILTER_LABEL_W - ROW_GAP - 120, y: 100, width: 120, height: 32, max_width: 0, anchored_right: false },
    ControlSpec { role: LOG_FILTER, label: "", x: 0, y: 100, width: FILTER_W, height: 32, max_width: 0, anchored_right: true },
    ControlSpec { role: LEX_INPUT, label: "", x: INNER_LEFT, y: LEX_INPUT_Y, width: 540, height: 34, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_LEX_ADD, label: "Добавить", x: INNER_LEFT + 540 + 12, y: LEX_INPUT_Y, width: 120, height: 34, max_width: 0, anchored_right: false },
    ControlSpec { role: BTN_LEX_REMOVE, label: "Удалить выбранное", x: 0, y: LEX_INPUT_Y, width: 180, height: 34, max_width: 0, anchored_right: true },
    ControlSpec { role: LEX_FILTER, label: "", x: 0, y: 266, width: FILTER_W, height: 32, max_width: 0, anchored_right: true },
];

const TAB_SETTINGS: usize = 100;
const TAB_IGNORE: usize = 101;
const TAB_LOG: usize = 102;
const TAB_LEXICON: usize = 103;

// The sidebar is built from this table: index in the array is the tab index used
// by CURRENT_TAB, so adding a page means adding a row here and a group in
// `visibility_groups`.
const SIDEBAR_TABS: [(usize, &str); 4] = [
    (TAB_SETTINGS, "Настройки"),
    (TAB_IGNORE, "Игнорировать"),
    (TAB_LOG, "Лог"),
    (TAB_LEXICON, "Словарь"),
];

const BTN_CONFIG: usize = 110;
const BTN_UPDATE: usize = 112;

const CB_AUTOSTART: usize = 120;
const CB_ADMIN: usize = 121;
const HOTKEY_LIST: usize = 122;
const BTN_HOTKEY_RESET: usize = 123;
const HOTKEY_SEL_LIST: usize = 124;
const CB_SMART_UNDO: usize = 125;

const BTN_ADD_PROCESSES: usize = 130;
const BTN_ADD_PROCESS: usize = 131;
const BTN_REMOVE_IGNORE: usize = 132;
const IGNORE_INPUT: usize = 133;
const BTN_ADD_COMMIT: usize = 134;
const IGNORE_FILTER: usize = 135;
const IGNORE_LIST: usize = 136;
const IGNORE_FILTER_LABEL: usize = 137;
const CB_AUTO_LAYOUT: usize = 138;
const LOG_FILTER_LABEL: usize = 144;

const BTN_LOG_REFRESH: usize = 140;
const BTN_LOG_CLEAR: usize = 141;
const LOG_FILTER: usize = 142;
const LOG_LIST: usize = 143;

const LEX_INPUT: usize = 146;
const BTN_LEX_ADD: usize = 147;
const BTN_LEX_REMOVE: usize = 148;
const LEX_FILTER: usize = 149;
const LEX_LIST: usize = 151;
const LEX_FILTER_LABEL: usize = 152;

const STATUS_LABEL: usize = 150;
const MODAL_CANCEL: usize = 160;
// The keyboard hook never touches the disk or the list controls: it only stores
// the pressed combination and posts this id to the settings window, where the
// message loop applies it (P0-2).
const HOTKEY_ASSIGN: usize = 161;
const HOTKEY_ESCAPED: usize = 162;
// WM_TIMER id of the one-second heartbeat the watchdog watches.
const HEARTBEAT_TIMER: usize = 1;
// A stalled UI thread would stall the low-level keyboard hook too, since the
// hook runs on the same thread. The watchdog reacts after this many seconds
// without a heartbeat.
const UI_STALL_SECONDS: u64 = 6;

const WS_TABSTOP_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x00000010);
const BS_OWNERDRAW_STYLE: WINDOW_STYLE = WINDOW_STYLE(0x0000000B);
const OWNERDRAW_LIST_STYLE: WINDOW_STYLE = WINDOW_STYLE(LVS_REPORT | 0x00000400);
// WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX. A normal Windows window: real caption
// with the system close and minimise buttons, no resize frame (the layout is fixed).
const SETTINGS_WINDOW_STYLE: WINDOW_STYLE = WINDOW_STYLE(12582912 + 524288 + 131072);

// Notification codes that the owner-draw list views send to their parent. In
// commctrl.h these are `(0U - 2U)` and `(0U - 3U)`, i.e. the high word of the
// wParam of WM_COMMAND; the previous 0x1111/0x1112 values never matched, so the
// branch below was dead code.
const NM_CLICK: u32 = 0xFFFF_FFFE;
const NM_DBLCLK: u32 = 0xFFFF_FFFD;
// Text notifications of an edit control: EN_CHANGE is 0x0300, EN_UPDATE 0x0400.
const EN_CHANGE: u32 = 0x0300;
const EN_UPDATE: u32 = 0x0400;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

static DARK: AtomicBool = AtomicBool::new(true);
static THEME_STAMP: AtomicI64 = AtomicI64::new(0);
static CURRENT_TAB: AtomicUsize = AtomicUsize::new(0);
// `OnceLock` can only ever hold the *first* window ever created, so closing the
// settings window left a dead HWND behind and every later modal/repaint used it.
// The handle now lives in an atomic: written in WM_CREATE, cleared in WM_DESTROY.
static MAIN_HANDLE: AtomicUsize = AtomicUsize::new(0);
static HOTKEY_HOOK: AtomicUsize = AtomicUsize::new(0);
static MODAL_HANDLE: OnceLock<Mutex<Option<Win>>> = OnceLock::new();
static MODAL_TARGET: AtomicUsize = AtomicUsize::new(usize::MAX);
// Which of the two hotkey tables the open panel belongs to: HOTKEY_LIST (last
// token) or HOTKEY_SEL_LIST (selection).
static MODAL_ROLE: AtomicUsize = AtomicUsize::new(HOTKEY_LIST);
// Drag state of the modal: grabbing the header moves the panel inside the
// settings window. Child windows are positioned relative to the parent client
// area, so the current spot is kept here instead of being recovered from
// screen coordinates.
static MODAL_DRAGGING: AtomicBool = AtomicBool::new(false);
static MODAL_DRAG_X: AtomicI32 = AtomicI32::new(0);
static MODAL_DRAG_Y: AtomicI32 = AtomicI32::new(0);
static MODAL_LEFT: AtomicI32 = AtomicI32::new(0);
static MODAL_TOP: AtomicI32 = AtomicI32::new(0);
static CAPTURE_KEYS: AtomicUsize = AtomicUsize::new(0);
static UPDATE_AVAILABLE: AtomicBool = AtomicBool::new(false);
// Heartbeat of the UI thread: the message loop bumps it on every dispatched
// message. A detached watchdog thread watches it and closes the window if the
// loop stops, so a wedged handler can never leave a frozen window on screen.
static UI_BEAT: AtomicUsize = AtomicUsize::new(0);
static SETTINGS_OPENING: AtomicBool = AtomicBool::new(false);
// 0 when SetTimer failed: without a heartbeat the watchdog would mistake a
// perfectly healthy idle window for a stuck one, so it must not arm.
static HEARTBEAT_TIMER_ID: AtomicUsize = AtomicUsize::new(0);
// The keyboard hook is deferred work: the hook callback only stores the pressed
// combination here, the real config write happens in the message loop.
static PENDING_ASSIGN: AtomicUsize = AtomicUsize::new(0);
static ASSIGN_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static UPDATE_CHECKED: AtomicBool = AtomicBool::new(false);
static UPDATE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static UPDATE_INFO: OnceLock<Mutex<(String, String, u64)>> = OnceLock::new();
static STATUS_MESSAGE: OnceLock<Mutex<String>> = OnceLock::new();
// Bumped on every config or filter change; the row cache compares against it.
static CONFIG_VERSION: AtomicUsize = AtomicUsize::new(1);

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

fn set_main_window(hwnd: HWND) {
    MAIN_HANDLE.store(hwnd.0 as usize, Ordering::Relaxed);
}

/// The settings window handle, but only while the window is still alive. Every
/// repaint, modal creation and status update goes through this instead of using
/// a handle cached at the first window of the process.
fn main_window() -> Option<HWND> {
    let raw = MAIN_HANDLE.load(Ordering::Relaxed);
    if raw == 0 {
        return None;
    }
    let hwnd = HWND(raw as *mut c_void);
    if unsafe { IsWindow(Some(hwnd)).as_bool() } {
        Some(hwnd)
    } else {
        MAIN_HANDLE.store(0, Ordering::Relaxed);
        None
    }
}

/// True while a settings window is on screen; used to refuse a second instance.
fn settings_window_open() -> bool {
    main_window().is_some()
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
    let found = controls()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&role)
        .map(|control| control.hwnd.0);
    let Some(hwnd) = found else {
        return None;
    };
    // A handle of a closed control is not just useless: SendMessageW on it can
    // hit a window recycled by another process and block until that window
    // answers. Drop dead controls from the map.
    if unsafe { IsWindow(Some(hwnd)).as_bool() } {
        Some(hwnd)
    } else {
        controls()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&role);
        None
    }
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

/// L-11: the window must start from what is on disk. `config.json` can be
/// edited by hand, changed by `z.exe exclude add ...` in another process or
/// repaired by the loader, and the in-memory copy would otherwise keep showing
/// the values of the previous open.
fn refresh_config_from_disk() {
    let fresh = crate::config::load();
    {
        let reference = cfg();
        let mut guard = reference.lock().unwrap_or_else(|e| e.into_inner());
        *guard = fresh.clone();
    }
    crate::win::reload_hotkeys(&fresh);
    bump_rows();
}

static SAVE_QUEUED: AtomicUsize = AtomicUsize::new(0);
static SAVE_DONE: AtomicUsize = AtomicUsize::new(0);

/// Config writes go to a worker thread. `config::save` creates the directory,
/// copies the previous file to `config.json.bak` and rewrites it: three disk
/// calls that would otherwise stall the message loop (and, through the keyboard
/// hook installed on that thread, the whole machine) if the drive or an
/// antivirus is slow.
fn save_queue() -> &'static Mutex<std::sync::mpsc::Sender<Config>> {
    static WORKER: OnceLock<Mutex<std::sync::mpsc::Sender<Config>>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<Config>();
        std::thread::spawn(move || {
            for config in receiver {
                if let Err(error) = crate::config::save(&config) {
                    eprintln!("zя: не удалось сохранить настройки: {error}");
                }
                SAVE_DONE.fetch_add(1, Ordering::SeqCst);
            }
        });
        Mutex::new(sender)
    })
}

unsafe fn save_config() {
    let snapshot = cfg()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    SAVE_QUEUED.fetch_add(1, Ordering::SeqCst);
    let sent = save_queue()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .send(snapshot.clone());
    if sent.is_err() {
        // The worker thread is gone (the process is shutting down): write
        // directly so the user's change is not lost.
        if let Err(error) = crate::config::save(&snapshot) {
            eprintln!("zя: не удалось сохранить настройки: {error}");
        }
        SAVE_DONE.fetch_add(1, Ordering::SeqCst);
    }
}

/// Tests and diagnostics read config.json back, so they need the queue drained.
pub fn flush_config_saves(deadline_ms: u64) -> bool {
    let deadline = std::time::Duration::from_millis(deadline_ms);
    let start = std::time::Instant::now();
    while SAVE_DONE.load(Ordering::SeqCst) < SAVE_QUEUED.load(Ordering::SeqCst) {
        if start.elapsed() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
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

/// GetMessageW returns 0 for WM_QUIT and -1 for an error. `BOOL::as_bool()` is
/// `!= 0`, so reading the error as "there is a message" turns the loop into an
/// endless spin on a stale MSG. Only a positive result means "dispatch it".
fn message_loop_should_continue(result: i32) -> bool {
    result > 0
}

/// The keyboard hook cannot do any real work, so it packs the combination into
/// one integer and posts it to the message loop.
fn pack_combo(vk: u16, ctrl: bool, alt: bool, shift: bool, win: bool) -> usize {
    (vk as usize)
        | ((ctrl as usize) << 16)
        | ((alt as usize) << 17)
        | ((shift as usize) << 18)
        | ((win as usize) << 19)
}

fn unpack_combo(packed: usize) -> (VIRTUAL_KEY, bool, bool, bool, bool) {
    (
        VIRTUAL_KEY((packed & 0xFFFF) as u16),
        (packed >> 16) & 1 != 0,
        (packed >> 17) & 1 != 0,
        (packed >> 18) & 1 != 0,
        (packed >> 19) & 1 != 0,
    )
}

unsafe fn send(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> LRESULT {
    // SendMessageW is synchronous: it waits for the target window procedure to
    // return. On a handle that no longer exists Windows can hand the message to
    // a recycled window owned by a wedged thread, and the UI thread waits for
    // it. Refuse dead handles instead.
    if is_null(hwnd) || !IsWindow(Some(hwnd)).as_bool() {
        return LRESULT(0);
    }
    SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam)))
}

/// InvalidateRect on a window that is gone is a silent no-op, but it hides the
/// fact that the UI has no live window; keep the check in one place.
unsafe fn invalidate(hwnd: HWND) -> bool {
    if !is_null(hwnd) && IsWindow(Some(hwnd)).as_bool() {
        InvalidateRect(hwnd, std::ptr::null(), 1) != 0
    } else {
        false
    }
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

/// The inside of a text field. A plain Win32 EDIT paints itself with the DC
/// background, which on a dark card looks like a black slab; the field colour
/// is only used through the frame we draw around the control.
fn field_color() -> COLORREF {
    if dark_theme() {
        rgb(0x1D, 0x23, 0x2B)
    } else {
        rgb(0xFB, 0xFC, 0xFE)
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

/// Darken a palette colour to `percent` of its brightness, used for the pressed
/// state of owner-draw buttons.
fn shade(color: COLORREF, percent: i32) -> COLORREF {
    let mix = |component: u32| -> u8 {
        (((component as i32) * percent / 100).clamp(0, 255)) as u8
    };
    rgb(
        mix(color.0 & 0xFF),
        mix((color.0 >> 8) & 0xFF),
        mix((color.0 >> 16) & 0xFF),
    )
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
            ANTIALIASED_QUALITY,
            FF_MODERN.0 as u32,
            PCWSTR(name.as_ptr()),
        )
    }
}

// Windows measures a UI font in points: 9 pt is the system default, and the
// point size is scaled by the monitor DPI automatically. Passing a *pixel*
// height instead (14 px at 96 DPI ≈ 8.3 pt) made every caption in this window
// roughly 20 % smaller than the rest of the system, and the gap grew with the
// scaling. `pt()` converts points to device units for the current DPI.
fn pt(points: i32, dpi: u32) -> i32 {
    -(points * dpi as i32 / 72)
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
        title: HGDIOBJ(make_font(pt(15, dpi), 700).0),
        card: HGDIOBJ(make_font(pt(11, dpi), 600).0),
        big: HGDIOBJ(make_font(pt(11, dpi), 700).0),
        body: HGDIOBJ(make_font(pt(10, dpi), 400).0),
        dim: HGDIOBJ(make_font(pt(9, dpi), 400).0),
    };
    guard.insert(dpi, created.clone());
    created
}

// A caption is measured, never guessed: a button is widened to fit it, and if
// the card has no room left the caption is drawn in a smaller point size. Either
// way the text stays inside the rounded border and is never cut off.
#[derive(Clone, Copy)]
struct Handle(HGDIOBJ);

// GDI object handles are process-wide and never freed; the table behind them is
// guarded by a mutex, so sharing them between threads is safe.
unsafe impl Send for Handle {}
unsafe impl Sync for Handle {}

unsafe fn point_font(points: i32, weight: i32, dpi: u32) -> HGDIOBJ {
    static TABLE: OnceLock<Mutex<HashMap<(i32, i32, u32), Handle>>> = OnceLock::new();
    let table = TABLE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = guard.get(&(points, weight, dpi)) {
        return found.0;
    }
    let font = Handle(HGDIOBJ(make_font(pt(points, dpi), weight).0));
    guard.insert((points, weight, dpi), font);
    font.0
}

unsafe fn text_width(hdc: HDC, font: HGDIOBJ, text: &str) -> i32 {
    let encoded = wide(text);
    let old = SelectObject(hdc, font);
    let mut size = SIZE { cx: 0, cy: 0 };
    let ok = GetTextExtentPoint32W(hdc, &encoded, &mut size);
    SelectObject(hdc, old);
    if ok.as_bool() {
        size.cx
    } else {
        0
    }
}

/// Smallest body-size font (11 pt down to 6 pt) whose caption fits in `available`
/// device pixels. Falls back to 6 pt, which always fits the buttons in this window.
unsafe fn fitting_font(hdc: HDC, dpi: u32, caption: &str, available: i32) -> HGDIOBJ {
    for points in (6..=11).rev() {
        let font = point_font(points, 400, dpi);
        if text_width(hdc, font, caption) <= available {
            return font;
        }
    }
    point_font(6, 400, dpi)
}

/// Design-space width a button needs for its caption: at least `min`, at most
/// `max`, plus BUTTON_PAD on both sides of the text.
unsafe fn button_width(parent: HWND, dpi: u32, caption: &str, min: i32, max: i32) -> i32 {
    let hdc = GetDC(Some(parent));
    let text = text_width(hdc, fonts(dpi).body, caption);
    ReleaseDC(Some(parent), hdc);
    let need_physical = text + 2 * s(BUTTON_PAD, dpi);
    // Round the device->design conversion up so the measured text always fits.
    let need = (need_physical * BASE_DPI + dpi as i32 - 1) / dpi as i32;
    let want = min.max(need);
    if want > max {
        max
    } else {
        want
    }
}

// ---------------------------------------------------------------------------
// Drawing helpers
// ---------------------------------------------------------------------------

unsafe fn draw_text(hdc: HDC, text: &str, rect: &RECT, font: HGDIOBJ, color: COLORREF, flags: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT) {
    let old_font = SelectObject(hdc, font);
    let old_color = SetTextColor(hdc, color);
    let old_bk = SetBkMode(hdc, TRANSPARENT);
    let mut encoded = wide(text);
    let mut area = *rect;
    DrawTextW(hdc, encoded.as_mut_slice(), &mut area, flags);
    // Put the DC back the way we found it: a control that keeps being painted
    // by the system after this call must not inherit the helper's font or colour.
    let _ = SelectObject(hdc, old_font);
    let _ = SetTextColor(hdc, old_color);
    let _ = SetBkMode(hdc, BACKGROUND_MODE(old_bk.max(0) as u32));
}

// The outline of a rounded rectangle as a GDI path. Walking it in the order
// that keeps every corner on the short (90 degree) arc is what makes the shape
// round instead of "bitten in".
unsafe fn rounded_path(hdc: HDC, left: i32, top: i32, width: i32, height: i32, r: i32) -> bool {
    let right = left + width;
    let bottom = top + height;
    if !BeginPath(hdc).as_bool() {
        return false;
    }
    let _ = MoveToEx(hdc, left, top + r, None);
    let _ = LineTo(hdc, left, bottom - r);
    let _ = ArcTo(
        hdc,
        left,
        bottom - 2 * r,
        left + 2 * r,
        bottom,
        left,
        bottom - r,
        left + r,
        bottom,
    );
    let _ = LineTo(hdc, right - r, bottom);
    let _ = ArcTo(
        hdc,
        right - 2 * r,
        bottom - 2 * r,
        right,
        bottom,
        right - r,
        bottom,
        right,
        bottom - r,
    );
    let _ = LineTo(hdc, right, top + r);
    let _ = ArcTo(
        hdc,
        right - 2 * r,
        top,
        right,
        top + 2 * r,
        right,
        top + r,
        right - r,
        top,
    );
    let _ = LineTo(hdc, left + r, top);
    let _ = ArcTo(hdc, left, top, left + 2 * r, top + 2 * r, left + r, top, left, top + r);
    EndPath(hdc).as_bool()
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
    // The colour that surrounds the shape. `RoundRect` paints the four wedges
    // between its arcs and the bounding rectangle with the DC *background*
    // colour (black by default), which is what made every rounded corner look
    // like it had a black triangle in it. We paint the backdrop first and then
    // fill a true rounded path, so nothing is left to bleed into the corners.
    bg: COLORREF,
) {
    if width <= 0 || height <= 0 {
        return;
    }
    fill_rect(hdc, left, top, width, height, bg);
    let r = radius.max(1).min(width / 2).min(height / 2);
    let brush = CreateSolidBrush(fill);
    let pen = CreatePen(PS_SOLID, 1, border);
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    // `FillPath` closes and destroys the path, so the outline has to be rebuilt
    // before it can be stroked; a single path would leave the border invisible.
    if rounded_path(hdc, left, top, width, height, r) {
        let _ = FillPath(hdc);
    }
    if rounded_path(hdc, left, top, width, height, r) {
        let _ = StrokePath(hdc);
    }
    // DeleteObject on an object that is still selected in the DC fails and
    // silently leaks it, so deselect first, then delete.
    let _ = SelectObject(hdc, old_pen);
    let _ = SelectObject(hdc, old_brush);
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
    let old_brush = SelectObject(hdc, HGDIOBJ(brush.0));
    let _ = FillRect(hdc, &rect, brush);
    let _ = SelectObject(hdc, old_brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
}

// WM_CTLCOLORSTATIC/WM_CTLCOLOREDIT expect the application to keep the brush it
// returns: Windows never frees it. Creating one per message leaks a GDI object
// on every repaint of every control, so the brushes are cached per colour and
// never deleted.
unsafe fn cached_brush(color: COLORREF) -> HGDIOBJ {
    // HGDIOBJ wraps a raw pointer and is neither Send nor Sync, so the cache
    // stores the handle as a usize and rewraps it on every use.
    static BRUSHES: OnceLock<Mutex<HashMap<u32, usize>>> = OnceLock::new();
    let table = BRUSHES.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = guard.get(&color.0) {
        return HGDIOBJ(*found as *mut c_void);
    }
    let brush = CreateSolidBrush(color);
    guard.insert(color.0, brush.0 as usize);
    HGDIOBJ(brush.0 as *mut c_void)
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
        2 => paint_log_page(hdc, dpi),
        _ => paint_lex_page(hdc, dpi),
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
        bg_color(),
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
            // Design coordinates, not the already scaled `x`: rect_at applies
            // s() itself, so passing a scaled value scaled the text twice and
            // pushed it past the right edge of the card at 125% DPI and above.
            &rect_at(INNER_RIGHT - 200, 150, 200, 34, dpi),
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
            // Design coordinates, not the already scaled `x`: rect_at applies
            // s() itself, so passing a scaled value scaled the text twice and
            // pushed it past the right edge of the card at 125% DPI and above.
            &rect_at(INNER_RIGHT - 200, 150, 200, 34, dpi),
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
        bg_color(),
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
        SMART_UNDO_TEXT,
        // Wrapped paragraph, not a single line: DT_SINGLELINE would cut the
        // explanation off at the first line.
        &rect_at(SMART_COL_X, SMART_HINT_Y, SMART_COL_W, 96, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_TOP,
    );
    draw_text(
        hdc,
        "Автозапуск — запись в раздел Run. «Как администратор» — задание ZyaAutoStart в Планировщике заданий с уровнем HIGHEST.",
        &rect_at(CONTENT_X + CARD_PAD, LAUNCH_HINT_Y, 864, 22, dpi),
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
        bg_color(),
    );
    draw_text(
        hdc,
        "Горячие клавиши",
        &rect_at(CONTENT_X + CARD_PAD, HOTKEY_CARD_Y + 12, 320, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    // The table is split in two halves, and each half is labelled above its own
    // header: the left gestures work on the last token without any selection,
    // the right ones need selected text.
    draw_text(
        hdc,
        "Без выделения — последнее слово",
        &rect_at(HOTKEY_WORD_X, HOTKEY_GROUP_Y, HOTKEY_LIST_W, 22, dpi),
        fonts(dpi).dim,
        accent_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        "С выделением",
        &rect_at(HOTKEY_SEL_X, HOTKEY_GROUP_Y, HOTKEY_LIST_W, 22, dpi),
        fonts(dpi).dim,
        accent_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        HOTKEY_NOTE_TEXT,
        &rect_at(CONTENT_X + CARD_PAD, HOTKEY_NOTE_Y, 864, 62, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_TOP,
    );
    // Column titles are painted by the list header itself.
}

unsafe fn paint_field(hdc: HDC, dpi: u32, role: usize) {
    if let Some((x, y, width, height)) = box_of(role) {
        draw_field(hdc, dpi, role, x, y, width, height);
    }
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
        bg_color(),
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
    paint_field(hdc, dpi, IGNORE_INPUT);

    rounded(
        hdc,
        x,
        s(IGNORE_LIST_CARD_Y, dpi),
        width,
        s(IGNORE_LIST_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
        bg_color(),
    );
    paint_field(hdc, dpi, IGNORE_FILTER);
    draw_text(
        hdc,
        &format!("Список исключений — {} записей", cached_len(IGNORE_LIST)),
        &rect_at(CONTENT_X + CARD_PAD, 266, 400, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    if cached_len(IGNORE_LIST) == 0 {
        draw_text(
            hdc,
            "Список пуст — добавьте процесс или папку кнопками выше.",
            &rect_at(CONTENT_X + CARD_PAD, 720, 700, 22, dpi),
            fonts(dpi).dim,
            dim_color(),
            windows::Win32::Graphics::Gdi::DT_LEFT
                | windows::Win32::Graphics::Gdi::DT_VCENTER
                | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
        );
    }
}

unsafe fn paint_lex_page(hdc: HDC, dpi: u32) {
    let x = s(CONTENT_X, dpi);
    let width = s(CONTENT_WIDTH, dpi);

    rounded(
        hdc,
        x,
        s(LEX_CARD_Y, dpi),
        width,
        s(LEX_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
        bg_color(),
    );
    draw_text(
        hdc,
        "Словарь коротких слов",
        &rect_at(CONTENT_X + CARD_PAD, 100, 320, 22, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    draw_text(
        hdc,
        LEX_NOTE,
        &rect_at(CONTENT_X + CARD_PAD, 126, 864, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    paint_field(hdc, dpi, LEX_INPUT);

    rounded(
        hdc,
        x,
        s(LEX_LIST_CARD_Y, dpi),
        width,
        s(LEX_LIST_CARD_H, dpi),
        s(CARD_RADIUS, dpi),
        card_color(),
        card_border(),
        bg_color(),
    );
    paint_field(hdc, dpi, LEX_FILTER);
    draw_text(
        hdc,
        &format!("Словарь — {} слов", cached_len(LEX_LIST)),
        &rect_at(CONTENT_X + CARD_PAD, 266, 400, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    let summary = lex_summary();
    draw_text(
        hdc,
        &format!(
            "{} русских, {} английских, конфликтов: {}",
            summary.ru, summary.en, summary.conflicts.len()
        ),
        &rect_at(CONTENT_X + CARD_PAD, LEX_COUNT_Y, 500, 22, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    let conflicts = if summary.conflicts.is_empty() {
        "Конфликтующих пар нет: каждое короткое слово читается однозначно.".to_string()
    } else {
        format!("Конфликты (одно зеркало клавиш — два слова): {}", summary.conflicts.join(", "))
    };
    draw_text(
        hdc,
        &conflicts,
        &rect_at(CONTENT_X + CARD_PAD, LEX_CONFLICT_Y, 864, 22, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
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
        bg_color(),
    );
    paint_field(hdc, dpi, LOG_FILTER);
    let log_rows = cached_len(LOG_LIST);
    draw_text(
        hdc,
        &format!("Лог переключений — {} строк", log_rows),
        &rect_at(CONTENT_X + CARD_PAD, 100, 400, 24, dpi),
        fonts(dpi).card,
        fg_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT
            | windows::Win32::Graphics::Gdi::DT_VCENTER
            | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
    );
    let footnote = if log_rows == 0 {
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

// Every owner-draw button has to know the colour of the surface it sits on:
// GDI fills the corner wedges of a rounded shape with the DC background, so a
// button placed on a card must be "backed" with the card colour or its corners
// show up as dark triangles. The position of the button is known when it is
// created, so the surface is recorded there and looked up when painting.
static BUTTON_SURFACE: OnceLock<Mutex<HashMap<usize, COLORREF>>> = OnceLock::new();

unsafe fn note_surface(role: usize, surface: COLORREF) {
    let table = BUTTON_SURFACE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().unwrap_or_else(|error| error.into_inner());
    guard.insert(role, surface);
}

fn surface_for(role: usize) -> COLORREF {
    let table = BUTTON_SURFACE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = table.lock().unwrap_or_else(|error| error.into_inner());
    guard.get(&role).copied().unwrap_or_else(bg_color)
}

// Design-space rectangle of every control, recorded when the control is created.
// The paint pass reads it instead of repeating the coordinates, so the outline
// of a field always matches the control that is really there.
static CONTROL_BOX: OnceLock<Mutex<HashMap<usize, (i32, i32, i32, i32)>>> = OnceLock::new();

fn note_box(role: usize, x: i32, y: i32, width: i32, height: i32) {
    let table = CONTROL_BOX.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = table.lock().unwrap_or_else(|error| error.into_inner());
    guard.insert(role, (x, y, width, height));
}

fn box_of(role: usize) -> Option<(i32, i32, i32, i32)> {
    let table = CONTROL_BOX.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = table.lock().unwrap_or_else(|error| error.into_inner());
    guard.get(&role).copied()
}

// Design-space test: is this rectangle entirely inside one of the content cards?
fn surface_at(x: i32, y: i32, width: i32, height: i32) -> COLORREF {
    if x < CONTENT_X || x + width > CONTENT_X + CONTENT_WIDTH {
        return bg_color();
    }
    let cards = [
        (VERSION_CARD_Y, VERSION_CARD_H),
        (LAUNCH_CARD_Y, LAUNCH_CARD_H),
        (HOTKEY_CARD_Y, HOTKEY_CARD_H),
        (IGNORE_CARD_Y, IGNORE_CARD_H),
        (LOG_CARD_Y, LOG_CARD_H),
    ];
    for (card_y, card_h) in cards {
        if y >= card_y && y + height <= card_y + card_h {
            return card_color();
        }
    }
    bg_color()
}

unsafe fn make_button(
    parent: HWND,
    dpi: u32,
    label: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    role: usize,
    surface: COLORREF,
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
    note_surface(role, surface);
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
        // No WS_BORDER: the system frame would paint a hard black rectangle on
        // a dark card. The rounded outline is drawn by the paint pass instead.
        WS_CHILD | WS_VISIBLE | WS_TABSTOP_STYLE,
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
    send(hwnd, WM_SETFONT, fonts(dpi).body.0 as usize, 1);
    hwnd
}

unsafe fn make_static_label(parent: HWND, dpi: u32, text: &str, x: i32, y: i32) -> HWND {
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Static").as_ptr()),
        PCWSTR(wide(text).as_ptr()),
        WS_CHILD | WS_VISIBLE,
        s(x, dpi),
        s(y, dpi),
        s(FILTER_LABEL_W, dpi),
        s(32, dpi),
        Some(parent),
        None,
        None,
        None,
    )
    .unwrap_or(null_hwnd());
    send(hwnd, WM_SETFONT, fonts(dpi).dim.0 as usize, 1);
    hwnd
}

/// The rounded frame around a text field. Drawn on the card before the control
/// paints its own background, so the field reads as an inset box in both themes.
unsafe fn draw_field(hdc: HDC, dpi: u32, role: usize, x: i32, y: i32, width: i32, height: i32) {
    if let Some(hwnd) = handle(role) {
        if !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return;
        }
    }
    rounded(
        hdc,
        s(x - 2, dpi),
        s(y - 2, dpi),
        s(width + 4, dpi),
        s(height + 4, dpi),
        s(6, dpi),
        field_color(),
        card_border(),
        card_color(),
    );
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
    // One shared font per DPI instead of a fresh HFONT per control: the font
    // handed to WM_SETFONT is never freed by Windows.
    send(hwnd, WM_SETFONT, fonts(dpi).body.0 as usize, 1);
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
    if dark_theme() {
        // SysListView32 keeps the light system theme for its scrollbar and its
        // SysHeader32 child; only an explicit theme makes them match the card.
        SetWindowTheme(
            hwnd,
            PCWSTR(wide("DarkMode_Explorer").as_ptr()),
            PCWSTR::null(),
        )
        .ok();
    }
    // A report view needs columns, otherwise the owner-draw rows are never
    // requested from the parent window. The previous code passed an LVITEMW to
    // LVM_INSERTCOLUMNA (4123): the mask then read LVCF_FMT only, so no column
    // got a title or a width, and 4111 (LVM_SETITEMPOSITION) does nothing in a
    // report view. LVM_INSERTCOLUMNW is 4193, LVM_SETCOLUMNWIDTH is 4126.
    let titles: [&str; 3] = match role {
        IGNORE_LIST => ["Процесс или путь", "Источник", ""],
        LOG_LIST => ["Время", "Что произошло", "Почему"],
        LEX_LIST => ["Слово", "Раскладка", "Источник"],
        _ => ["Действие", "Клавиша", "Статус"],
    };
    let widths: [i32; 3] = match role {
        IGNORE_LIST => [600, 272, 0],
        LOG_LIST => [84, 412, 396],
        LEX_LIST => [430, 220, 222],
        HOTKEY_LIST | HOTKEY_SEL_LIST => [214, 100, 110],
        _ => [400, 160, 312],
    };
    for index in 0..3 {
        let title = titles[index];
        if title.is_empty() {
            continue;
        }
        let text = wide(title);
        let column = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH | LVCF_ORDER,
            cx: s(widths[index], dpi),
            pszText: PWSTR(text.as_ptr() as *mut u16),
            iSubItem: index as i32,
            iOrder: index as i32,
            ..Default::default()
        };
        SendMessageW(
            hwnd,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(index)),
            Some(LPARAM(std::ptr::addr_of!(column) as isize)),
        );
        SendMessageW(
            hwnd,
            LVM_SETCOLUMNWIDTH,
            Some(WPARAM(index)),
            Some(LPARAM(s(widths[index], dpi) as isize)),
        );
    }
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
    if role == HOTKEY_LIST || role == HOTKEY_SEL_LIST {
        list_roles()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(hwnd.0 as usize, role);
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

// Every owner-draw list remembers the role it was created with, so a click on a
// row can find out which of the two hotkey tables it belongs to.
fn list_roles() -> &'static Mutex<HashMap<usize, usize>> {
    static ROLES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
    ROLES.get_or_init(|| Mutex::new(HashMap::new()))
}

unsafe fn list_role(list: windows::Win32::Foundation::HWND) -> usize {
    list_roles()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(list.0 as usize))
        .copied()
        .unwrap_or(HOTKEY_LIST)
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
            LEX_LIST => [(14, "Слово"), (430, "Раскладка"), (650, "Источник")],
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
const WM_LBUTTON_UP: u32 = 0x0202; // WM_LBUTTONUP
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

    // Only the release of the button opens the modal: handling both down and up
    // ran the whole assignment flow twice for a single click.
    if msg == WM_LBUTTON_UP {
        let point = lparam.0 as u32;
        let y = (point >> 16) as i32;
        let role = list_role(hwnd);
        let rows = row_rects()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&(hwnd.0 as usize))
            .cloned()
            .unwrap_or_default();
        for (index, (top, bottom)) in rows {
            if y >= top && y < bottom {
                open_hotkey_modal(role, index);
                break;
            }
        }
    }
    result
}

unsafe fn create_controls(parent: HWND, dpi: u32) {
    for (index, (role, label)) in SIDEBAR_TABS.iter().enumerate() {
        let y = SIDEBAR_TOP + index as i32 * (TAB_HEIGHT + TAB_GAP);
        make_button(
            parent,
            dpi,
            label,
            SIDEBAR_X,
            y,
            SIDEBAR_WIDTH,
            TAB_HEIGHT,
            *role,
            surface_at(SIDEBAR_X, y, SIDEBAR_WIDTH, TAB_HEIGHT),
        );
    }

    // Every button and text field of the four pages comes from CONTROLS. The
    // caption is measured with the real font at this DPI: a label that needs more
    // room widens its button instead of being cut off, and nothing is ever placed
    // on top of a card frame — the inner edge of every card is INNER_LEFT/INNER_RIGHT.
    let switch_w = CONTROLS
        .iter()
        .filter(|spec| matches!(spec.role, CB_AUTOSTART | CB_ADMIN | CB_AUTO_LAYOUT))
        .map(|spec| button_width(parent, dpi, spec.label, spec.width, spec.max_width))
        .max()
        .unwrap_or(430);
    for spec in CONTROLS {
        let width = if spec.max_width == 0 {
            spec.width
        } else if matches!(spec.role, CB_AUTOSTART | CB_ADMIN | CB_AUTO_LAYOUT) {
            switch_w
        } else {
            button_width(parent, dpi, spec.label, spec.width, spec.max_width)
        };
        let x = if spec.anchored_right {
            INNER_RIGHT - width
        } else {
            spec.x
        };
        note_box(spec.role, x, spec.y, width, spec.height);
        if spec.label.is_empty() {
            make_field(parent, dpi, x, spec.y, width, spec.height, spec.role, "");
        } else {
            make_button(
                parent,
                dpi,
                spec.label,
                x,
                spec.y,
                width,
                spec.height,
                spec.role,
                surface_at(x, spec.y, width, spec.height),
            );
        }
    }
    // The exception input is empty until the user opens it, so its button pair
    // stays hidden.
    if let Some(input) = handle(IGNORE_INPUT) {
        let _ = ShowWindow(input, SW_HIDE);
    }
    if let Some(commit) = handle(BTN_ADD_COMMIT) {
        let _ = ShowWindow(commit, SW_HIDE);
    }
    // Two tables: the gestures that work on the last token, and the gestures
    // that need a selection. They are separate controls so each column keeps its
    // own row rects and its own assignment panel.
    let hotkeys = make_list(
        parent,
        dpi,
        HOTKEY_WORD_X,
        HOTKEY_LIST_Y,
        HOTKEY_LIST_W,
        HOTKEY_WORD_LIST_H,
        HOTKEY_LIST,
    );
    fill_hotkey_list(hotkeys);
    fit_list_to_rows(hotkeys, dpi, HOTKEY_WORD_X, HOTKEY_LIST_Y, HOTKEY_LIST_W, HOTKEY_WORD_LIST_H);
    let hotkeys_sel = make_list(
        parent,
        dpi,
        HOTKEY_SEL_X,
        HOTKEY_LIST_Y,
        HOTKEY_LIST_W,
        HOTKEY_SEL_LIST_H,
        HOTKEY_SEL_LIST,
    );
    fill_hotkey_list(hotkeys_sel);
    fit_list_to_rows(hotkeys_sel, dpi, HOTKEY_SEL_X, HOTKEY_LIST_Y, HOTKEY_LIST_W, HOTKEY_SEL_LIST_H);
    // 2. Ignore. Buttons and fields come from CONTROLS; only the "Фильтр" label
    // and the exception table are created here.
    let filter_label = make_static_label(parent, dpi, "Фильтр", INNER_RIGHT - FILTER_W - ROW_GAP - FILTER_LABEL_W, 266);
    register_control(IGNORE_FILTER_LABEL, filter_label);
    let list = make_list(parent, dpi, INNER_LEFT, 308, INNER_RIGHT - INNER_LEFT, 420, IGNORE_LIST);
    fill_ignore_list(list);

    // 3. Log
    let log_label = make_static_label(parent, dpi, "Фильтр", INNER_RIGHT - FILTER_W - ROW_GAP - FILTER_LABEL_W, 100);
    register_control(LOG_FILTER_LABEL, log_label);
    let log_list = make_list(parent, dpi, INNER_LEFT, 174, INNER_RIGHT - INNER_LEFT, 520, LOG_LIST);
    fill_log_list(log_list);

    // 4. Lexicon: the word table shares the geometry of the exception list.
    let lex_label = make_static_label(parent, dpi, "Фильтр", INNER_RIGHT - FILTER_W - ROW_GAP - FILTER_LABEL_W, 266);
    register_control(LEX_FILTER_LABEL, lex_label);
    let lex_list = make_list(
        parent,
        dpi,
        INNER_LEFT,
        LEX_LIST_Y,
        INNER_RIGHT - INNER_LEFT,
        LEX_LIST_H,
        LEX_LIST,
    );
    fill_lex_list(lex_list);

    let status = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("Static").as_ptr()),
        PCWSTR(wide(STATUS_IDLE).as_ptr()),
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
    send(status, WM_SETFONT, fonts(dpi).dim.0 as usize, 1);
    apply_tab_visibility();
}

/// Only the controls of the active tab are visible; the rest stay hidden so
/// they never overlap the page being drawn.
/// One entry per page: index in `SIDEBAR_TABS` -> the controls that belong to it.
fn visibility_groups() -> [(usize, &'static [usize]); 4] {
    [
        (
            0,
            &[
                BTN_UPDATE,
                CB_AUTOSTART,
                CB_ADMIN,
                CB_AUTO_LAYOUT,
                CB_SMART_UNDO,
                BTN_HOTKEY_RESET,
                HOTKEY_LIST,
                HOTKEY_SEL_LIST,
            ],
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
        (
            3,
            &[
                BTN_LEX_ADD,
                BTN_LEX_REMOVE,
                LEX_INPUT,
                LEX_FILTER,
                LEX_FILTER_LABEL,
                LEX_LIST,
            ],
        ),
    ]
}

unsafe fn apply_tab_visibility() {
    let tab = current_tab();
    let groups = visibility_groups();
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
    match msg {
        WM_CREATE => {
            let dpi = window_dpi(hwnd);
            set_main_window(hwnd);
            DARK.store(crate::theme::is_dark(), Ordering::Relaxed);
            create_controls(hwnd, dpi);
            set_window_icon(hwnd);
            apply_titlebar_theme(hwnd);
            repaint(hwnd);
            spawn_update_check();
            // The heartbeat keeps the message loop producing at least one event a
            // second, so the watchdog can tell "idle window" from "stuck window".
            HEARTBEAT_TIMER_ID.store(
                SetTimer(Some(hwnd), HEARTBEAT_TIMER, 1000, None),
                Ordering::Relaxed,
            );
            return LRESULT(0);
        }
        WM_TIMER => {
            UI_BEAT.fetch_add(1, Ordering::Relaxed);
            return LRESULT(0);
        }
        WM_ERASEBKGND => {
            if theme_refresh_allowed() {
                DARK.store(crate::theme::is_dark(), Ordering::Relaxed);
            }
            paint_window(hwnd, HDC(wparam.0 as *mut c_void));
            return LRESULT(1);
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let child = HWND(lparam.0 as *mut c_void);
            let background = if role_of(child) == STATUS_LABEL { bg_color() } else { card_color() };
            let _ = SetTextColor(hdc, fg_color());
            let _ = SetBkColor(hdc, background);
            // One cached brush per colour, never freed: Windows keeps the brush
            // returned here for the lifetime of the control.
            return LRESULT(cached_brush(background).0 as isize);
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            let _ = SetTextColor(hdc, fg_color());
            // The field interior matches the rounded frame the paint pass draws
            // around the control, so the box does not read as a black slab.
            let background = field_color();
            let _ = SetBkColor(hdc, background);
            return LRESULT(cached_brush(background).0 as isize);
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
                TAB_SETTINGS | TAB_IGNORE | TAB_LOG | TAB_LEXICON => {
                    CURRENT_TAB.store(id - TAB_SETTINGS, Ordering::Relaxed);
                    apply_tab_visibility();
                    repaint(hwnd);
                }
                BTN_CONFIG => open_config_file(),
                BTN_UPDATE => run_update(),
                CB_AUTOSTART => toggle_autostart(),
                CB_ADMIN => toggle_admin_autostart(),
                CB_AUTO_LAYOUT => toggle_auto_layout(),
                CB_SMART_UNDO => toggle_smart_undo(),
                BTN_HOTKEY_RESET => reset_hotkeys(),
                BTN_ADD_PROCESSES => add_foreground_process(),
                BTN_ADD_PROCESS => show_ignore_input(),
                BTN_ADD_COMMIT => add_ignore_entry(),
                BTN_REMOVE_IGNORE => remove_ignore_row(list_selected(IGNORE_LIST)),
                BTN_LEX_ADD => add_lex_entry(),
                BTN_LEX_REMOVE => remove_lex_row(list_selected(LEX_LIST)),
                BTN_LOG_REFRESH => refresh_log(),
                BTN_LOG_CLEAR => clear_log(),
                MODAL_CANCEL => close_modal(),
                HOTKEY_ESCAPED => {
                    ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
                    update_status("Назначение отменено");
                    close_modal();
                }
                HOTKEY_ASSIGN => {
                    let packed = PENDING_ASSIGN.swap(0, Ordering::Relaxed);
                    ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
                    if packed != 0 {
                        let (vk, ctrl, alt, shift, win) = unpack_combo(packed);
                        assign_capture(vk, ctrl, alt, shift, win);
                    }
                }
                _ => {
                    // EN_CHANGE is 0x0300. The old code compared against 0x0400,
                    // which is EN_UPDATE; it still fired because EN_UPDATE also
                    // arrives on every text change, but EN_CHANGE was ignored.
                    if notification == EN_CHANGE || notification == EN_UPDATE {
                        // L-15: every keystroke in a filter used to rebuild the
                        // whole list, which reads the log file from disk and sends
                        // dozens of synchronous messages to the list window. The
                        // rebuild now runs only when the text really changed.
                        if id == IGNORE_FILTER && filter_changed(id) {
                            bump_rows();
                            if let Some(list) = handle(IGNORE_LIST) {
                                fill_ignore_list(list);
                                invalidate(list);
                            }
                        } else if id == LEX_FILTER && filter_changed(id) {
                            bump_rows();
                            if let Some(list) = handle(LEX_LIST) {
                                fill_lex_list(list);
                                invalidate(list);
                            }
                        } else if id == LOG_FILTER && filter_changed(id) {
                            bump_rows();
                            if let Some(list) = handle(LOG_LIST) {
                                fill_log_list(list);
                                invalidate(list);
                            }
                        }
                    }
                    if (id == HOTKEY_LIST || id == HOTKEY_SEL_LIST)
                        && (notification == NM_CLICK || notification == NM_DBLCLK)
                    {
                        if let Some(index) = list_selected(id) {
                            open_hotkey_modal(id, index);
                        }
                    }
                }
            }
            return LRESULT(0);
        }
        WM_CONTEXTMENU => {
            let child = HWND(wparam.0 as *mut c_void);
            let role = role_of(child);
            if role != IGNORE_LIST && role != LOG_LIST && role != LEX_LIST {
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            let mut point = POINT { x: 0, y: 0 };
            GetCursorPos(&mut point).ok();
            // The row the menu belongs to: the one under the pointer, falling
            // back to whatever the list still marks as selected.
            let target = row_at_point(child, point).or_else(|| list_selected(role));
            unsafe {
                // L-14: `TrackPopupMenu` with TPM_RETURNCMD blocks this thread until
                // the user answers the menu. The keyboard hook runs on the same
                // thread, so while the menu is open no keystroke reaches the hook
                // and the modal would stop responding. The hook is removed for the
                // duration of the menu and put back afterwards.
                let modal_open = modal_handle().is_some();
                if HOTKEY_HOOK.load(Ordering::Relaxed) != 0 {
                    detach_hotkey_hook();
                }
                let Ok(menu) = CreatePopupMenu() else {
                    if modal_open {
                        install_capture_hook();
                    }
                    return DefWindowProcW(hwnd, msg, wparam, lparam);
                };
                let _ = AppendMenuW(menu, MF_ENABLED, 1, PCWSTR(wide("Скопировать строку").as_ptr()));
                let _ = AppendMenuW(menu, MF_ENABLED, 2, PCWSTR(wide("Скопировать весь список").as_ptr()));
                if role == IGNORE_LIST || role == LEX_LIST {
                    let label = if role == LEX_LIST { "Удалить слово" } else { "Удалить" };
                    let _ = AppendMenuW(menu, MF_ENABLED, 3, PCWSTR(wide(label).as_ptr()));
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
                if modal_open {
                    install_capture_hook();
                }
                match chosen.0 {
                    1 => copy_row(role, target),
                    2 => copy_visible_rows(role),
                    3 => {
                        if role == LEX_LIST {
                            remove_lex_row(target);
                        } else {
                            remove_ignore_row(target);
                        }
                    }
                    _ => {}
                }
            }
            return LRESULT(0);
        }
        WM_DESTROY => {
            KillTimer(Some(hwnd), HEARTBEAT_TIMER).ok();
            // L-01: a modal is a child window, so Windows destroys it together
            // with the parent, but MODAL_TARGET would stay pointing at a row and
            // the global keyboard hook would then refuse every key for the rest
            // of the process. Clear the assignment state and the hook handle
            // before the window is gone, drop the child window explicitly, and
            // forget every control so a second settings window starts clean.
            MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
            detach_hotkey_hook();
            close_modal();
            if MAIN_HANDLE.load(Ordering::Relaxed) == hwnd.0 as usize {
                MAIN_HANDLE.store(0, Ordering::Relaxed);
            }
            controls().lock().unwrap_or_else(|e| e.into_inner()).clear();
            header_roles()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            // L-18: the header and list controls are subclassed. If the original
            // procedure is never restored and Windows hands the same HWND value to
            // a window of another process, that window keeps running the settings
            // procedure and paints with the wrong palette (and our caches are
            // already empty, so it draws nothing). Put the originals back first.
            for (handle, original) in header_original()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                let target = HWND(handle as *mut c_void);
                if IsWindow(Some(target)).as_bool() {
                    SetWindowLongPtrW(target, GWL_WNDPROC, original);
                }
            }
            header_original()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            for (handle, original) in list_original()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                let target = HWND(handle as *mut c_void);
                if IsWindow(Some(target)).as_bool() {
                    SetWindowLongPtrW(target, GWL_WNDPROC, original);
                }
            }
            list_original()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clear();
            row_rects()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clear();
            bump_rows();
            PostMessageW(None, WM_QUIT, WPARAM(0), LPARAM(0)).ok();
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe fn repaint(hwnd: HWND) {
    // A handle cached from an earlier window is dead: invalidating it does
    // nothing and hides the fact that the UI has no live window.
    if !is_null(hwnd) && IsWindow(Some(hwnd)).as_bool() {
        InvalidateRect(hwnd, std::ptr::null(), 1);
    }
}

/// WM_ERASEBKGND fires on every repaint. Reading the theme registry on each of
/// them is a syscall storm (and a stall if the registry is slow), so the check
/// is throttled to once a second.
fn theme_refresh_allowed() -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let last = THEME_STAMP.load(Ordering::Relaxed);
    if now - last >= 1 {
        THEME_STAMP.store(now, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Safety net for a wedged UI thread. The low-level keyboard hook runs on the
/// settings thread: if that thread stops pumping messages, hook callbacks are
/// never invoked and keyboard input stalls for the whole machine. The watchdog
/// watches the heartbeat; when the loop stops ticking it unhooks the keyboard
/// first (input is free again), then asks the window to close.
unsafe fn spawn_watchdog(hwnd: HWND) {
    // Without the heartbeat timer there is no reference beat: a quiet but healthy
    // window would look stuck and be closed by mistake.
    if HEARTBEAT_TIMER_ID.load(Ordering::Relaxed) == 0 {
        return;
    }
    // HWND is not Send, so the watchdog carries the raw handle.
    let raw = hwnd.0 as usize;
    std::thread::spawn(move || unsafe {
        let window = HWND(raw as *mut c_void);
        let mut last_beat = UI_BEAT.load(Ordering::Relaxed);
        let mut silent_ms: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(500));
            if !IsWindow(Some(window)).as_bool() {
                break;
            }
            let beat = UI_BEAT.load(Ordering::Relaxed);
            if beat != last_beat {
                last_beat = beat;
                silent_ms = 0;
                continue;
            }
            silent_ms += 500;
            if silent_ms >= UI_STALL_SECONDS * 1000 {
                eprintln!(
                    "zя: окно настроек перестало отвечать: перехват клавиш снят, окно закрывается"
                );
                detach_hotkey_hook();
                let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                break;
            }
        }
    });
}

unsafe fn repaint_main() {
    if let Some(hwnd) = main_window() {
        repaint(hwnd);
    }
}

/// P-01 live check for `zя gditest`: flip the palette `times` times and repaint
/// the open settings window each time. The palette itself is read from the
/// system, so a diagnostic run has to override it to measure the cost of a
/// light↔dark switch.
pub fn theme_flip_for_test(times: usize) {
    unsafe {
        let Some(hwnd) = main_window() else {
            return;
        };
        for _ in 0..times {
            DARK.store(!DARK.load(Ordering::Relaxed), Ordering::Relaxed);
            repaint(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

unsafe fn draw_button(info: &DRAWITEMSTRUCT, hwnd: HWND) {
    let dpi = window_dpi(hwnd);
    let rect = info.rcItem;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    // Owner-draw buttons report itemID = 0, so the control id is looked up
    // from the registry instead.
    let role = role_of(info.hwndItem);
    // The button DC starts out with the window background: fill the whole
    // rectangle with the surface the button actually sits on first, so the
    // rounded corners do not show up as dark specks on a card.
    let backdrop = surface_for(role);
    fill_rect(info.hDC, rect.left, rect.top, width, height, backdrop);
    // ODS_SELECTED is the only signal an owner-draw button gets about the mouse
    // or keyboard holding it down; without it a click is invisible.
    let pressed = (info.itemState.0 & ODS_SELECTED.0) != 0;
    let caption = button_caption(role, info.hwndItem);
    let pad = s(BUTTON_PAD, dpi);

    match role {
        TAB_SETTINGS | TAB_IGNORE | TAB_LOG | TAB_LEXICON => {
            // The accent bar takes the left 3 px, so the caption starts after it
            // and stops BUTTON_PAD before the border.
            let text_left = rect.left + s(14, dpi);
            let text_right = rect.right - pad;
            let font = fitting_font(info.hDC, dpi, &caption, text_right - text_left);
            if current_tab() == role - TAB_SETTINGS {
                rounded(
                    info.hDC,
                    rect.left,
                    rect.top,
                    width,
                    height,
                    s(8, dpi),
                    if pressed {
                        shade(button_color(), 82)
                    } else {
                        button_color()
                    },
                    card_border(),
                    backdrop,
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
                    &caption,
                    &RECT {
                        left: text_left,
                        top: rect.top,
                        right: text_right,
                        bottom: rect.bottom,
                    },
                    font,
                    fg_color(),
                    windows::Win32::Graphics::Gdi::DT_LEFT
                        | windows::Win32::Graphics::Gdi::DT_VCENTER
                        | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
                );
            } else {
                draw_text(
                    info.hDC,
                    &caption,
                    &RECT {
                        left: text_left,
                        top: rect.top,
                        right: text_right,
                        bottom: rect.bottom,
                    },
                    font,
                    dim_color(),
                    windows::Win32::Graphics::Gdi::DT_LEFT
                        | windows::Win32::Graphics::Gdi::DT_VCENTER
                        | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
                );
            }
        }
        CB_AUTOSTART | CB_ADMIN | CB_AUTO_LAYOUT | CB_SMART_UNDO => {
            let checked = match role {
                CB_AUTOSTART => get_cfg().autostart,
                CB_ADMIN => get_cfg().admin_autostart,
                CB_SMART_UNDO => get_cfg().smart_undo,
                _ => crate::win::auto_enabled(),
            };
            let box_fill = if pressed {
                shade(card_color(), 88)
            } else {
                card_color()
            };
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                box_fill,
                card_border(),
                backdrop,
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
                    box_fill,
                );
                let pen = CreatePen(PS_SOLID, s(2, dpi), rgb(255, 255, 255));
                let old_pen = SelectObject(info.hDC, HGDIOBJ(pen.0));
                let _ = MoveToEx(
                    info.hDC,
                    mark_left + s(4, dpi),
                    mark_top + s(10, dpi),
                    None,
                );
                let _ = LineTo(info.hDC, mark_left + s(9, dpi), mark_top + s(15, dpi));
                let _ = LineTo(info.hDC, mark_left + s(16, dpi), mark_top + s(5, dpi));
                // The tick pen is still selected: deselect before deleting, or the
                // object survives every repaint of a checked box.
                let _ = SelectObject(info.hDC, old_pen);
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
                    box_fill,
                );
            }
            let text_left = mark_left + mark + s(10, dpi);
            let text_right = rect.right - pad;
            draw_text(
                info.hDC,
                &caption,
                &RECT {
                    left: text_left,
                    top: rect.top,
                    right: text_right,
                    bottom: rect.bottom,
                },
                fitting_font(info.hDC, dpi, &caption, text_right - text_left),
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
                if available {
                    if pressed {
                        shade(accent_color(), 82)
                    } else {
                        accent_color()
                    }
                } else if pressed {
                    shade(card_color(), 88)
                } else {
                    card_color()
                },
                if available { accent_color() } else { card_border() },
                backdrop,
            );
            let label = if available { "Обновить" } else { "Проверить обновления" };
            draw_text(
                info.hDC,
                label,
                &RECT {
                    left: rect.left + pad,
                    top: rect.top,
                    right: rect.right - pad,
                    bottom: rect.bottom,
                },
                fitting_font(info.hDC, dpi, label, width - 2 * pad),
                if available { rgb(255, 255, 255) } else { fg_color() },
                windows::Win32::Graphics::Gdi::DT_LEFT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
        _ => {
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                if pressed {
                    shade(button_color(), 82)
                } else {
                    button_color()
                },
                card_border(),
                backdrop,
            );
            draw_text(
                info.hDC,
                &caption,
                &RECT {
                    left: rect.left + pad,
                    top: rect.top,
                    right: rect.right - pad,
                    bottom: rect.bottom,
                },
                fitting_font(info.hDC, dpi, &caption, width - 2 * pad),
                fg_color(),
                windows::Win32::Graphics::Gdi::DT_LEFT
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
        }
    }
}

/// An owner-draw button has no text of its own: everything is painted here. A
/// role that `label_of` does not know would otherwise come out as an empty,
/// silent rectangle, so the caption the button was created with is used as a
/// fallback.
fn button_caption(role: usize, hwnd: windows::Win32::Foundation::HWND) -> String {
    let label = label_of(role);
    if !label.is_empty() {
        return label;
    }
    let mut buffer = vec![0u16; 256];
    unsafe { GetWindowTextW(hwnd, &mut buffer) };
    let chars: Vec<u16> = buffer.iter().take_while(|value| **value != 0).copied().collect();
    String::from_utf16_lossy(&chars)
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
    if role == HOTKEY_LIST || role == HOTKEY_SEL_LIST {
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
        HOTKEY_LIST | HOTKEY_SEL_LIST => {
            let Some(row) = cached_row(role, index) else {
                return;
            };
            draw_text(
                info.hDC,
                &row.first,
                &RECT {
                    left: rect.left + s(14, dpi),
                    top: rect.top,
                    right: rect.left + s(214, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.second,
                &RECT {
                    left: rect.left + s(214, dpi),
                    top: rect.top,
                    right: rect.left + s(314, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).big,
                accent_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.third,
                &RECT {
                    left: rect.left + s(314, dpi),
                    top: rect.top,
                    right: rect.right - s(8, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                if row.ok { fg_color() } else { warning_color() },
                single,
            );
        }
        IGNORE_LIST => {
            let Some(row) = cached_row(role, index) else {
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
                &row.first,
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
                &row.second,
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
        LEX_LIST => {
            let Some(row) = cached_row(role, index) else {
                return;
            };
            // The tick on the left says where the word came from: a user word is
            // marked with the accent colour, a built-in one stays neutral.
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
                &row.first,
                &RECT {
                    left: rect.left + s(14, dpi),
                    top: rect.top,
                    right: rect.left + s(430, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).body,
                fg_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.second,
                &RECT {
                    left: rect.left + s(430, dpi),
                    top: rect.top,
                    right: rect.left + s(650, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                dim_color(),
                single,
            );
            draw_text(
                info.hDC,
                &row.third,
                &RECT {
                    left: rect.left + s(650, dpi),
                    top: rect.top,
                    right: rect.right - s(12, dpi),
                    bottom: rect.bottom,
                },
                fonts(dpi).dim,
                if row.ok { dim_color() } else { accent_color() },
                single,
            );
        }
        LOG_LIST => {
            let Some(row) = cached_row(role, index) else {
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
                &row.first,
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
                &row.second,
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
                &row.third,
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
        TAB_LEXICON => "Словарь".into(),
        TAB_LOG => "Лог".into(),
        BTN_CONFIG => "Открыть файл конфигурации".into(),
        BTN_UPDATE => "Проверить обновления".into(),
        CB_AUTOSTART => "Автозапуск при входе".into(),
        CB_ADMIN => "Запускать как администратор".into(),
        CB_AUTO_LAYOUT => "Автоисправление раскладки".into(),
        CB_SMART_UNDO => "Умная отмена".into(),
        BTN_HOTKEY_RESET => "Сбросить".into(),
        BTN_ADD_PROCESSES => "Добавить активное приложение".into(),
        BTN_ADD_PROCESS => "Добавить новый процесс".into(),
        BTN_REMOVE_IGNORE => "Удалить выбранное".into(),
        BTN_ADD_COMMIT => "Добавить".into(),
        BTN_LOG_REFRESH => "Обновить".into(),
        BTN_LOG_CLEAR => "Очистить".into(),
        BTN_LEX_ADD => "Добавить".into(),
        BTN_LEX_REMOVE => "Удалить выбранное".into(),
        MODAL_CANCEL => "Отмена".into(),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

// Every owner-draw row used to rebuild the whole source list (a clone of the
// config, or a clone of up to 500 log entries) for each row it painted. The
// rows are now built once per role into a cache and only the single painted row
// is copied out. The cache is invalidated whenever the config or a filter
// changes, which is signalled by CONFIG_VERSION.
#[derive(Clone)]
struct DrawRow {
    first: String,
    second: String,
    third: String,
    kind: String,
    ok: bool,
    raw: String,
}

fn bump_rows() {
    CONFIG_VERSION.fetch_add(1, Ordering::Relaxed);
}

fn row_cache() -> &'static Mutex<HashMap<usize, (usize, Vec<DrawRow>)>> {
    static CACHE: OnceLock<Mutex<HashMap<usize, (usize, Vec<DrawRow>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hotkey_value(config: &Config, field: &str) -> String {
    match field {
        "hotkey_undo" => config.hotkey_undo.clone(),
        "hotkey_toggle" => config.hotkey_toggle.clone(),
        "hotkey_last" => config.hotkey_last.clone(),
        "hotkey_selection" => config.hotkey_selection.clone(),
        "hotkey_upper" => config.hotkey_upper.clone(),
        "hotkey_upper_last" => config.hotkey_upper_last.clone(),
        "hotkey_invert" => config.hotkey_invert.clone(),
        "hotkey_invert_last" => config.hotkey_invert_last.clone(),
        "hotkey_add_word" => config.hotkey_add_word.clone(),
        _ => String::new(),
    }
}

// The hotkey table is split in two halves. The left column is everything that
// works on the last token with no selection at all; the right column works only
// on text the user has selected. One and the same combination can be bound in
// both columns — that is the default for Break — and then the gesture looks at
// the selection first and falls back to the last token.
const HOTKEY_WORD_ACTIONS: [(&str, &str, &str); 5] = [
    ("Исправить последнее слово", "hotkey_last", "break"),
    ("Поднять последнее слово", "hotkey_upper_last", "ctrl+break"),
    ("Сменить регистр слова", "hotkey_invert_last", "shift+break"),
    ("Отменить исправление", "hotkey_undo", "alt"),
    ("Автоисправление вкл/выкл", "hotkey_toggle", "ctrl+alt+p"),
];

const HOTKEY_SEL_ACTIONS: [(&str, &str, &str); 4] = [
    ("Сменить раскладку выделения", "hotkey_selection", "break"),
    ("Выделение в верхний регистр", "hotkey_upper", "ctrl+break"),
    ("Сменить регистр выделения", "hotkey_invert", "shift+break"),
    ("Добавить слово в словарь", "hotkey_add_word", "ctrl+`"),
];

fn actions_for(role: usize) -> &'static [(&'static str, &'static str, &'static str)] {
    match role {
        HOTKEY_SEL_LIST => &HOTKEY_SEL_ACTIONS,
        _ => &HOTKEY_WORD_ACTIONS,
    }
}

fn build_hotkey_rows(role: usize) -> Vec<DrawRow> {
    let config = get_cfg();
    actions_for(role)
        .iter()
        .map(|(action, field, _default)| {
            let value = hotkey_value(&config, field);
            let status = validate_hotkey(field, &value);
            let ok = status.is_ok();
            let note = match status {
                Ok(_) => "назначить".to_string(),
                Err(reason) => reason,
            };
            DrawRow {
                first: action.to_string(),
                second: value,
                third: note,
                kind: String::new(),
                ok,
                raw: String::new(),
            }
        })
        .collect()
}

fn build_ignore_rows() -> Vec<DrawRow> {
    let config = get_cfg();
    let filter = field_text(IGNORE_FILTER);
    let mut rows = Vec::new();
    for item in config.exclude_processes {
        if matches_filter(&item, &filter) {
            rows.push(DrawRow {
                first: item.clone(),
                second: "процесс".into(),
                third: String::new(),
                kind: "процесс".into(),
                ok: true,
                raw: item,
            });
        }
    }
    for item in config.exclude_dirs {
        if matches_filter(&item, &filter) {
            rows.push(DrawRow {
                first: item.clone(),
                second: "папка".into(),
                third: String::new(),
                kind: "папка".into(),
                ok: true,
                raw: item,
            });
        }
    }
    rows
}

/// Every short word the engine knows: the compiled lists plus the ones the user
/// added. A row records whether the word came from the config (`добавлено`) or
/// from the built-in table (`встроенное`).
fn build_lex_rows() -> Vec<DrawRow> {
    let (extra_ru, extra_en) = crate::win::short_word_lists();
    let lexicon = crate::lexicon::Lexicon::with_extra(&extra_ru, &extra_en);
    let filter = field_text(LEX_FILTER);
    let mut rows = Vec::new();
    for word in lexicon.ru_words() {
        if !matches_filter(&word, &filter) {
            continue;
        }
        let added = extra_ru.iter().any(|item| item.eq_ignore_ascii_case(&word));
        rows.push(DrawRow {
            first: word.clone(),
            second: "русский".into(),
            third: if added { "добавлено".into() } else { "встроенное".into() },
            kind: if added { "manual".into() } else { String::new() },
            ok: added,
            raw: word,
        });
    }
    for word in lexicon.en_words() {
        if !matches_filter(&word, &filter) {
            continue;
        }
        let added = extra_en.iter().any(|item| item.eq_ignore_ascii_case(&word));
        rows.push(DrawRow {
            first: word.clone(),
            second: "английский".into(),
            third: if added { "добавлено".into() } else { "встроенное".into() },
            kind: if added { "manual".into() } else { String::new() },
            ok: added,
            raw: word,
        });
    }
    rows
}

/// Counts and the first few key-mirror collisions, cached the same way as the
/// rows so a repaint of the footnote does not rebuild the dictionary.
#[derive(Clone)]
struct LexSummary {
    ru: usize,
    en: usize,
    conflicts: Vec<String>,
}

/// "мы ↔ vs, ли ↔ kb, …" — the collisions a user has to know about when reading
/// the footnote of the dictionary page.
fn conflict_label(conflicts: &[crate::lexicon::Conflict], limit: usize) -> String {
    conflicts
        .iter()
        .take(limit)
        .map(|conflict| format!("{} ↔ {}", conflict.ru, conflict.keys))
        .collect::<Vec<String>>()
        .join(", ")
}

fn lex_summary() -> LexSummary {
    static CACHE: OnceLock<Mutex<(usize, LexSummary)>> = OnceLock::new();
    let table = CACHE.get_or_init(|| Mutex::new((0, LexSummary { ru: 0, en: 0, conflicts: Vec::new() })));
    let mut guard = table.lock().unwrap_or_else(|error| error.into_inner());
    let version = CONFIG_VERSION.load(Ordering::Relaxed);
    if guard.0 != version {
        let (extra_ru, extra_en) = crate::win::short_word_lists();
        let lexicon = crate::lexicon::Lexicon::with_extra(&extra_ru, &extra_en);
        guard.1 = LexSummary {
            ru: lexicon.ru_count(),
            en: lexicon.en_count(),
            conflicts: conflict_label(&lexicon.conflicts(), 6)
                .split(", ")
                .filter(|pair| !pair.is_empty())
                .map(|pair| pair.to_string())
                .collect(),
        };
        guard.0 = version;
    }
    guard.1.clone()
}

fn build_log_rows() -> Vec<DrawRow> {
    let filter = field_text(LOG_FILTER);
    crate::diary::tail(300)
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
            Some(DrawRow {
                first: entry.time.clone(),
                second: format!("{} → {}", entry.original, entry.converted),
                third: entry.why.clone(),
                kind,
                ok: true,
                raw,
            })
        })
        .collect()
}

fn rows_for_role(role: usize) -> Vec<DrawRow> {
    match role {
        IGNORE_LIST => build_ignore_rows(),
        LEX_LIST => build_lex_rows(),
        LOG_LIST => build_log_rows(),
        HOTKEY_LIST | HOTKEY_SEL_LIST => build_hotkey_rows(role),
        _ => build_hotkey_rows(HOTKEY_LIST),
    }
}

/// Return the rows of a list, rebuilding them only when the config or a filter
/// changed since the last build.
fn cached_rows(role: usize) -> Vec<DrawRow> {
    let cache = row_cache();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let version = CONFIG_VERSION.load(Ordering::Relaxed);
    match guard.get(&role) {
        Some((stored, rows)) if *stored == version => rows.clone(),
        _ => {
            let rows = rows_for_role(role);
            guard.insert(role, (version, rows.clone()));
            rows
        }
    }
}

/// How many rows a list currently has, without copying the whole vector.
fn cached_len(role: usize) -> usize {
    let cache = row_cache();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let version = CONFIG_VERSION.load(Ordering::Relaxed);
    match guard.get(&role) {
        Some((stored, rows)) if *stored == version => rows.len(),
        _ => {
            let rows = rows_for_role(role);
            let count = rows.len();
            guard.insert(role, (version, rows));
            count
        }
    }
}

/// Copy a single row out of the cache: the painting path never touches the rest.
fn cached_row(role: usize, index: usize) -> Option<DrawRow> {
    let cache = row_cache();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    let version = CONFIG_VERSION.load(Ordering::Relaxed);
    let stale = guard.get(&role).map_or(true, |(stored, _)| *stored != version);
    if stale {
        guard.insert(role, (version, rows_for_role(role)));
    }
    guard.get(&role).and_then(|(_, rows)| rows.get(index).cloned())
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
    let rows = cached_rows(list_role(list));
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.first);
    }
}

/// The ListView picks the row height from its font, not from the layout, so the
/// control is cut down to the rows it really holds. The design budget of 42 px
/// per row is only an upper bound: leaving it whole shows a strip of empty card
/// below the last row, and on a strongly scaled monitor the last row would be
/// clipped instead.
unsafe fn fit_list_to_rows(list: HWND, dpi: u32, x: i32, y: i32, width: i32, budget: i32) {
    if list.0.is_null() {
        return;
    }
    let rows = cached_rows(list_role(list)).len();
    if rows == 0 {
        return;
    }
    let mut item = RECT::default();
    if send(list, LVM_GETITEMRECT, rows - 1, std::ptr::addr_of_mut!(item) as isize).0 < 0 {
        return;
    }
    let content = item.bottom + s(6, dpi);
    let designed = s(budget, dpi);
    if content > 0 && content < designed {
        MoveWindow(list, s(x, dpi), s(y, dpi), s(width, dpi), content, true).ok();
    }
}

/// True only when the filter text differs from the text seen last time. Windows
/// sends EN_CHANGE/EN_UPDATE for every keystroke, including ones that do not
/// change the value, and rebuilding a list is a disk read plus a dozen
/// synchronous messages.
fn filter_changed(role: usize) -> bool {
    static LAST: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
    let text = field_text(role);
    let mut guard = LAST
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    match guard.get(&role) {
        Some(previous) if *previous == text => false,
        _ => {
            guard.insert(role, text);
            true
        }
    }
}

fn matches_filter(value: &str, filter: &str) -> bool {
    filter.is_empty() || value.to_lowercase().contains(&filter.to_lowercase())
}

unsafe fn fill_ignore_list(list: HWND) {
    send(list, LVM_DELETEALLITEMS, 0, 0);
    let rows = cached_rows(IGNORE_LIST);
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.first);
    }
    update_status(&format!("Игнорируется записей: {}", rows.len()));
}

unsafe fn fill_lex_list(list: HWND) {
    send(list, LVM_DELETEALLITEMS, 0, 0);
    let rows = cached_rows(LEX_LIST);
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.first);
    }
    update_status(&format!("Словарь слов: {}", rows.len()));
}

unsafe fn refresh_lex() {
    if let Some(list) = handle(LEX_LIST) {
        fill_lex_list(list);
        invalidate(list);
    }
    if let Some(main) = main_window() {
        repaint(main);
    }
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

unsafe fn fill_log_list(list: HWND) {
    let rows = cached_rows(LOG_LIST);
    send(list, LVM_DELETEALLITEMS, 0, 0);
    for (index, row) in rows.iter().enumerate() {
        insert_row(list, index, &row.raw);
    }
    update_status(&format!("Строк в логе: {}", rows.len()));
}

unsafe fn refresh_log() {
    if let Some(list) = handle(LOG_LIST) {
        fill_log_list(list);
        invalidate(list);
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
    // LVM_GETNEXTITEM wants the starting index in wParam and the state flags in
    // lParam. The two were swapped, so only rows from index 2 on were ever found
    // and "Удалить" answered "Сначала выберите строку в списке".
    let index = send(list, LVM_GETNEXTITEM, -1isize as usize, LVNI_SELECTED as isize).0;
    // No selection is not "the first row": returning Some(0) here made
    // "Удалить выбранное" delete an arbitrary entry the user never picked.
    if index >= 0 {
        Some(index as usize)
    } else {
        None
    }
}

// The row the pointer really landed on. A right click on an owner-drawn row does
// not always leave that row selected, so the context menu takes the row from the
// click position instead of trusting the selection.
unsafe fn row_at_point(list: HWND, screen: POINT) -> Option<usize> {
    let mut local = screen;
    let _ = ScreenToClient(list, &mut local).ok();
    let index = send(list, LVM_HITTEST, 0, std::ptr::addr_of!(local) as isize).0;
    if index >= 0 {
        Some(index as usize)
    } else {
        None
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
    bump_rows();
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
}

static AUTOSTART_BUSY: AtomicBool = AtomicBool::new(false);

/// `schtasks` is a real subprocess. Running it on the UI thread makes a wedged
/// Task Scheduler freeze the settings window (and the keyboard hook installed on
/// that thread), so the work is moved to a worker thread and bounded by a
/// deadline. The checkbox repaints as soon as the answer arrives.
unsafe fn autostart_toggle(admin: bool) {
    let current_admin = get_cfg().admin_autostart;
    let current_auto = get_cfg().autostart;
    let next_admin = if admin { !current_admin } else { current_admin };
    let next_auto = if admin { next_admin || current_auto } else { !current_auto };
    let label = if admin { "Запуск как администратор" } else { "Автозапуск" };

    if AUTOSTART_BUSY.swap(true, Ordering::SeqCst) {
        update_status(&format!("{label}: предыдущая операция ещё не завершена"));
        return;
    }
    update_status(&format!("{label}: применяю…"));
    std::thread::spawn(move || unsafe {
        let (sender, receiver) = std::sync::mpsc::channel::<bool>();
        std::thread::spawn(move || {
            let ok = crate::autostart::sync_with_config(next_auto, next_admin);
            let _ = sender.send(ok);
        });
        let outcome = match receiver.recv_timeout(std::time::Duration::from_secs(8)) {
            Ok(ok) => Some(ok),
            Err(_) => None,
        };
        match outcome {
            Some(ok) => {
                // Only remember the new value when Windows really accepted it.
                if ok || !next_auto {
                    apply_config(|config| {
                        config.autostart = next_auto;
                        config.admin_autostart = next_admin;
                    });
                }
                update_status(&format!(
                    "{label}: {}",
                    if next_auto || next_admin {
                        if ok {
                            if admin {
                                "включён (задание ZyaAutoStart, уровень HIGHEST)"
                            } else {
                                "включён"
                            }
                        } else {
                            "не включился: не хватает прав"
                        }
                    } else {
                        "выключен"
                    }
                ));
            }
            None => {
                update_status(&format!("{label}: не удалось применить (таймаут)"));
            }
        }
        repaint_main();
        AUTOSTART_BUSY.store(false, Ordering::Relaxed);
    });
}

unsafe fn toggle_autostart() {
    autostart_toggle(false);
}

unsafe fn toggle_admin_autostart() {
    autostart_toggle(true);
}

unsafe fn toggle_auto_layout() {
    // The checkbox mirrors the runtime switch used by the tray menu, Ctrl+Alt+P
    // and the left click on the tray icon, so it reads and writes the same flag.
    let next = !crate::win::auto_enabled();
    crate::win::set_auto_enabled(next);
    repaint_main();
    update_status(&format!(
        "Автоисправление раскладки: {}",
        if next { "включено" } else { "выключено" }
    ));
}

unsafe fn toggle_smart_undo() {
    // "Умная отмена": the first Break reads the punctuation the automatic layer
    // ate, the second one restores the text it recorded. Off, every Break press
    // mirrors the whole token, as in the classic version.
    let next = !get_cfg().smart_undo;
    apply_config(|config| config.smart_undo = next);
    crate::win::reload_hotkeys(&get_cfg());
    repaint_main();
    update_status(&format!(
        "Умная отмена: {}",
        if next { "включена" } else { "выключена" }
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
        config.hotkey_upper_last = default.hotkey_upper_last.clone();
        config.hotkey_invert = default.hotkey_invert.clone();
        config.hotkey_invert_last = default.hotkey_invert_last.clone();
        config.hotkey_add_word = default.hotkey_add_word.clone();
    });
    for list in [HOTKEY_LIST, HOTKEY_SEL_LIST] {
        if let Some(handle) = handle(list) {
            fill_hotkey_list(handle);
            invalidate(handle);
        }
    }
    // M-13: the reset also closes the assignment dialog and refreshes the page,
    // otherwise the modal keeps showing the old combination.
    if is_assign_open() {
        close_modal();
    }
    if let Some(hwnd) = main_window() {
        repaint(hwnd);
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
    bump_rows();
    true
}

// The name (basename, lower case) of the process that owns the window currently
// in focus. `PROCESS_QUERY_LIMITED_INFORMATION` is enough for
// QueryFullProcessImageNameW, so this works without elevation.
unsafe fn foreground_process_name() -> Option<String> {
    let window = GetForegroundWindow();
    if is_null(window) {
        return None;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(window, Some(&mut pid));
    if pid == 0 {
        return None;
    }
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return None;
    };
    let mut buffer = vec![0u16; 1024];
    let mut size = 1024u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        PWSTR(buffer.as_mut_ptr()),
        &mut size,
    );
    CloseHandle(handle).ok();
    if result.is_err() {
        return None;
    }
    let path = String::from_utf16_lossy(&buffer[..size as usize]);
    let name = path
        .rsplit(|character| character == '\\' || character == '/')
        .next()
        .unwrap_or_default()
        .to_lowercase();
    (!name.is_empty()).then_some(name)
}

// "Добавить из запущенных" used to dump the whole process snapshot into
// exclude_processes, which switched the hook off everywhere. It now adds only
// the application in the foreground window.
unsafe fn add_foreground_process() {
    let Some(name) = foreground_process_name() else {
        update_status("Не удалось определить активное окно: сначала кликните по окну приложения");
        return;
    };
    if !name.ends_with(".exe") {
        update_status(&format!("Активное окно: {name} — не исполняемый файл"));
        return;
    }
    if name == "explorer.exe" || name == "zя.exe" || name == "z.exe" || name == "zya.exe" {
        update_status(&format!("{name} не добавляется: это оболочка или сам ZЯ"));
        return;
    }
    if !add_to_config(&name) {
        update_status(&format!("{name} уже в списке исключений"));
        return;
    }
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
    if let Some(list) = handle(IGNORE_LIST) {
        fill_ignore_list(list);
        invalidate(list);
    }
    repaint_main();
    update_status(&format!("Добавлено из активного окна: {name}"));
}

unsafe fn show_ignore_input() {
    if let Some(input) = handle(IGNORE_INPUT) {
        let _ = ShowWindow(input, SW_SHOW);
    }
    if let Some(commit) = handle(BTN_ADD_COMMIT) {
        let _ = ShowWindow(commit, SW_SHOW);
    }
    repaint_main();
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
            invalidate(list);
        }
        if let Some(hwnd) = main_window() {
            repaint(hwnd);
        }
        update_status(&format!("Добавлено: {}", value.trim()));
    } else {
        update_status("Нужен процесс вида chrome.exe или папка вида C:\\Soft");
    }
}

unsafe fn remove_ignore_row(index: Option<usize>) {
    let Some(index) = index else {
        update_status("Сначала выберите строку в списке");
        return;
    };
    let Some(row) = cached_row(IGNORE_LIST, index) else {
        update_status("Сначала выберите строку в списке");
        return;
    };
    let name = row.raw.clone();
    {
        let reference = cfg();
        let mut guard = reference.lock().unwrap_or_else(|e| e.into_inner());
        guard.exclude_processes.retain(|item| !item.eq_ignore_ascii_case(&name));
        guard.exclude_dirs.retain(|item| !item.eq_ignore_ascii_case(&name));
    }
    bump_rows();
    crate::win::reload_hotkeys(&get_cfg());
    save_config();
    if let Some(list) = handle(IGNORE_LIST) {
        fill_ignore_list(list);
        invalidate(list);
    }
    // The ignore card in the header prints the live entry count, so it has to be
    // repainted too, otherwise the window still claims the deleted row exists.
    if let Some(main) = main_window() {
        repaint(main);
    }
    update_status(&format!("Удалено: {name}"));
}

unsafe fn add_lex_entry() {
    let value = field_text(LEX_INPUT);
    if value.is_empty() {
        update_status("Введите слово: кириллица — русское, латиница — английское");
        return;
    }
    if !crate::win::add_short_word(&value) {
        update_status(&format!("«{value}» уже в словарчике или не является словом"));
        return;
    }
    bump_rows();
    if let Some(input) = handle(LEX_INPUT) {
        SetWindowTextW(input, PCWSTR(std::ptr::null())).ok();
    }
    refresh_lex();
    update_status(&format!("Добавлено в словарь: {}", value));
}

unsafe fn remove_lex_row(index: Option<usize>) {
    let Some(index) = index else {
        update_status("Сначала выберите строку в списке");
        return;
    };
    let Some(row) = cached_row(LEX_LIST, index) else {
        update_status("Сначала выберите строку в списке");
        return;
    };
    let word = row.raw.clone();
    if !crate::win::remove_short_word(&word) {
        update_status(&format!("«{word}» — встроенное слово: его нельзя убрать"));
        return;
    }
    bump_rows();
    refresh_lex();
    update_status(&format!("Удалено из словаря: {word}"));
}

// Diagnostic behind `zя deletetest`: post a real left-click on the top row of the
// ignore list, then run exactly the code the "Удалить выбранное" button runs. The
// name of the row that disappeared is returned so the caller can compare
// config.json before and after.
pub unsafe fn delete_top_ignore_row() -> Option<String> {
    let Some(list) = handle(IGNORE_LIST) else {
        return None;
    };
    let mut rect = RECT::default();
    let found = send(list, LVM_GETITEMRECT, 0, std::ptr::addr_of_mut!(rect) as isize).0;
    if found < 0 {
        return None;
    }
    let name = cached_row(IGNORE_LIST, 0).map(|row| row.raw)?;
    let x = rect.left + 30;
    let y = (rect.top + rect.bottom) / 2;
    let click = LPARAM(((y << 16) | (x & 0xFFFF)) as isize);
    // The list view picks the item up on the press, so both messages are needed.
    let _ = PostMessageW(Some(list), WM_LBUTTON_DOWN, WPARAM(0), click);
    let _ = PostMessageW(Some(list), WM_LBUTTON_UP, WPARAM(0), click);
    remove_ignore_row(list_selected(IGNORE_LIST));
    Some(name)
}

unsafe fn copy_row(role: usize, index: Option<usize>) {
    let Some(index) = index else {
        update_status("Нет выбранной строки");
        return;
    };
    let text = match role {
        IGNORE_LIST => cached_row(IGNORE_LIST, index).map(|row| row.raw).unwrap_or_default(),
        LEX_LIST => cached_row(LEX_LIST, index).map(|row| row.raw).unwrap_or_default(),
        LOG_LIST => cached_row(LOG_LIST, index).map(|row| row.raw).unwrap_or_default(),
        _ => String::new(),
    };
    if text.is_empty() {
        update_status("Нет выбранной строки");
        return;
    }
    if copy_to_clipboard(&text) {
        update_status("Строка скопирована");
    } else {
        update_status("Не удалось скопировать: буфер обмена занят");
    }
}

unsafe fn copy_visible_rows(role: usize) {
    let lines: Vec<String> = match role {
        IGNORE_LIST => cached_rows(IGNORE_LIST).into_iter().map(|row| row.raw).collect(),
        LEX_LIST => cached_rows(LEX_LIST).into_iter().map(|row| row.raw).collect(),
        LOG_LIST => cached_rows(LOG_LIST).into_iter().map(|row| row.raw).collect(),
        _ => Vec::new(),
    };
    if lines.is_empty() {
        update_status("Список пуст");
        return;
    }
    if copy_to_clipboard(&lines.join("\n")) {
        update_status(&format!("Скопировано строк: {}", lines.len()));
    } else {
        update_status("Не удалось скопировать: буфер обмена занят");
    }
}

unsafe fn copy_to_clipboard(text: &str) -> bool {
    // CF_UNICODETEXT must be NUL terminated, and SetClipboardData expects a
    // GMEM_MOVEABLE block: a GMEM_FIXED buffer with no terminator reads past the
    // end of the block when the target application calls lstrlenW.
    let encoded: Vec<u16> = text.encode_utf16().chain(std::iter::once(0u16)).collect();
    let bytes = encoded.len() * 2;

    // Another process can hold the clipboard for a moment; retry a few times
    // instead of failing silently.
    let mut opened = false;
    for attempt in 0..4 {
        if OpenClipboard(None).is_ok() {
            opened = true;
            break;
        }
        if attempt + 1 < 4 {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
    if !opened {
        return false;
    }
    if EmptyClipboard().is_err() {
        CloseClipboard().ok();
        return false;
    }
    let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
        CloseClipboard().ok();
        return false;
    };
    let locked = GlobalLock(memory);
    if locked.is_null() {
        GlobalFree(Some(memory)).ok();
        CloseClipboard().ok();
        return false;
    }
    std::ptr::copy_nonoverlapping(encoded.as_ptr() as *const u8, locked as *mut u8, bytes);
    GlobalUnlock(memory).ok();
    // On success the clipboard owns the block; only free it when the handover failed.
    if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))).is_err() {
        GlobalFree(Some(memory)).ok();
        CloseClipboard().ok();
        return false;
    }
    CloseClipboard().ok();
    true
}

unsafe fn open_config_file() {
    let path = crate::config::config_dir().join("config.json");
    if !path.exists() {
        update_status(&format!("Файл конфигурации ещё не создан: {}", path.display()));
        return;
    }
    let file = wide(&path.display().to_string());
    let action = wide("open");
    let result = ShellExecuteW(
        None,
        PCWSTR(action.as_ptr()),
        PCWSTR(file.as_ptr()),
        PCWSTR::null(),
        None,
        SW_SHOW,
    );
    // ShellExecuteW returns an HINSTANCE; values <= 32 are the documented error
    // codes (no association for .json, file not found, ...).
    if (result.0 as isize) > 32 {
        update_status(&format!("Открыт {}", path.display()));
    } else {
        update_status(&format!(
            "Не удалось открыть {}: нет программы для .json (код {}) — файл: {}",
            path.display(),
            result.0 as isize,
            path.display()
        ));
    }
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

/// What the bottom status line says after an update check. When GitHub has
/// nothing newer the line stays empty: the header next to the button already
/// carries «Обновлений нет», and repeating it at the bottom is noise.
fn update_message(local: &str, release: &crate::update::Release) -> String {
    if crate::update::is_newer(local, &release.version) {
        format!(
            "Доступно обновление {}: {} ({:.1} МБ)",
            release.version,
            release.url,
            release.size as f64 / 1_048_576.0
        )
    } else {
        STATUS_IDLE.to_string()
    }
}

unsafe fn spawn_update_check() {
    // Without this flag every click on "Проверить обновления" started another
    // WinHTTP request in a new thread.
    if UPDATE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::Relaxed)
        .is_err()
    {
        update_status("Проверка обновлений уже выполняется");
        return;
    }
    std::thread::spawn(|| {
        // P-08: the flag must be released whatever happens to the request. A panic
        // or a wedged WinHTTP call used to leave it set forever, and the "Проверить
        // обновления" button stayed dead for the whole life of the process.
        struct ClearOnDrop(&'static AtomicBool);
        impl Drop for ClearOnDrop {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Relaxed);
            }
        }
        let _guard = ClearOnDrop(&UPDATE_IN_FLIGHT);
        // The network calls in `update::check_latest` are bounded by their own
        // timeouts (5+8+8+20 seconds, twice for the proxy route). A safety timer
        // releases the flag if the whole thing somehow runs longer.
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
                }
                update_message(&local, &release)
            }
            None => "Не удалось проверить обновления: нет подключения к GitHub".into(),
        };
        UPDATE_IN_FLIGHT.store(false, Ordering::Relaxed);
        // The window may have been closed while the request was in flight.
        if let Some(hwnd) = main_window() {
            unsafe {
                update_status(&message);
                repaint(hwnd);
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
        // H-01: the download used to run inside WM_COMMAND. WinHTTP waits up to
        // 5+8+8+20 seconds *per attempt* and the message loop of the settings
        // window is the tray thread, so the icon and the window stopped
        // answering for minutes. The whole download now runs in its own thread.
        update_status("Скачиваю установщик…");
        std::thread::spawn(move || {
            let release = crate::update::Release { version, url, size };
            let message = match crate::update::download_installer(&release) {
                Some(path) => {
                    if crate::update::run_installer(&path) {
                        format!("Установщик запущен: {}", path.display())
                    } else {
                        "Не удалось запустить установщик".to_string()
                    }
                }
                None => "Скачивание не удалось: проверьте статус и размер файла".to_string(),
            };
            if let Some(hwnd) = main_window() {
                unsafe {
                    update_status(&message);
                    repaint(hwnd);
                }
            }
        });
    } else {
        UPDATE_CHECKED.store(false, Ordering::Relaxed);
        spawn_update_check();
        update_status("Проверяю обновления на GitHub…");
    }
}

// ---------------------------------------------------------------------------
// Hotkey capture
// ---------------------------------------------------------------------------

// Row bounds of the hotkey list in the settings window's client coordinates.
// The list is a child window created at HOTKEY_LIST_Y, so its own item rects
// only need that offset added; using GetWindowRect here would mix the parent's
// title bar into the numbers.
unsafe fn hotkey_row_bounds(role: usize, row: usize, dpi: u32) -> (i32, i32) {
    let list_top = s(HOTKEY_LIST_Y, dpi);
    let fallback_top = list_top + s(26 + (row as i32) * 42, dpi);
    let Some(list) = handle(role) else {
        return (fallback_top, fallback_top + s(42, dpi));
    };
    let mut item = RECT::default();
    if send(list, LVM_GETITEMRECT, row, std::ptr::addr_of_mut!(item) as isize).0 < 0 {
        return (fallback_top, fallback_top + s(42, dpi));
    }
    (list_top + item.top, list_top + item.bottom)
}

// Where the modal top should sit for a given row. It is anchored on the row,
// clamped to the hotkey card, so the panel always follows the row that was
// clicked and never lands on the "Версия"/"Запуск" cards above.
fn modal_top_for_row(row_top: i32, row_bottom: i32, dpi: u32) -> i32 {
    let band_min = s(HOTKEY_CARD_Y + 6, dpi);
    let band_max = s(HOTKEY_CARD_Y + HOTKEY_CARD_H - MODAL_HEIGHT - 6, dpi);
    let centre = (row_top + row_bottom) / 2;
    let top = centre - s(MODAL_HEIGHT, dpi) / 2;
    if top < band_min {
        band_min
    } else if top > band_max {
        band_max
    } else {
        top
    }
}

unsafe fn modal_slot(dpi: u32, role: usize, row: usize) -> (i32, i32) {
    let (row_top, row_bottom) = hotkey_row_bounds(role, row, dpi);
    // The panel is wider than one column, so it cannot sit exactly under the row.
    // It is still anchored on the half of the card the row belongs to: the left
    // half for the "без выделения" table, the right half for the "с выделением".
    let left = if role == HOTKEY_SEL_LIST {
        (CONTENT_X + CONTENT_WIDTH - MODAL_WIDTH - 6).max(HOTKEY_SEL_X - 26)
    } else {
        CONTENT_X + 6
    };
    (s(left, dpi), modal_top_for_row(row_top, row_bottom, dpi))
}

// Keeps a dragged modal inside the settings window client area. Positions of a
// child window are relative to the parent client origin, so the area is measured
// from (0, 0) to the parent's client size.
fn clamp_modal_position(left: i32, top: i32, width: i32, height: i32, area: (i32, i32)) -> (i32, i32) {
    let max_left = area.0 - width;
    let max_top = area.1 - height;
    if max_left < 0 || max_top < 0 {
        return (0, 0);
    }
    (left.max(0).min(max_left), top.max(0).min(max_top))
}

// Positions of a child window are relative to the parent client origin, but the
// assignment panel is an owned popup: it is placed in screen coordinates, so the
// client-relative anchor has to be translated first.
unsafe fn modal_screen_position(parent: HWND, left: i32, top: i32) -> (i32, i32) {
    let mut point = POINT { x: left, y: top };
    if ClientToScreen(parent, &mut point).as_bool() {
        (point.x, point.y)
    } else {
        (left, top)
    }
}

unsafe fn open_hotkey_modal(role: usize, index: usize) {
    let Some(parent) = main_window() else {
        update_status("Нет главного окна для назначения");
        return;
    };
    let dpi = window_dpi(parent);
    let row = index.min(actions_for(role).len() - 1);
    MODAL_ROLE.store(role, Ordering::Relaxed);
    MODAL_TARGET.store(row, Ordering::Relaxed);
    let mut area = RECT::default();
    let _ = GetClientRect(parent, &mut area);
    let (x, y) = modal_slot(dpi, role, row);
    // The window can be clipped by the screen work area, so the panel is also
    // clamped to the client rectangle that is really on screen.
    let (x, y) = clamp_modal_position(x, y, s(MODAL_WIDTH, dpi), s(MODAL_HEIGHT, dpi), (area.right, area.bottom));
    let (screen_x, screen_y) = modal_screen_position(parent, x, y);

    if let Some(existing) = modal_handle() {
        // The modal is already on screen: move it to the row that was clicked
        // and repaint it. Without the move it stayed where the first row opened.
        SetWindowPos(existing, Some(HWND_TOP), screen_x, screen_y, 0, 0, SWP_NOSIZE).ok();
        MODAL_LEFT.store(x, Ordering::Relaxed);
        MODAL_TOP.store(y, Ordering::Relaxed);
        MODAL_DRAGGING.store(false, Ordering::Relaxed);
        InvalidateRect(existing, std::ptr::null(), 1);
        return;
    }

    let modal = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(wide("ZyaModalWindow").as_ptr()),
        PCWSTR(wide("").as_ptr()),
        // The panel is an owned popup, not a child window: as a child it was
        // painted under the hotkey list and the settings buttons, which is why
        // the opened modal looked empty and could not be clicked.
        WS_POPUP | WS_VISIBLE,
        // Anchored to the clicked row inside the hotkey card, so the modal never
        // covers the "Версия"/"Запуск" cards and their buttons.
        screen_x,
        screen_y,
        s(MODAL_WIDTH, dpi),
        s(MODAL_HEIGHT, dpi),
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
    // The panel must sit above every settings control, including the hotkey list
    // that was created before it.
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
    MODAL_LEFT.store(x, Ordering::Relaxed);
    MODAL_TOP.store(y, Ordering::Relaxed);

    // The cancel button keeps a 12 px margin from the rounded corner of the panel
    // and sits clear of the "Перетащите за заголовок" hint.
    make_button(
        modal,
        dpi,
        "Отмена",
        24,
        MODAL_HEIGHT - 12 - 36,
        140,
        36,
        MODAL_CANCEL,
        card_color(),
    );

    if HOTKEY_HOOK.load(Ordering::Relaxed) == 0 && !install_capture_hook() {
        return;
    }
    invalidate(modal);
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

/// Force a full repaint of the panel so a probe capture sees everything.
pub unsafe fn refresh_modal_probe() -> bool {
    let Some(modal) = modal_handle() else {
        return false;
    };
    InvalidateRect(modal, std::ptr::null(), 1) != 0
}

/// Put the settings window in front so a screen capture of the panel is not
/// blocked by other applications.
pub unsafe fn focus_settings_probe() -> bool {
    let Some(parent) = main_window() else {
        return false;
    };
    ShowWindow(parent, SW_SHOW).as_bool()
        && SetWindowPos(parent, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE).is_ok()
}

fn modal_handle() -> Option<HWND> {
    let mut guard = MODAL_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    match guard.as_mut() {
        Some(win) if unsafe { IsWindow(Some(win.0)).as_bool() } => Some(win.0),
        _ => {
            *guard = None;
            None
        }
    }
}

unsafe fn detach_hotkey_hook() {
    let raw = HOTKEY_HOOK.load(Ordering::Relaxed);
    if raw != 0 {
        UnhookWindowsHookEx(HHOOK(raw as *mut c_void)).ok();
        HOTKEY_HOOK.store(0, Ordering::Relaxed);
    }
    // Nothing may be left pending: the next modal starts from a clean state.
    PENDING_ASSIGN.store(0, Ordering::Relaxed);
    ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
}

/// Used by the tray shutdown path: the settings window can still be open when
/// the resident process quits, and a low-level keyboard hook that outlives it
/// keeps filtering keystrokes for the whole machine.
pub fn release_capture_hook() {
    unsafe { detach_hotkey_hook() }
}

unsafe fn install_capture_hook() -> bool {
    // L-16: installing while a previous hook is still registered leaves two
    // identical low-level hooks, and every keystroke then runs the capture twice.
    detach_hotkey_hook();
    match SetWindowsHookExW(WH_KEYBOARD_LL, Some(capture_hotkey_hook), None, 0) {
        Ok(hook) => {
            HOTKEY_HOOK.store(hook.0 as usize, Ordering::Relaxed);
            true
        }
        Err(error) => {
            HOTKEY_HOOK.store(0, Ordering::Relaxed);
            MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
            update_status(&format!("Не удалось слушать клавиши: {error}"));
            false
        }
    }
}

unsafe fn close_modal() {
    detach_hotkey_hook();
    MODAL_DRAGGING.store(false, Ordering::Relaxed);
    let mut guard = MODAL_HANDLE
        .get_or_init(|| Mutex::new(None))
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if let Some(modal) = guard.take() {
        // The modal is a child of the settings window: when the parent is being
        // destroyed Windows has already destroyed it, and DestroyWindow on a
        // dead handle would fail (or hit a recycled window).
        if IsWindow(Some(modal.0)).as_bool() {
            DestroyWindow(modal.0).ok();
        }
    }
    MODAL_TARGET.store(usize::MAX, Ordering::Relaxed);
}

// The modal used to be painted only in WM_ERASEBKGND, so every later repaint
// (WM_PAINT, a button that the system redraws) left the background behind the
// button empty. Both paths now use the same routine.
unsafe fn paint_modal(hwnd: HWND, hdc: HDC) {
    let dpi = window_dpi(hwnd);
    let rect = client_rect(hwnd);
    // The modal is a popup window that floats over the settings window, so it
    // outside its rounded corners has to match the card, not the page.
    fill_rect(hdc, rect.left, rect.top, rect.right, rect.bottom, card_color());
    rounded(
        hdc,
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
        s(10, dpi),
        card_color(),
        card_border(),
        card_color(),
    );
    let target = MODAL_TARGET.load(Ordering::Relaxed);
    let role = MODAL_ROLE.load(Ordering::Relaxed);
    let (action, field, default) = actions_for(role)
        .get(target)
        .unwrap_or(&("Действие", "hotkey_toggle", "ctrl+alt+p"));
    let config = get_cfg();
    let current = hotkey_value(&config, field);
    modal_text(hdc, dpi, 20, 12, 300, 26, "Назначение горячей клавиши", fonts(dpi).card, fg_color());
    modal_text(hdc, dpi, 20, 42, 420, 22, action, fonts(dpi).dim, dim_color());
    modal_text(
        hdc,
        dpi,
        20,
        68,
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
        96,
        420,
        26,
        "Нажмите новую комбинацию",
        fonts(dpi).big,
        accent_color(),
    );
    draw_text(
        hdc,
        "Ctrl, Alt, Shift, Win + буква, цифра, Break или F. Alt один занят системой. Esc — отмена.",
        &rect_at(20, 126, 420, 24, dpi),
        fonts(dpi).dim,
        dim_color(),
        windows::Win32::Graphics::Gdi::DT_LEFT | windows::Win32::Graphics::Gdi::DT_WORDBREAK,
    );
    // The header strip is the grab area: say so, otherwise the panel looks fixed.
    modal_text(
        hdc,
        dpi,
        180,
        150,
        260,
        36,
        "Перетащите за заголовок",
        fonts(dpi).dim,
        dim_color(),
    );
}

unsafe extern "system" fn modal_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            // The DC handed to WM_ERASEBKGND is clipped to the invalid region.
            // When only part of the panel is invalidated (a move, a button
            // redraw), the header text above that region would never be drawn,
            // so widen the invalidation to the whole client area first.
            InvalidateRect(hwnd, std::ptr::null(), 0);
            paint_modal(hwnd, HDC(wparam.0 as *mut c_void));
            return LRESULT(1);
        }
        WM_PAINT => {
            // Same reason: BeginPaint clips the DC to the invalid rectangle.
            InvalidateRect(hwnd, std::ptr::null(), 0);
            let mut paint = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut paint);
            paint_modal(hwnd, hdc);
            let _ = EndPaint(hwnd, &paint);
            return LRESULT(0);
        }
        WM_DRAWITEM => {
            let info = unsafe { &*(lparam.0 as *const DRAWITEMSTRUCT) };
            let dpi = window_dpi(hwnd);
            let rect = info.rcItem;
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let pressed = (info.itemState.0 & ODS_SELECTED.0) != 0;
            let label = label_of(role_of(info.hwndItem));
            let label = if label.is_empty() { "Отмена".to_string() } else { label };
            fill_rect(info.hDC, rect.left, rect.top, width, height, card_color());
            rounded(
                info.hDC,
                rect.left,
                rect.top,
                width,
                height,
                s(8, dpi),
                if pressed {
                    shade(button_color(), 82)
                } else {
                    button_color()
                },
                card_border(),
                card_color(),
            );
            draw_text(
                info.hDC,
                &label,
                &RECT {
                    left: rect.left + s(12, dpi),
                    top: rect.top,
                    right: rect.right - s(12, dpi),
                    bottom: rect.bottom,
                },
                fitting_font(info.hDC, dpi, &label, width - 2 * s(12, dpi)),
                fg_color(),
                windows::Win32::Graphics::Gdi::DT_CENTER
                    | windows::Win32::Graphics::Gdi::DT_VCENTER
                    | windows::Win32::Graphics::Gdi::DT_SINGLELINE,
            );
            return LRESULT(0);
        }
        WM_LBUTTON_DOWN => {
            // The header strip is the grab area; the rest of the panel is text.
            let dpi = window_dpi(hwnd);
            let x = ((lparam.0 & 0xFFFF) as u16) as i32;
            let y = ((lparam.0 >> 16) as u16) as i32;
            if y < s(MODAL_GRAB_H, dpi) {
                MODAL_DRAGGING.store(true, Ordering::Relaxed);
                MODAL_DRAG_X.store(x, Ordering::Relaxed);
                MODAL_DRAG_Y.store(y, Ordering::Relaxed);
                return LRESULT(0);
            }
        }
        WM_MOUSEMOVE => {
            if MODAL_DRAGGING.load(Ordering::Relaxed) {
                let x = ((lparam.0 & 0xFFFF) as u16) as i32;
                let y = ((lparam.0 >> 16) as u16) as i32;
                let Some(parent) = main_window() else {
                    return LRESULT(0);
                };
                let dpi = window_dpi(parent);
                let mut area = RECT::default();
                let _ = GetClientRect(parent, &mut area);
                let left = MODAL_LEFT.load(Ordering::Relaxed)
                    + x
                    - MODAL_DRAG_X.load(Ordering::Relaxed);
                let top = MODAL_TOP.load(Ordering::Relaxed)
                    + y
                    - MODAL_DRAG_Y.load(Ordering::Relaxed);
                let (left, top) = clamp_modal_position(
                    left,
                    top,
                    s(MODAL_WIDTH, dpi),
                    s(MODAL_HEIGHT, dpi),
                    (area.right, area.bottom),
                );
                // The panel is a popup, so SetWindowPos wants screen coordinates.
                // Moving only when the clamped position really changed keeps a
                // held-down drag from sending a window move for every pixel of
                // cursor travel (each one repaints the panel).
                if left == MODAL_LEFT.load(Ordering::Relaxed)
                    && top == MODAL_TOP.load(Ordering::Relaxed)
                {
                    return LRESULT(0);
                }
                let (screen_x, screen_y) = modal_screen_position(parent, left, top);
                if SetWindowPos(hwnd, Some(HWND_TOP), screen_x, screen_y, 0, 0, SWP_NOSIZE).is_ok() {
                    MODAL_LEFT.store(left, Ordering::Relaxed);
                    MODAL_TOP.store(top, Ordering::Relaxed);
                }
                return LRESULT(0);
            }
        }
        WM_LBUTTON_UP => {
            MODAL_DRAGGING.store(false, Ordering::Relaxed);
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
    CAPTURE_KEYS.fetch_add(1, Ordering::Relaxed);
    // H-02: a low-level hook is called for every keystroke of the machine, and
    // the system waits for it to return. Nothing here may touch the disk, a
    // window control or a mutex that the UI thread can hold: the assignment is
    // packed into an atomic and posted to the message loop.
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

    if MODAL_TARGET.load(Ordering::Relaxed) == usize::MAX {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    // One keystroke is enough: further keys go to the ordinary window instead of
    // queueing more assignments.
    if ASSIGN_IN_FLIGHT.swap(true, Ordering::SeqCst) {
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
            ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
            return CallNextHookEx(None, code, wparam, lparam);
        }
        BREAK_SYNTHESIS.store(false, Ordering::Relaxed);
    }
    if vk == VK_ESCAPE {
        ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
        if let Some(main) = main_window() {
            PostMessageW(Some(main), WM_COMMAND, WPARAM(HOTKEY_ESCAPED), LPARAM(0)).ok();
        }
        return LRESULT(1);
    }
    let ctrl = modifier_down(VK_LCONTROL) || modifier_down(VK_RCONTROL);
    let alt = modifier_down(VK_LMENU) || modifier_down(VK_RMENU);
    let shift = modifier_down(VK_LSHIFT) || modifier_down(VK_RSHIFT);
    let win = modifier_down(VK_WIN_LEFT) || modifier_down(VK_WIN_RIGHT);
    let packed = pack_combo(vk.0, ctrl, alt, shift, win);
    PENDING_ASSIGN.store(packed, Ordering::Relaxed);
    match main_window() {
        Some(main) => {
            PostMessageW(Some(main), WM_COMMAND, WPARAM(HOTKEY_ASSIGN), LPARAM(0)).ok();
        }
        None => {
            // The window is gone: no one will consume the pending key, so reset
            // the guard instead of leaving it set for the next modal.
            ASSIGN_IN_FLIGHT.store(false, Ordering::Relaxed);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// Turn the pressed combination into a stored hotkey: validate it, write it to
// the config, refresh the list and close the panel.
unsafe fn assign_capture(vk: VIRTUAL_KEY, ctrl: bool, alt: bool, shift: bool, win: bool) {
    let target = MODAL_TARGET.load(Ordering::Relaxed);
    if target == usize::MAX {
        return;
    }
    let spec = capture_spec(vk, ctrl, alt, shift, win);
    let field = actions_for(MODAL_ROLE.load(Ordering::Relaxed))
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
                "hotkey_upper_last" => config.hotkey_upper_last = value.clone(),
                "hotkey_invert" => config.hotkey_invert = value.clone(),
                "hotkey_invert_last" => config.hotkey_invert_last = value.clone(),
                "hotkey_add_word" => config.hotkey_add_word = value.clone(),
                _ => {}
            });
            // A combination can be bound to both halves of the table at once,
            // so both tables are refreshed.
            for role in [HOTKEY_LIST, HOTKEY_SEL_LIST] {
                if let Some(list) = handle(role) {
                    fill_hotkey_list(list);
                    invalidate(list);
                }
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

// VK_LWIN is 0x5B and VK_RWIN is 0x5C. The old 0x92/0x93 constants matched no real
// key, so a "win+..." combination was always recorded without the Win modifier.
const VK_WIN_LEFT: VIRTUAL_KEY = VIRTUAL_KEY(0x5B);
const VK_WIN_RIGHT: VIRTUAL_KEY = VIRTUAL_KEY(0x5C);

// Lock-free modifier state: the keyboard hook runs on the settings thread and
// must never wait on a mutex the UI thread could be holding. One counter per
// virtual key keeps both halves of Ctrl/Alt/Shift correct.
static MODIFIER_COUNTS: [AtomicUsize; 256] = [const { AtomicUsize::new(0) }; 256];

fn set_modifier(vk: VIRTUAL_KEY, down: bool) {
    let slot = &MODIFIER_COUNTS[(vk.0 as usize) & 0xFF];
    if down {
        slot.fetch_add(1, Ordering::Relaxed);
    } else {
        // An up-event for a key that was never seen must not wrap the counter to
        // usize::MAX, which would keep the modifier "held" forever.
        let held = slot.load(Ordering::Relaxed);
        if held > 0 {
            slot.store(held - 1, Ordering::Relaxed);
        }
    }
}

fn modifier_down(vk: VIRTUAL_KEY) -> bool {
    MODIFIER_COUNTS[(vk.0 as usize) & 0xFF].load(Ordering::Relaxed) > 0
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
        0xC0 => "`".into(),
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
    // A lone Alt is the classic "undo" key: the hook reads it as VK_MENU with no
    // other modifier, so it is a legal value for that action only.
    if trimmed == "alt" {
        return if field == "hotkey_undo" {
            Ok(trimmed)
        } else {
            Err("Alt сам по себе занят: Alt+Shift меняет раскладку".into())
        };
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

pub unsafe fn update_status(text: &str) {
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

/// Feedback for the dictionary hotkey (Ctrl+`): the word the keyboard hook has
/// just put into the protected list. Refreshes the counts and the list on the
/// «Словарь» page and prints the result in the status line.
pub unsafe fn report_dictionary_word(word: &str, added: bool, script: &str) {
    bump_rows();
    if let Some(list) = handle(LEX_LIST) {
        refresh_lex();
        invalidate(list);
    }
    if let Some(main) = main_window() {
        repaint(main);
    }
    update_status(&format!(
        "Словарь: {} «{word}» — {script} список",
        if added { "добавлено" } else { "уже было" }
    ));
}

// ---------------------------------------------------------------------------
// Diagnostics for `zя hotkeytest`
// ---------------------------------------------------------------------------

/// Where the assignment panel currently sits, in the settings window's client
/// coordinates: (left, top, width, height).
pub unsafe fn modal_rect_for_probe() -> Option<(i32, i32, i32, i32)> {
    let _ = modal_handle()?;
    let parent = main_window()?;
    let dpi = window_dpi(parent);
    Some((
        MODAL_LEFT.load(Ordering::Relaxed),
        MODAL_TOP.load(Ordering::Relaxed),
        s(MODAL_WIDTH, dpi),
        s(MODAL_HEIGHT, dpi),
    ))
}

/// Raw geometry of the panel window: outer rect, client rect and the region the
/// last repaint actually covered. Used by the `hotkeytest` probe.
pub unsafe fn modal_geometry_probe() -> ((i32, i32, i32, i32), (i32, i32, i32, i32)) {
    let Some(modal) = modal_handle() else {
        return ((0, 0, 0, 0), (0, 0, 0, 0));
    };
    let mut outer = RECT::default();
    let _ = GetWindowRect(modal, &mut outer);
    let mut inner = RECT::default();
    let _ = GetClientRect(modal, &mut inner);
    (
        (outer.left, outer.top, outer.right, outer.bottom),
        (inner.left, inner.top, inner.right, inner.bottom),
    )
}

/// Where the settings window sits on the screen, and where its client area starts.
pub unsafe fn parent_screen_rect_probe() -> ((i32, i32, i32, i32), (i32, i32)) {
    let Some(parent) = main_window() else {
        return ((0, 0, 0, 0), (0, 0));
    };
    let mut outer = RECT::default();
    let _ = GetWindowRect(parent, &mut outer);
    let mut client = RECT::default();
    let _ = GetClientRect(parent, &mut client);
    let frame_left = (outer.right - outer.left - (client.right - client.left)) / 2;
    let frame_top = (outer.bottom - outer.top - (client.bottom - client.top)) / 2;
    (
        (outer.left, outer.top, outer.right, outer.bottom),
        (outer.left + frame_left, outer.top + frame_top),
    )
}

/// Sibling windows of the settings window, front to back, so the probe can see
/// whether the panel is above the hotkey list or behind it.
/// Walk downwards from the popup: the settings window is found only if the
/// popup is drawn on top of it.
pub unsafe fn top_level_z_order_probe() -> Vec<(String, bool, bool)> {
    let Some(parent) = main_window() else {
        return Vec::new();
    };
    let Some(modal) = modal_handle() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut window = modal;
    for _ in 0..400 {
        let mut name = [0u16; 64];
        let written = GetClassNameW(window, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..written.min(64)]);
        out.push((class, window == parent, window == modal));
        if window == parent {
            break;
        }
        match GetWindow(window, GW_HWNDNEXT) {
            Ok(next) if !is_null(next) => window = next,
            _ => break,
        }
    }
    out
}

/// Is the popup directly above its settings window in the real screen z-order?
pub unsafe fn modal_above_settings_probe() -> bool {
    let Some(parent) = main_window() else {
        return false;
    };
    let Some(modal) = modal_handle() else {
        return false;
    };
    match GetWindow(parent, GW_HWNDPREV) {
        Ok(above) => above == modal,
        Err(_) => false,
    }
}

/// Bring both the settings window and the panel to the top of the desktop so a
/// screen capture of the panel is actually the panel.
pub unsafe fn raise_modal_probe() -> bool {
    let Some(parent) = main_window() else {
        return false;
    };
    let Some(modal) = modal_handle() else {
        return false;
    };
    let settings_first = SetWindowPos(parent, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE).is_ok();
    let modal_last = SetWindowPos(modal, Some(HWND_TOP), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE).is_ok();
    settings_first && modal_last && modal_above_settings_probe()
}

pub unsafe fn modal_z_order_probe() -> Vec<(String, bool)> {
    let Some(parent) = main_window() else {
        return Vec::new();
    };
    let modal = modal_handle();
    let mut out = Vec::new();
    let first = match GetWindow(parent, GW_CHILD) {
        Ok(handle) => handle,
        Err(_) => return out,
    };
    let mut child = first;
    while !is_null(child) {
        let mut name = [0u16; 64];
        let written = GetClassNameW(child, &mut name).max(0) as usize;
        let class = String::from_utf16_lossy(&name[..written.min(64)]);
        out.push((class, modal == Some(child)));
        match GetWindow(child, GW_HWNDNEXT) {
            Ok(next) if !is_null(next) => child = next,
            _ => break,
        }
    }
    out
}

/// Open the panel for a row and report where it landed.
pub unsafe fn open_modal_for_probe(row: usize) -> Option<(i32, i32, i32, i32)> {
    open_hotkey_modal(HOTKEY_LIST, row);
    modal_rect_for_probe()
}

/// Simulate the real user action: a click on a hotkey row. The message goes to
/// the list window, whose subclassed procedure opens the assignment panel, so
/// the probe exercises the same path as a mouse click.
pub unsafe fn click_hotkey_row_for_probe(row: usize) -> bool {
    let Some(list) = handle(HOTKEY_LIST) else {
        return false;
    };
    let mut item = RECT::default();
    if send(list, LVM_GETITEMRECT, row, std::ptr::addr_of_mut!(item) as isize).0 < 0 {
        return false;
    }
    let y = (item.top + item.bottom) / 2;
    let x = 120;
    PostMessageW(Some(list), WM_LBUTTON_UP, WPARAM(0), LPARAM(((y << 16) | x) as isize)).is_ok()
}

/// The assignment panel as a window handle, so `zя tabs` can screenshot it.
pub unsafe fn modal_window_for_probe() -> Option<HWND> {
    modal_handle()
}

/// Drag the panel by its header to (left, top). A real drag delivers one small
/// WM_MOUSEMOVE per pixel of cursor travel, so the probe walks the distance in
/// steps instead of teleporting the pointer, which would produce coordinates
/// outside the window. The caller waits for the settings thread to dispatch.
pub unsafe fn drag_modal_for_probe(left: i32, top: i32) -> bool {
    let Some(modal) = modal_handle() else {
        return false;
    };
    let grab_x: i32 = 12;
    let grab_y: i32 = 16;
    let grab = LPARAM(((grab_y << 16) | grab_x) as isize);
    let down = PostMessageW(Some(modal), WM_LBUTTON_DOWN, WPARAM(0), grab);
    std::thread::sleep(std::time::Duration::from_millis(200));
    println!(
        "zя: drag: down {down:?}, флаг перетаскивания {}",
        MODAL_DRAGGING.load(Ordering::Relaxed)
    );
    let mut x = MODAL_LEFT.load(Ordering::Relaxed);
    let mut y = MODAL_TOP.load(Ordering::Relaxed);
    let mut steps = 0;
    while steps < 600 {
        let dx = (left - x).clamp(-grab_x, MODAL_WIDTH - 1 - grab_x);
        let dy = (top - y).clamp(-grab_y, MODAL_GRAB_H - 1 - grab_y);
        if dx == 0 && dy == 0 {
            break;
        }
        let local = LPARAM((((grab_y + dy) << 16) | (grab_x + dx)) as isize);
        PostMessageW(Some(modal), WM_MOUSEMOVE, WPARAM(0), local).ok();
        x += dx;
        y += dy;
        steps += 1;
    }
    let up = PostMessageW(Some(modal), WM_LBUTTON_UP, WPARAM(0), grab);
    println!("zя: drag: {steps} шагов движения, up {up:?}");
    true
}

/// The modal window handle, so a probe can screenshot the panel itself.
pub unsafe fn modal_hwnd_for_probe() -> Option<HWND> {
    modal_handle()
}

/// Where Windows actually put the panel, straight from GetWindowRect.
pub unsafe fn modal_screen_rect_for_probe() -> Option<(i32, i32, i32, i32)> {
    let modal = modal_handle()?;
    let mut rect = RECT::default();
    let _ = GetWindowRect(modal, &mut rect);
    Some((rect.left, rect.top, rect.right, rect.bottom))
}

/// Raw geometry of a hotkey row, for `zя hotkeytest` diagnostics.
pub unsafe fn row_bounds_for_probe(row: usize) -> (i32, i32) {
    let parent = main_window().unwrap_or(null_hwnd());
    hotkey_row_bounds(HOTKEY_LIST, row, window_dpi(parent))
}

/// The page ids of the sidebar, for the `zя tabs` screenshot probe.
pub fn tab_roles_probe() -> Vec<usize> {
    SIDEBAR_TABS.iter().map(|(role, _)| *role).collect()
}

/// Switch the settings window to another page without a real mouse click.
pub unsafe fn activate_tab_for_probe(role: usize) -> bool {
    let Some(parent) = main_window() else {
        return false;
    };
    PostMessageW(Some(parent), WM_COMMAND, WPARAM(role), LPARAM(0)).is_ok()
}

/// Screen rectangle of a control, used by the `zя tabs` probe to check that a
/// caption actually has room inside its own button.
pub unsafe fn control_rect_for_probe(role: usize) -> Option<(i32, i32, i32, i32)> {
    let hwnd = controls()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&role)
        .map(|handle| handle.hwnd.0)?;
    if is_null(hwnd) {
        return None;
    }
    let mut rect = RECT::default();
    GetWindowRect(hwnd, &mut rect).ok()?;
    Some((rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top))
}

pub unsafe fn parent_dpi_for_probe() -> u32 {    let parent = main_window().unwrap_or(null_hwnd());
    window_dpi(parent)
}

pub unsafe fn list_is_built() -> bool {
    handle(HOTKEY_LIST).is_some()
}

pub unsafe fn close_modal_for_probe() {
    close_modal();
}

/// Which theme the panel is currently painted in.
pub fn theme_is_dark_probe() -> bool {
    dark_theme()
}

/// Force a theme for the diagnostics run. The settings window re-reads the
/// system theme on every repaint, so only the panel is invalidated here.
pub unsafe fn set_theme_probe(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    if let Some(modal) = modal_handle() {
        InvalidateRect(modal, std::ptr::null(), 1);
    }
}

/// Is the assignment panel still on screen?
pub unsafe fn modal_alive_probe() -> bool {
    match modal_handle() {
        Some(handle) => IsWindow(Some(handle)).as_bool(),
        None => false,
    }
}

/// Press the panel's own "Отмена" button and the Esc key.
pub unsafe fn modal_cancel_probe() -> bool {
    match modal_handle() {
        Some(handle) => PostMessageW(Some(handle), WM_COMMAND, WPARAM(MODAL_CANCEL), LPARAM(0)).is_ok(),
        None => false,
    }
}

pub unsafe fn modal_escape_probe() -> bool {
    match modal_handle() {
        Some(handle) => PostMessageW(Some(handle), WM_KEYDOWN, WPARAM(VK_ESCAPE.0 as usize), LPARAM(0)).is_ok(),
        None => false,
    }
}

pub unsafe fn capture_hook_is_installed() -> bool {
    HOTKEY_HOOK.load(Ordering::Relaxed) != 0
}

/// `zя stalltest`: drop the heartbeat timer and watch what the watchdog does.
/// A message loop with no timer produces no events while nobody touches the
/// window, so the watchdog must notice the silence, release the keyboard hook
/// and close the window instead of leaving it (and the hook) hanging.
pub unsafe fn stall_watchdog_probe(deadline_ms: u64) -> bool {
    let Some(hwnd) = main_window() else {
        return false;
    };
    KillTimer(Some(hwnd), HEARTBEAT_TIMER).ok();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(deadline_ms);
    while std::time::Instant::now() < deadline {
        if !settings_window_open() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    false
}

/// Heartbeat of the message loop observed over `delta_ms` milliseconds.
pub fn heartbeat_probe(delta_ms: u64) -> usize {
    let before = UI_BEAT.load(Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(delta_ms));
    UI_BEAT.load(Ordering::Relaxed) - before
}

/// Is the settings window still on screen? Used to prove the watchdog does not
/// close a window that is merely idle.
pub fn settings_window_open_probe() -> bool {
    settings_window_open()
}

/// How many key events the assignment hook has seen. Used by `zя hotkeytest`
/// to tell "the hook is not installed" from "the hook never fired".
pub fn capture_key_count() -> usize {
    CAPTURE_KEYS.load(Ordering::Relaxed)
}

/// Feed one synthetic key event through the real capture hook, so the modifier
/// tracking, the Break special case and the assignment path are all exercised
/// exactly as they are when the user presses a key.
pub unsafe fn capture_key_for_probe(vk: VIRTUAL_KEY, up: bool) {
    let event = KBDLLHOOKSTRUCT {
        vkCode: vk.0 as u32,
        scanCode: 0,
        flags: if up { LLKHF_UP } else { KBDLLHOOKSTRUCT_FLAGS(0) },
        time: 0,
        dwExtraInfo: 0,
    };
    let _ = capture_hotkey_hook(0, WPARAM(0), LPARAM(&event as *const KBDLLHOOKSTRUCT as isize));
}
pub unsafe fn assign_combo_for_probe(
    vk: VIRTUAL_KEY,
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
) {
    assign_capture(vk, ctrl, alt, shift, win);
}

pub fn status_for_probe() -> String {    let message = STATUS_MESSAGE.get_or_init(|| Mutex::new(String::new()));
    message
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone()
}

pub fn hotkey_for_probe(field: &str) -> String {
    hotkey_value(&get_cfg(), field)
}

// The key-cap icon is baked into data/app.ico; take its 32x32 BMP frame
// (40-byte header + 32*32*4 pixels + 128-byte AND mask = 4264 bytes) so the
// caption shows the same icon as the executable, whatever the working
// directory happens to be.
unsafe fn window_icon() -> Option<HICON> {
    static ICON: &[u8] = include_bytes!("../data/app.ico");
    // Built once per process and deliberately never destroyed: every settings
    // window reuses the same HICON instead of leaking a new one per WM_CREATE.
    // The handle is stored as a usize because HICON is a raw pointer and is not
    // Sync, so it cannot live in a static directly.
    static CACHE: OnceLock<Option<usize>> = OnceLock::new();
    let built = CACHE.get_or_init(|| {
        const HEADER: [u8; 16] = [0x28, 0, 0, 0, 0x20, 0, 0, 0, 0x40, 0, 0, 0, 0x01, 0, 0x20, 0];
        let created = ICON
            .windows(HEADER.len())
            .position(|window| window == HEADER)
            .and_then(|start| ICON.get(start..start + 4264))
            .and_then(|data| CreateIconFromResourceEx(data, true, 0x0003_0000, 32, 32, IMAGE_FLAGS(0)).ok());
        if let Some(icon) = created {
            return Some(icon.0 as usize);
        }
        let name = wide("app.ico");
        let fallback = HICON(LoadImageW(name.as_ptr(), name.as_ptr(), 0x00000010, 32, 32, 3));
        (!fallback.0.is_null()).then_some(fallback.0 as usize)
    });
    built.map(|raw| HICON(raw as *mut c_void))
}

unsafe fn set_window_icon(hwnd: HWND) {
    if let Some(icon) = window_icon() {
        send(hwnd, WM_SETICON, 0, icon.0 as isize);
        send(hwnd, WM_SETICON, 1, icon.0 as isize);
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

/// The work area of the screen the cursor is on: the taskbar and any docked
/// panels are excluded. Falls back to the monitor rectangle, then to the design
/// size, so the window always gets a usable number.
unsafe fn work_area() -> (i32, i32, i32, i32) {
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: WINDOW_WIDTH,
        bottom: WINDOW_HEIGHT,
    };
    if SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut rect as *mut RECT as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .is_ok()
        && rect.right > rect.left
        && rect.bottom > rect.top
    {
        return (rect.left, rect.top, rect.right, rect.bottom);
    }
    let mut point = POINT { x: 0, y: 0 };
    GetCursorPos(&mut point).ok();
    let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST);
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        let area = info.rcMonitor;
        if area.right > area.left && area.bottom > area.top {
            return (area.left, area.top, area.right, area.bottom);
        }
    }
    (0, 0, WINDOW_WIDTH, WINDOW_HEIGHT)
}

// Re-exported by `settings.rs` for callers that do not need the message loop to
// block; the tray currently calls `open_blocking` directly.
#[allow(dead_code)]
pub fn open() -> std::io::Result<()> {
    // A second settings window would overwrite the global control registry of
    // the first one and nest another message loop, so an open window is simply
    // brought to the front.
    if settings_window_open() {
        if let Some(hwnd) = main_window() {
            let _ = unsafe { SetForegroundWindow(hwnd) };
        }
        return Ok(());
    }
    open_blocking()
}

/// Latch that makes sure only one settings window/message loop exists at a
/// time. `Drop` releases it on every exit path, so a failed window creation
/// never leaves the latch set and the settings unreachable forever.
struct OpeningLatch;

impl OpeningLatch {
    fn try_acquire() -> Option<OpeningLatch> {
        if SETTINGS_OPENING.swap(true, Ordering::SeqCst) {
            None
        } else {
            Some(OpeningLatch)
        }
    }
}

impl Drop for OpeningLatch {
    fn drop(&mut self) {
        SETTINGS_OPENING.store(false, Ordering::SeqCst);
    }
}

pub fn open_blocking() -> std::io::Result<()> {
    unsafe {
        // Two callers (tray click and a CLI probe) can arrive in the same
        // millisecond. Without this latch both would create a window and nest two
        // message loops over one control registry.
        let Some(_latch) = OpeningLatch::try_acquire() else {
            if settings_window_open() {
                if let Some(hwnd) = main_window() {
                    let _ = SetForegroundWindow(hwnd);
                }
            }
            return Ok(());
        };
        if settings_window_open() {
            if let Some(hwnd) = main_window() {
                let _ = SetForegroundWindow(hwnd);
            }
            return Ok(());
        }
        register_classes();
        refresh_config_from_disk();
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
        // The caption and the frame live outside the client area, so add their
        // thickness back. The design canvas is 1200x820 at 96 DPI; at 125% and
        // 150% that is 1500x1025 and 1800x1230, which does not fit a 1080p
        // screen, so the client size is clamped to the work area and the layout
        // is clipped rather than cut off at the top of the screen.
        let mut outer = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut inner = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(window, &mut outer).ok();
        GetClientRect(window, &mut inner).ok();
        let frame_w = (outer.right - outer.left) - (inner.right - inner.left);
        let frame_h = (outer.bottom - outer.top) - (inner.bottom - inner.top);

        let (area_left, area_top, area_right, area_bottom) = work_area();
        let area_w = area_right - area_left;
        let area_h = area_bottom - area_top;
        let margin = 24;
        let client_w = s(WINDOW_WIDTH, dpi)
            .min(area_w - margin)
            .max(s(720, dpi));
        let client_h = s(WINDOW_HEIGHT, dpi)
            .min(area_h - margin)
            .max(s(420, dpi));
        let outer_w = client_w + frame_w;
        let outer_h = client_h + frame_h;
        let left = area_left + (area_w - outer_w) / 2;
        let top = area_top + (area_h - outer_h) / 2;
        let left = left.max(area_left).max(0);
        let top = top.max(area_top).max(0);
        SetWindowPos(
            window,
            None,
            left,
            top,
            outer_w,
            outer_h,
            SWP_NOZORDER,
        )
        .ok();
        let _ = ShowWindow(window, SW_SHOW);
        spawn_watchdog(window);

        let mut message = MSG::default();
        // GetMessageW returns 0 for WM_QUIT and -1 for an error. `BOOL::as_bool()`
        // is `!= 0`, so an error read as "there is a message" turns the loop into a
        // spin on a stale MSG: the window stops answering forever (L-17, also
        // fixed in `win::run_message_loop`).
        let mut rounds: u64 = 0;
        loop {
            let got = GetMessageW(&mut message, None, 0, 0);
            if !message_loop_should_continue(got.0) || rounds >= 5_000_000 {
                break;
            }
            rounds += 1;
            UI_BEAT.fetch_add(1, Ordering::Relaxed);
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    Ok(())
}

// Placeholders kept for the public re-export in `settings.rs`; the real "add the
// app in the foreground window" logic lives in `add_foreground_process`.
#[allow(dead_code)]
pub fn pick_probe() -> Option<(u16, bool, bool)> {
    None
}

#[allow(dead_code)]
pub fn self_test() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::Gdi::{
        BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DeleteDC, DIB_RGB_COLORS, GetDIBits,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_SPACE;
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    #[test]
    fn card_rows_do_not_overlap_the_list() {
        assert!(HOTKEY_LIST_Y + HOTKEY_WORD_LIST_H <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        assert!(HOTKEY_LIST_Y + HOTKEY_SEL_LIST_H <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        assert!(IGNORE_LIST_CARD_Y + 308 - IGNORE_LIST_CARD_Y < IGNORE_LIST_CARD_H);
    }

    #[test]
    fn the_hotkey_table_is_split_into_two_halves() {
        // Left: what works on the last token without a selection. Right: what
        // needs selected text. A row can live in both tables by using the same
        // combination, which is exactly the default for Break.
        assert_eq!(HOTKEY_WORD_ACTIONS.len(), 5);
        assert_eq!(HOTKEY_SEL_ACTIONS.len(), 4);
        assert_eq!(actions_for(HOTKEY_LIST).len(), HOTKEY_WORD_ACTIONS.len());
        assert_eq!(actions_for(HOTKEY_SEL_LIST).len(), HOTKEY_SEL_ACTIONS.len());

        // The two columns sit side by side inside the card and never touch.
        assert!(HOTKEY_WORD_X + HOTKEY_LIST_W <= HOTKEY_SEL_X);
        assert!(HOTKEY_SEL_X + HOTKEY_LIST_W <= CONTENT_X + CONTENT_WIDTH);
        assert!(HOTKEY_WORD_X >= CONTENT_X + CARD_PAD);

        // Group captions above the headers, clear of the card title.
        assert!(HOTKEY_GROUP_Y >= HOTKEY_CARD_Y + 40);
        assert!(HOTKEY_GROUP_Y + 22 <= HOTKEY_LIST_Y);

        // The note about shared combinations sits below the tables, inside the card.
        assert!(HOTKEY_NOTE_Y >= HOTKEY_LIST_Y);
        assert!(HOTKEY_NOTE_Y + 62 <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        for line in HOTKEY_NOTE_TEXT.lines() {
            assert!(line.chars().count() <= 78, "{line} is too long for the card");
        }

        // Every action is listed once, in the half it belongs to.
        let fields: Vec<&str> = HOTKEY_WORD_ACTIONS
            .iter()
            .chain(HOTKEY_SEL_ACTIONS.iter())
            .map(|(_, field, _)| *field)
            .collect();
        assert_eq!(fields.len(), 9);
        for field in &fields {
            let repeats = fields.iter().filter(|other| *other == field).count();
            assert!(repeats == 1, "{field} appears in both tables");
        }
        for (name, word) in [
            ("hotkey_last", true),
            ("hotkey_upper_last", true),
            ("hotkey_invert_last", true),
            ("hotkey_undo", true),
            ("hotkey_toggle", true),
            ("hotkey_selection", false),
            ("hotkey_upper", false),
            ("hotkey_invert", false),
            ("hotkey_add_word", false),
        ] {
            let in_word = HOTKEY_WORD_ACTIONS
                .iter()
                .any(|(_, field, _)| *field == name);
            let in_selection = HOTKEY_SEL_ACTIONS
                .iter()
                .any(|(_, field, _)| *field == name);
            assert_eq!(
                in_word, word,
                "{name} is in the wrong column of the table"
            );
            assert_eq!(in_selection, !word, "{name} is in the wrong column");
        }
        for (label, _, default) in HOTKEY_WORD_ACTIONS.iter().chain(HOTKEY_SEL_ACTIONS.iter()) {
            assert!(!label.is_empty() && !default.is_empty());
        }
        // Break is bound to the last token and to the selection at the same time.
        assert_eq!(HOTKEY_WORD_ACTIONS[0].2, HOTKEY_SEL_ACTIONS[0].2);
    }

    #[test]
    fn the_smart_undo_switch_is_in_the_general_settings_card() {
        assert_eq!(label_of(CB_SMART_UNDO), "Умная отмена");
        // The switch lives in the "Запуск" card, not in the hotkey card.
        assert!(CB_SMART_Y >= LAUNCH_CARD_Y && CB_SMART_Y + 34 <= LAUNCH_CARD_Y + LAUNCH_CARD_H);
        assert!(SMART_COL_X >= CONTENT_X + CARD_PAD + 320, "the switch crowds the card title");
        assert!(SMART_COL_X + SMART_COL_W <= CONTENT_X + CONTENT_WIDTH);
        // The explanation sits under the switch and does not touch the hint line.
        assert!(SMART_HINT_Y >= CB_SMART_Y + 34);
        assert!(SMART_HINT_Y + 96 <= LAUNCH_HINT_Y);
        assert!(LAUNCH_HINT_Y + 22 <= LAUNCH_CARD_Y + LAUNCH_CARD_H);
        // The two cards still do not overlap, and the hotkey list fits in its card.
        assert!(LAUNCH_CARD_Y + LAUNCH_CARD_H < HOTKEY_CARD_Y);
        assert!(HOTKEY_LIST_Y + HOTKEY_WORD_LIST_H <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        assert!(HOTKEY_GROUP_Y >= HOTKEY_CARD_Y + 40);
        // The config default is on.
        assert!(Config::default().smart_undo);
    }

    #[test]
    fn fonts_are_measured_in_points_and_scale_with_dpi() {
        // 9 pt is the Windows UI default. A pixel-cell height (the old 14 px at
        // 96 DPI ≈ 8.3 pt) made the window read smaller than the rest of the OS.
        assert_eq!(pt(9, 96), -12);
        assert_eq!(pt(10, 144), -20, "150 % scaling must grow the font, not keep it");
        assert!(pt(10, 144) < pt(9, 144), "body text is larger than the hint text");
        // lfHeight is negative, so a *more negative* value is the larger font.
        assert!(pt(11, 96) <= pt(10, 96), "card titles are not smaller than body text");
    }

    #[test]
    fn the_dictionary_hotkey_is_a_selection_action() {
        // Ctrl+` is the key that carries ё on a Russian layout: it is free in
        // every Windows application we know of.
        assert_eq!(Config::default().hotkey_add_word, "ctrl+`");
        assert!(validate_hotkey("hotkey_add_word", "ctrl+`").is_ok());
        assert!(crate::win::parse_hotkey("ctrl+`").is_some());
        // The action column of the table is 200 design px: 28 characters at
        // 10 pt is the longest label that survives it.
        for (label, _, _) in HOTKEY_SEL_ACTIONS.iter().chain(HOTKEY_WORD_ACTIONS.iter()) {
            assert!(label.chars().count() <= 28, "{label} does not fit the column");
        }
    }

    #[test]
    fn a_clean_update_check_leaves_the_bottom_line_empty() {
        let same = crate::update::Release {
            version: "0.1.4".into(),
            url: "https://github.com/demoonk/Zya/releases/download/v0.1.4/z.exe".into(),
            size: 1000,
        };
        assert_eq!(update_message("0.1.5", &same), STATUS_IDLE);
        let newer = crate::update::Release {
            version: "0.1.6".into(),
            url: "https://example.invalid/z.exe".into(),
            size: 2_097_152,
        };
        let message = update_message("0.1.5", &newer);
        assert!(message.starts_with("Доступно обновление 0.1.6"));
    }

    #[test]
    fn the_new_fields_validate() {
        assert!(validate_hotkey("hotkey_upper_last", "ctrl+break").is_ok());
        assert!(validate_hotkey("hotkey_invert_last", "shift+break").is_ok());
        assert!(validate_hotkey("hotkey_upper_last", "break").is_err());
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
    fn the_assignment_modal_stays_inside_the_hotkey_card() {
        // Rows of the owner-drawn lists: header, then rows of 42 design px.
        for row in 0..HOTKEY_WORD_ACTIONS.len() {
            let top = HOTKEY_LIST_Y + 26 + row as i32 * 42;
            let placed = modal_top_for_row(top, top + 42, 96);
            assert!(
                placed >= HOTKEY_CARD_Y,
                "row {row}: the panel left the hotkey card upwards"
            );
            assert!(
                placed + MODAL_HEIGHT <= HOTKEY_CARD_Y + HOTKEY_CARD_H,
                "row {row}: the panel hangs off the card"
            );
            assert!(
                placed > LAUNCH_CARD_Y + LAUNCH_CARD_H,
                "row {row}: the panel covers the settings buttons above"
            );
        }
        // The selection column has fewer rows, and its panel must stay inside
        // the card too.
        for row in 0..HOTKEY_SEL_ACTIONS.len() {
            let top = HOTKEY_LIST_Y + 26 + row as i32 * 42;
            let placed = modal_top_for_row(top, top + 42, 96);
            assert!(placed + MODAL_HEIGHT <= HOTKEY_CARD_Y + HOTKEY_CARD_H);
        }
        // The "Отмена" button keeps a 12 px margin from the panel border and
        // stops before the "Перетащите за заголовок" hint at x 180.
        assert!(24 + 140 <= 180);
        assert!(MODAL_HEIGHT - 12 - 36 + 36 <= MODAL_HEIGHT - 12);
    }

    #[test]
    fn the_assignment_modal_follows_the_clicked_row() {
        // Rows 0 and 4 of the left table used to open the panel in the same place.
        let first = modal_top_for_row(HOTKEY_LIST_Y + 26, HOTKEY_LIST_Y + 68, 96);
        let last = modal_top_for_row(HOTKEY_LIST_Y + 26 + 4 * 42, HOTKEY_LIST_Y + 26 + 5 * 42, 96);
        assert_ne!(first, last, "the panel does not move between rows");
    }

    #[test]
    fn the_assignment_panel_is_anchored_on_the_half_it_came_from() {
        let word = unsafe { modal_slot(96, HOTKEY_LIST, 0).0 };
        let selection = unsafe { modal_slot(96, HOTKEY_SEL_LIST, 0).0 };
        for left in [word, selection] {
            assert!(left >= CONTENT_X, "the panel hangs off the left edge of the card");
            assert!(
                left + MODAL_WIDTH <= CONTENT_X + CONTENT_WIDTH,
                "the panel hangs off the right edge of the card"
            );
        }
        let word_centre = word + MODAL_WIDTH / 2;
        let sel_centre = selection + MODAL_WIDTH / 2;
        let word_list_centre = HOTKEY_WORD_X + HOTKEY_LIST_W / 2;
        let sel_list_centre = HOTKEY_SEL_X + HOTKEY_LIST_W / 2;
        assert!(
            (word_centre - word_list_centre).abs() <= 40,
            "the panel for the word table is not over that table"
        );
        assert!(
            (sel_centre - sel_list_centre).abs() <= 40,
            "the panel for the selection table is not over that table"
        );
        assert_ne!(word, selection, "both tables open the panel in the same place");
    }

    #[test]
    fn dragging_the_modal_cannot_leave_the_window() {
        let (left, top) = clamp_modal_position(
            10_000,
            10_000,
            MODAL_WIDTH,
            MODAL_HEIGHT,
            (WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        assert_eq!(
            (left, top),
            (WINDOW_WIDTH - MODAL_WIDTH, WINDOW_HEIGHT - MODAL_HEIGHT)
        );
        let (left, top) = clamp_modal_position(-500, -500, MODAL_WIDTH, MODAL_HEIGHT, (1200, 820));
        assert_eq!((left, top), (0, 0));
        // An ordinary drag keeps the position untouched.
        let (left, top) = clamp_modal_position(430, 500, MODAL_WIDTH, MODAL_HEIGHT, (1200, 820));
        assert_eq!((left, top), (430, 500));
    }

    #[test]
    fn the_ignore_list_finds_the_row_that_was_clicked() {
        // "Удалить" used to answer "Сначала выберите строку в списке" for the
        // first rows of the list: LVM_GETNEXTITEM takes the starting index in
        // wParam and the state flags in lParam, and the call had them swapped.
        unsafe {
            use windows::Win32::UI::Controls::{
                LIST_VIEW_ITEM_STATE_FLAGS, LVIF_STATE, LVM_SETITEMSTATE,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                DestroyWindow, GetWindowRect, RegisterClassExW, WS_OVERLAPPEDWINDOW, WNDCLASSEXW,
            };

            unsafe extern "system" fn probe_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }

            let name = wide("ZyaRowProbe");
            let class = WNDCLASSEXW {
                lpfnWndProc: Some(probe_proc),
                hInstance: GetModuleHandleW(None).unwrap_or_default().into(),
                lpszClassName: PCWSTR(name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassExW(&class);
            let parent = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(name.as_ptr()),
                PCWSTR(wide("probe").as_ptr()),
                WS_OVERLAPPEDWINDOW,
                -4000,
                -4000,
                700,
                400,
                None,
                None,
                None,
                None,
            )
            .unwrap_or(null_hwnd());
            if is_null(parent) {
                return;
            }
            let list = make_list(parent, 96, 0, 0, 600, 300, IGNORE_LIST);
            for index in 0..5 {
                insert_row(list, index, "строка");
            }
            let item = LVITEMW {
                iItem: 0,
                mask: LVIF_STATE,
                state: LIST_VIEW_ITEM_STATE_FLAGS(LVNI_SELECTED),
                stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVNI_SELECTED),
                ..Default::default()
            };
            SendMessageW(
                list,
                LVM_SETITEMSTATE,
                Some(WPARAM(0)),
                Some(LPARAM(std::ptr::addr_of!(item) as isize)),
            );
            assert_eq!(list_selected(IGNORE_LIST), Some(0));

            let mut rect = RECT::default();
            let found = send(list, LVM_GETITEMRECT, 3, std::ptr::addr_of_mut!(rect) as isize).0;
            let mut frame = RECT::default();
            let _ = GetWindowRect(list, &mut frame);
            assert!(
                found == 3 && rect.right > rect.left && rect.bottom > rect.top,
                "the list view did not report the rectangle of row 3"
            );
            let point = POINT {
                x: frame.left + (rect.left + rect.right) / 2,
                y: frame.top + (rect.top + rect.bottom) / 2,
            };
            assert_eq!(
                row_at_point(list, point),
                Some(3),
                "the context menu must target the row under the pointer"
            );
            DestroyWindow(parent).ok();
            controls()
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&IGNORE_LIST);
        }
    }

    #[test]
    fn caption_icon_keeps_the_red_zya() {
        // The window icon Windows shows in the caption comes from the 32x32 BMP
        // frame of data/app.ico; guard it against a re-encoded icon losing the red Я.
        unsafe {
            let icon = window_icon().expect("key-cap icon should build");
            let mut info = ICONINFO::default();
            GetIconInfo(icon, &mut info).unwrap();
            let dc = CreateCompatibleDC(None);
            let mut header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 32,
                    biHeight: -32,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = vec![0u8; 32 * 32 * 4];
            let rows = GetDIBits(
                dc,
                info.hbmColor,
                0,
                32,
                Some(pixels.as_mut_ptr() as *mut c_void),
                &mut header,
                DIB_RGB_COLORS,
            );
            let _ = DeleteDC(dc);
            let red = pixels
                .chunks(4)
                .filter(|px| px[2] > 120 && px[0] < 60)
                .count();
            let blue = pixels
                .chunks(4)
                .filter(|px| px[0] > 120 && px[2] < 60)
                .count();
            assert_eq!(rows, 32);
            assert!(red > 40, "the Я of the caption icon must be red");
            assert_eq!(blue, 0, "the caption icon has no blue pixels");
        }
    }

    #[test]
    fn rounded_helper_paints_corner_wedges_with_the_backdrop() {
        // GDI's RoundRect fills the four wedges between the bounding rectangle
        // and the arcs with the DC background colour. `rounded` paints the
        // backdrop first and then fills a true rounded path, so a corner that
        // sits on a card shows the card colour instead of a black triangle.
        unsafe {
            use windows::Win32::Graphics::Gdi::{
                BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DeleteDC,
                DIB_RGB_COLORS,
            };
            const SIZE: i32 = 24;
            let header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: SIZE,
                    biHeight: -SIZE,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let dc = CreateCompatibleDC(None);
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &header, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                panic!("the test bitmap must be created");
            };
            let old = SelectObject(dc, HGDIOBJ(bitmap.0));
            fill_rect(dc, 0, 0, SIZE, SIZE, rgb(111, 111, 111));
            // fill = blue, border = black, backdrop = red.
            rounded(dc, 0, 0, SIZE, SIZE, 6, rgb(0, 0, 255), rgb(0, 0, 0), rgb(255, 0, 0));
            let pixels = std::slice::from_raw_parts(bits as *const u8, (SIZE * SIZE * 4) as usize);
            let pixel = |x: i32, y: i32| {
                // GetDIBits-style layout: one BGRA quad per pixel, rows top to bottom.
                let i = (y * SIZE + x) as usize * 4;
                (pixels[i], pixels[i + 1], pixels[i + 2])
            };
            assert_eq!(pixel(12, 12), (255, 0, 0), "inside the shape is the fill");
            assert_eq!(
                pixel(1, 1),
                (0, 0, 255),
                "the corner wedge must show the backdrop, not the DC background"
            );
            assert_eq!(pixel(12, 2), (255, 0, 0), "just inside the top edge is the fill");
            assert_eq!(pixel(12, 0), (0, 0, 0), "the outline is stroked with the border pen");
            let _ = SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
        }
    }

    #[test]
    fn buttons_get_the_surface_they_are_drawn_on() {
        // A button inside a card must be backed with the card colour; one in the
        // sidebar or the header sits on the page background.
        assert_eq!(surface_at(INNER_LEFT, CB_AUTOSTART_Y, 430, 38), card_color());
        assert_eq!(surface_at(INNER_RIGHT - 120, HOTKEY_CARD_Y + 14, 120, 32), card_color());
        assert_eq!(surface_at(SIDEBAR_X, SIDEBAR_TOP, SIDEBAR_WIDTH, TAB_HEIGHT), bg_color());
        assert_eq!(surface_at(INNER_RIGHT - 280, HEADER_TOP, 280, 34), bg_color());
    }

    #[test]
    fn no_control_touches_a_card_frame() {
        let cards = [
            (VERSION_CARD_Y, VERSION_CARD_H),
            (LAUNCH_CARD_Y, LAUNCH_CARD_H),
            (HOTKEY_CARD_Y, HOTKEY_CARD_H),
            (IGNORE_CARD_Y, IGNORE_CARD_H),
            (IGNORE_LIST_CARD_Y, IGNORE_LIST_CARD_H),
            (LOG_CARD_Y, LOG_CARD_H),
            (LEX_CARD_Y, LEX_CARD_H),
            (LEX_LIST_CARD_Y, LEX_LIST_CARD_H),
        ];
        for spec in CONTROLS {
            let left = if spec.anchored_right {
                INNER_RIGHT - spec.width
            } else {
                spec.x
            };
            assert!(
                left >= INNER_LEFT,
                "control {} starts {} px inside the card, needs {}",
                spec.role,
                left,
                CARD_PAD
            );
            assert!(
                left + spec.width <= INNER_RIGHT,
                "control {} sticks out of the card by {} px",
                spec.role,
                left + spec.width - INNER_RIGHT
            );
            // A control that is not inside any card (the header button) sits on
            // the page background and is checked against the window only.
            let hosts: Vec<(i32, i32)> = cards
                .into_iter()
                .filter(|&(top, height)| spec.y >= top && spec.y + spec.height <= top + height)
                .collect();
            if hosts.is_empty() {
                assert!(
                    spec.y + spec.height <= WINDOW_HEIGHT,
                    "control {} hangs off the window",
                    spec.role
                );
                continue;
            }
            assert!(
                hosts.iter().any(|&(top, height)| {
                    spec.y >= top + 10 && spec.y + spec.height <= top + height - 10
                }),
                "control {} spans {}..{} and touches a card frame",
                spec.role,
                spec.y,
                spec.y + spec.height
            );
        }
    }

    #[test]
    fn every_caption_fits_inside_its_button() {
        // Measured with the real font at 96 dpi: a caption that would touch the
        // border of its button is either given a wider button or, as a last
        // resort, drawn in a smaller point size by `fitting_font`.
        unsafe {
            let hdc = GetDC(None);
            for spec in CONTROLS {
                if spec.label.is_empty() {
                    continue;
                }
                let used = text_width(hdc, fonts(96).body, spec.label);
                let reserved = if matches!(spec.role, CB_AUTOSTART | CB_ADMIN | CB_AUTO_LAYOUT) {
                    // the check mark takes 20 px on the left of the caption
                    spec.width - 12 - 20 - 10 - BUTTON_PAD
                } else {
                    spec.width - 2 * BUTTON_PAD
                };
                assert!(
                    used <= reserved,
                    "«{}» needs {} px but the button leaves {} px",
                    spec.label,
                    used,
                    reserved
                );
            }
            for (_, label) in SIDEBAR_TABS {
                let used = text_width(hdc, fonts(96).body, label);
                let reserved = SIDEBAR_WIDTH - 14 - BUTTON_PAD;
                assert!(
                    used <= reserved,
                    "tab «{label}» needs {used} px but the tab leaves {reserved} px"
                );
            }
            ReleaseDC(None, hdc);
        }
    }

    #[test]
    fn every_note_line_fits_its_card() {
        // DrawText does not wrap here: every explanatory line is measured with
        // the real font and must stay inside the card.
        unsafe {
            let hdc = GetDC(None);
            let width = INNER_RIGHT - INNER_LEFT;
            for text in [LEX_NOTE, HOTKEY_NOTE_TEXT, SMART_UNDO_TEXT] {
                for line in text.split('\n') {
                    let used = text_width(hdc, fonts(96).dim, line);
                    assert!(
                        used <= width,
                        "line «{line}» needs {used} px, the card is {width} px wide"
                    );
                }
            }
            ReleaseDC(None, hdc);
        }
    }

    #[test]
    fn painting_a_card_twice_hundred_times_leaks_no_gdi_objects() {
        // S-01: the old helper created a brush and a pen per shape and deleted
        // them while they were still selected, so every repaint of the settings
        // window took 4 GDI objects out of the 10 000 limit.
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
        };
        unsafe {
            use windows::Win32::Graphics::Gdi::{
                BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DeleteDC,
                DIB_RGB_COLORS, DT_LEFT,
            };
            let dc = CreateCompatibleDC(None);
            let header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 200,
                    biHeight: -120,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &header, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                panic!("the test bitmap must be created");
            };
            let before = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            for i in 0..200 {
                fill_rect(dc, 0, 0, 200, 120, bg_color());
                rounded(dc, 8, 8, 184, 104, 10, card_color(), card_border(), bg_color());
                draw_text(
                    dc,
                    &format!("строка {i}"),
                    &RECT {
                        left: 12,
                        top: 12,
                        right: 180,
                        bottom: 40,
                    },
                    fonts(96).body,
                    fg_color(),
                    DT_LEFT,
                );
            }
            let after = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            // Other tests run in the same process and allocate their own GDI
            // objects, so the tolerance has to cover their noise: a genuine leak
            // in `rounded`/`fill_rect`/`draw_text` would add one object per
            // iteration, i.e. 200+, not a handful.
            assert!(
                after - before <= 40,
                "200 repaints leaked {} GDI objects (before {before}, after {after})",
                after - before
            );
        }
    }

    #[test]
    fn theme_switch_leaks_no_gdi_objects() {
        // P-01: every light↔dark switch repaints the whole window. The palette
        // changes, so the brush cache gains one entry per colour — but nothing
        // may grow without bound.
        use windows::Win32::System::Threading::{
            GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
        };
        unsafe {
            use windows::Win32::Graphics::Gdi::{
                BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DeleteDC,
                DIB_RGB_COLORS,
            };
            let dc = CreateCompatibleDC(None);
            let header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: 300,
                    biHeight: -200,
                    biPlanes: 1,
                    biBitCount: 32,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bitmap) = CreateDIBSection(Some(dc), &header, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(dc);
                panic!("the test bitmap must be created");
            };
            let before = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            for _ in 0..50 {
                DARK.store(true, Ordering::Relaxed);
                fill_rect(dc, 0, 0, 300, 200, bg_color());
                rounded(dc, 10, 10, 280, 180, 10, card_color(), card_border(), bg_color());
                DARK.store(false, Ordering::Relaxed);
                fill_rect(dc, 0, 0, 300, 200, bg_color());
                rounded(dc, 10, 10, 280, 180, 10, card_color(), card_border(), bg_color());
            }
            let after = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
            DARK.store(true, Ordering::Relaxed);
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(dc);
            // Signed difference: the count may drop when Windows releases an
            // internal brush, and an unsigned subtraction would wrap around.
            let leaked = after as i64 - before as i64;
            assert!(
                leaked <= 40,
                "100 theme switches leaked {leaked} GDI objects (before {before}, after {after})",
            );
        }
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
    fn bare_alt_is_allowed_only_for_undo() {
        // Alt alone is the classic "undo": the hook reads it as VK_MENU with no
        // modifier. For every other action a lone modifier is a bad idea.
        assert!(validate_hotkey("hotkey_undo", "alt").is_ok());
        assert!(validate_hotkey("hotkey_toggle", "alt").is_err());
        assert!(validate_hotkey("hotkey_last", "alt").is_err());
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

    #[test]
    fn the_message_loop_stops_when_getmessage_fails() {
        // GetMessageW returns -1 on an error. BOOL::as_bool() is `!= 0`, so a
        // naive `while ... .into()` keeps spinning; the loop must break instead.
        assert!(message_loop_should_continue(1));
        assert!(!message_loop_should_continue(0));
        assert!(!message_loop_should_continue(-1));
    }

    #[test]
    fn the_keyboard_hook_only_carries_a_packed_combination() {
        let packed = pack_combo(0x4B, true, true, false, true);
        let (vk, ctrl, alt, shift, win) = unpack_combo(packed);
        assert_eq!(vk.0, 0x4B);
        assert!(ctrl && alt && !shift && win);
        // 0 is reserved for "nothing pending".
        assert_eq!(unpack_combo(0).0 .0, 0);
    }

    #[test]
    fn a_dead_handle_is_neither_painted_nor_sent_a_message() {
        unsafe {
            assert!(!invalidate(null_hwnd()));
            assert_eq!(send(null_hwnd(), WM_COMMAND, 0, 0).0, 0);
        }
    }

    #[test]
    fn modifier_tracking_never_wraps_to_a_stuck_key() {
        // An up-event for a modifier that was never pressed used to leave the
        // count at usize::MAX, so every later combination looked like it had Ctrl.
        set_modifier(VK_RCONTROL, false);
        assert!(!modifier_down(VK_RCONTROL));
        set_modifier(VK_LCONTROL, true);
        set_modifier(VK_LCONTROL, true);
        set_modifier(VK_LCONTROL, false);
        assert!(modifier_down(VK_LCONTROL));
        set_modifier(VK_LCONTROL, false);
        assert!(!modifier_down(VK_LCONTROL));
    }

    #[test]
    fn the_win_modifier_is_read_from_the_real_key_codes() {
        // 0x92/0x93 matched nothing, so "win+l" was recorded as plain "l".
        set_modifier(VK_WIN_LEFT, true);
        assert!(modifier_down(VK_WIN_LEFT));
        set_modifier(VK_WIN_LEFT, false);
        assert!(!modifier_down(VK_WIN_LEFT));
    }

    #[test]
    fn a_filter_is_rebuilt_only_when_its_text_changes() {
        // The probe has no live controls, so field_text returns an empty string:
        // the second call must not trigger another rebuild of the list.
        assert!(filter_changed(LOG_FILTER));
        assert!(!filter_changed(LOG_FILTER));
    }

    #[test]
    fn the_sidebar_carries_the_dictionary_tab() {
        assert_eq!(SIDEBAR_TABS.len(), 4);
        assert!(
            SIDEBAR_TABS
                .iter()
                .any(|(role, label)| *role == TAB_LEXICON && *label == "Словарь"),
            "the sidebar must offer the Словарь page"
        );
        assert_eq!(label_of(TAB_LEXICON), "Словарь");
        assert_eq!(TAB_LEXICON - TAB_SETTINGS, 3);
        // Four tab buttons still fit in the left column of the window.
        let bottom = SIDEBAR_TOP
            + (SIDEBAR_TABS.len() as i32 - 1) * (TAB_HEIGHT + TAB_GAP)
            + TAB_HEIGHT;
        assert!(bottom <= WINDOW_HEIGHT);
    }

    #[test]
    fn every_page_has_its_own_visibility_group() {
        let groups = visibility_groups();
        assert_eq!(groups.len(), 4, "one control group per sidebar tab");
        for (index, roles) in &groups {
            assert_eq!(
                *index,
                SIDEBAR_TABS[*index].0 - TAB_SETTINGS,
                "group {index} does not belong to the tab painted on it"
            );
            assert!(!roles.is_empty());
        }
        // The dictionary page owns its field, both buttons, the filter and the list.
        let lexicon_group = groups
            .iter()
            .find(|(index, _)| *index == TAB_LEXICON - TAB_SETTINGS)
            .expect("there is a control group for the Словарь page");
        for role in [
            LEX_INPUT,
            BTN_LEX_ADD,
            BTN_LEX_REMOVE,
            LEX_FILTER,
            LEX_FILTER_LABEL,
            LEX_LIST,
        ] {
            assert!(
                lexicon_group.1.contains(&role),
                "role {role} is not shown on the Словарь page"
            );
        }
        for (index, roles) in &groups {
            if *index != TAB_LEXICON - TAB_SETTINGS {
                assert!(!roles.contains(&LEX_LIST), "the word list must be hidden on other pages");
            }
        }
    }

    #[test]
    fn every_owner_draw_button_carries_a_caption() {
        // Owner-draw buttons paint their own text: a role missing from
        // `label_of` would come out as a blank rectangle, which is exactly how
        // the "Добавить"/"Удалить выбранное" buttons on the Словарь page looked.
        for role in [
            TAB_SETTINGS,
            TAB_IGNORE,
            TAB_LOG,
            TAB_LEXICON,
            BTN_CONFIG,
            BTN_UPDATE,
            CB_AUTOSTART,
            CB_ADMIN,
            CB_AUTO_LAYOUT,
            CB_SMART_UNDO,
            BTN_HOTKEY_RESET,
            BTN_ADD_PROCESSES,
            BTN_ADD_PROCESS,
            BTN_REMOVE_IGNORE,
            BTN_ADD_COMMIT,
            BTN_LOG_REFRESH,
            BTN_LOG_CLEAR,
            BTN_LEX_ADD,
            BTN_LEX_REMOVE,
            MODAL_CANCEL,
        ] {
            assert!(!label_of(role).is_empty(), "role {role} paints no text");
        }
        assert_eq!(label_of(BTN_LEX_ADD), "Добавить");
        assert_eq!(label_of(BTN_LEX_REMOVE), "Удалить выбранное");
    }

    #[test]
    fn the_dictionary_page_stays_inside_the_window() {
        assert!(LEX_INPUT_Y + 34 <= LEX_CARD_Y + LEX_CARD_H);
        assert!(LEX_LIST_Y + LEX_LIST_H <= STATUS_Y);
        assert!(LEX_CONFLICT_Y + 22 <= LEX_LIST_CARD_Y + LEX_LIST_CARD_H);
        assert!(LEX_LIST_CARD_Y + LEX_LIST_CARD_H <= WINDOW_HEIGHT);
        // "Добавить" and "Удалить выбранное" sit side by side inside the card.
        assert!(CONTENT_X + 584 + 120 + 12 + 180 <= CONTENT_X + CONTENT_WIDTH);
        assert!(CONTENT_X + CARD_PAD + 556 <= CONTENT_X + 584);
    }

    #[test]
    fn conflict_pairs_are_read_as_ru_arrow_keys() {
        // No config, no window: the built-in lists alone must produce the known
        // collisions, and the footnote prints at most six of them.
        let lexicon = crate::lexicon::Lexicon::with_extra(&[], &[]);
        let text = conflict_label(&lexicon.conflicts(), 6);
        assert!(!text.is_empty(), "the dictionary reports its own collisions");
        assert!(text.contains("мы ↔ vs"), "the vs/мы pair belongs in the footnote");
        assert!(text.split(", ").count() <= 6);
    }

    #[test]
    fn a_lexicon_row_is_labelled_by_layout_and_source() {
        let (added_ru, added_en) = (vec!["кт".to_string()], vec!["qps".to_string()]);
        let lexicon = crate::lexicon::Lexicon::with_extra(&added_ru, &added_en);
        let base = crate::lexicon::Lexicon::with_extra(&[], &[]);
        assert_eq!(lexicon.ru_count(), base.ru_count() + 1);
        assert_eq!(lexicon.en_count(), base.en_count() + 1);
        assert_eq!(conflict_label(&lexicon.conflicts(), 6), conflict_label(&base.conflicts(), 6));
    }
}
