//! Native Win32 settings window: processes, folders, switch log.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::core::{PCSTR, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::{
    LVCF_TEXT, LVCF_WIDTH, LVIF_TEXT, LVCOLUMNW, LVITEMW, LVNI_SELECTED, LVM_DELETEALLITEMS,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETEXTENDEDLISTVIEWSTYLE,
    LVS_EX_FULLROWSELECT, LVS_REPORT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CallNextHookEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowTextW,
    PostMessageW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowLongPtrW,
    SetWindowTextW, ShowWindow, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    GWLP_WNDPROC, HHOOK, HMENU, MF_STRING, MSG, TPM_RIGHTBUTTON, TrackPopupMenu, WM_COMMAND,
    WM_CREATE, WM_DESTROY, WM_QUIT, WH_MOUSE_LL, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD,
    WS_OVERLAPPEDWINDOW, WS_VISIBLE, SW_HIDE, SW_SHOW,
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

const WM_CONTEXTMENU: u32 = 0x007B;
const WM_LBUTTONDOWN: u32 = 0x0202;

const WS_TABSTOP: WINDOW_STYLE = WINDOW_STYLE(0x00001000);
const WS_BORDER: WINDOW_STYLE = WINDOW_STYLE(0x00800000);

const ID_TAB_PROCESSES: usize = 101;
const ID_TAB_FOLDER: usize = 102;
const ID_TAB_LOG: usize = 103;
const ID_TARGET: usize = 104;
const ID_ADD_PROCESS: usize = 105;
const ID_FILTER: usize = 106;
const ID_DIR_PATH: usize = 107;
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
static DIR_PATH_HANDLE: AtomicUsize = AtomicUsize::new(0);
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

fn add_column(list: HWND, title: &str) {
    if list.0.is_null() {
        return;
    }
    let w = wide(title);
    let column = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH,
        cx: 860,
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

fn refresh_processes(filter: &str) {
    let exclusions = crate::win::exclude_processes();
    let filter_lower = filter.to_lowercase();
    let mut items = Vec::new();

    if filter_lower.is_empty() {
        for name in exclusions.iter().rev() {
            items.push((name.clone(), true));
        }
        if items.is_empty() {
            items.push((format!("Нет исключённых процессов"), false));
        }
    } else {
        for name in running_processes() {
            if name.to_lowercase().contains(&filter_lower) {
                let excluded = exclusions.iter().any(|x| x.eq_ignore_ascii_case(&name));
                items.push((name.clone(), excluded));
            }
        }
        if items.is_empty() {
            items.push((format!("Нет процессов по фильтру \"{filter}\""), false));
        }
    }

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
    show(&TARGET_BUTTON_HANDLE, show_target);
    show(&PROC_BUTTON_HANDLE, show_add_process);
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

unsafe extern "system" fn listview_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
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
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            refresh(0, "");
            set_status("Выберите вкладку. Правая кнопка на записи — меню.");
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
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
                    // EN_CHANGE: the typed text filters the process list live.
                    let notification = ((lparam.0 >> 16) & 0xFFFF) as u32;
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
                        if let Ok(hook) = SetWindowsHookExW(
                            WH_MOUSE_LL,
                            Some(target_mouse_hook),
                            None,
                            0,
                        ) {
                            TARGET_HOOK.store(to_handle_hook(hook), Ordering::Relaxed);
                        }
                    }
                    set_status(
                        "Кликните окно приложения, которое нужно исключить. Нажатие мыши выбирает его процесс.",
                    );
                }
                ID_ADD_PROCESS => {
                    if let Some(index) = selected_list_index(get_handle(&LIST_HANDLE)) {
                        let guard = PROC_ITEMS.get_or_init(|| Mutex::new(Vec::new()));
                        let items = guard.lock().unwrap_or_else(|e| e.into_inner());
                        if index < items.len() {
                            let raw = items[index].0.clone();
                            if raw.is_empty() || raw.starts_with("Нет процессов") {
                                set_status("Нет выбранного процесса.");
                            } else if crate::win::add_exclude_process(&raw) {
                                set_status(&format!("Добавлен процесс: {raw}"));
                            } else {
                                set_status(&format!("Процесс уже исключён: {raw}"));
                            }
                            refresh_current();
                        }
                    }
                }
                ID_ADD_DIR => {
                    let path = text_of(get_handle(&DIR_PATH_HANDLE)).trim().to_string();
                    if path.is_empty() {
                        set_status("Введите путь к папке или нажмите «Указать на диске».");
                    } else if crate::win::add_exclude_dir(&path) {
                        set_status(&format!("Добавлена папка: {path}"));
                        refresh_current();
                    } else {
                        set_status(&format!("Папка уже исключена: {path}"));
                        refresh_current();
                    }
                }
                ID_PICK_DIR => {
                    if let Some(path) = pick_folder(hwnd) {
                        let dir_edit = get_handle(&DIR_PATH_HANDLE);
                        let _ = SetWindowTextW(dir_edit, PCWSTR(wide(&path).as_ptr()));
                        if crate::win::add_exclude_dir(&path) {
                            set_status(&format!("Добавлена папка: {path}"));
                        } else {
                            set_status(&format!("Папка уже исключена: {path}"));
                        }
                        refresh_current();
                    } else {
                        set_status("Не удалось выбрать папку на диске.");
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
                            if !raw.is_empty() && !raw.starts_with("Нет процессов") {
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

fn create_controls(parent: HWND) {
    set_handle(&WINDOW_HANDLE, parent);

    let button_style = WS_CHILD | WS_VISIBLE | WS_TABSTOP;

    let _tab_process = create_control(
        parent,
        "Button",
        "Процессы",
        10,
        10,
        120,
        28,
        button_style,
        ID_TAB_PROCESSES,
    );
    let _tab_folder = create_control(
        parent,
        "Button",
        "Папка",
        135,
        10,
        120,
        28,
        button_style,
        ID_TAB_FOLDER,
    );
    let _tab_log = create_control(
        parent,
        "Button",
        "Лог переключений",
        260,
        10,
        170,
        28,
        button_style,
        ID_TAB_LOG,
    );
    let target = create_control(
        parent,
        "Button",
        "Указать по мыши",
        435,
        10,
        150,
        28,
        button_style,
        ID_TARGET,
    );
    let add_process = create_control(
        parent,
        "Button",
        "Добавить из списка",
        590,
        10,
        170,
        28,
        button_style,
        ID_ADD_PROCESS,
    );

    let filter = create_control(
        parent,
        "Edit",
        "Фильтр процессов",
        10,
        45,
        300,
        28,
        WS_CHILD | WS_VISIBLE,
        ID_FILTER,
    );
    let status = create_control(
        parent,
        "Edit",
        "",
        320,
        45,
        440,
        28,
        WS_CHILD | WS_VISIBLE,
        ID_STATUS,
    );

    let dir_path = create_control(
        parent,
        "Edit",
        "Путь к папке",
        10,
        45,
        420,
        28,
        WS_CHILD | WS_VISIBLE,
        ID_DIR_PATH,
    );
    let add_dir = create_control(
        parent,
        "Button",
        "Добавить",
        435,
        45,
        110,
        28,
        button_style,
        ID_ADD_DIR,
    );
    let pick_dir = create_control(
        parent,
        "Button",
        "Указать на диске",
        550,
        45,
        210,
        28,
        button_style,
        ID_PICK_DIR,
    );

    let list = create_control(
        parent,
        "SysListView32",
        "",
        10,
        80,
        880,
        420,
        WS_CHILD | WS_VISIBLE | WS_BORDER | WINDOW_STYLE(LVS_REPORT),
        ID_LIST,
    );
    add_column(list, "Элемент");
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
    set_handle(&STATUS_HANDLE, status);
    set_handle(&DIR_PATH_HANDLE, dir_path);
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

fn pick_folder(parent: HWND) -> Option<String> {
    unsafe {
        let library_name = wide("comdlg32.dll");
        let library = LoadLibraryW(PCWSTR(library_name.as_ptr())).ok()?;
        let proc = GetProcAddress(library, PCSTR(b"GetOpenFolder\0".as_ptr()))?;
        type GetOpenFolderFn = unsafe extern "system" fn(HWND, u32, *mut *mut u16) -> i32;
        let get_open_folder: GetOpenFolderFn = std::mem::transmute(proc);

        let mut path: *mut u16 = std::ptr::null_mut();
        if get_open_folder(parent, 0, &mut path) != 0 {
            return None;
        }
        if path.is_null() {
            return None;
        }

        let mut chars = Vec::new();
        for i in 0..usize::MAX {
            let character = *path.add(i);
            if character == 0 {
                break;
            }
            chars.push(character);
        }

        if chars.is_empty() {
            return None;
        }
        Some(String::from_utf16_lossy(&chars))
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
            80,
            80,
            920,
            540,
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