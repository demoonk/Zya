//! Windows low-level keyboard hook: intercepts every keystroke, decides whether
//! the word just typed was produced with the wrong layout, rewrites it and
//! switches the layout.

use std::path::Path;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, ERROR_SHARING_VIOLATION, GENERIC_READ,
    GENERIC_WRITE, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_ALWAYS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardLayoutList, SendInput, ToUnicodeEx, HKL, INPUT,
    INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    VIRTUAL_KEY, VK_BACK, VK_C, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_F1, VK_F10,
    VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_LCONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_P, VK_PAUSE, VK_RCONTROL, VK_RETURN, VK_RMENU, VK_RSHIFT,
    VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, FindWindowW, GetClassNameW, GetForegroundWindow, GetMessageW,
    GetWindow, GetWindowTextW, GetWindowThreadProcessId, IsWindow, IsWindowVisible, PostMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, GW_CHILD, GW_HWNDNEXT, HHOOK,
    KBDLLHOOKSTRUCT, LLKHF_UP, MSG, WH_KEYBOARD_LL, WM_COMMAND,
};

use crate::engine::{Config, Decision, Engine};

#[link(name = "user32")]
extern "system" {
    fn SetProcessDpiAwarenessContext(value: isize) -> i32;
}

/// The layout of the settings window and the tray menu is designed for 96 DPI
/// and multiplied by the real display scale, so the process has to be DPI-aware
/// first; otherwise Windows bitmap-stretches the whole window and the numbers
/// no longer match the pixels.
pub fn make_dpi_aware() {
    unsafe {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        if SetProcessDpiAwarenessContext(-4) == 0 {
            SetProcessDpiAwarenessContext(-3);
        }
    }
}

const RU_LANGID: u32 = 0x0419;
const EN_LANGID: u32 = 0x0409;
const DOWN_MASK: i16 = -32768; // 0x8000: "key is down" bit of GetAsyncKeyState
/// Base of the tag we put on every event we inject ourselves, so the hook can
/// tell its own keystrokes apart from the user's.
///
/// The process id is added to the tag: when a *second* copy of zя is resident,
/// both copies rewrite the same word and the user sees a double switch. Events
/// tagged by another process are recognised and ignored, and the automatic layer
/// of the losing copy stands down.
const SELF_MAGIC: usize = 0x5A59_4100;

fn self_tag() -> usize {
    static TAG: OnceLock<usize> = OnceLock::new();
    *TAG.get_or_init(|| SELF_MAGIC + std::process::id() as usize)
}

/// An event injected by a different zя process (old builds used the bare base).
fn foreign_tag(extra: usize) -> bool {
    extra != self_tag() && extra >= SELF_MAGIC && extra < SELF_MAGIC + 0x1_0000_0000
}

const MODIFIERS: [u16; 9] = [
    VK_SHIFT.0,
    VK_LSHIFT.0,
    VK_RSHIFT.0,
    VK_CONTROL.0,
    VK_LCONTROL.0,
    VK_RCONTROL.0,
    VK_MENU.0,
    VK_LMENU.0,
    VK_RMENU.0,
];

/// A configured hotkey: modifier combination + the key itself.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u16,
}

fn vk_of(name: &str) -> Option<u16> {
    let n = name.to_lowercase();
    Some(match n.as_str() {
        "pause" | "break" => VK_PAUSE.0,
        "space" => VK_SPACE.0,
        "enter" | "return" => VK_RETURN.0,
        "tab" => VK_TAB.0,
        "escape" | "esc" => VK_ESCAPE.0,
        "backspace" | "bksp" => VK_BACK.0,
        "delete" => VK_DELETE.0,
        "f1" => VK_F1.0,
        "f2" => VK_F2.0,
        "f3" => VK_F3.0,
        "f4" => VK_F4.0,
        "f5" => VK_F5.0,
        "f6" => VK_F6.0,
        "f7" => VK_F7.0,
        "f8" => VK_F8.0,
        "f9" => VK_F9.0,
        "f10" => VK_F10.0,
        "f11" => VK_F11.0,
        "f12" => VK_F12.0,
        _ => {
            let b = n.as_bytes();
            if b.len() == 1 {
                if b[0].is_ascii_alphabetic() {
                    (b[0] - b'a' + 0x41) as u16
                } else if b[0].is_ascii_digit() {
                    (b[0] - b'0' + 0x30) as u16
                } else {
                    return None;
                }
            } else {
                return None;
            }
        }
    })
}

/// "ctrl+alt+p", "pause", "f2" -> Hotkey. `None` when the name is unknown.
pub fn parse_hotkey(spec: &str) -> Option<Hotkey> {
    let mut h = Hotkey::default();
    for part in spec.to_lowercase().split('+') {
        let p = part.trim();
        match p {
            "ctrl" | "control" | "lctrl" | "rctrl" => h.ctrl = true,
            "alt" | "menu" | "lalt" | "ralt" => h.alt = true,
            "shift" => h.shift = true,
            "win" | "meta" | "lwin" | "rwin" => h.win = true,
            "" => continue,
            _ => {
                h.vk = vk_of(p)?;
            }
        }
    }
    if h.vk == 0 {
        None
    } else {
        Some(h)
    }
}

pub struct App {
    pub engine: Engine,
    pub cfg: Config,
    /// Characters of the word currently being typed, as the OS produced them.
    word: String,
    /// True once a digit or code-like separator appeared in the current run:
    /// identifiers, URLs and file names are never rewritten.
    identifier: bool,
    /// Last automatic conversion, used by the Alt undo hotkey.
    last: Option<Decision>,
    /// The character that closed the converted word (space, comma, ...).
    last_boundary: Option<char>,
    auto: bool,
    switches: u32,
    /// Layout we believe is active in the foreground window. GetKeyboardLayout is
    /// stale for some modern apps (Notepad on Windows 11 keeps reporting 0409
    /// after a real Alt+Shift switch), so we seed it from the OS when the
    /// foreground window changes and then track every Alt+Shift that happens.
    lang: u32,
    /// Last layout reported by the focused-control probe, used only to print the
    /// diagnostic line once per change.
    lang_debug: u32,
    /// Foreground window the input-site walk below was done for, and the child
    /// control of that window that actually receives the keystrokes.
    site_window: usize,
    site_ctrl: usize,
    /// The window that was focused while the pending word was typed. A rewrite
    /// must never land in a different application than the one the word belongs
    /// to.
    word_window: usize,
    /// Layout id reported by the OS for the foreground window's thread.
    last_fg: usize,
    /// Modifier bookkeeping, so we can tell a genuine Alt+Shift layout switch
    /// from Alt+Shift+<key> shortcuts. `GetAsyncKeyState` is not reliable inside
    /// a low-level hook, so we track down/up ourselves.
    alt_down: bool,
    shift_down: bool,
    ctrl_down: bool,
    win_down: bool,
    /// Alt+Shift has been pressed together but the chord has not been released
    /// yet. A real language switch only happens for a *clean* chord: pressing
    /// Alt+Shift+3 is an application shortcut and must not flip anything.
    alt_shift_pending: bool,
    /// Alt was pressed alone; the undo is executed on its release so that
    /// Alt+<key> shortcuts do not cancel the last conversion.
    alt_pending: bool,
    /// Cached "does the foreground window look like a login form", recomputed
    /// only when the foreground window handle changes.
    secret: bool,
    secret_fg: usize,
    /// Configured hotkeys (see `hotkey_*` in config.json).
    hotkey_undo: Hotkey,
    hotkey_toggle: Hotkey,
    hotkey_last: Hotkey,
    hotkey_selection: Hotkey,
    hotkey_upper: Hotkey,
    hotkey_invert: Hotkey,
    /// The token that closed most recently, needed by the single-letter rules
    /// of the engine ("P30 b P40" vs "a b c d").
    prev_word: String,
    /// Characters of the token currently being typed, everything up to the next
    /// separator. `-` belongs to the token ("wi-fi" is one word), whitespace and
    /// punctuation close it. Used by the manual layout-switch hotkey.
    run: String,
    /// The token that was closed most recently, together with the separator that
    /// closed it. Lets Break work after a space has already been typed.
    closed: Option<(String, Option<char>)>,
    /// Set after a successful selection gesture. The caret-based fallback must
    /// not run then: the user had a selection, the application simply refused
    /// to hand it over, and deleting characters would corrupt their text.
    selection_done: bool,
    /// Set when a keystroke tagged by *another* zя process reaches this hook: a
    /// second instance is resident and the two of them would rewrite the same
    /// word. The automatic layer of this copy then stands down.
    foreign: bool,
    /// The last few automatic conversions, newest first. Used by the ping-pong
    /// guard: if a token is flipped back to exactly what it was a moment ago,
    /// the layout tracking (or a competing hook) is fighting itself.
    recent_flips: Vec<(String, String, std::time::Instant)>,
}

static APP: OnceLock<Mutex<App>> = OnceLock::new();
static HOOK: OnceLock<usize> = OnceLock::new();
static LOCK_FILE: OnceLock<usize> = OnceLock::new();
static EVENTS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn event_count() -> i64 {
    EVENTS.load(std::sync::atomic::Ordering::Relaxed)
}

/// True when a keystroke injected by *another* zя process has been seen: two
/// copies are hooking the keyboard at the same time.
pub fn foreign_instance_seen() -> bool {
    APP.get()
        .map_or(false, |a| a.lock().map_or(false, |a| a.foreign))
}

fn app() -> &'static Mutex<App> {
    APP.get_or_init(|| {
        let cfg = crate::config::load();
        let auto = cfg.auto_enabled;
        let undo_key = parse_hotkey(&cfg.hotkey_undo).unwrap_or(Hotkey {
            alt: true,
            vk: VK_LMENU.0,
            ..Default::default()
        });
        let toggle_key = parse_hotkey(&cfg.hotkey_toggle).unwrap_or(Hotkey {
            ctrl: true,
            alt: true,
            vk: VK_P.0,
            ..Default::default()
        });
        let last_key = parse_hotkey(&cfg.hotkey_last).unwrap_or(Hotkey {
            vk: VK_PAUSE.0,
            ..Default::default()
        });
        let selection_key = parse_hotkey(&cfg.hotkey_selection).unwrap_or(Hotkey {
            vk: VK_PAUSE.0,
            ..Default::default()
        });
        let upper_key = parse_hotkey(&cfg.hotkey_upper).unwrap_or(Hotkey {
            ctrl: true,
            vk: VK_PAUSE.0,
            ..Default::default()
        });
        let invert_key = parse_hotkey(&cfg.hotkey_invert).unwrap_or(Hotkey {
            shift: true,
            vk: VK_PAUSE.0,
            ..Default::default()
        });
        let model = crate::model::Model::load().expect("language model data");
        Mutex::new(App {
            engine: Engine::new(model, cfg.clone()),
            cfg,
            word: String::new(),
            identifier: false,
            last: None,
            last_boundary: None,
            auto,
            switches: 0,
            lang: EN_LANGID,
            lang_debug: 0,
            last_fg: 0,
            word_window: 0,
            alt_down: false,
            shift_down: false,
            ctrl_down: false,
            win_down: false,
            alt_shift_pending: false,
            alt_pending: false,
            secret: false,
            secret_fg: 0,
            site_window: 0,
            site_ctrl: 0,
            hotkey_undo: undo_key,
            hotkey_toggle: toggle_key,
            hotkey_last: last_key,
            hotkey_selection: selection_key,
            hotkey_upper: upper_key,
            hotkey_invert: invert_key,
            run: String::new(),
            closed: None,
            prev_word: String::new(),
            selection_done: false,
            foreign: false,
            recent_flips: Vec::new(),
        })
    })
}

fn is_down(vk: u16) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) & DOWN_MASK != 0 }
}

pub fn verbose() -> bool {
    static V: OnceLock<bool> = OnceLock::new();
    *V.get_or_init(|| std::env::var("ZYA_VERBOSE").is_ok())
}

unsafe fn state_array(a: &App) -> [u8; 256] {
    let mut s = [0u8; 256];
    for vk in MODIFIERS {
        // Prefer the state the hook observed itself; GetAsyncKeyState can lag or
        // report 0 inside a low-level hook.
        let tracked = (vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0) && a.shift_down
            || (vk == VK_LMENU.0 || vk == VK_RMENU.0) && a.alt_down
            || (vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0) && a.ctrl_down;
        if tracked || is_down(vk) {
            s[vk as usize] = 1;
        }
    }
    // ToUnicodeEx also looks at the generic VK_SHIFT byte for the shift state.
    if a.shift_down || is_down(VK_SHIFT.0) {
        s[VK_SHIFT.0 as usize] = 1;
    }
    // Caps Lock is a toggle: the low bit is the "on" state.
    if GetAsyncKeyState(VK_CAPITAL.0 as i32) & 1 != 0 {
        s[VK_CAPITAL.0 as usize] = 1;
    }
    s
}

/// Keyboard layout of the thread owning the foreground window.
fn foreground_hkl() -> HKL {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return GetKeyboardLayout(0);
        }
        let tid = GetWindowThreadProcessId(hwnd, None);
        let hkl = GetKeyboardLayout(tid);
        if hkl.0.is_null() {
            GetKeyboardLayout(0)
        } else {
            hkl
        }
    }
}

fn langid_of(hkl: HKL) -> u32 {
    (hkl.0 as u64 as u32) & 0xFFFF
}

/// HKL handle for a known layout langid, taken from the installed layout list.
fn hkl_for(langid: u32) -> HKL {
    static CACHE: OnceLock<(usize, usize)> = OnceLock::new();
    let (en, ru) = CACHE.get_or_init(|| unsafe {
        let mut list = [HKL(std::ptr::null_mut()); 32];
        let n = GetKeyboardLayoutList(Some(list.as_mut_slice())) as usize;
        let mut en = GetKeyboardLayout(0).0 as usize;
        let mut ru = en;
        for &h in list.iter().take(n) {
            match langid_of(h) {
                EN_LANGID => en = h.0 as usize,
                RU_LANGID => ru = h.0 as usize,
                _ => {}
            }
        }
        (en, ru)
    });
    HKL(if langid == RU_LANGID {
        *ru as *mut std::ffi::c_void
    } else {
        *en as *mut std::ffi::c_void
    })
}

/// Re-read the layout when the foreground window changes; once we have seen a
/// real layout switch we stop trusting GetKeyboardLayout (it is stale in apps
/// such as Windows 11 Notepad) and rely on our own bookkeeping.
/// Layout id reported by the OS for the thread that owns the foreground window.
unsafe fn current_langid() -> u32 {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return 0;
    }
    langid_of(GetKeyboardLayout(GetWindowThreadProcessId(hwnd, None)))
}

unsafe fn window_class(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 128];
    let n = GetClassNameW(hwnd, &mut buf);
    if n > 0 {
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    } else {
        String::new()
    }
}

fn handle_of(hwnd: HWND) -> usize {
    hwnd.0 as usize
}

fn hwnd_of(handle: usize) -> HWND {
    HWND(handle as *mut std::ffi::c_void)
}

/// A child window that actually receives the typed characters. Windows keeps the
/// keyboard layout *per thread*, and in modern applications the text control is
/// owned by a different thread than the top-level window: the Windows 11 Notepad
/// editor (`NotepadTextBox` / `RichEditD2DPT`) and every Chromium/Electron based
/// app (Teams, browsers) live in a worker thread whose layout changes with
/// Alt+Shift while the root window's thread keeps the old one.
fn is_input_site(class: &str) -> bool {
    let c = class.to_lowercase();
    [
        "edit",
        "richedit",
        "richtext",
        "notepadtextbox",
        "text",
        "input",
        "textarea",
        "renderwidgethost",
        "widgetwin",
        "document",
        "console",
    ]
    .iter()
    .any(|needle| c.contains(needle))
}

/// Walk the visible descendants of the foreground window and return the keyboard
/// layout of the deepest window that looks like an input site. Falls back to the
/// root window when no such control is found.
unsafe fn focused_site() -> HWND {
    let root = GetForegroundWindow();
    if root.0.is_null() {
        return root;
    }
    let null = HWND(std::ptr::null_mut());
    let mut site = root;
    let mut queue = vec![root];
    let mut head = 0;
    // Breadth-first: deeper controls are visited later, so the deepest input
    // site wins.
    while head < queue.len() {
        let w = queue[head];
        head += 1;
        let mut child = GetWindow(w, GW_CHILD).unwrap_or(null);
        while !child.0.is_null() {
            if IsWindowVisible(child).as_bool() {
                queue.push(child);
                if is_input_site(&window_class(child)) {
                    site = child;
                }
            }
            child = GetWindow(child, GW_HWNDNEXT).unwrap_or(null);
        }
    }
    site
}

/// The site walk is done once per foreground window, not once per keystroke:
/// typing runs through here thousands of times a minute and the window tree of
/// a Chromium app is large enough to matter.
unsafe fn focused_site_cached(a: &mut App) -> HWND {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return fg;
    }
    if a.site_window != handle_of(fg) || !IsWindow(Some(fg)).as_bool() {
        a.site_window = handle_of(fg);
        a.site_ctrl = handle_of(focused_site());
    }
    let site = hwnd_of(a.site_ctrl);
    if site.0.is_null() || !IsWindow(Some(site)).as_bool() {
        a.site_ctrl = handle_of(focused_site());
    }
    hwnd_of(a.site_ctrl)
}

unsafe fn focused_langid_in(a: &mut App) -> u32 {
    let site = focused_site_cached(a);
    if site.0.is_null() {
        return 0;
    }
    langid_of(GetKeyboardLayout(GetWindowThreadProcessId(site, None)))
}

/// Diagnostic helper used by the tests: (layout of the focused control, layout of
/// the root window, class of the focused control).
pub fn layout_probe() -> (u32, u32, String) {
    unsafe {
        let site = focused_site();
        (
            langid_of(GetKeyboardLayout(GetWindowThreadProcessId(site, None))),
            current_langid(),
            window_class(site),
        )
    }
}

pub fn active_layout_langid() -> u32 {
    unsafe { current_langid() }
}

/// Force the tracked layout to flip without typing: used by `trayprobe` to see
/// the icon change and by the focus guard tests.
pub fn flip_layout_now() {
    unsafe {
        let mut a = app().lock().unwrap();
        flip_tracked(&mut a);
    }
}

/// Re-read the layout of the focused control into the tracked state.
pub fn sync_layout_now() {
    unsafe {
        let mut a = app().lock().unwrap();
        sync_layout(&mut a);
        sync_tray(&a);
    }
}

/// Does the hook currently consider `hwnd` to be the window that owns the text
/// being typed? Used by the focus test to verify the internal state, not just
/// what the OS reports.
pub fn foreground_window_matches(hwnd: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow().0 as usize;
        let want = hwnd.0 as usize;
        APP.get().map_or(false, |a| {
            let a = a.lock().unwrap();
            a.word_window == want || a.last_fg == want
        }) && fg == want
    }
}

/// Re-read the active layout from the OS. The keyboard layout of the thread that
/// owns the focused control is exactly the one `ToUnicodeEx` must use for the
/// keystroke we are about to translate, so it is the only trustworthy source.
/// The root window is queried only as a fallback (its thread can report a stale
/// layout, which is why Notepad and Teams used to be "blind"). The tracked value
/// is kept only as a last resort when both queries fail.
unsafe fn sync_layout(a: &mut App) {
    let lid = focused_langid_in(a);
    if lid == RU_LANGID || lid == EN_LANGID {
        a.lang = lid;
        if verbose() && a.lang_debug != lid {
            let site = focused_site_cached(a);
            println!(
                "layout sync {:#06x}: site {} thread {}",
                lid,
                window_class(site),
                GetWindowThreadProcessId(site, None)
            );
            a.lang_debug = lid;
        }
        return;
    }
    // No input control under focus (desktop, a game, a dialog without a text
    // box): the layout of the foreground window's thread is the real one.
    let lid = langid_of(foreground_hkl());
    if lid == RU_LANGID || lid == EN_LANGID {
        a.lang = lid;
        return;
    }
    let lid = current_langid();
    if lid == RU_LANGID || lid == EN_LANGID {
        a.lang = lid;
    }
}

unsafe fn flip_tracked(a: &mut App) {
    // A low-level hook sees the Alt+Shift release *before* the OS has applied the
    // layout switch, so GetKeyboardLayout still reports the old layout at this
    // instant. Flipping relative to that value is what the OS will do a moment
    // later, which keeps the tracked layout in step with the real one.
    sync_layout(a);
    if a.lang == RU_LANGID || a.lang == EN_LANGID {
        a.lang = other_layout(a.lang);
    }
    sync_tray(a);
}

fn other_layout(lang: u32) -> u32 {
    if lang == RU_LANGID {
        EN_LANGID
    } else {
        RU_LANGID
    }
}

unsafe fn foreground_text() -> String {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut title = vec![0u16; 256];
    let n = GetWindowTextW(hwnd, &mut title);
    let t = if n > 0 {
        String::from_utf16_lossy(&title[..n as usize])
    } else {
        String::new()
    };
    let mut cls = vec![0u16; 64];
    let cn = GetClassNameW(hwnd, &mut cls);
    let c = if cn > 0 {
        String::from_utf16_lossy(&cls[..cn as usize])
    } else {
        String::new()
    };
    format!("{t} {c}")
}

/// Full path of the executable owning the foreground window (memoised per pid).
unsafe fn foreground_process() -> String {
    let hwnd = GetForegroundWindow();
    if hwnd.0.is_null() {
        return String::new();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return String::new();
    }
    static CACHE: OnceLock<Mutex<std::collections::HashMap<u32, String>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    {
        let guard = cache.lock().unwrap();
        if let Some(v) = guard.get(&pid) {
            return v.clone();
        }
    }
    let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
        Ok(h) => h,
        Err(_) => return String::new(),
    };
    let mut buf = vec![0u16; 1024];
    let mut len = 1024u32;
    let name = match QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_WIN32,
        windows::core::PWSTR(buf.as_mut_ptr()),
        &mut len,
    ) {
        Ok(()) => String::from_utf16_lossy(&buf[..len as usize]),
        Err(_) => String::new(),
    };
    let _ = CloseHandle(handle);
    {
        let mut guard = cache.lock().unwrap();
        // PIDs are recycled and a long-lived resident process would otherwise
        // keep growing this map; a coarse reset is cheap and safe.
        if guard.len() > 512 {
            guard.clear();
        }
        guard.insert(pid, name.clone());
    }
    name
}

/// Windows that look like a login/password form. `ES_PASSWORD` (0x0010) is the
/// same bit as `WS_CHILD`, so the window style cannot be used to detect password
/// boxes; we fall back to the window class and title instead.
unsafe fn is_secret_field() -> bool {
    let root = GetForegroundWindow();
    if root.0.is_null() {
        return false;
    }
    let mut cls = [0u16; 64];
    let n = GetClassNameW(root, &mut cls);
    let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]).to_lowercase();
    let title = unsafe { foreground_text() }.to_lowercase();
    const NEEDLES: [&str; 6] = ["password", "пароль", "logon", "login", " signin", "sign in"];
    class.contains("password")
        || title.contains("password")
        || title.contains("пароль")
        || title.contains("logon")
        || title.contains("вход")
        || NEEDLES.iter().any(|p| title.contains(p))
}

/// Character this physical key would produce in layout `hkl`.
unsafe fn char_in_layout(vkey: u16, scan: u32, state: &[u8; 256], hkl: HKL) -> Option<char> {
    let mut buf = [0u16; 8];
    let n = ToUnicodeEx(vkey as u32, scan, state, &mut buf, 0, Some(hkl));
    if n > 0 {
        char::from_u32(buf[0] as u32)
    } else {
        None
    }
}

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
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
                dwExtraInfo: self_tag(),
            },
        },
    }
}

fn unicode(c: char, up: bool) -> INPUT {
    INPUT {
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
                dwExtraInfo: self_tag(),
            },
        },
    }
}

/// Delete `from` and type `to` in one SendInput batch. `flip` adds the
/// Alt+Shift layout switch at the end of the batch.
unsafe fn rewrite(delete: usize, text: &str, trailing: Option<char>, flip: bool) -> u32 {
    // A single atomic batch keeps the injected events in order relative to the
    // keystrokes that follow; separate calls can interleave with the message
    // queue of the focused window.
    let mut inputs: Vec<INPUT> = Vec::with_capacity(delete * 2 + text.chars().count() * 2 + 6);
    for _ in 0..delete {
        inputs.push(key(VK_BACK, false));
        inputs.push(key(VK_BACK, true));
    }
    for c in text.chars() {
        inputs.push(unicode(c, false));
        inputs.push(unicode(c, true));
    }
    // The boundary character (space, comma, ...) was already typed by the app,
    // so put it back after the corrected word.
    if let Some(t) = trailing {
        inputs.push(unicode(t, false));
        inputs.push(unicode(t, true));
    }
    // Alt+Shift: the standard Russian/English layout switch, only when the
    // corrected token actually came from the other layout.
    if flip {
        inputs.push(key(VK_LMENU, false));
        inputs.push(key(VK_LSHIFT, false));
        inputs.push(key(VK_LSHIFT, true));
        inputs.push(key(VK_LMENU, true));
    }
    if verbose() {
        println!(
            "rewrite tid={} delete={delete} text={text:?} trailing={trailing:?} flip={flip}",
            unsafe { windows::Win32::System::Threading::GetCurrentThreadId() }
        );
    }
    let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    if n == 0 && verbose() {
        println!(
            "rewrite SendInput failed: {:?}",
            windows::Win32::Foundation::GetLastError()
        );
    }
    n
}

fn matches(h: Hotkey, vk: u16, ctrl: bool, alt: bool, shift: bool, win: bool) -> bool {
    if h.vk == vk
        && h.vk == VK_PAUSE.0
        && !h.ctrl
        && !h.alt
        && !h.win
        && !alt
        && !win
        && h.shift == shift
    {
        // Some keyboards report the Break/Pause key with a synthetic Ctrl state.
        // For Break hotkeys we should not require the user to hold Ctrl.
        return true;
    }
    h.vk == vk && h.ctrl == ctrl && h.alt == alt && h.shift == shift && h.win == win
}

/// Should the automatic layer touch the foreground window at all?
unsafe fn context_blocked(a: &App) -> bool {
    if a.secret {
        return true;
    }
    let process = foreground_process().to_lowercase();
    // Never correct our own windows: typing in the settings box would otherwise
    // fight the hook.
    if process.ends_with("zя.exe") || process.ends_with("z.exe") {
        return true;
    }
    let title = foreground_text().to_lowercase();
    if title.contains("password") || title.contains("пароль") {
        return true;
    }
    if a.cfg
        .exclude_processes
        .iter()
        .any(|p| !p.is_empty() && process.contains(&p.to_lowercase()))
    {
        return true;
    }
    a.cfg.exclude_dirs.iter().any(|d| {
        let d = d.trim().to_lowercase();
        !d.is_empty() && process.starts_with(&d)
    })
}

use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// CF_UNICODETEXT.
const CF_UNICODE: u32 = 13;

/// The Unicode clipboard contents, or None when there is no text.
pub unsafe fn clipboard_text() -> Option<String> {
    if OpenClipboard(None).is_err() {
        return None;
    }
    let out = match GetClipboardData(CF_UNICODE) {
        Ok(handle) => {
            let h = HGLOBAL(handle.0);
            let ptr = GlobalLock(h) as *const u16;
            let mut s = String::new();
            if !ptr.is_null() {
                let mut i = 0;
                while *ptr.add(i) != 0 {
                    if let Some(ch) = char::from_u32(*ptr.add(i) as u32) {
                        s.push(ch);
                    }
                    i += 1;
                }
            }
            GlobalUnlock(h).ok();
            Some(s)
        }
        Err(_) => None,
    };
    CloseClipboard().ok();
    out
}

unsafe fn set_clipboard(text: &str) {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    if OpenClipboard(None).is_err() {
        return;
    }
    let _ = EmptyClipboard();
    if let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) {
        let ptr = GlobalLock(handle) as *mut u16;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
            GlobalUnlock(handle).ok();
            // Ownership of the memory block moves to the clipboard.
            SetClipboardData(CF_UNICODE, Some(HANDLE(handle.0))).ok();
        }
    }
    CloseClipboard().ok();
}

/// Clipboard text, exposed for tests and diagnostics.
pub fn clipboard_snapshot() -> Option<String> {
    unsafe { clipboard_text() }
}

unsafe fn send_combo(modifier: VIRTUAL_KEY, other: VIRTUAL_KEY) {
    let inputs = [
        key(modifier, false),
        key(other, false),
        key(other, true),
        key(modifier, true),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

unsafe fn send_layout_flip() {
    let inputs = [
        key(VK_LMENU, false),
        key(VK_LSHIFT, false),
        key(VK_LSHIFT, true),
        key(VK_LMENU, true),
    ];
    SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelOp {
    SwapLayout,
    Upper,
    Invert,
}

fn swap_case(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_uppercase() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                c.to_uppercase().next().unwrap_or(c)
            }
        })
        .collect()
}

/// The pure part of a selection gesture: what the selected fragment should
/// become. Kept separate from the clipboard plumbing so it can be tested
/// in-process; the replacement itself is the application's job.
fn transform_selection(op: SelOp, selection: &str) -> String {
    match op {
        SelOp::SwapLayout => crate::layout::map_word(selection),
        SelOp::Upper => selection.to_uppercase(),
        SelOp::Invert => swap_case(selection),
    }
}

/// A marker we put into the clipboard before pressing Ctrl+C. If the app did
/// not copy anything, the marker is still there, so we know there was no
/// selection instead of accidentally pasting the user's old clipboard.
const CLIP_SENTINEL: &str = "\u{1}zya-no-selection\u{1}";

/// Copy whatever the user selected, transform it and paste it back. Returns the
/// transformed text, or None when nothing was selected. The original clipboard
/// contents are restored afterwards.
///
/// This must not run inside the hook callback: SendInput events injected while
/// the hook is still executing only reach the application after the callback
/// returns, so the Ctrl+C would land after the clipboard read.
unsafe fn selection_action(op: SelOp) -> Option<(String, String)> {
    // The gesture fires on key-down, while the user is still holding Ctrl/Alt/
    // Shift. Injecting our own Ctrl+C on top of a held Ctrl makes the target
    // application see a stray modifier-up in the middle of the paste, so wait
    // for the physical modifiers to be released first.
    for _ in 0..30 {
        let held = [
            VK_LCONTROL.0,
            VK_RCONTROL.0,
            VK_LMENU.0,
            VK_RMENU.0,
            VK_LSHIFT.0,
            VK_RSHIFT.0,
        ]
        .iter()
        .any(|vk| is_down(*vk));
        if !held {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let saved = clipboard_text();
    // A marker, not an empty clipboard: some applications keep the clipboard
    // locked, so EmptyClipboard can fail and Ctrl+C would then appear to have
    // copied the previous contents.
    set_clipboard(CLIP_SENTINEL);
    send_combo(VK_LCONTROL, VK_C);
    let mut selection = None;
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Some(s) = clipboard_text() {
            if !s.is_empty() && s != CLIP_SENTINEL {
                selection = Some(s);
                break;
            }
        }
    }
    let Some(selection) = selection else {
        if verbose() {
            println!("selection {op:?}: nothing was copied (marker still in clipboard)");
        }
        if let Some(s) = &saved {
            set_clipboard(s);
        }
        return None;
    };
    let transformed = transform_selection(op, &selection);
    if verbose() {
        println!(
            "selection op {op:?}: saved {:?} -> read {:?} -> transform {:?}",
            saved, selection, transformed
        );
    }
    if transformed != selection {
        set_clipboard(&transformed);
        send_combo(VK_LCONTROL, VK_V);
        if op == SelOp::SwapLayout {
            send_layout_flip();
        }
        // The target application reads the clipboard when it processes WM_PASTE,
        // which happens after SendInput returns. Restoring the user's clipboard
        // immediately made it paste the *old* clipboard over the selection —
        // the "Break inserts my clipboard instead of replacing the text" bug.
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    if let Some(s) = &saved {
        set_clipboard(s);
    }
    Some((selection, transformed))
}

static GESTURE_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Run a selection gesture on a side thread so the injected Ctrl+C/Ctrl+V can
/// actually reach the focused application. `prefer_token` asks for the
/// last-word fallback when nothing is selected.
///
/// Only one gesture runs at a time. Pressing Break twice in a row used to start
/// two of them, and both deleted and retyped the same token — the visible
/// "double switch". The second press is swallowed.
fn spawn_selection(op: SelOp, lang: u32, prefer_token: bool) {
    if GESTURE_BUSY.swap(true, std::sync::atomic::Ordering::Relaxed) {
        if verbose() {
            println!("gesture {op:?} still running; this Break is ignored");
        }
        return;
    }
    std::thread::spawn(move || {
        unsafe {
            selection_gesture(op, lang, prefer_token);
        }
        GESTURE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
    });
}

unsafe fn selection_gesture(op: SelOp, lang: u32, prefer_token: bool) {
    let Some((before, text)) = selection_action(op) else {
        // Nothing was copied. That is only a "no selection" case when the
        // clipboard still holds our marker *and* no earlier selection gesture
        // went through; otherwise the app had a selection it did not copy, and
        // deleting characters would eat the user's text.
        let had_selection = {
            let mut a = app().lock().unwrap();
            let had = a.selection_done;
            a.selection_done = false;
            had
        };
        if prefer_token && !had_selection {
            let mut a = app().lock().unwrap();
            if op == SelOp::SwapLayout {
                swap_last_token(&mut a);
            } else {
                change_last_case(&mut a, op);
            }
        }
        return;
    };

    let mut a = app().lock().unwrap();
    // A selection gesture is a replacement too: it belongs in the diary and in
    // the switch counter, otherwise the log looks empty while Break works.
    if before != text {
        a.switches += 1;
        let why = match op {
            SelOp::SwapLayout => "вручную: смена раскладки выделения",
            SelOp::Upper => "вручную: верхний регистр выделения",
            SelOp::Invert => "вручную: смена регистра выделения",
        };
        record_flip(&mut a, &before, &text);
        crate::diary::log(
            &Decision {
                original: before,
                converted: text.clone(),
                target: if op == SelOp::SwapLayout {
                    crate::engine::TargetLayout::Russian
                } else {
                    crate::engine::TargetLayout::English
                },
                why: why.to_string(),
                flip: op == SelOp::SwapLayout,
            },
            a.switches,
        );
    }
    if op == SelOp::SwapLayout {
        a.lang = other_layout(lang);
    }
    a.word.clear();
    a.run.clear();
    a.closed = Some((text, None));
    a.last = None;
    a.selection_done = true;
    sync_tray(&a);
}

fn record_flip(a: &mut App, original: &str, converted: &str) {
    a.recent_flips.push((
        original.to_string(),
        converted.to_string(),
        std::time::Instant::now(),
    ));
    if a.recent_flips.len() > 12 {
        a.recent_flips.remove(0);
    }
}

/// Switch the layout of the token typed most recently. `run` is the text that is
/// still open (no separator yet); otherwise the token a separator just closed,
/// separator included.
fn swap_last_token(a: &mut App) {
    let (token, boundary) = if !a.word.is_empty() {
        (a.word.clone(), None)
    } else if let Some((t, b)) = a.closed.clone() {
        (t, b)
    } else {
        return;
    };
    let mirrored = crate::layout::map_word(&token);
    if mirrored == token {
        return;
    }
    let target = if mirrored.chars().any(crate::layout::is_cyrillic) {
        crate::engine::TargetLayout::Russian
    } else {
        crate::engine::TargetLayout::English
    };
    let delete = token.chars().count() + boundary.map_or(0, |_| 1);
    unsafe { rewrite(delete, &mirrored, boundary, true) };
    a.lang = other_layout(a.lang);
    a.word.clear();
    a.run.clear();
    a.closed = Some((mirrored.clone(), boundary));
    a.prev_word = mirrored.clone();
    record_flip(a, &token, &mirrored);
    let d = Decision {
        original: token,
        converted: mirrored,
        target,
        why: "вручную: смена раскладки последнего слова".into(),
        flip: true,
    };
    a.switches += 1;
    crate::diary::log(&d, a.switches);
    a.last = Some(d);
}

fn change_last_case(a: &mut App, op: SelOp) {
    let (token, boundary) = if !a.word.is_empty() {
        (a.word.clone(), None)
    } else if let Some((t, b)) = a.closed.clone() {
        (t, b)
    } else {
        return;
    };

    let transformed = transform_selection(op, &token);
    if transformed == token {
        return;
    }

    let delete = token.chars().count() + boundary.map_or(0, |_| 1);
    unsafe { rewrite(delete, &transformed, boundary, false) };

    a.word.clear();
    a.run.clear();
    a.closed = Some((transformed.clone(), boundary));
    a.prev_word = transformed.clone();
    a.last = None;
    a.last_boundary = boundary;

    a.switches += 1;
    crate::diary::log(
        &Decision {
            original: token,
            converted: transformed.clone(),
            target: crate::engine::TargetLayout::Russian,
            why: "вручную: смена регистра последнего слова".into(),
            flip: false,
        },
        a.switches,
    );
}

fn sync_tray(a: &App) {
    crate::tray::refresh(a.lang == RU_LANGID, a.auto);
}

/// Undo the last automatic correction: delete the rewritten word (and the key
/// that closed it) and type the original text back in the previous layout.
/// When a modifier is still physically held, the injection is deferred so the
/// keystrokes do not arrive as WM_SYSKEYDOWN.
fn undo_last(a: &mut App, deferred: bool) {
    let Some(d) = a.last.clone() else {
        return;
    };
    if !a.word.is_empty() {
        return;
    }
    let boundary = a.last_boundary;
    let delete = d.converted.chars().count() + boundary.map_or(0, |_| 1);
    let original = d.original.clone();
    let flip = d.flip;
    if flip {
        a.lang = other_layout(a.lang);
    }
    sync_tray(a);
    a.last = None;
    a.last_boundary = None;
    std::thread::spawn(move || {
        if deferred {
            // The injected backspaces would arrive as Ctrl+Backspace (or
            // Alt+Backspace) while the shortcut modifiers are still physically
            // held, so wait until the user has released them.
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        unsafe { rewrite(delete, &original, boundary, flip) };
    });
    if verbose() {
        println!("undo: {} -> {}", d.converted, d.original);
    }
}

fn boundary_to_write(c: char, flip: bool) -> char {
    // If the corrected token ends on a layout-ambiguous punctuation key, write
    // the punctuation produced by the corrected layout: Russian `б`/`ю` become
    // English `,`/`.` when the word flips from RU to EN.
    if flip && crate::layout::is_ambiguous(c) {
        crate::layout::map_key(c).unwrap_or(c)
    } else {
        c
    }
}

unsafe fn decide_word_on_boundary(a: &mut App, word: &str, c: char) -> bool {
    if verbose() {
        println!("boundary: word={word:?} langid={} auto={}", a.lang, a.auto);
    }

    if !a.auto {
        a.last = None;
        return false;
    }

    let fg_id = GetForegroundWindow().0 as usize;
    if a.secret_fg != fg_id {
        a.secret_fg = fg_id;
        a.secret = is_secret_field();
    }
    if context_blocked(a) {
        a.last = None;
        return false;
    }
    if verbose() {
        println!(
            "   context: {} / {}",
            foreground_process(),
            foreground_text()
        );
    }

    let prev = a.prev_word.clone();
    match a.engine.decide_with(word, &prev) {
        Some(d) => {
            // Ping-pong guard. A token that is flipped back to exactly what it
            // was a couple of seconds ago is not a correction, it is the same
            // correction fighting itself (stale layout tracking or a second
            // hook). Repeating the *same* correction is still allowed.
            let ping_pong = a.recent_flips.iter().any(|(orig, conv, at)| {
                at.elapsed() < std::time::Duration::from_secs(5)
                    && *orig == d.converted
                    && *conv == d.original
            });
            if ping_pong {
                if verbose() {
                    println!("skip {word}: ping-pong guard (just flipped back)");
                }
                a.last = None;
                return false;
            }
            a.switches += 1;
            a.recent_flips.push((
                d.original.clone(),
                d.converted.clone(),
                std::time::Instant::now(),
            ));
            if a.recent_flips.len() > 12 {
                a.recent_flips.remove(0);
            }
            if verbose() {
                println!("decision: {word} -> {}", d.converted);
            }
            crate::diary::log(&d, a.switches);

            let swallow = !c.is_control();
            // Only switch the layout if it is not already where the corrected
            // token needs it. Injecting Alt+Shift when the OS is already on the
            // target layout is what made the layout drift.
            let target_lid = match d.target {
                crate::engine::TargetLayout::Russian => RU_LANGID,
                crate::engine::TargetLayout::English => EN_LANGID,
            };
            let need_flip = d.flip && current_langid() != target_lid;
            let boundary = boundary_to_write(c, need_flip);
            let n = rewrite(
                d.original.chars().count(),
                &d.converted,
                if swallow { Some(boundary) } else { None },
                need_flip,
            );
            if verbose() {
                println!("   rewrite injected {n} events");
            }

            if need_flip {
                a.lang = target_lid;
            }
            sync_tray(a);
            a.word.clear();
            a.prev_word = d.converted.clone();
            a.closed = Some((d.converted.clone(), if swallow { Some(c) } else { None }));
            a.last = Some(d);
            a.last_boundary = if swallow { Some(c) } else { None };
            return swallow;
        }
        None => {
            a.last = None;
            a.prev_word = word.to_string();
            return false;
        }
    }
}

fn stand_down_against_foreign() {
    // `ZYA_IGNORE_FOREIGN=1` keeps the automatic layer on even when another copy
    // is hooking the keyboard; the self-tests use it to measure their own hook.
    if std::env::var("ZYA_IGNORE_FOREIGN").is_ok() {
        return;
    }
    let mut a = app().lock().unwrap();
    if a.foreign {
        return;
    }
    a.foreign = true;
    a.auto = false;
    eprintln!(
        "zя: клавиатуру уже перехватывает другой экземпляр (наши события помечены тегом {:#x}, \
        чужие — нет). Автоматическое исправление в этом процессе выключено, \
        ручные клавиши (Break, Alt) продолжают работать.",
        self_tag()
    );
    crate::diary::log(
        &Decision {
            original: "второй экземпляр".to_string(),
            converted: "автовыключено".to_string(),
            target: crate::engine::TargetLayout::English,
            why: "клавиатуру уже перехватывает другой экземпляр zя".to_string(),
            flip: false,
        },
        a.switches,
    );
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    EVENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // WH_KEYBOARD_LL delivers both key down (code 1) and key up (code 0 is
    // sometimes reported for injected events), so we filter by down-state below.
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    if verbose() {
        let k = unsafe { *(lparam.0 as *const KBDLLHOOKSTRUCT) };
        println!(
            "raw code={code} vk={} scan={} extra={} flags={:?}",
            k.vkCode, k.scanCode, k.dwExtraInfo, k.flags
        );
    }
    let kbd = *(lparam.0 as *const KBDLLHOOKSTRUCT);
    let vk = kbd.vkCode as u16;

    // Never react to events we injected ourselves (identified by the tag).
    if kbd.dwExtraInfo == self_tag() {
        if verbose() {
            println!(
                "self-injected tid={} vk={} scan={}",
                unsafe { windows::Win32::System::Threading::GetCurrentThreadId() },
                vk,
                kbd.scanCode
            );
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // A zя-tagged event that does not carry *our* pid means another copy of zя
    // is hooking the keyboard at the same time. Both copies delete and retype
    // the same word, which is the classic "double switch". The copy that noticed
    // it stops correcting automatically; its manual hotkeys keep working.
    if foreign_tag(kbd.dwExtraInfo) {
        if verbose() {
            println!(
                "foreign-injected vk={vk} scan={} extra={}",
                kbd.scanCode, kbd.dwExtraInfo
            );
        }
        stand_down_against_foreign();
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Track the real layout ourselves. GetKeyboardLayout() is stale for some
    // modern apps, so every genuine Alt+Shift (and every switch we perform)
    // updates App::lang. We also track Ctrl/Win because GetAsyncKeyState is not
    // reliable inside the hook.
    let (win_held, ctrl_held, alt_held, shift_held) = {
        let mut a = app().lock().unwrap();
        let is_shift = vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0;
        let is_alt = vk == VK_LMENU.0 || vk == VK_RMENU.0;
        let is_ctrl = vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0;
        let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
        if is_shift || is_alt || is_ctrl || is_win {
            if kbd.flags.contains(LLKHF_UP) {
                if is_shift {
                    a.shift_down = false;
                } else if is_alt {
                    a.alt_down = false;
                } else if is_ctrl {
                    a.ctrl_down = false;
                } else {
                    a.win_down = false;
                }
                // The chord is only a language switch when it is released with
                // no other key pressed in between.
                if (is_shift || is_alt) && a.alt_shift_pending && !a.alt_down && !a.shift_down {
                    a.alt_shift_pending = false;
                    flip_tracked(&mut a);
                }
            } else {
                // Any second modifier cancels the "Alt alone" undo gesture.
                if a.alt_down && (is_shift || is_ctrl || is_win) {
                    a.alt_pending = false;
                }
                if is_shift {
                    a.shift_down = true;
                    if a.alt_down {
                        a.alt_shift_pending = true;
                    }
                } else if is_alt {
                    a.alt_down = true;
                    if a.shift_down {
                        a.alt_shift_pending = true;
                    }
                } else if is_ctrl {
                    a.ctrl_down = true;
                } else {
                    a.win_down = true;
                }
            }
        } else if a.alt_shift_pending && a.alt_down && a.shift_down {
            // Alt+Shift+<key> is an application shortcut, not a layout switch.
            a.alt_shift_pending = false;
        }
        (a.win_down, a.ctrl_down, a.alt_down, a.shift_down)
    };

    // Win+Space cycles the input language through the OS. Re-seed the tracked
    // layout so the hook does not drift out of sync.
    if vk == VK_SPACE.0 && win_held {
        let mut a = app().lock().unwrap();
        a.last_fg = 0;
        sync_layout(&mut a);
    }

    // Alt alone undoes the last conversion. The decision is taken on release so
    // that Alt+<key> shortcuts (Alt+3, Alt+F4) never trigger an undo.
    if (vk == VK_LMENU.0 || vk == VK_RMENU.0) && !ctrl_held {
        if !kbd.flags.contains(LLKHF_UP) {
            app().lock().unwrap().alt_pending = true;
        } else {
            let mut a = app().lock().unwrap();
            let was_alone = a.alt_pending;
            a.alt_pending = false;
            if was_alone && a.hotkey_undo.alt && a.hotkey_undo.vk == VK_LMENU.0 {
                undo_last(&mut a, true);
            }
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Key-up events carry the LLKHF_UP flag; only presses are interesting.
    if kbd.flags.contains(LLKHF_UP) {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Precise focus control. The pending word belongs to the window that was
    // focused while it was typed. If the foreground window changed in between,
    // the buffer is dropped: rewriting it would inject backspaces and text into
    // a different application, which is the "то работает, то не работает" case.
    {
        let fg = GetForegroundWindow().0 as usize;
        let mut a = app().lock().unwrap();
        if fg != a.word_window {
            if verbose() && (!a.word.is_empty() || !a.run.is_empty()) {
                println!(
                    "focus change {:#x} -> {:#x}: pending {:?} dropped",
                    a.word_window, fg, a.word
                );
            }
            a.word.clear();
            a.run.clear();
            a.closed = None;
            a.word_window = fg;
        }
    }

    {
        let mut a = app().lock().unwrap();
        // Toggle automatic switching: the tray icon (left click or the menu) or
        // the configured hotkey (default Ctrl+Alt+P).
        if matches(
            a.hotkey_toggle,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        ) {
            a.auto = !a.auto;
            println!(
                "zя: автопереключение {}",
                if a.auto {
                    "включено"
                } else {
                    "выключено"
                }
            );
            sync_tray(&a);
            // Swallow the key so the shortcut does not also reach the focused app.
            return LRESULT(1);
        }
        // Manual gestures: Break switches the layout of the selection (or of the
        // last word), Ctrl+Break upper-cases the selection, Shift+Break swaps its
        // case. All of them work even when automatic switching is off.
        if matches(
            a.hotkey_upper,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        ) {
            spawn_selection(SelOp::Upper, a.lang, true);
            return LRESULT(1);
        }
        if matches(
            a.hotkey_invert,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        ) {
            spawn_selection(SelOp::Invert, a.lang, false);
            return LRESULT(1);
        }
        let selection_key = matches(
            a.hotkey_selection,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        );
        let last_key = matches(a.hotkey_last, vk, ctrl_held, alt_held, shift_held, win_held);
        if selection_key || last_key {
            spawn_selection(SelOp::SwapLayout, a.lang, last_key);
            return LRESULT(1);
        }
        // Undo hotkey that is not the bare-Alt gesture handled above.
        if matches(a.hotkey_undo, vk, ctrl_held, alt_held, shift_held, win_held) {
            undo_last(&mut a, ctrl_held || alt_held || shift_held || win_held);
            return LRESULT(1);
        }
    }

    // Ctrl+anything is a shortcut, not text.
    if ctrl_held || is_down(VK_LCONTROL.0) || is_down(VK_RCONTROL.0) {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let mut a = app().lock().unwrap();

    // A key pressed while Alt is held is a shortcut gesture, not text: the
    // current word run is interrupted.
    if alt_held {
        a.alt_pending = false;
        a.word.clear();
        a.identifier = false;
        return CallNextHookEx(None, code, wparam, lparam);
    }

    if vk == VK_BACK.0 {
        a.word.pop();
        if !a.run.is_empty() {
            a.run.pop();
        }
        a.closed = None;
        a.last = None;
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let state = state_array(&a);
    sync_layout(&mut a);
    sync_tray(&a);
    let lid = a.lang;
    if lid != RU_LANGID && lid != EN_LANGID {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let hkl = hkl_for(lid);

    let Some(mut c) = char_in_layout(vk, kbd.scanCode, &state, hkl) else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    if verbose() {
        println!("key {c:?} langid {lid} word {:?}", &a.word);
    }
    // ToUnicodeEx inside a low-level hook does not always honour the shift
    // state, so normalise letter case from the modifiers we tracked ourselves.
    if a.shift_down && c.is_alphabetic() {
        c = c.to_uppercase().next().unwrap_or(c);
    }

    // Requested word-end triggers: `? & / ^` behave like a space and flush the
    // pending buffer immediately.
    let latin_boundary =
        crate::layout::is_latin_boundary_letter(c) && a.word.chars().any(crate::layout::is_latin);
    if crate::layout::is_boundary_trigger(c) || latin_boundary {
        a.identifier = false;
        if !a.run.is_empty() {
            a.closed = Some((a.run.clone(), Some(c)));
        }
        a.run.clear();

        if !a.word.is_empty() {
            let word = std::mem::take(&mut a.word);
            if decide_word_on_boundary(&mut a, &word, c) {
                return LRESULT(1);
            }
        } else {
            a.last_boundary = None;
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Track the physical token for the manual Break hotkey: everything up to the
    // next separator counts as one word, and `-` is *not* a separator.
    if c.is_whitespace() {
        if !a.run.is_empty() {
            a.closed = Some((a.run.clone(), Some(c)));
        }
        a.run.clear();
        a.identifier = false;

        if !a.word.is_empty() {
            let word = std::mem::take(&mut a.word);
            if decide_word_on_boundary(&mut a, &word, c) {
                return LRESULT(1);
            }
        }
        return CallNextHookEx(None, code, wparam, lparam);
    } else if c == '-' {
        a.run.push('-');
    } else if c.is_alphanumeric()
        || crate::layout::is_ambiguous(c)
        || matches!(
            c,
            '_' | '/'
                | '\\'
                | ':'
                | '#'
                | '@'
                | '='
                | '*'
                | '{'
                | '}'
                | '<'
                | '>'
                | '$'
                | '%'
                | '^'
                | '|'
                | '~'
                | '`'
        )
    {
        a.run.push(c);
    } else {
        if !a.run.is_empty() {
            a.closed = Some((a.run.clone(), Some(c)));
        }
        a.run.clear();
    }

    // Letters (and layout-ambiguous punctuation) extend the current word.
    if c.is_alphabetic() || crate::layout::is_ambiguous(c) {
        // Typing again means the user has moved on from any selection gesture:
        // the caret-based Break fallback is allowed once more.
        a.selection_done = false;
        a.word.push(c);
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // A hyphen inside a word keeps the token open: "wi-fi" is one word.
    if c == '-' && !a.word.is_empty() {
        a.word.push('-');
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // A digit or a code-like separator means the run of characters is an
    // identifier/URL. It is kept in the buffer only when it continues a token
    // that already has letters: the engine protects identifiers as a whole,
    // but a mistyped model number such as "ввк4" (DDR4) must still be seen.
    if !c.is_whitespace()
        && (c.is_ascii_digit()
            || matches!(
                c,
                '_' | '/'
                    | '\\'
                    | ':'
                    | '#'
                    | '@'
                    | '='
                    | '*'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '$'
                    | '%'
                    | '^'
                    | '|'
                    | '~'
                    | '`'
            ))
    {
        a.identifier = true;
        if !a.word.is_empty() {
            a.word.push(c);
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }
    // Any remaining non-letter key is a word boundary, not text. It flushes the
    // buffer exactly like a space.
    a.identifier = false;
    if a.word.is_empty() {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let word = std::mem::take(&mut a.word);
    if decide_word_on_boundary(&mut a, &word, c) {
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn guard_mutex_exists(name: &str) -> bool {
    unsafe {
        let wide: Vec<u16> = format!("{name}\0").encode_utf16().collect();
        match CreateMutexW(None, true, PCWSTR(wide.as_ptr())) {
            Ok(_handle) => GetLastError() == ERROR_ALREADY_EXISTS,
            Err(_) => false,
        }
    }
}

/// True when another copy of zя is already resident. Two instances would both
/// hook the keyboard and rewrite the same word twice — the classic cause of
/// "double switches". The named mutex lives for the whole process.
pub fn another_instance_running() -> bool {
    if guard_mutex_exists("Local\\ZyaLayoutGuard") {
        return true;
    }
    if lock_file_taken() {
        return true;
    }
    // Belt and braces: every resident copy owns a hidden tray window of class
    // "ZyaTray". We check this *before* our own tray window is created, so a
    // found window means another copy is resident even when the mutex and the
    // lock file are both unreadable (restricted tokens, sandboxed launches).
    unsafe {
        let class: Vec<u16> = "ZyaTray\0".encode_utf16().collect();
        match FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
            Ok(h) => !h.0.is_null(),
            Err(_) => false,
        }
    }
}

fn lock_file_taken() -> bool {
    unsafe {
        let path = crate::config::config_dir().join("instance.lock");
        let wide: Vec<u16> = path.to_string_lossy().encode_utf16().collect();
        let handle = CreateFileW(
            PCWSTR(wide.as_ptr()),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_NONE,
            None,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        );
        match handle {
            Ok(h) => {
                // Keep the handle alive for the whole process: it is what makes
                // the lock visible to the next copy of zя.
                LOCK_FILE.set(h.0 as usize).ok();
                false
            }
            // Only a sharing violation proves another live copy holds the file.
            // Any other error (restricted tokens, a read-only profile folder)
            // tells us nothing, and must not be reported as "another instance".
            Err(_) => GetLastError() == ERROR_SHARING_VIOLATION,
        }
    }
}

/// Ask every *other* resident copy of zя to quit through its hidden tray window
/// (class "ZyaTray", menu command 4 = "Выход"). Used by `zя.exe stop` and by the
/// installer, so an old build cannot keep hooking the keyboard next to the new
/// one. Returns how many copies were stopped.
pub fn stop_other_instances() -> usize {
    unsafe {
        let class: Vec<u16> = "ZyaTray\0".encode_utf16().collect();
        let title: Vec<u16> = "zя\0".encode_utf16().collect();
        let mut stopped = 0;
        for _ in 0..8 {
            let hwnd = match FindWindowW(PCWSTR(class.as_ptr()), PCWSTR::null()) {
                Ok(h) if !h.0.is_null() => h,
                _ => match FindWindowW(PCWSTR::null(), PCWSTR(title.as_ptr())) {
                    Ok(h) if !h.0.is_null() => h,
                    _ => break,
                },
            };
            if verbose() {
                println!("zя: tray window of another instance = {:?}", hwnd);
            }
            if PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(4), LPARAM(0)).is_err() {
                if verbose() {
                    println!("zя: не удалось послать команду выхода другому экземпляру");
                }
                break;
            }
            stopped += 1;
            std::thread::sleep(std::time::Duration::from_millis(400));
        }
        stopped
    }
}

pub fn install_hook() -> anyhow::Result<()> {
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)?;
        HOOK.set(hook.0 as usize).ok();
        Ok(())
    }
}

pub fn remove_hook() {
    unsafe {
        if let Some(&h) = HOOK.get() {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut std::ffi::c_void));
        }
    }
}

pub fn start_focus_guard() {
    std::thread::spawn(|| {
        let mut last_fg = unsafe { GetForegroundWindow() }.0 as usize;
        let mut last_tick = std::time::Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let fg = unsafe { GetForegroundWindow() }.0 as usize;

            // The tray letters must always show the layout Windows is actually
            // using, not the one we last wrote. Re-read it on every focus change
            // and once a second while nothing changes.
            if fg != last_fg || last_tick.elapsed() >= std::time::Duration::from_secs(1) {
                last_tick = std::time::Instant::now();
                let mut a = app().lock().unwrap();
                if fg != last_fg {
                    last_fg = fg;
                    a.last_fg = fg;
                    a.secret_fg = fg;
                    a.secret = unsafe { is_secret_field() };
                }
                unsafe { sync_layout(&mut a) };
                sync_tray(&a);
            }

            // The buffer itself is *not* cleared here. Dropping it on the poll
            // tick is racy: the hook already resets it on the first keystroke of
            // the new window (see the focus check in hook_proc), and clearing it
            // here would eat the first letter typed after a switch — the
            // "то работает, то не работает" symptom.
        }
    });
}

pub fn run_message_loop() {
    unsafe {
        let mut msg = MSG::default();
        loop {
            let r = GetMessageW(&mut msg, None, 0, 0);
            if !r.as_bool() {
                break;
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

pub fn auto_enabled() -> bool {
    app().lock().unwrap().auto
}

pub fn toggle_auto() {
    let mut a = app().lock().unwrap();
    a.auto = !a.auto;
    sync_tray(&a);
    println!(
        "zя: автопереключение {}",
        if a.auto {
            "включено"
        } else {
            "выключено"
        }
    );
}

pub fn switch_count() -> u32 {
    app().lock().unwrap().switches
}

pub fn current_layout_langid() -> u32 {
    app().lock().unwrap().lang
}

pub fn current_layout_is_russian() -> bool {
    current_layout_langid() == RU_LANGID
}

pub fn configured_toggle() -> Hotkey {
    app().lock().unwrap().hotkey_toggle
}

pub fn configured_undo() -> Hotkey {
    app().lock().unwrap().hotkey_undo
}

pub fn configured_last() -> Hotkey {
    app().lock().unwrap().hotkey_last
}

pub fn configured_selection() -> Hotkey {
    app().lock().unwrap().hotkey_selection
}

pub fn configured_upper() -> Hotkey {
    app().lock().unwrap().hotkey_upper
}

pub fn configured_invert() -> Hotkey {
    app().lock().unwrap().hotkey_invert
}

pub fn set_auto(on: bool) {
    let mut a = app().lock().unwrap();
    a.auto = on;
    sync_tray(&a);
}

/// Replace the process exclusions (used by the tests and by `zя exclude`).
pub fn set_exclude_processes(list: &[String]) {
    let mut a = app().lock().unwrap();
    a.cfg.exclude_processes = list.to_vec();
}

/// Replace the folder exclusions: any foreground process whose executable lives
/// under one of these paths is left alone.
pub fn set_exclude_dirs(list: &[String]) {
    let mut a = app().lock().unwrap();
    a.cfg.exclude_dirs = list.to_vec();
}

pub fn exclude_processes() -> Vec<String> {
    app().lock().unwrap().cfg.exclude_processes.clone()
}

pub fn exclude_dirs() -> Vec<String> {
    app().lock().unwrap().cfg.exclude_dirs.clone()
}

pub fn add_exclude_process(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let lower = name.to_lowercase();
    let mut a = app().lock().unwrap();
    if a.cfg
        .exclude_processes
        .iter()
        .any(|p| p.to_lowercase() == lower)
    {
        return false;
    }
    a.cfg.exclude_processes.push(name.to_string());
    crate::config::save(&a.cfg).ok();
    true
}

pub fn remove_exclude_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    let mut a = app().lock().unwrap();
    let before = a.cfg.exclude_processes.len();
    a.cfg
        .exclude_processes
        .retain(|p| p.to_lowercase() != lower);
    if a.cfg.exclude_processes.len() == before {
        return false;
    }
    crate::config::save(&a.cfg).ok();
    true
}

pub fn add_exclude_dir(dir: &str) -> bool {
    let dir = dir.trim();
    if dir.is_empty() {
        return false;
    }
    let lower = dir.to_lowercase();
    let mut a = app().lock().unwrap();
    if a.cfg.exclude_dirs.iter().any(|d| d.to_lowercase() == lower) {
        return false;
    }
    a.cfg.exclude_dirs.push(dir.to_string());
    crate::config::save(&a.cfg).ok();
    true
}

pub fn remove_exclude_dir(dir: &str) -> bool {
    let lower = dir.to_lowercase();
    let mut a = app().lock().unwrap();
    let before = a.cfg.exclude_dirs.len();
    a.cfg.exclude_dirs.retain(|d| d.to_lowercase() != lower);
    if a.cfg.exclude_dirs.len() == before {
        return false;
    }
    crate::config::save(&a.cfg).ok();
    true
}

/// Process/file name of the application owning `hwnd`, e.g. "chrome.exe".
pub fn window_process_name(hwnd: HWND) -> String {
    unsafe {
        if hwnd.0.is_null() {
            return String::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return String::new();
        }
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        let mut buf = vec![0u16; 1024];
        let mut len = 1024u32;
        let name = match QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        ) {
            Ok(()) => String::from_utf16_lossy(&buf[..len as usize]),
            Err(_) => String::new(),
        };
        let _ = CloseHandle(handle);
        Path::new(&name)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// Add a word to the user dictionary (whitelist), so the hook no longer changes it.
pub fn add_dictionary_word(word: &str) -> bool {
    let letters: String = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    let lower = letters.to_lowercase();
    let mut a = app().lock().unwrap();
    if a.cfg
        .whitelist
        .iter()
        .any(|w| w.eq_ignore_ascii_case(&lower))
    {
        return false;
    }
    a.cfg.whitelist.push(letters);
    if let Err(e) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить словарь: {e}");
    }
    true
}

pub fn dictionary_words() -> Vec<String> {
    app().lock().unwrap().cfg.whitelist.clone()
}

/// Remove a word from the user dictionary. Returns true when it was there.
pub fn remove_dictionary_word(word: &str) -> bool {
    let lower = word.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let mut a = app().lock().unwrap();
    let before = a.cfg.whitelist.len();
    a.cfg.whitelist.retain(|w| !w.eq_ignore_ascii_case(&lower));
    if a.cfg.whitelist.len() == before {
        return false;
    }
    crate::config::save(&a.cfg).ok();
    true
}

/// True when automatic correction is currently suppressed for the foreground
/// window (secret field, excluded process or excluded folder).
pub fn context_blocked_now() -> bool {
    let a = app().lock().unwrap();
    unsafe { context_blocked(&a) }
}

pub fn hide_console() {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let h = GetConsoleWindow();
        if !h.0.is_null() {
            let _ = ShowWindow(h, SW_HIDE);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_instance_guard_detects_a_second_copy() {
        // A private name keeps the test independent from the real resident app
        // that may be running on this machine.
        let name = "Local\\ZyaLayoutGuardTest";
        let first = guard_mutex_exists(name);
        let second = guard_mutex_exists(name);
        assert!(!first, "the first copy must see no other instance");
        assert!(second, "a second copy must be rejected");
    }

    #[test]
    fn break_hotkey_tolerates_synthetic_ctrl() {
        let h = parse_hotkey("shift+break").unwrap();
        assert!(
            matches(h, VK_PAUSE.0, true, false, true, false),
            "Break often carries Ctrl"
        );
        assert!(
            matches(h, VK_PAUSE.0, false, false, true, false),
            "plain Shift+Break"
        );
        assert!(
            !matches(h, VK_PAUSE.0, false, true, true, false),
            "Alt must not sneak in"
        );

        let p = parse_hotkey("break").unwrap();
        assert!(
            matches(p, VK_PAUSE.0, true, false, false, false),
            "Break key can be reported with Ctrl"
        );
    }

    #[test]
    fn hotkey_specs_parse() {
        let h = parse_hotkey("ctrl+alt+p").unwrap();
        assert!(h.ctrl && h.alt && !h.shift && !h.win);
        assert_eq!(h.vk, VK_P.0);

        let p = parse_hotkey("Pause").unwrap();
        assert_eq!(p.vk, VK_PAUSE.0);
        assert!(!p.alt && !p.ctrl);

        assert!(
            parse_hotkey("alt").is_none(),
            "a bare modifier is not a hotkey"
        );
        assert!(parse_hotkey("bogus").is_none());

        let f = parse_hotkey("f12").unwrap();
        assert_eq!(f.vk, VK_F12.0);
    }

    #[test]
    fn hotkey_matching_is_exact() {
        let h = parse_hotkey("ctrl+alt+p").unwrap();
        assert!(matches(h, VK_P.0, true, true, false, false));
        assert!(
            !matches(h, VK_P.0, true, false, false, false),
            "extra key required"
        );
        assert!(
            !matches(h, VK_P.0, true, true, true, false),
            "no extra modifiers"
        );
    }

    /// The selected fragment is transformed on its own; the text around it is
    /// never part of the decision. This is the logic the hook relies on.
    #[test]
    fn selection_fragment_is_transformed_in_place() {
        let text = "начало ghbdtn vbh конец";
        let selection = "ghbdtn vbh";
        let converted = transform_selection(SelOp::SwapLayout, selection);
        assert_eq!(converted, "привет мир");
        // Only the fragment changes: prefix and suffix stay byte-identical.
        let (prefix, suffix) = text.split_once(selection).unwrap();
        assert_eq!(
            format!("{prefix}{converted}{suffix}"),
            "начало привет мир конец"
        );
    }

    #[test]
    fn selection_case_operations_keep_the_fragment_boundaries() {
        assert_eq!(
            transform_selection(SelOp::Upper, "ghbdtn vbh"),
            "GHBDTN VBH"
        );
        assert_eq!(transform_selection(SelOp::Invert, "Ghbdtn"), "gHBDTN");
        assert_eq!(transform_selection(SelOp::Invert, "привет"), "ПРИВЕТ");
        // Text that needs no change stays exactly as it was; Cyrillic is
        // mirrored to Latin by the same rule the hook uses for whole words.
        assert_eq!(transform_selection(SelOp::SwapLayout, "привет"), "ghbdtn");
        assert_eq!(transform_selection(SelOp::SwapLayout, "hello"), "руддщ");
    }
}
