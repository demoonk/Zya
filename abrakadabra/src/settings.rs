//! Native Win32 settings window: processes, folders, switch log.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::Controls::{
    LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_TEXT, LVIS_SELECTED, LVITEMW, LVM_DELETEALLITEMS,
    LVM_GETITEMCOUNT, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVNI_SELECTED, LVS_EX_FULLROWSELECT, LVS_REPORT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, FindWindowExW, FindWindowW, GetCursorPos, GetMessageW,
    GetWindowLongPtrW, GetWindowTextW, PostMessageW, RegisterClassW, SendMessageW,
    SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, SetWindowsHookExW,
    ShowWindow, TrackPopupMenu, TranslateMessage, UnhookWindowsHookEx, GWLP_WNDPROC, HHOOK, HMENU,
    MF_STRING, MSG, SWP_NOACTIVATE, SWP_NOZORDER, SW_HIDE, SW_SHOW, TPM_RIGHTBUTTON, WH_MOUSE_LL,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_QUIT, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE,
};

#[link(name = "user32")]
extern "system" {
    fn OpenClipboard(hwnd: *mut c_void) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, memory: usize) -> i32;
    fn CloseClipboard() -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GlobalAlloc(flags: u32, size: usize) -> usize;
}

// GetDpiForWindow sits in the HiDpi module, which is not enabled in Cargo.toml,
// so it is declared directly against user32 like the clipboard helpers above.
#[link(name = "user32")]
extern "system" {
    fn GetDpiForWindow(hwnd: *mut c_void) -> u32;
}

// CoTaskMemFree is exported by shell32 as well; linking against combase is not
// available with the MinGW toolchain used here.
#[link(name = "shell32")]
extern "system" {
    fn CoTaskMemFree(pv: *const c_void);
}

const WM_CONTEXTMENU: u32 = 0x007B;
const WM_LBUTTONDOWN: u32 = 0x0202;

/// Every coordinate below is designed for 96 DPI and multiplied by dpi / 96, so
/// a 150% display (144 DPI) turns the 920x540 window into 1380x810 and keeps the
/// controls readable instead of clipped.
const BASE_DPI: i32 = 96;
const WINDOW_WIDTH: i32 = 920;
const WINDOW_HEIGHT: i32 = 540;
const WINDOW_LEFT: i32 = 80;
const WINDOW_TOP: i32 = 80;

const WS_TABSTOP: WINDOW_STYLE = WINDOW_STYLE(0x00001000);
const WS_BORDER: WINDOW_STYLE = WINDOW_STYLE(0x00800000);

const ID_TAB_PROCESSES: usize = 101;
const ID_TAB_FOLDER: usize = 102;
const ID_TAB_LOG: usize = 103;
const ID_TARGET: usize = 104;
const ID_ADD_PROCESS: usize = 105;
const ID_FILTER: usize = 106;
const ID_DIR_PATH: usize = 107;
const ID_FILTER_LABEL: usize = 112;
const ID_DIR_LABEL: usize = 113;
const ID_ADD_DIR: usize = 108;
const ID_PICK_DIR: usize = 109;
const ID_STATUS: usize = 110;
const ID_LIST: usize = 111;
const CMD_REMOVE: usize = 201;
const CMD_COPY: usize = 202;
const CMD_DICT: usize = 203;

type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

#[repr(C)]
struct MouseHookStruct {
    point: POINT,
    hwnd: HWND,
    flags: u32,
    extra_info: usize,
}

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);
static CURRENT_TAB: AtomicUsize = AtomicUsize::new(0);
static CONTEXT_INDEX: AtomicUsize = AtomicUsize::new(usize::MAX);
static TARGET_ACTIVE: AtomicBool = AtomicBool::new(false);

static WINDOW_HANDLE: AtomicUsize = AtomicUsize::new(0);
static LIST_HANDLE: AtomicUsize = AtomicUsize::new(0);
static FILTER_HANDLE: AtomicUsize = AtomicUsize::new(0);
static FILTER_LABEL_HANDLE: AtomicUsize = AtomicUsize::new(0);
static DIR_PATH_HANDLE: AtomicUsize = AtomicUsize::new(0);
static DIR_LABEL_HANDLE: AtomicUsize = AtomicUsize::new(0);
static STATUS_HANDLE: AtomicUsize = AtomicUsize::new(0);
static PROC_BUTTON_HANDLE: AtomicUsize = AtomicUsize::new(0);
static TARGET_BUTTON_HANDLE: AtomicUsize = AtomicUsize::new(0);
static DIR_ADD_BUTTON_HANDLE: AtomicUsize = AtomicUsize::new(0);
static DIR_PICK_BUTTON_HANDLE: AtomicUsize = AtomicUsize::new(0);
static TARGET_HOOK: AtomicUsize = AtomicUsize::new(0);

static LISTVIEW_ORIG: OnceLock<WndProc> = OnceLock::new();

static PROC_ITEMS: OnceLock<Mutex<Vec<(String, bool)>>> = OnceLock::new();
static DIR_ITEMS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static LOG_ITEMS: OnceLock<Mutex<Vec<crate::diary::Entry>>> = OnceLock::new();

fn to_handle(hwnd: HWND) -> usize {
    hwnd.0 as usize
}

fn from_handle(handle: usize) -> HWND {
    HWND(handle as *mut c_void)
}

fn get_handle(slot: &AtomicUsize) -> HWND {
    from_handle(slot.load(Ordering::Relaxed))
}

fn set_handle(slot: &AtomicUsize, hwnd: HWND) {
    slot.store(to_handle(hwnd), Ordering::Relaxed);
}

/// DPI of a window, never below the 96 DPI the layout was designed for.
fn window_dpi(hwnd: HWND) -> i32 {
    if hwnd.0.is_null() {
        return BASE_DPI;
    }
    let dpi = unsafe { GetDpiForWindow(hwnd.0) };
    if dpi < BASE_DPI as u32 {
        BASE_DPI
    } else {
        dpi as i32
    }
}

/// Turn a 96-DPI design coordinate into pixels for the current DPI.
fn scale(value: i32, dpi: i32) -> i32 {
    (value * dpi + BASE_DPI / 2) / BASE_DPI
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn text_of(hwnd: HWND) -> String {
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut buf = [0u16; 1024];
    unsafe {
        let n = GetWindowTextW(hwnd, &mut buf);
        if n > 0 {
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        } else {
            String::new()
        }
    }
}

fn set_status(text: &str) {
    let status = get_handle(&STATUS_HANDLE);
    if status.0.is_null() {
        return;
    }
    let w = wide(text);
    unsafe {
        let _ = SetWindowTextW(status, PCWSTR(w.as_ptr()));
    }
}

fn running_processes() -> Vec<String> {
    unsafe {
        let snapshot = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(handle) => handle,
            Err(_) => return Vec::new(),
        };

        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

        let mut names = Vec::new();
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = String::from_utf16_lossy(&entry.szExeFile)
                    .split('\0')
                    .next()
                    .unwrap_or_default()
                    .to_string();
                if !name.is_empty() {
                    names.push(name);
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        names.sort();
        names.dedup();
        names
    }
}

fn selected_list_index(list: HWND) -> Option<usize> {
    if list.0.is_null() {
        return None;
    }
    unsafe {
        let result = SendMessageW(
            list,
            LVM_GETNEXTITEM,
            Some(WPARAM((-1i32) as usize)),
            Some(LPARAM(LVNI_SELECTED as isize)),
        );
        let idx = result.0 as i32;
        if idx >= 0 {
            Some(idx as usize)
        } else {
            None
        }
    }
}

fn insert_item(list: HWND, index: usize, text: &str) {
    if list.0.is_null() {
        return;
    }
    let w = wide(text);
    let item = LVITEMW {
        mask: LVIF_TEXT,
        iItem: index as i32,
        iSubItem: 0,
        pszText: PWSTR(w.as_ptr() as *mut u16),
        cchTextMax: w.len() as i32,
        ..Default::default()
    };
    unsafe {
        let _ = SendMessageW(
            list,
            LVM_INSERTITEMW,
            Some(WPARAM(index)),
            Some(LPARAM(&item as *const LVITEMW as isize)),
        );
    }
}

fn add_column(list: HWND, title: &str, width: i32) {
    if list.0.is_null() {
        return;
    }
    let w = wide(title);
    let column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: width,
        pszText: PWSTR(w.as_ptr() as *mut u16),
        cchTextMax: w.len() as i32,
        iSubItem: 0,
        ..Default::default()
    };
    unsafe {
        let _ = SendMessageW(
            list,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(0)),
            Some(LPARAM(&column as *const LVCOLUMNW as isize)),
        );
    }
}

fn process_display(raw: &str, excluded: bool) -> String {
    if excluded {
        format!("{raw} — исключён")
    } else {
        raw.to_string()
    }
}

/// Placeholder rows ("Нет процессов ...") are not real processes: the buttons
/// must not try to add them to the exclusions.
fn is_placeholder(raw: &str) -> bool {
    raw.is_empty() || raw.starts_with("Нет")
}

/// Processes that can never be a foreground application, so they only clutter
/// the list. They are hidden only when no filter is typed: typing their name in
/// the filter still shows them, and an exclusion is always listed so the user
/// can remove it.
const PROCESS_NOISE: &[&str] = &["Idle", "csrss.exe", "wininit.exe", "services.exe"];

fn is_noise(name: &str) -> bool {
    PROCESS_NOISE
        .iter()
        .any(|noise| noise.eq_ignore_ascii_case(name))
}

/// Rows for the process list: already excluded names first (so they are easy to
/// find and remove), then the running processes that are not excluded yet,
/// alphabetically. A process that is both excluded and running is shown once.
fn build_process_items(
    running: &[String],
    exclusions: &[String],
    hide_noise: bool,
) -> Vec<(String, bool)> {
    let mut items = Vec::new();

    for name in exclusions {
        if is_placeholder(name) {
            continue;
        }
        items.push((name.clone(), true));
    }

    for name in running {
        if exclusions
            .iter()
            .any(|excluded| excluded.eq_ignore_ascii_case(name))
        {
            continue;
        }
        if hide_noise && is_noise(name) {
            continue;
        }
        items.push((name.clone(), false));
    }

    items
}

fn refresh_processes(filter: &str) {
    let exclusions = crate::win::exclude_processes();
    let filter_lower = filter.to_lowercase();
    let running = running_processes();

    let items = if filter_lower.is_empty() {
        build_process_items(&running, &exclusions, true)
    } else {
        let matching_running: Vec<String> = running
            .iter()
            .filter(|name| name.to_lowercase().contains(&filter_lower))
            .cloned()
            .collect();
        let matching_exclusions: Vec<String> = exclusions
            .iter()
            .filter(|name| name.to_lowercase().contains(&filter_lower))
            .cloned()
            .collect();
        build_process_items(&matching_running, &matching_exclusions, false)
    };

    let items = if items.is_empty() {
        if filter_lower.is_empty() {
            vec![("Нет процессов".to_string(), false)]
        } else {
            vec![(format!("Нет процессов по фильтру \"{filter}\""), false)]
        }
    } else {
        items
    };

    let guard = PROC_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
    let mut store = guard.lock().unwrap_or_else(|e| e.into_inner());
    *store = items.clone();

    let list = get_handle(&LIST_HANDLE);
    unsafe {
        let _ = SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    for (i, item) in items.iter().enumerate() {
        insert_item(list, i, &process_display(&item.0, item.1));
    }
}

fn refresh_dirs() {
    let dirs = crate::win::exclude_dirs();
    let guard = DIR_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
    let mut store = guard.lock().unwrap_or_else(|e| e.into_inner());
    *store = dirs.clone();

    let list = get_handle(&LIST_HANDLE);
    unsafe {
        let _ = SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    for (i, dir) in dirs.iter().enumerate() {
        insert_item(list, i, dir);
    }
}

fn refresh_logs() {
    let mut entries = crate::diary::recent();
    if entries.is_empty() {
        // Nothing switched in this process yet: show the tail of the diary file
        // so the tab is not blank on a fresh start.
        entries = crate::diary::tail(50);
    }
    entries.reverse();

    let guard = LOG_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
    let mut store = guard.lock().unwrap_or_else(|e| e.into_inner());
    *store = entries.clone();

    let list = get_handle(&LIST_HANDLE);
    unsafe {
        let _ = SendMessageW(list, LVM_DELETEALLITEMS, Some(WPARAM(0)), Some(LPARAM(0)));
    }
    for (i, entry) in entries.iter().enumerate() {
        insert_item(
            list,
            i,
            &format!("{} {} -> {}", entry.time, entry.original, entry.converted),
        );
    }
}

fn refresh(tab: usize, filter: &str) {
    CURRENT_TAB.store(tab, Ordering::Relaxed);
    CONTEXT_INDEX.store(usize::MAX, Ordering::Relaxed);
    match tab {
        0 => refresh_processes(filter),
        1 => refresh_dirs(),
        _ => refresh_logs(),
    }
}

fn refresh_current() {
    let tab = CURRENT_TAB.load(Ordering::Relaxed);
    if tab == 0 {
        refresh_processes(&text_of(get_handle(&FILTER_HANDLE)));
    } else if tab == 1 {
        refresh_dirs();
    } else {
        refresh_logs();
    }
}

fn show_only(tab: usize) {
    let show_filter = tab == 0;
    let show_target = tab == 0;
    let show_add_process = tab == 0;
    let show_dir = tab == 1;

    let show = |slot: &AtomicUsize, visible: bool| {
        let hwnd = get_handle(slot);
        if !hwnd.0.is_null() {
            unsafe {
                let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
            }
        }
    };

    show(&FILTER_HANDLE, show_filter);
    show(&FILTER_LABEL_HANDLE, show_filter);
    show(&TARGET_BUTTON_HANDLE, show_target);
    show(&PROC_BUTTON_HANDLE, show_add_process);
    show(&DIR_LABEL_HANDLE, show_dir);
    show(&DIR_PATH_HANDLE, show_dir);
    show(&DIR_ADD_BUTTON_HANDLE, show_dir);
    show(&DIR_PICK_BUTTON_HANDLE, show_dir);
}

fn create_control(
    parent: HWND,
    class: &str,
    text: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    style: WINDOW_STYLE,
    id: usize,
) -> HWND {
    let class_name = wide(class);
    let title = wide(text);
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            style,
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut c_void)),
            None,
            None,
        )
        .unwrap_or(HWND::default())
    }
}

fn call_original(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match LISTVIEW_ORIG.get().copied() {
            Some(fn_ptr) => fn_ptr(hwnd, msg, wparam, lparam),
            None => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

unsafe extern "system" fn listview_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CONTEXTMENU {
        let from_param = lparam.0 as i32;
        let index = if from_param >= 0 {
            Some(from_param as usize)
        } else {
            selected_list_index(hwnd)
        };

        if let Some(index) = index {
            CONTEXT_INDEX.store(index, Ordering::Relaxed);
            let tab = CURRENT_TAB.load(Ordering::Relaxed);

            let menu = match CreatePopupMenu() {
                Ok(menu) => menu,
                Err(_) => return call_original(hwnd, msg, wparam, lparam),
            };

            unsafe {
                if tab == 0 || tab == 1 {
                    let label = wide("Удалить из исключений");
                    let _ = AppendMenuW(menu, MF_STRING, CMD_REMOVE, PCWSTR(label.as_ptr()));
                } else {
                    let copy = wide("Скопировать");
                    let dictionary = wide("Добавить в словарь");
                    let _ = AppendMenuW(menu, MF_STRING, CMD_COPY, PCWSTR(copy.as_ptr()));
                    let _ = AppendMenuW(menu, MF_STRING, CMD_DICT, PCWSTR(dictionary.as_ptr()));
                }

                let settings = get_handle(&WINDOW_HANDLE);
                if !settings.0.is_null() {
                    let _ = SetForegroundWindow(settings);
                    let mut point = POINT { x: 0, y: 0 };
                    if GetCursorPos(&mut point).is_ok() {
                        let _ = TrackPopupMenu(
                            menu,
                            TPM_RIGHTBUTTON,
                            point.x,
                            point.y - 4,
                            None,
                            settings,
                            None,
                        );
                    }
                }

                let _ = DestroyMenu(menu);
            }

            return LRESULT(0);
        }
    }

    call_original(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn target_mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if TARGET_ACTIVE.load(Ordering::Relaxed) && code == 0 && wparam.0 as u32 == WM_LBUTTONDOWN {
        unsafe {
            let mouse_ptr = (lparam.0 as *const MouseHookStruct) as *const MouseHookStruct;
            if mouse_ptr.is_null() {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            let clicked = (*mouse_ptr).hwnd;
            let settings = get_handle(&WINDOW_HANDLE);
            if clicked != settings && !clicked.0.is_null() {
                let name = crate::win::window_process_name(clicked);
                if !name.is_empty() {
                    if crate::win::add_exclude_process(&name) {
                        set_status(&format!("Добавлен процесс: {name}"));
                    } else {
                        set_status(&format!("Процесс уже исключён: {name}"));
                    }

                    TARGET_ACTIVE.store(false, Ordering::Relaxed);
                    let hook = TARGET_HOOK.load(Ordering::Relaxed);
                    if hook != 0 {
                        let _ = UnhookWindowsHookEx(HHOOK(hook as *mut c_void));
                        TARGET_HOOK.store(0, Ordering::Relaxed);
                    }
                    refresh_current();
                }
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}

unsafe extern "system" fn settings_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if crate::win::verbose() && (msg == 0x0201 || msg == 0x0202) {
        println!(
            "zя: настройки msg {msg} wparam {} lparam {}",
            wparam.0, lparam.0
        );
    }
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            refresh(0, "");
            set_status("Выберите вкладку. Правая кнопка на записи — меню.");
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            if crate::win::verbose() {
                println!("zя: настройки WM_COMMAND id {id}");
            }
            match id {
                ID_TAB_PROCESSES => {
                    show_only(0);
                    refresh(0, &text_of(get_handle(&FILTER_HANDLE)));
                }
                ID_TAB_FOLDER => {
                    show_only(1);
                    refresh(1, "");
                }
                ID_TAB_LOG => {
                    show_only(2);
                    refresh(2, "");
                }
                ID_FILTER => {
                    // A control's WM_COMMAND packs the notification code in the
                    // high word of wparam (MAKEWPARAM(id, EN_CHANGE)); lparam is
                    // just the handle of the Edit that sent it.
                    let notification = ((wparam.0 >> 16) & 0xFFFF) as u32;
                    if notification == 0x0400 {
                        refresh_processes(&text_of(get_handle(&FILTER_HANDLE)));
                    }
                }
                ID_TARGET => {
                    unsafe {
                        TARGET_ACTIVE.store(true, Ordering::Relaxed);
                        let old = TARGET_HOOK.load(Ordering::Relaxed);
                        if old != 0 {
                            let _ = UnhookWindowsHookEx(HHOOK(old as *mut c_void));
                            TARGET_HOOK.store(0, Ordering::Relaxed);
                        }
                        if let Ok(hook) =
                            SetWindowsHookExW(WH_MOUSE_LL, Some(target_mouse_hook), None, 0)
                        {
                            TARGET_HOOK.store(to_handle_hook(hook), Ordering::Relaxed);
                        }
                    }
                    set_status(
                        "Кликните окно приложения, которое нужно исключить. Нажатие мыши выбирает его процесс.",
                    );
                }
                ID_ADD_PROCESS => {
                    if let Some(index) = selected_list_index(get_handle(&LIST_HANDLE)) {
                        // Read the row, then release the guard: refresh_current
                        // locks PROC_ITEMS again, and holding it here deadlocks
                        // the whole settings window.
                        let raw = PROC_ITEMS
                            .get_or_init(|| Mutex::new(Vec::new()))
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(index)
                            .map(|item| item.0.clone())
                            .unwrap_or_default();
                        if crate::win::verbose() {
                            println!("zя: настройки add process «{raw}»");
                        }
                        if is_placeholder(&raw) {
                            set_status("Нет выбранного процесса.");
                        } else if crate::win::add_exclude_process(&raw) {
                            set_status(&format!("Добавлен процесс: {raw}"));
                        } else {
                            set_status(&format!("Процесс уже исключён: {raw}"));
                        }
                        refresh_current();
                        if crate::win::verbose() {
                            println!("zя: настройки add process: done");
                        }
                    }
                }
                ID_ADD_DIR => {
                    add_dir_from_ui(&text_of(get_handle(&DIR_PATH_HANDLE)));
                }
                ID_PICK_DIR => {
                    if crate::win::verbose() {
                        println!("zя: настройки: открываю диалог выбора папки");
                    }
                    if let Some(path) = pick_folder(hwnd) {
                        add_dir_from_ui(&path);
                    } else {
                        set_status("Диалог выбора папки недоступен или был отменён.");
                    }
                    if crate::win::verbose() {
                        println!("zя: настройки: диалог закрыт");
                    }
                }
                CMD_REMOVE => {
                    let index = CONTEXT_INDEX.load(Ordering::Relaxed);
                    let tab = CURRENT_TAB.load(Ordering::Relaxed);
                    if index != usize::MAX {
                        if tab == 0 {
                            let guard = PROC_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
                            let raw = guard
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get(index)
                                .map(|x| x.0.clone())
                                .unwrap_or_default();
                            if !raw.is_empty() && !is_placeholder(&raw) {
                                if crate::win::remove_exclude_process(&raw) {
                                    set_status(&format!("Удалён из исключений: {raw}"));
                                } else {
                                    set_status(&format!("Не удалось удалить: {raw}"));
                                }
                                refresh_current();
                            }
                        } else if tab == 1 {
                            let guard = DIR_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
                            let raw = guard
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get(index)
                                .cloned()
                                .unwrap_or_default();
                            if !raw.is_empty() {
                                if crate::win::remove_exclude_dir(&raw) {
                                    set_status(&format!("Удалена папка: {raw}"));
                                } else {
                                    set_status(&format!("Не удалось удалить: {raw}"));
                                }
                                refresh_current();
                            }
                        }
                    }
                }
                CMD_COPY => {
                    let index = CONTEXT_INDEX.load(Ordering::Relaxed);
                    if index != usize::MAX {
                        let guard = LOG_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
                        let entry = guard
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(index)
                            .cloned();
                        if let Some(e) = entry {
                            copy_to_clipboard(&format!(
                                "{} {} -> {} ({})",
                                e.time, e.original, e.converted, e.why
                            ));
                            set_status(&format!("Скопировано: {} -> {}", e.original, e.converted));
                        }
                    }
                }
                CMD_DICT => {
                    let index = CONTEXT_INDEX.load(Ordering::Relaxed);
                    if index != usize::MAX {
                        let guard = LOG_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
                        let entry = guard
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .get(index)
                            .cloned();
                        if let Some(e) = entry {
                            if crate::win::add_dictionary_word(&e.original) {
                                set_status(&format!("Добавлено в словарь: {}", e.original));
                            } else {
                                set_status(&format!("В словаре уже есть: {}", e.original));
                            }
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            TARGET_ACTIVE.store(false, Ordering::Relaxed);
            let hook = TARGET_HOOK.load(Ordering::Relaxed);
            if hook != 0 {
                let _ = UnhookWindowsHookEx(HHOOK(hook as *mut c_void));
                TARGET_HOOK.store(0, Ordering::Relaxed);
            }
            let _ = PostMessageW(Some(hwnd), WM_QUIT, WPARAM(0), LPARAM(0));
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn to_handle_hook(hook: HHOOK) -> usize {
    hook.0 as usize
}

/// Normalize a folder path typed by the user: trim the spaces, resolve a
/// relative path against the current directory, and drop the trailing separator
/// so the stored value is stable and the "already excluded" check works.
fn normalize_dir(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let absolute = std::path::absolute(trimmed)
        .unwrap_or_else(|_| std::path::PathBuf::from(trimmed))
        .to_string_lossy()
        .to_string();

    let mut normalized = absolute;
    while normalized.ends_with('\\') || normalized.ends_with('/') {
        normalized.pop();
    }

    // "C:\" must not collapse to the bare drive "C:", which would match nothing.
    if normalized.ends_with(':') {
        normalized.push('\\');
    }

    normalized
}

/// Shared path for both "Добавить" and "Указать на диске": normalize, store,
/// and report what really happened.
fn add_dir_from_ui(raw: &str) {
    let path = normalize_dir(raw);
    if path.is_empty() {
        set_status("Введите путь к папке или нажмите «Указать на диске».");
        return;
    }

    let edit = get_handle(&DIR_PATH_HANDLE);
    if !edit.0.is_null() {
        unsafe {
            let _ = SetWindowTextW(edit, PCWSTR(wide(&path).as_ptr()));
        }
    }

    if crate::win::add_exclude_dir(&path) {
        let note = if std::path::Path::new(&path).is_dir() {
            ""
        } else {
            " (папки нет на диске)"
        };
        set_status(&format!("Добавлена папка: {path}{note}"));
    } else {
        set_status(&format!("Папка уже исключена: {path}"));
    }
    refresh_current();
}

fn create_controls(parent: HWND) {
    set_handle(&WINDOW_HANDLE, parent);

    // All design coordinates are for 96 DPI; px() scales them for this window.
    let dpi = window_dpi(parent);
    let px = |value: i32| scale(value, dpi);

    let button_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;

    let _tab_process = create_control(
        parent,
        "Button",
        "Процессы",
        px(10),
        px(10),
        px(120),
        px(28),
        button_style,
        ID_TAB_PROCESSES,
    );
    let _tab_folder = create_control(
        parent,
        "Button",
        "Папка",
        px(135),
        px(10),
        px(120),
        px(28),
        button_style,
        ID_TAB_FOLDER,
    );
    let _tab_log = create_control(
        parent,
        "Button",
        "Лог переключений",
        px(260),
        px(10),
        px(170),
        px(28),
        button_style,
        ID_TAB_LOG,
    );
    let target = create_control(
        parent,
        "Button",
        "Указать по мыши",
        px(435),
        px(10),
        px(150),
        px(28),
        button_style,
        ID_TARGET,
    );
    let add_process = create_control(
        parent,
        "Button",
        "Добавить из списка",
        px(590),
        px(10),
        px(170),
        px(28),
        button_style,
        ID_ADD_PROCESS,
    );

    // A label control, not the text of the box itself: putting the caption inside
    // the Edit made the filter search for the word "Фильтр процессов" and the
    // path box start with a literal label.
    let filter_label = create_control(
        parent,
        "Static",
        "Фильтр процессов",
        px(10),
        px(45),
        px(130),
        px(24),
        WS_CHILD | WS_VISIBLE,
        ID_FILTER_LABEL,
    );
    let filter = create_control(
        parent,
        "Edit",
        "",
        px(145),
        px(45),
        px(300),
        px(28),
        WS_CHILD | WS_VISIBLE,
        ID_FILTER,
    );
    let dir_label = create_control(
        parent,
        "Static",
        "Путь к папке",
        px(10),
        px(45),
        px(120),
        px(24),
        WS_CHILD | WS_VISIBLE,
        ID_DIR_LABEL,
    );
    let dir_path = create_control(
        parent,
        "Edit",
        "",
        px(135),
        px(45),
        px(300),
        px(28),
        WS_CHILD | WS_VISIBLE,
        ID_DIR_PATH,
    );
    let add_dir = create_control(
        parent,
        "Button",
        "Добавить",
        px(460),
        px(45),
        px(100),
        px(28),
        button_style,
        ID_ADD_DIR,
    );
    let pick_dir = create_control(
        parent,
        "Button",
        "Указать на диске",
        px(570),
        px(45),
        px(210),
        px(28),
        button_style,
        ID_PICK_DIR,
    );

    // The status line has its own row: it used to sit on top of the folder path
    // box, which made the typed path unreadable.
    let status = create_control(
        parent,
        "Edit",
        "",
        px(10),
        px(80),
        px(880),
        px(26),
        WS_CHILD | WS_VISIBLE | WS_BORDER,
        ID_STATUS,
    );

    let list = create_control(
        parent,
        "SysListView32",
        "",
        px(10),
        px(110),
        px(880),
        px(380),
        WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(LVS_REPORT),
        ID_LIST,
    );
    add_column(list, "Элемент", px(860));
    unsafe {
        let _ = SendMessageW(
            list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            Some(WPARAM(0)),
            Some(LPARAM(LVS_EX_FULLROWSELECT as isize)),
        );
    }

    unsafe {
        let original = GetWindowLongPtrW(list, GWLP_WNDPROC);
        let original_proc: WndProc = std::mem::transmute(original as usize);
        let _ = LISTVIEW_ORIG.set(original_proc);
        let current_proc: WndProc = listview_proc;
        let _ = SetWindowLongPtrW(list, GWLP_WNDPROC, current_proc as isize);
    }

    set_handle(&FILTER_HANDLE, filter);
    set_handle(&FILTER_LABEL_HANDLE, filter_label);
    set_handle(&STATUS_HANDLE, status);
    set_handle(&DIR_PATH_HANDLE, dir_path);
    set_handle(&DIR_LABEL_HANDLE, dir_label);
    set_handle(&LIST_HANDLE, list);
    set_handle(&PROC_BUTTON_HANDLE, add_process);
    set_handle(&TARGET_BUTTON_HANDLE, target);
    set_handle(&DIR_ADD_BUTTON_HANDLE, add_dir);
    set_handle(&DIR_PICK_BUTTON_HANDLE, pick_dir);

    show_only(0);
    refresh(0, "");
}

fn copy_to_clipboard(text: &str) {
    let mut encoded = text.encode_utf16().collect::<Vec<u16>>();
    encoded.push(0);

    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        let _ = EmptyClipboard();
        let memory = GlobalAlloc(0x0002, encoded.len() * 2);
        if memory != 0 {
            let ptr = memory as *mut u16;
            if !ptr.is_null() {
                std::ptr::copy_nonoverlapping(encoded.as_ptr(), ptr, encoded.len());
                let _ = SetClipboardData(13, memory);
            }
        }
        let _ = CloseClipboard();
    }
}

fn utf16_buffer_to_string(buffer: &[u16]) -> String {
    let end = buffer
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end])
}

/// The folder the user picked, or None when the dialog was cancelled or is not
/// available on this system.
fn pick_folder(parent: HWND) -> Option<String> {
    unsafe {
        use windows::Win32::UI::Shell::{
            SHBrowseForFolderW, SHGetPathFromIDListEx, SHParseDisplayName, BROWSEINFOW,
            GPFIDL_DEFAULT,
        };

        // Open the browser rooted at the folder of the app itself: it is where
        // real exclusion candidates live, and a rooted pidl always resolves to a
        // plain path.
        let exe = crate::autostart::current_exe();
        let start = exe
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "C:\\".to_string());
        let root_name = wide(&start);
        let mut root_pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST =
            std::ptr::null_mut();
        let has_root =
            SHParseDisplayName(PCWSTR(root_name.as_ptr()), None, &mut root_pidl, 0, None).is_ok();

        let title = wide("Выберите папку для исключения");
        let info = BROWSEINFOW {
            hwndOwner: parent,
            pidlRoot: if has_root {
                root_pidl
            } else {
                std::ptr::null_mut()
            },
            lpszTitle: PCWSTR(title.as_ptr()),
            ..Default::default()
        };

        let id = SHBrowseForFolderW(&info);
        if crate::win::verbose() {
            println!("zя: настройки: диалог закрыт, pidl {}", id as usize);
        }
        if has_root {
            CoTaskMemFree(root_pidl as *const c_void);
        }
        if id.is_null() {
            return None;
        }

        let mut buffer = [0u16; 260];
        let found = SHGetPathFromIDListEx(id, &mut buffer, GPFIDL_DEFAULT).as_bool();
        CoTaskMemFree(id as *const c_void);
        if crate::win::verbose() {
            println!("zя: настройки: путь из диалога -> {found}");
        }

        if !found {
            return None;
        }

        let text = utf16_buffer_to_string(&buffer);
        if text.is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

fn run_window() {
    unsafe {
        let class_name = wide("ZyaSettings");
        let window_class = windows::Win32::UI::WindowsAndMessaging::WNDCLASSW {
            lpfnWndProc: Some(settings_wndproc),
            style: windows::Win32::UI::WindowsAndMessaging::WNDCLASS_STYLES(0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        let _ = RegisterClassW(&window_class);

        let title = wide("Настройки zя");
        let window = match CreateWindowExW(
            WINDOW_EX_STYLE(0),
            PCWSTR(class_name.as_ptr()),
            PCWSTR(title.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            WINDOW_LEFT,
            WINDOW_TOP,
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
            None,
            None,
            None,
            None,
        ) {
            Ok(window) => window,
            Err(_) => {
                SETTINGS_OPEN.store(false, Ordering::Relaxed);
                return;
            }
        };

        // WM_CREATE already built the controls with the DPI of this window, so
        // grow the window itself to the same scale before it is shown.
        let dpi = window_dpi(window);
        if dpi != BASE_DPI {
            match SetWindowPos(
                window,
                None,
                scale(WINDOW_LEFT, dpi),
                scale(WINDOW_TOP, dpi),
                scale(WINDOW_WIDTH, dpi),
                scale(WINDOW_HEIGHT, dpi),
                SWP_NOZORDER | SWP_NOACTIVATE,
            ) {
                Ok(_) => {}
                Err(e) => println!("zя: не удалось увеличить окно настроек: {e}"),
            }
        }

        set_handle(&WINDOW_HANDLE, window);
        let _ = ShowWindow(window, SW_SHOW);

        let mut message = MSG::default();
        loop {
            let got = GetMessageW(&mut message, None, 0, 0);
            if !got.as_bool() {
                break;
            }
            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }
    }

    SETTINGS_OPEN.store(false, Ordering::Relaxed);
}

/// Non-blocking version used from the tray: the settings window runs in its own
/// thread with its own message loop.
pub fn open() {
    if SETTINGS_OPEN.load(Ordering::Relaxed) {
        return;
    }
    SETTINGS_OPEN.store(true, Ordering::Relaxed);
    std::thread::spawn(run_window);
}

/// Blocking version for `z.exe settings` from the command line.
pub fn open_blocking() {
    if SETTINGS_OPEN.load(Ordering::Relaxed) {
        return;
    }
    SETTINGS_OPEN.store(true, Ordering::Relaxed);
    run_window();
}

/// Drive the real window from the inside: click every button through its own
/// `WM_COMMAND` and report what the list and the status line ended up doing.
pub fn pick_probe() {
    unsafe {
        if std::env::var("ZYA_KEEP_PICK").unwrap_or_default() != "1" {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(3));
                let title = wide("Browse for Folder");
                if let Ok(d) = FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
                    println!("probe: диалог есть, закрываю");
                    let _ = PostMessageW(Some(d), WM_CLOSE, WPARAM(0), LPARAM(0));
                } else {
                    println!("probe: диалога нет");
                }
            });
        }
        println!("probe: вызываю диалог выбора папки");
        let path = pick_folder(HWND(std::ptr::null_mut()));
        println!("probe: результат {path:?}");
    }
}

pub fn self_test() {
    open();
    std::thread::sleep(std::time::Duration::from_secs(2));
    let window = get_handle(&WINDOW_HANDLE);
    if window.0.is_null() {
        println!("settings self-test: window was not created");
        return;
    }
    let list = get_handle(&LIST_HANDLE);
    let status = get_handle(&STATUS_HANDLE);

    let rows = || unsafe {
        SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0 as usize
    };
    let status_text = || text_of(status);

    unsafe {
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_TAB_PROCESSES)),
            Some(LPARAM(0)),
        );
        println!("self-test: процессы, строк {}", rows());

        // Pick a process that is not excluded yet, so we test the real add path.
        let excluded: Vec<String> = crate::win::exclude_processes();
        let target = PROC_ITEMS
            .get_or_init(|| Mutex::new(Vec::new()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .enumerate()
            .find(|(_, item)| !excluded.iter().any(|e| e.eq_ignore_ascii_case(&item.0)))
            .map(|(index, item)| (index, item.0.clone()));

        if let Some((index, name)) = target {
            let item = LVITEMW {
                iItem: index as i32,
                state: LVIS_SELECTED,
                stateMask: LVIS_SELECTED,
                ..Default::default()
            };
            SendMessageW(
                list,
                0x102B, // LVM_SETITEMSTATE
                Some(WPARAM(index as usize)),
                Some(LPARAM(&item as *const LVITEMW as isize)),
            );
            println!("self-test: выбран «{name}» (строка {index})");
            println!("self-test: отправляю WM_COMMAND «Добавить из списка»");
            SendMessageW(
                window,
                WM_COMMAND,
                Some(WPARAM(ID_ADD_PROCESS)),
                Some(LPARAM(0)),
            );
            println!("self-test: «Добавить из списка» -> {}", status_text());
            println!("self-test: строк после {}", rows());
            println!(
                "self-test: исключённые процессы: {:?}",
                crate::win::exclude_processes()
            );
            println!("self-test: отправляю WM_COMMAND «удалить»");
            CONTEXT_INDEX.store(index, Ordering::Relaxed);
            SendMessageW(
                window,
                WM_COMMAND,
                Some(WPARAM(CMD_REMOVE)),
                Some(LPARAM(0)),
            );
            println!("self-test: удалить процесс -> {}", status_text());
            println!(
                "self-test: исключённые после удаления: {:?}",
                crate::win::exclude_processes()
            );
        } else {
            println!("self-test: все процессы уже исключены");
        }

        println!("self-test: отправляю WM_COMMAND «Папка»");
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_TAB_PROCESSES)),
            Some(LPARAM(0)),
        );
        let filter = get_handle(&FILTER_HANDLE);
        let filter_value = to_handle(filter) as isize;
        let _ = SetWindowTextW(filter, PCWSTR(wide("notepad").as_ptr()));
        let change = (106usize << 16) | 0x0400usize;
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(change)),
            Some(LPARAM(filter_value)),
        );
        println!("self-test: фильтр «notepad» -> строк {}", rows());
        let _ = SetWindowTextW(filter, PCWSTR(wide("").as_ptr()));
        let change = (106usize << 16) | 0x0400usize;
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(change)),
            Some(LPARAM(filter_value)),
        );
        println!("self-test: пустой фильтр -> строк {}", rows());
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_TAB_FOLDER)),
            Some(LPARAM(0)),
        );
        println!("self-test: папка, строк {}", rows());
        let _ = SetWindowTextW(
            get_handle(&DIR_PATH_HANDLE),
            PCWSTR(wide("C:\\Users\\demOOn\\Documents\\Abrakadra").as_ptr()),
        );
        println!("self-test: отправляю WM_COMMAND «Добавить» папку");
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_ADD_DIR)),
            Some(LPARAM(0)),
        );
        println!("self-test: «Добавить» -> {}", status_text());
        println!("self-test: строк после {}", rows());
        CONTEXT_INDEX.store(0, Ordering::Relaxed);
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(CMD_REMOVE)),
            Some(LPARAM(0)),
        );
        println!("self-test: удалить папку -> {}", status_text());
        println!(
            "self-test: папки после удаления {:?}",
            crate::win::exclude_dirs()
        );

        // "Указать на диске" opens a modal dialog. A helper thread closes it after
        // two seconds so the test does not hang, and we check the dialog appeared.
        if std::env::var("ZYA_TEST_PICK").unwrap_or_default() == "1" {
            // Close the modal dialog from a helper thread so the test terminates:
            // click its "Cancel" button the way a user would, or select the root
            // folder and press OK when ZYA_TEST_PICK_OK is set.
            let accept = std::env::var("ZYA_TEST_PICK_OK").unwrap_or_default() == "1";
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(2));
                let title = wide("Browse for Folder");
                let dialog = match FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
                    Ok(d) => d,
                    Err(_) => {
                        println!("self-test: диалог выбора папки не появился");
                        return;
                    }
                };
                println!("self-test: диалог выбора папки открыт");
                let label = if accept { "OK" } else { "Cancel" };
                let button = FindWindowExW(
                    Some(dialog),
                    None,
                    PCWSTR(wide("Button").as_ptr()),
                    PCWSTR(wide(label).as_ptr()),
                );
                match button {
                    Ok(b) => {
                        let _ = SendMessageW(b, 0x00F5 /* BM_CLICK */, None, None);
                    }
                    Err(_) => {
                        let _ = PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
            });
            println!("self-test: отправляю WM_COMMAND «Указать на диске»");
            SendMessageW(
                window,
                WM_COMMAND,
                Some(WPARAM(ID_PICK_DIR)),
                Some(LPARAM(0)),
            );
            println!("self-test: «Указать на диске» -> {}", status_text());
        }

        println!("self-test: отправляю WM_COMMAND «Лог»");
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_TAB_LOG)),
            Some(LPARAM(0)),
        );
        println!("self-test: лог, строк {}", rows());
        println!(
            "self-test: записи лога: {:?}",
            LOG_ITEMS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .take(3)
                .map(|e| format!("{} {} -> {}", e.time, e.original, e.converted))
                .collect::<Vec<_>>()
        );

        // A process that is not excluded yet, then remove it again.
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_TAB_PROCESSES)),
            Some(LPARAM(0)),
        );
        let _ = SetWindowTextW(get_handle(&FILTER_HANDLE), PCWSTR(wide("notepad").as_ptr()));
        let target = LVITEMW {
            iItem: 0,
            state: LVIS_SELECTED,
            stateMask: LVIS_SELECTED,
            ..Default::default()
        };
        SendMessageW(
            list,
            0x102B, // LVM_SETITEMSTATE
            Some(WPARAM(0)),
            Some(LPARAM(&target as *const LVITEMW as isize)),
        );
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(ID_ADD_PROCESS)),
            Some(LPARAM(0)),
        );
        println!("self-test: добавить notepad -> {}", status_text());
        println!("self-test: строк после {}", rows());
        CONTEXT_INDEX.store(0, Ordering::Relaxed);
        SendMessageW(
            window,
            WM_COMMAND,
            Some(WPARAM(CMD_REMOVE)),
            Some(LPARAM(0)),
        );
        println!("self-test: удалить notepad -> {}", status_text());
        println!("self-test: строк после {}", rows());
        println!(
            "self-test: текущие исключения: {:?}",
            crate::win::exclude_processes()
        );
        println!("self-test: папки: {:?}", crate::win::exclude_dirs());

        println!("self-test: готово");
        if std::env::var("ZYA_TEST_HOLD").unwrap_or_default() == "1" {
            // Leave the window on screen so its layout can be photographed.
            let tab = std::env::var("ZYA_TEST_TAB").unwrap_or_default();
            let id = match tab.as_str() {
                "folder" => ID_TAB_FOLDER,
                "log" => ID_TAB_LOG,
                _ => ID_TAB_PROCESSES,
            };
            SendMessageW(window, WM_COMMAND, Some(WPARAM(id)), Some(LPARAM(0)));
            println!("self-test: окно держится 8 с, вкладка {tab}");
            std::thread::sleep(std::time::Duration::from_secs(8));
        }
    }
    unsafe {
        let _ = DestroyWindow(window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_scales_with_the_display_dpi() {
        assert_eq!(scale(WINDOW_WIDTH, 96), 920);
        assert_eq!(scale(WINDOW_HEIGHT, 96), 540);
        assert_eq!(scale(WINDOW_WIDTH, 144), 1380);
        assert_eq!(scale(WINDOW_HEIGHT, 144), 810);
        assert_eq!(scale(10, 144), 15);
        assert_eq!(scale(0, 144), 0);
        // A window that does not exist yet falls back to the design DPI.
        assert_eq!(window_dpi(HWND::default()), BASE_DPI);
    }

    #[test]
    fn empty_filter_lists_running_processes_with_excluded_first() {
        let running = vec![
            "chrome.exe".to_string(),
            "csrss.exe".to_string(),
            "notepad.exe".to_string(),
        ];
        let excluded = vec!["chrome.exe".to_string()];
        let items = build_process_items(&running, &excluded, true);

        assert_eq!(
            items,
            vec![
                ("chrome.exe".to_string(), true),
                ("notepad.exe".to_string(), false),
            ]
        );
        assert_eq!(
            process_display(&items[0].0, items[0].1),
            "chrome.exe — исключён"
        );
    }

    #[test]
    fn only_pseudo_processes_are_hidden_from_the_full_list() {
        let running = vec![
            "Idle".to_string(),
            "svchost.exe".to_string(),
            "explorer.exe".to_string(),
        ];
        let items = build_process_items(&running, &[], true);
        assert_eq!(items.len(), 2, "svchost and explorer must stay visible");
        assert!(items.iter().any(|(name, _)| name == "svchost.exe"));
    }

    #[test]
    fn a_filtered_process_is_never_hidden() {
        let running = vec!["csrss.exe".to_string()];
        let matches = build_process_items(&running, &[], false);
        assert_eq!(matches, vec![("csrss.exe".to_string(), false)]);
    }

    #[test]
    fn placeholder_rows_are_not_processes() {
        assert!(is_placeholder("Нет процессов по фильтру \"qq\""));
        assert!(is_placeholder("Нет процессов"));
        assert!(is_placeholder(""));
        assert!(!is_placeholder("chrome.exe"));
    }

    #[test]
    fn folder_paths_are_normalized_before_storing() {
        assert_eq!(normalize_dir("  C:\\Games\\My\\  "), "C:\\Games\\My");
        assert_eq!(normalize_dir("C:\\Games/"), "C:\\Games");
        assert_eq!(normalize_dir("C:\\"), "C:\\");
        assert_eq!(normalize_dir(""), "");

        let relative = normalize_dir("Documents");
        assert!(
            relative.starts_with("C:\\") || relative.starts_with("\\\\"),
            "relative path must be resolved: {relative}"
        );
        assert!(!relative.ends_with('\\'));
    }
}
