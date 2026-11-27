//! Windows low-level keyboard hook: intercepts every keystroke, decides whether
//! the word just typed was produced with the wrong layout, rewrites it and
//! switches the layout.

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, GlobalFree, ERROR_ALREADY_EXISTS, ERROR_SHARING_VIOLATION,
    GENERIC_READ, GENERIC_WRITE, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, RECT, WPARAM,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_NONE, OPEN_ALWAYS,
};
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyboardLayout, GetKeyboardLayoutList, GetKeyState, SendInput, ToUnicodeEx,
    HKL, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_C, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_DOWN,
    VK_END, VK_ESCAPE, VK_F1, VK_F10, VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7,
    VK_F8, VK_F9, VK_HOME, VK_LCONTROL, VK_LEFT, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_NEXT,
    VK_OEM_3, VK_P, VK_PAUSE, VK_PRIOR, VK_RCONTROL, VK_RETURN, VK_RIGHT, VK_RMENU, VK_RSHIFT,
    VK_RWIN, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, FindWindowExW, GetClassNameW, GetForegroundWindow,
    GetMessageW, GetWindow, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, PostMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    GW_CHILD, GW_HWNDNEXT, GWL_STYLE, HHOOK, KBDLLHOOKSTRUCT, LLKHF_UP, MSG, MOUSEHOOKSTRUCT,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_COMMAND, WM_APP, WM_LBUTTONDOWN, WM_LBUTTONDBLCLK,
    WM_RBUTTONDOWN, WM_MBUTTONDOWN, GetWindowRect,
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
/// `SELF_MAGIC` rounded down to a 16-bit window: every tag we (or another copy
/// of zя) produces lives in `SELF_BASE..SELF_BASE+0x10000`.
const SELF_BASE: usize = SELF_MAGIC & !0xFFFF;

fn self_tag() -> usize {
    static TAG: OnceLock<usize> = OnceLock::new();
    *TAG.get_or_init(|| SELF_MAGIC + std::process::id() as usize)
}

/// An event injected by a different zя process (old builds used the bare base).
///
/// The window is 16 bits wide on purpose. The previous bound
/// `SELF_MAGIC + 0x1_0000_0000` covered 65% of every 32-bit value, so any
/// unrelated program that tags its injected keystrokes (AutoHotkey, PowerToys,
/// remote-access drivers) made us believe a second copy of zя was resident and
/// switched the automatic layer off for the rest of the process.
fn foreign_tag(extra: usize) -> bool {
    if extra == self_tag() {
        return false;
    }
    extra >= SELF_BASE && extra < SELF_BASE + 0x1_0000
}

// ---------------------------------------------------------------------------
// H-02: everything that is not a decision runs outside the hook callback.
//
// A `WH_KEYBOARD_LL` callback is limited by `LowLevelHooksTimeout` (about 300 ms
// on Windows 10+). Writing `diario.log`, printing to the console and refreshing
// the tray icon from inside it can stall the callback long enough for Windows to
// silently remove the hook — "it stopped working until I restarted it". The hook
// now only classifies the keystroke and pushes the work into a bounded queue; a
// single worker thread created with the hook performs the file I/O, the console
// output and the tray update.
// ---------------------------------------------------------------------------

enum Deferred {
    Diary(Decision, u32),
    Tray { russian: bool, auto: bool },
    Note(String),
    Persist(Box<Config>),
}

static SINK: OnceLock<mpsc::Sender<Deferred>> = OnceLock::new();
static PENDING: AtomicUsize = AtomicUsize::new(0);
const MAX_PENDING: usize = 512;

fn sink() -> &'static mpsc::Sender<Deferred> {
    SINK.get_or_init(|| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(item) = rx.recv() {
                PENDING.fetch_sub(1, Ordering::Relaxed);
                match item {
                    Deferred::Diary(decision, switches) => crate::diary::log(&decision, switches),
                    Deferred::Tray { russian, auto } => crate::tray::refresh(russian, auto),
                    Deferred::Note(text) => println!("{text}"),
                    Deferred::Persist(cfg) => {
                        if let Err(error) = crate::config::save(&cfg) {
                            eprintln!("zя: не удалось сохранить настройки: {error}");
                        }
                    }
                }
            }
        });
        tx
    })
}

/// Start the worker with the hook so the first keystroke never pays for the
/// thread creation.
fn ensure_worker() {
    sink();
}

/// Queue a side effect. When the queue is full the hook is more than 512
/// keystrokes behind the application: dropping the journal line is far better
/// than blocking the callback.
fn defer(item: Deferred) {
    if PENDING.load(Ordering::Relaxed) >= MAX_PENDING {
        return;
    }
    PENDING.fetch_add(1, Ordering::Relaxed);
    let _ = sink().send(item);
}

/// L-12: `log_enabled` actually means "no journal".
fn log_decision(a: &App, decision: &Decision) {
    if !a.cfg.log_enabled {
        return;
    }
    defer(Deferred::Diary(decision.clone(), a.switches));
}

fn defer_tray(a: &App) {
    defer(Deferred::Tray {
        russian: a.lang == RU_LANGID,
        auto: a.auto,
    });
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
        // The key that carries `ё` on a Russian layout and `` ` `` on an English
        // one. The hook sees the virtual key, not the character, so Ctrl+` works
        // in both layouts.
        "`" | "grave" | "backtick" => VK_OEM_3.0,
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

/// Which modifier keys are physically held, tracked per *physical* key.
///
/// A single `bool` per pair used to mean "Shift is down": releasing RShift while
/// LShift was still held cleared the whole pair, so a chord such as
/// Alt+LShift+3 was mistaken for a clean Alt+Shift layout switch, and the
/// letters that followed were typed in the wrong case.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct HeldMods {
    lshift: bool,
    rshift: bool,
    lalt: bool,
    ralt: bool,
    lctrl: bool,
    rctrl: bool,
    lwin: bool,
    rwin: bool,
}

impl HeldMods {
    fn set(&mut self, vk: u16, down: bool) -> bool {
        match vk {
            v if v == VK_LSHIFT.0 => self.lshift = down,
            v if v == VK_RSHIFT.0 => self.rshift = down,
            v if v == VK_LMENU.0 => self.lalt = down,
            v if v == VK_RMENU.0 => self.ralt = down,
            v if v == VK_LCONTROL.0 => self.lctrl = down,
            v if v == VK_RCONTROL.0 => self.rctrl = down,
            v if v == VK_LWIN.0 => self.lwin = down,
            v if v == VK_RWIN.0 => self.rwin = down,
            _ => return false,
        }
        true
    }

    fn shift(&self) -> bool {
        self.lshift || self.rshift
    }
    fn alt(&self) -> bool {
        self.lalt || self.ralt
    }
    fn ctrl(&self) -> bool {
        self.lctrl || self.rctrl
    }
    fn win(&self) -> bool {
        self.lwin || self.rwin
    }
    /// State of one specific physical key, for `ToUnicodeEx`.
    fn exact(&self, vk: u16) -> bool {
        match vk {
            v if v == VK_LSHIFT.0 => self.lshift,
            v if v == VK_RSHIFT.0 => self.rshift,
            v if v == VK_LMENU.0 => self.lalt,
            v if v == VK_RMENU.0 => self.ralt,
            v if v == VK_LCONTROL.0 => self.lctrl,
            v if v == VK_RCONTROL.0 => self.rctrl,
            v if v == VK_LWIN.0 => self.lwin,
            v if v == VK_RWIN.0 => self.rwin,
            _ => false,
        }
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
    /// Layout we just requested with an injected Alt+Shift, before the OS has
    /// confirmed it. This prevents `sync_layout()` from immediately overwriting
    /// our intended target with the still-stale focused-control value.
    pending_target: Option<u32>,
    pending_at: std::time::Instant,
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
    /// a low-level hook, so we track every down/up ourselves, separately for the
    /// left and the right key of each pair.
    mods: HeldMods,
    /// Key whose *press* we swallowed as a hotkey: its release must be swallowed
    /// too, otherwise the application sees a lone key-up and thinks a modifier
    /// is stuck.
    swallow_up: Option<u16>,
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
    hotkey_upper_last: Hotkey,
    hotkey_invert: Hotkey,
    hotkey_invert_last: Hotkey,
    /// Put the selected word into the dictionary the engine protects.
    hotkey_add_word: Hotkey,
    /// "Умная отмена" (see `smart_undo` in config.json): Break reads the
    /// punctuation keys the automatic layer ate, and the second press restores
    /// the recorded text instead of mirroring the token again.
    smart_undo: bool,
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
    /// The last manual Break swap: (text before, text after, boundary key, layout
    /// before). Pressing Break a second time by mistake has to hand back the
    /// text exactly as it was — re-mirroring it through the engine can turn a
    /// half-corrected token into something else.
    manual_swap: Option<(String, String, Option<char>, bool)>,
}

static APP: OnceLock<Mutex<App>> = OnceLock::new();
/// L-21: an `AtomicUsize` instead of a `OnceLock` so the hook can be removed and
/// re-installed (after Windows drops it, or after a settings change).
static HOOK: AtomicUsize = AtomicUsize::new(0);
static LOCK_FILE: OnceLock<usize> = OnceLock::new();
static EVENTS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static NUMPAD_DOT_SWALLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn event_count() -> i64 {
    EVENTS.load(std::sync::atomic::Ordering::Relaxed)
}

fn apply_hotkeys(a: &mut App, cfg: &Config) {
    a.hotkey_undo = parse_hotkey(&cfg.hotkey_undo).unwrap_or(Hotkey {
        alt: true,
        vk: VK_LMENU.0,
        ..Default::default()
    });
    a.hotkey_toggle = parse_hotkey(&cfg.hotkey_toggle).unwrap_or(Hotkey {
        ctrl: true,
        alt: true,
        vk: VK_P.0,
        ..Default::default()
    });
    a.hotkey_last = parse_hotkey(&cfg.hotkey_last).unwrap_or(Hotkey {
        vk: VK_PAUSE.0,
        ..Default::default()
    });
    a.hotkey_selection = parse_hotkey(&cfg.hotkey_selection).unwrap_or(Hotkey {
        vk: VK_PAUSE.0,
        ..Default::default()
    });
    a.hotkey_upper = parse_hotkey(&cfg.hotkey_upper).unwrap_or(Hotkey {
        ctrl: true,
        vk: VK_PAUSE.0,
        ..Default::default()
    });
    a.hotkey_upper_last = parse_hotkey(&cfg.hotkey_upper_last).unwrap_or(a.hotkey_upper);
    a.hotkey_invert = parse_hotkey(&cfg.hotkey_invert).unwrap_or(Hotkey {
        shift: true,
        vk: VK_PAUSE.0,
        ..Default::default()
    });
    a.hotkey_invert_last = parse_hotkey(&cfg.hotkey_invert_last).unwrap_or(a.hotkey_invert);
    a.hotkey_add_word = parse_hotkey(&cfg.hotkey_add_word).unwrap_or(Hotkey {
        ctrl: true,
        vk: VK_OEM_3.0,
        ..Default::default()
    });
    a.smart_undo = cfg.smart_undo;
}

/// Push a config into the *live* app: the hook's copy and the Engine the hook
/// asks for a decision. Every mutation of the dictionary or the exclusion list
/// has to go through here, otherwise the resident Engine keeps the old
/// whitelist (L-10).
///
/// `auto` is deliberately not touched: the automatic layer is a runtime switch
/// (menu, tray click, Ctrl+Alt+P) and reloading the hotkeys must not silently
/// turn it back on (M-02).
pub fn apply_config_to_app(cfg: &Config) {
    let Some(app) = APP.get() else {
        return;
    };
    let Some(mut a) = app.lock().ok() else {
        return;
    };
    a.cfg = cfg.clone();
    a.engine.cfg = cfg.clone();
    apply_hotkeys(&mut a, cfg);
}

pub fn reload_hotkeys(cfg: &Config) {
    apply_config_to_app(cfg);
}

/// Read `config.json` again and push it into the running engine. Used by the
/// settings window and by the tray when a CLI command in another process changed
/// the file (L-10).
pub fn reload_config_from_disk() {
    let cfg = crate::config::load();
    apply_config_to_app(&cfg);
}

/// A CLI invocation (`z.exe dict add ...`, `z.exe exclude add ...`) runs in its
/// own process: it rewrites `config.json`, but the resident copy still holds its
/// own `Engine` copy, so the whitelist/exclusions are silently ignored until the
/// next restart. Every resident copy owns a hidden "ZyaTray" window, so the CLI
/// posts `WM_APP + 1` to those windows and the receiver reloads the file.
pub fn notify_resident_reload() -> usize {
    unsafe {
        let mut notified = 0;
        for hwnd in foreign_tray_windows() {
            if PostMessageW(Some(hwnd), WM_APP + 1, WPARAM(0), LPARAM(0)).is_ok() {
                notified += 1;
            }
        }
        notified
    }
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
        // The same gesture can be bound twice: once for a selection, once for
        // the last token. Nothing is selected, so the "last word" binding is the
        // one that fires; when it is the same combination as the selection one,
        // the gesture tries the selection first and falls back to the token.
        let upper_last_key = parse_hotkey(&cfg.hotkey_upper_last).unwrap_or(upper_key);
        let invert_last_key = parse_hotkey(&cfg.hotkey_invert_last).unwrap_or(invert_key);
        let add_word_key = parse_hotkey(&cfg.hotkey_add_word).unwrap_or(Hotkey {
            ctrl: true,
            vk: VK_OEM_3.0,
            ..Default::default()
        });
        let model = crate::model::Model::load().expect("language model data");
        let smart = cfg.smart_undo;
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
            pending_target: None,
            pending_at: std::time::Instant::now(),
            last_fg: 0,
            word_window: 0,
            mods: HeldMods::default(),
            swallow_up: None,
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
            hotkey_upper_last: upper_last_key,
            hotkey_invert: invert_key,
            hotkey_invert_last: invert_last_key,
            hotkey_add_word: add_word_key,
            smart_undo: smart,
            run: String::new(),
            closed: None,
            prev_word: String::new(),
            selection_done: false,
            foreign: false,
            recent_flips: Vec::new(),
            manual_swap: None,
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
        // Prefer the state the hook observed itself, per physical key;
        // GetAsyncKeyState can lag or report 0 inside a low-level hook.
        if a.mods.exact(vk) || is_down(vk) {
            s[vk as usize] = 1;
        }
    }
    // ToUnicodeEx also looks at the generic VK_SHIFT byte for the shift state.
    if a.mods.shift() || is_down(VK_SHIFT.0) {
        s[VK_SHIFT.0 as usize] = 1;
    }
    // L-19: Caps Lock is a toggle, and the low bit of GetKeyState is exactly that
    // toggle state. GetAsyncKeyState's low bit means "was pressed at some point
    // since the previous call", so PRIVEТ typed with Caps Lock on came back as
    // "привет".
    if unsafe { GetKeyState(VK_CAPITAL.0 as i32) & 1 } != 0 {
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
///
/// L-20: the cache used to be built once and never refreshed, so after the user
/// added a second Russian layout (or an English variant) every injected
/// Alt+Shift still pointed at the handle from process start. The list is now
/// re-read every few seconds, and the signature of the list decides whether the
/// cached handles are still valid.
fn hkl_for(langid: u32) -> HKL {
    static CACHE: Mutex<(i64, usize, usize, usize, usize)> = Mutex::new((0, 0, 0, 0, 0));
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // One lock only: taking the mutex twice on the same path (a stale cache
    // falls through to the rebuild below) deadlocked the hook on the very first
    // keystroke.
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if cache.0 == 0 || now - cache.0 >= 5 {
        let (en, ru, count, signature) = unsafe {
            let mut list = [HKL(std::ptr::null_mut()); 32];
            let n = GetKeyboardLayoutList(Some(list.as_mut_slice())) as usize;
            let mut en = 0usize;
            let mut ru = 0usize;
            let mut signature = 0usize;
            for &h in list.iter().take(n) {
                signature = signature.rotate_left(5) ^ (h.0 as usize);
                match langid_of(h) {
                    EN_LANGID => en = h.0 as usize,
                    RU_LANGID => ru = h.0 as usize,
                    _ => {}
                }
            }
            (en, ru, n, signature)
        };
        // Nothing installed: fall back to the thread's own layout instead of
        // pretending the English handle is the Russian one.
        let fallback = unsafe { GetKeyboardLayout(0) }.0 as usize;
        let (en, ru) = (
            if en == 0 { fallback } else { en },
            if ru == 0 { fallback } else { ru },
        );
        cache.0 = now;
        cache.1 = en;
        cache.2 = ru;
        cache.3 = signature;
        cache.4 = count;
    }
    let (en, ru) = (cache.1, cache.2);
    HKL(if langid == RU_LANGID {
        ru as *mut std::ffi::c_void
    } else {
        en as *mut std::ffi::c_void
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
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
        flip_tracked(&mut a);
    }
}

/// Re-read the layout of the focused control into the tracked state.
pub fn sync_layout_now() {
    unsafe {
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
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
            let a = a.lock().unwrap_or_else(|error| error.into_inner());
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
unsafe fn mark_pending_layout(a: &mut App, target: u32) {
    a.pending_target = Some(target);
    a.pending_at = std::time::Instant::now();
}

unsafe fn sync_layout(a: &mut App) {
    // A flip we just injected can lag behind GetKeyboardLayout: the OS has not
    // yet applied the new layout to the focused thread. Keep the intended target
    // for a short window instead of trusting the stale probe and undoing it.
    if let Some(target) = a.pending_target {
        if a.pending_at.elapsed() < std::time::Duration::from_millis(900) {
            let site = focused_langid_in(a);
            let root = langid_of(foreground_hkl());
            if site == target || root == target {
                a.lang = target;
                a.pending_target = None;
                return;
            }
            if a.lang == target {
                return;
            }
            a.pending_target = None;
        } else {
            a.pending_target = None;
        }
    }

    let lid = focused_langid_in(a);
    if lid == RU_LANGID || lid == EN_LANGID {
        a.lang = lid;
        if verbose() && a.lang_debug != lid {
            let site = focused_site_cached(a);
            defer(Deferred::Note(format!(
                "layout sync {:#06x}: site {} thread {}",
                lid,
                window_class(site),
                GetWindowThreadProcessId(site, None)
            )));
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
        let target = other_layout(a.lang);
        a.lang = target;
        mark_pending_layout(a, target);
    }
    // H-02: the tray is refreshed off the hook callback.
    defer_tray(a);
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
        let guard = cache.lock().unwrap_or_else(|error| error.into_inner());
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
        let mut guard = cache.lock().unwrap_or_else(|error| error.into_inner());
        // PIDs are recycled and a long-lived resident process would otherwise
        // keep growing this map; a coarse reset is cheap and safe.
        if guard.len() > 512 {
            guard.clear();
        }
        guard.insert(pid, name.clone());
    }
    name
}

/// `ES_PASSWORD` is 0x0020. The old comment claimed it was 0x0010 and collided
/// with `WS_CHILD`: 0x0010 is `ES_LOWERCASE` and `WS_CHILD` is 0x40000000, so the
/// style of an edit control can and should be read (L-24).
const ES_PASSWORD: isize = 0x0020;

unsafe fn style_of(hwnd: HWND) -> isize {
    GetWindowLongPtrW(hwnd, GWL_STYLE)
}

/// True when the focused window (or any of its visible controls) is a password
/// box. A password is never a mistyped layout: rewriting it and writing it to
/// `diario.log` is a privacy leak.
unsafe fn is_secret_field() -> bool {
    let root = GetForegroundWindow();
    if root.0.is_null() {
        return false;
    }
    if style_of(root) & ES_PASSWORD != 0 {
        return true;
    }
    // The password control is a child of the dialog, so look at the controls.
    // The walk is deliberately shallow: this runs once per foreground window.
    let null = HWND(std::ptr::null_mut());
    let mut queue = vec![root];
    let mut head = 0;
    while head < queue.len() && queue.len() < 80 {
        let w = queue[head];
        head += 1;
        let mut child = GetWindow(w, GW_CHILD).unwrap_or(null);
        while !child.0.is_null() {
            if IsWindowVisible(child).as_bool() {
                if style_of(child) & ES_PASSWORD != 0 {
                    return true;
                }
                queue.push(child);
            }
            child = GetWindow(child, GW_HWNDNEXT).unwrap_or(null);
        }
    }
    // Fallback for applications that draw their own text field: the class and the
    // title still tell us this is a login form.
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

/// The physical key behind a control character, if it has one.
fn trailing_vk(c: char) -> Option<VIRTUAL_KEY> {
    match c {
        '\r' | '\n' => Some(VK_RETURN),
        '\t' => Some(VK_TAB),
        _ => None,
    }
}

/// The key that closed the word, put back into the batch. Enter and Tab are
/// control keys: sending them as a Unicode character does nothing, the
/// application needs the physical VK_RETURN/VK_TAB (L-02).
fn push_trailing(inputs: &mut Vec<INPUT>, c: char) {
    match trailing_vk(c) {
        Some(vk) => {
            inputs.push(key(vk, false));
            inputs.push(key(vk, true));
        }
        None => {
            inputs.push(unicode(c, false));
            inputs.push(unicode(c, true));
        }
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
    // The boundary character (space, comma, Enter, ...) was absorbed by the hook,
    // so put it back after the corrected word.
    if let Some(t) = trailing {
        push_trailing(&mut inputs, t);
    }
    // Alt+Shift: the standard Russian/English layout switch, only when the
    // corrected token actually came from the other layout.
    if flip {
        inputs.push(key(VK_LMENU, false));
        inputs.push(key(VK_LSHIFT, false));
        inputs.push(key(VK_LSHIFT, true));
        inputs.push(key(VK_LMENU, true));
    }
    let n = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    if verbose() {
        defer(Deferred::Note(format!(
            "rewrite delete={delete} text={text:?} trailing={trailing:?} flip={flip} -> {n} events \
             (last error {:?})",
            if n == 0 {
                GetLastError()
            } else {
                windows::Win32::Foundation::ERROR_SUCCESS
            }
        )));
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
    // Never correct our own settings window: typing in the filter box would
    // otherwise fight the hook. We match the window, not the process, so the
    // built-in demo windows (and a second copy of the app) still behave normally.
    if process.ends_with("z.exe") {
        return true;
    }
    let cls = window_class(GetForegroundWindow()).to_lowercase();
    if cls == "zyasettings" {
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

/// The clipboard is a single, lockable resource. Another process can hold it for
/// a moment, and two of our own threads — a selection gesture on the side thread
/// and a "Скопировать строку" click in the settings window — must never open it
/// at the same time, otherwise one of them writes while the other is still
/// reading. Every clipboard access in this module goes through this lock.
fn clip_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Open the clipboard with retries: `OpenClipboard` fails while another process
/// (a clipboard manager, a remote-desktop tool) is in the middle of a write, and
/// a single attempt used to make the whole gesture silently fall back to the old
/// contents.
unsafe fn open_clipboard() -> bool {
    for _ in 0..5 {
        if OpenClipboard(None).is_ok() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// The Unicode clipboard contents, or None when there is no text.
pub unsafe fn clipboard_text() -> Option<String> {
    let _guard = clip_lock().lock().unwrap_or_else(|e| e.into_inner());
    if !open_clipboard() {
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

/// Put `text` into the clipboard. Returns true only when the write really
/// landed: the caller needs to know, because a failed write used to leave the
/// user's old contents in place and the gesture then pasted them.
unsafe fn set_clipboard(text: &str) -> bool {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let _guard = clip_lock().lock().unwrap_or_else(|e| e.into_inner());
    for _ in 0..4 {
        if OpenClipboard(None).is_ok() {
            let _ = EmptyClipboard();
            let mut written = false;
            if let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2) {
                let ptr = GlobalLock(handle) as *mut u16;
                if ptr.is_null() {
                    // GlobalUnlock failed or the block was never lockable: nobody
                    // else can use it, so free it here instead of leaking it.
                    GlobalFree(Some(handle)).ok();
                } else {
                    std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
                    GlobalUnlock(handle).ok();
                    // On success ownership of the block moves to the clipboard.
                    written = SetClipboardData(CF_UNICODE, Some(HANDLE(handle.0))).is_ok();
                    if !written {
                        GlobalFree(Some(handle)).ok();
                    }
                }
            }
            CloseClipboard().ok();
            if written {
                return true;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

/// Leave the clipboard empty instead of keeping our marker behind. Used when the
/// user's clipboard had no text to begin with, so nothing useful can be
/// restored.
unsafe fn clear_clipboard() -> bool {
    let _guard = clip_lock().lock().unwrap_or_else(|e| e.into_inner());
    if open_clipboard() {
        let cleared = EmptyClipboard().is_ok();
        CloseClipboard().ok();
        return cleared;
    }
    false
}

/// Empty the clipboard from another module (the Telegram probe), under the same
/// lock the gestures use.
pub fn clear_clipboard_text() -> bool {
    unsafe { clear_clipboard() }
}

/// Clipboard text, exposed for tests and diagnostics.
pub fn clipboard_snapshot() -> Option<String> {
    unsafe { clipboard_text() }
}

/// Write text into the clipboard from another module (the Telegram composer
/// borrows the clipboard the same way a gesture does).
pub fn set_clipboard_text(text: &str) -> bool {
    unsafe { set_clipboard(text) }
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
    /// Ctrl+`: copy the selection and put its word into the dictionary. The
    /// text itself is not touched, so the "transform" is the identity.
    AddWord,
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
        SelOp::AddWord => selection.to_string(),
    }
}

/// The word a selection gesture hands to the dictionary: the first run of
/// letters. Highlighting a whole phrase adds its first word, which is what the
/// user usually means.
fn selection_word(selection: &str) -> String {
    selection
        .split(|c: char| !c.is_alphabetic())
        .find(|part| !part.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// A marker we put into the clipboard before pressing Ctrl+C. If the app did
/// not copy anything, the marker is still there, so we know there was no
/// selection instead of accidentally pasting the user's old clipboard.
const CLIP_SENTINEL: &str = "\u{1}zya-no-selection\u{1}";

/// The dictionary is a list of *short* service words; anything longer than this
/// is a normal word the engine already judges from its letters.
const MAX_LEXICON_WORD: usize = 24;

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
    let marker_written = set_clipboard(CLIP_SENTINEL);
    send_combo(VK_LCONTROL, VK_C);
    let mut selection = None;
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Some(s) = clipboard_text() {
            if s.is_empty() || s == CLIP_SENTINEL {
                continue;
            }
            // The marker never landed (the clipboard was locked), so the only way
            // to tell "the user copied a selection" from "nothing was copied" is
            // that the contents changed. When they did not, Ctrl+C copied nothing,
            // and pasting would drop the user's old clipboard over the selection.
            if !marker_written && saved.as_ref() == Some(&s) {
                continue;
            }
            selection = Some(s);
            break;
        }
    }
    let Some(selection) = selection else {
        if verbose() {
            println!("selection {op:?}: nothing was copied (marker still in clipboard)");
        }
        restore_clipboard(&saved);
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
        if set_clipboard(&transformed) {
            send_combo(VK_LCONTROL, VK_V);
            if op == SelOp::SwapLayout {
                send_layout_flip();
            }
            // The target application reads the clipboard when it processes
            // WM_PASTE, which happens after SendInput returns. Restoring the
            // user's clipboard immediately made it paste the *old* clipboard over
            // the selection — the "Break inserts my clipboard instead of
            // replacing the text" bug. The old fixed 250 ms sleep was a gamble on
            // how fast the application handled the message; instead we keep the
            // transformed text until the application has demonstrably looked at
            // the clipboard (it changed), and in the worst case wait out the whole
            // grace window before handing the user's contents back.
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_millis(900);
            while std::time::Instant::now() < deadline {
                std::thread::sleep(std::time::Duration::from_millis(60));
                if clipboard_text().as_deref() != Some(transformed.as_str()) {
                    break;
                }
            }
        } else if verbose() {
            println!("selection op {op:?}: clipboard is locked, paste skipped");
        }
    }
    restore_clipboard(&saved);
    Some((selection, transformed))
}

/// Give the user's clipboard back. When there was no text to save, the marker
/// must not stay behind: a later Ctrl+V somewhere else would insert
/// "zya-no-selection".
unsafe fn restore_clipboard(saved: &Option<String>) {
    match saved {
        Some(text) => {
            if !set_clipboard(text) {
                // The clipboard is locked by another program. We cannot put the
                // user's text back, but leaving our marker behind is worse: their
                // next Ctrl+V anywhere would insert "zya-no-selection".
                if clipboard_text().as_deref() == Some(CLIP_SENTINEL) {
                    clear_clipboard();
                }
            }
        }
        None => {
            // Only wipe the clipboard when it holds our marker. A copied image or a
            // list of files has no CF_UNICODETEXT, and EmptyClipboard would destroy
            // formats the user still needs.
            if clipboard_text().as_deref() == Some(CLIP_SENTINEL) {
                clear_clipboard();
            }
        }
    }
}

static GESTURE_BUSY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Ownership of the clipboard and of the keystroke stream while a gesture runs.
/// Releasing happens in `Drop`, so an early return — or a panic unwinding the
/// gesture thread — can never leave the flag set forever, which would make the
/// hook pass every key through and silently stop the engine.
pub struct ClipboardGesture {
    _never_used: (),
}

impl ClipboardGesture {
    pub fn try_acquire() -> Option<ClipboardGesture> {
        if GESTURE_BUSY.swap(true, std::sync::atomic::Ordering::Relaxed) {
            None
        } else {
            Some(ClipboardGesture { _never_used: () })
        }
    }
}

impl Drop for ClipboardGesture {
    fn drop(&mut self) {
        GESTURE_BUSY.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What a manual gesture is bound to. The same combination can be bound to both
/// kinds at once ("Break" by default): then the selection is tried first and the
/// last token is the fallback.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Scope {
    Selection,
    Word,
    Both,
}

fn scope_of(selection_bound: bool, word_bound: bool) -> Scope {
    match (selection_bound, word_bound) {
        (true, true) => Scope::Both,
        (true, false) => Scope::Selection,
        (false, true) => Scope::Word,
        (false, false) => Scope::Both,
    }
}

/// Run a gesture on a side thread so the injected Ctrl+C/Ctrl+V can actually
/// reach the focused application. A gesture bound only to the last token needs
/// no clipboard dance at all.
///
/// Only one gesture runs at a time. Pressing Break twice in a row used to start
/// two of them, and both deleted and retyped the same token — the visible
/// "double switch". The second press is swallowed.
fn spawn_selection(op: SelOp, lang: u32, scope: Scope) {
    let Some(guard) = ClipboardGesture::try_acquire() else {
        if verbose() {
            println!("gesture {op:?} still running; this Break is ignored");
        }
        return;
    };
    std::thread::spawn(move || {
        unsafe {
            selection_gesture(op, lang, scope);
        }
        drop(guard);
    });
}

unsafe fn selection_gesture(op: SelOp, lang: u32, scope: Scope) {
    if scope == Scope::Word {
        // Nothing in the settings bound this key to a selection: work on the
        // last token straight away, without touching the clipboard.
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
        if op == SelOp::SwapLayout {
            swap_last_token(&mut a);
        } else {
            change_last_case(&mut a, op);
        }
        return;
    }

    let Some((before, text)) = selection_action(op) else {
        if op == SelOp::AddWord {
            crate::settings_ui::update_status(
                "Сначала выделите слово — Ctrl+` добавляет его в словарь",
            );
            defer(Deferred::Note("zя: словарь: выделения не было".into()));
            return;
        }
        // Nothing was copied. That is only a "no selection" case when the
        // clipboard still holds our marker *and* no earlier selection gesture
        // went through; otherwise the app had a selection it did not copy, and
        // deleting characters would eat the user's text.
        let had_selection = {
            let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
            let had = a.selection_done;
            a.selection_done = false;
            had
        };
        if scope == Scope::Both && !had_selection {
            let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
            if op == SelOp::SwapLayout {
                swap_last_token(&mut a);
            } else {
                change_last_case(&mut a, op);
            }
        }
        return;
    };

    if op == SelOp::AddWord {
        let word = selection_word(&text);
        if word.is_empty() {
            crate::settings_ui::update_status("В выделении нет букв — словарь не изменился");
            return;
        }
        let script = if crate::layout::is_cyrillic(word.chars().next().unwrap_or(' ')) {
            "русский"
        } else {
            "английский"
        };
        if word.chars().count() > MAX_LEXICON_WORD {
            crate::settings_ui::update_status(&format!(
                "«{word}» длиннее {MAX_LEXICON_WORD} букв: в словарь берут короткие слова"
            ));
            return;
        }
        let added = add_short_word(&word);
        crate::settings_ui::report_dictionary_word(&word, added, script);
        defer(Deferred::Note(format!(
            "zя: словарь: {} «{word}» в список {}",
            if added { "добавлено" } else { "уже было" },
            script
        )));
        return;
    }

    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    // A selection gesture is a replacement too: it belongs in the diary and in
    // the switch counter, otherwise the log looks empty while Break works.
    let changed = before != text;
    if changed {
        a.switches += 1;
        let why = match op {
            SelOp::SwapLayout => "вручную: смена раскладки выделения",
            SelOp::Upper => "вручную: верхний регистр выделения",
            SelOp::Invert => "вручную: смена регистра выделения",
            SelOp::AddWord => "вручную: слово добавлено в словарь",
        };
        record_flip(&mut a, &before, &text);
        log_decision(
            &a,
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
        );
    }
    if op == SelOp::SwapLayout && changed {
        let has_cyrillic = text.chars().any(crate::layout::is_cyrillic);
        let has_latin = text.chars().any(crate::layout::is_latin);
        let target = if has_cyrillic && !has_latin {
            RU_LANGID
        } else if has_latin && !has_cyrillic {
            EN_LANGID
        } else {
            other_layout(lang)
        };
        a.lang = target;
        mark_pending_layout(&mut a, target);
    }
    a.word.clear();
    a.run.clear();
    a.closed = Some((text, None));
    a.last = None;
    a.manual_swap = None;
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

/// The destructive rewrite below assumes the caret still sits right after the
/// token the hook tracked. Keys that move the caret are visible to the hook, but
/// a mouse click moves it invisibly: then the backspaces eat characters from the
/// middle of the word and the retyped token lands in the wrong place — the
/// "Break inserted garbage" report. A clipboard probe is not an option: the
/// classic Win32 Edit control does not extend a selection on Shift+Left, so
/// Ctrl+C would copy nothing and the check could not tell "no selection" from
/// "selection somewhere else". The mouse hook is the only reliable signal.
static MOUSE_HOOK: AtomicUsize = AtomicUsize::new(0);
/// Set when a click landed in the control that currently owns the caret.
static CARET_SUSPECT: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // L-32: `nCode` is not tested. Mouse events reach this hook with nCode 0
    // here, so comparing against HC_ACTION would silently drop every click.
    // The message in `wParam` is the reliable test, and plain cursor movement
    // is ignored because hovering does not put the caret anywhere.
    let msg = wparam.0;
    if msg == WM_LBUTTONDOWN as usize
        || msg == WM_RBUTTONDOWN as usize
        || msg == WM_MBUTTONDOWN as usize
        || msg == WM_LBUTTONDBLCLK as usize
    {
        let ms = unsafe { *(lparam.0 as *const MOUSEHOOKSTRUCT) };
        // `GetFocus` in a hook belongs to *our* thread, not to the application
        // being typed into, and the hook's `hwnd` is not always a real window
        // (it can be 0 or an internal handle). The point is always valid, so
        // the click is attributed to the foreground window by coordinates: a
        // click inside the window the user is typing in may have moved the
        // caret anywhere. A click in another window cannot.
        let fg = unsafe { GetForegroundWindow() };
        let mut rect = RECT::default();
        let inside = unsafe {
            GetWindowRect(fg, &mut rect).is_ok()
                && ms.pt.x >= rect.left
                && ms.pt.x < rect.right
                && ms.pt.y >= rect.top
                && ms.pt.y < rect.bottom
        };
        if verbose() {
            defer(Deferred::Note(format!(
                "mouse click {:#06x} at {},{} inside foreground {:?}: {}",
                msg, ms.pt.x, ms.pt.y, fg, inside
            )));
        }
        if inside {
            // L-32: the click may have put the caret anywhere, so the token the
            // keyboard tracking built up is no longer next to it. This is the
            // same "drop the buffer" rule L-26 applies to caret keys; without it
            // Break rewrites characters from the middle of the word — the
            // "Break turned my word into garbage" report.
            CARET_SUSPECT.store(true, Ordering::Relaxed);
            let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
            a.word.clear();
            a.run.clear();
            a.closed = None;
            a.last = None;
            a.last_boundary = None;
            a.manual_swap = None;
            a.selection_done = false;
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn caret_suspect() -> bool {
    CARET_SUSPECT.load(Ordering::Relaxed)
}

fn caret_suspect_note(a: &App) {
    if verbose() {
        defer(Deferred::Note(format!(
            "break refused: mouse click moved the caret, tracked token {:?}",
            &a.word
        )));
    }
    unsafe {
        crate::settings_ui::update_status(
            "Break: курсор сдвинут мышью — последнее слово не меняется",
        );
    }
}


fn swap_last_token(a: &mut App) {
    if caret_suspect() {
        caret_suspect_note(a);
        return;
    }
    let Some(mv) = crate::layout::plan_manual_move(
        &a.word,
        a.closed.clone(),
        &a.manual_swap,
        a.lang == RU_LANGID,
        a.smart_undo,
        &|s| a.engine.believable_word(s),
    ) else {
        return;
    };
    let lang_after = if mv.russian_after { RU_LANGID } else { EN_LANGID };
    let delete = mv.from.chars().count() + mv.boundary.map_or(0, |_| 1);
    let need_flip = a.lang != lang_after;
    let expected_events = (delete * 2
        + mv.to.chars().count() * 2
        + mv.boundary.map_or(0, |_| 2)
        + if need_flip { 4 } else { 0 })
        as u32;
    let n = unsafe { rewrite(delete, &mv.to, mv.boundary, need_flip) };
    if n >= expected_events {
        a.lang = lang_after;
        unsafe { mark_pending_layout(a, lang_after) };
    }
    a.word.clear();
    a.run.clear();
    a.closed = Some((mv.to.clone(), mv.boundary));
    a.prev_word = mv.to.clone();
    a.manual_swap = mv.undo;
    record_flip(a, &mv.from, &mv.to);
    let d = Decision {
        original: mv.from.clone(),
        converted: mv.to.clone(),
        target: if mv.to.chars().any(crate::layout::is_cyrillic) {
            crate::engine::TargetLayout::Russian
        } else {
            crate::engine::TargetLayout::English
        },
        why: mv.why.to_string(),
        flip: true,
    };
    a.switches += 1;
    log_decision(a, &d);
    a.last = Some(d);
    defer_tray(a);
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
    if caret_suspect() {
        caret_suspect_note(&*a);
        return;
    }
    unsafe { rewrite(delete, &transformed, boundary, false) };

    a.word.clear();
    a.run.clear();
    a.closed = Some((transformed.clone(), boundary));
    a.prev_word = transformed.clone();
    a.last = None;
    a.last_boundary = boundary;

    a.switches += 1;
    log_decision(
        a,
        &Decision {
            original: token,
            converted: transformed.clone(),
            target: crate::engine::TargetLayout::Russian,
            why: "вручную: смена регистра последнего слова".into(),
            flip: false,
        },
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
        let original_lid = if original.chars().any(crate::layout::is_cyrillic) {
            RU_LANGID
        } else {
            EN_LANGID
        };
        a.lang = original_lid;
        unsafe { mark_pending_layout(a, original_lid) };
    }
    defer_tray(a);
    a.last = None;
    a.last_boundary = None;
    std::thread::spawn(move || {
        if deferred {
            // The injected backspaces would arrive as Ctrl+Backspace (or
            // Alt+Backspace) while the shortcut modifiers are still physically
            // held, so wait until the user has released them.
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        // The same rule the manual gestures use: if the user clicked somewhere
        // in the meantime, the backspaces would eat the wrong text.
        if caret_suspect() {
            if verbose() {
                defer(Deferred::Note("undo refused: mouse click moved the caret".into()));
            }
            return;
        }
        unsafe { rewrite(delete, &original, boundary, flip) };
    });
    if verbose() {
        defer(Deferred::Note(format!("undo: {} -> {}", d.converted, d.original)));
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

/// The decision the hook made about the pending word. Planning is cheap and
/// touches neither the input stream nor the disk (H-02); only
/// `commit_correction` below calls `SendInput`, and the hook releases the App
/// lock before doing it.
struct Correction {
    decision: Decision,
    /// Character re-injected after the corrected word.
    trailing: char,
    /// The key the user physically pressed, remembered for the undo gesture.
    boundary: char,
    flip: bool,
    target_lid: u32,
    expected_events: u32,
}

fn plan_correction(a: &mut App, word: &str, c: char) -> Option<Correction> {
    if verbose() {
        defer(Deferred::Note(format!(
            "boundary: word={word:?} langid={} auto={}",
            a.lang, a.auto
        )));
    }

    if !a.auto {
        a.last = None;
        return None;
    }

    let fg_id = unsafe { GetForegroundWindow() }.0 as usize;
    if a.secret_fg != fg_id {
        a.secret_fg = fg_id;
        a.secret = unsafe { is_secret_field() };
    }
    if unsafe { context_blocked(a) } {
        a.last = None;
        return None;
    }
    if verbose() {
        defer(Deferred::Note(format!(
            "   context: {} / {}",
            unsafe { foreground_process() },
            unsafe { foreground_text() }
        )));
    }

    let prev = a.prev_word.clone();
    let d = match a.engine.decide_with(word, &prev) {
        Some(d) => d,
        None => {
            a.last = None;
            a.prev_word = word.to_string();
            return None;
        }
    };

    // Ping-pong guard. A token that is flipped back to exactly what it was a
    // couple of seconds ago is not a correction, it is the same correction
    // fighting itself (stale layout tracking or a second hook). Repeating the
    // *same* correction is still allowed.
    let ping_pong = a.recent_flips.iter().any(|(orig, conv, at)| {
        at.elapsed() < std::time::Duration::from_secs(5)
            && *orig == d.converted
            && *conv == d.original
    });
    if ping_pong {
        if verbose() {
            defer(Deferred::Note(format!(
                "skip {word}: ping-pong guard (just flipped back)"
            )));
        }
        a.last = None;
        return None;
    }

    // Only switch the layout if it is not already where the corrected token
    // needs it. Injecting Alt+Shift when the OS is already on the target layout
    // is what made the layout drift.
    let target_lid = match d.target {
        crate::engine::TargetLayout::Russian => RU_LANGID,
        crate::engine::TargetLayout::English => EN_LANGID,
    };

    // The layout that actually receives the keystroke is the layout of the
    // focused input control, not necessarily the root foreground window.
    // Chromium/Electron/Notepad have the text control on another thread, so
    // GetKeyboardLayout(GetForegroundWindow()) can remain stale and make us
    // skip a needed Alt+Shift.
    unsafe { sync_layout(a) };
    let need_flip = d.flip && a.lang != target_lid;

    // L-02: the boundary key is *always* absorbed. Enter and Tab used to be
    // passed through, and the injected backspaces then ate the text on the line
    // the Enter had already created. The key is re-injected as part of the same
    // batch, as a physical VK, so the application sees one continuous gesture.
    let expected_events = (d.original.chars().count() * 2
        + d.converted.chars().count() * 2
        + 2
        + if need_flip { 4 } else { 0 }) as u32;
    Some(Correction {
        decision: d,
        trailing: boundary_to_write(c, need_flip),
        boundary: c,
        flip: need_flip,
        target_lid,
        expected_events,
    })
}

/// Perform the injection and commit the state. Returns true when the boundary
/// key was absorbed, i.e. the hook must swallow it.
unsafe fn commit_correction(correction: &Correction, word: &str) -> bool {
    let d = &correction.decision;
    let n = rewrite(
        d.original.chars().count(),
        &d.converted,
        Some(correction.trailing),
        correction.flip,
    );

    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    if n == 0 {
        // L-04: SendInput injected nothing (UIPI, BlockInput, a protected field,
        // another input desktop). Nothing reached the application, so the
        // boundary key must go through untouched and the word must stay exactly
        // as the user typed it.
        a.last = None;
        a.last_boundary = None;
        a.prev_word = word.to_string();
        if verbose() {
            defer(Deferred::Note(format!(
                "rewrite refused for {word:?}: nothing was injected, the key is passed through"
            )));
        }
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
        defer(Deferred::Note(format!(
            "decision: {word} -> {} ({} events, {n} injected)",
            d.converted,
            correction.expected_events
        )));
    }
    log_decision(&a, d);

    if correction.flip && n >= correction.expected_events {
        a.lang = correction.target_lid;
        mark_pending_layout(&mut a, correction.target_lid);
    }
    defer_tray(&a);
    a.word.clear();
    a.prev_word = d.converted.clone();
    a.closed = Some((d.converted.clone(), Some(correction.boundary)));
    a.last = Some(d.clone());
    a.last_boundary = Some(correction.boundary);
    true
}

fn stand_down_against_foreign() {
    // `ZYA_IGNORE_FOREIGN=1` keeps the automatic layer on even when another copy
    // is hooking the keyboard; the self-tests use it to measure their own hook.
    if std::env::var("ZYA_IGNORE_FOREIGN").is_ok() {
        return;
    }
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    if a.foreign {
        return;
    }
    a.foreign = true;
    a.auto = false;
    defer(Deferred::Note(format!(
        "zя: клавиатуру уже перехватывает другой экземпляр (наши события помечены тегом {:#x}, \
        чужие — нет). Автоматическое исправление в этом процессе выключено, \
        ручные клавиши (Break, Alt) продолжают работать.",
        self_tag()
    )));
    log_decision(
        &a,
        &Decision {
            original: "второй экземпляр".to_string(),
            converted: "автовыключено".to_string(),
            target: crate::engine::TargetLayout::English,
            why: "клавиатуру уже перехватывает другой экземпляр zя".to_string(),
            flip: false,
        },
    );
}

/// L-26: keys that move the caret or clear a field without typing anything. They
/// end the pending word, but they are not a boundary the engine may rewrite: the
/// cursor is no longer behind the word, so backspaces would eat the wrong
/// characters. The buffers are simply dropped.
fn is_caret_movement(vk: u16) -> bool {
    matches!(
        vk,
        v if v == VK_LEFT.0 || v == VK_RIGHT.0 || v == VK_UP.0 || v == VK_DOWN.0
            || v == VK_HOME.0 || v == VK_END.0 || v == VK_PRIOR.0 || v == VK_NEXT.0
            || v == VK_DELETE.0 || v == VK_ESCAPE.0
            || (VK_F1.0..=VK_F12.0).contains(&v)
    )
}

fn is_identifier_char(c: char) -> bool {
    matches!(
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
}

/// A run that carries a digit or a code-like separator is a URL, a path, a model
/// number: the engine must not rewrite it.
fn looks_like_identifier(run: &str) -> bool {
    run.chars().any(|c| c.is_ascii_digit() || is_identifier_char(c))
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    EVENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    // WH_KEYBOARD_LL delivers both key down (code 1) and key up (code 0 is
    // sometimes reported for injected events), so we filter by down-state below.
    if code < 0 {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let kbd = *(lparam.0 as *const KBDLLHOOKSTRUCT);
    if verbose() {
        defer(Deferred::Note(format!(
            "raw code={code} vk={} scan={} extra={} flags={:?}",
            kbd.vkCode, kbd.scanCode, kbd.dwExtraInfo, kbd.flags
        )));
    }
    let vk = kbd.vkCode as u16;
    let up = kbd.flags.contains(LLKHF_UP);

    // Never react to events we injected ourselves (identified by the tag).
    if kbd.dwExtraInfo == self_tag() {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // A zя-tagged event that does not carry *our* pid means another copy of zя
    // is hooking the keyboard at the same time. Both copies delete and retype
    // the same word, which is the classic "double switch". The copy that noticed
    // it stops correcting automatically; its manual hotkeys keep working.
    if foreign_tag(kbd.dwExtraInfo) {
        if verbose() {
            defer(Deferred::Note(format!(
                "foreign-injected vk={vk} scan={} extra={}",
                kbd.scanCode, kbd.dwExtraInfo
            )));
        }
        stand_down_against_foreign();
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Track the real layout ourselves. GetKeyboardLayout() is stale for some
    // modern apps, so every genuine Alt+Shift (and every switch we perform)
    // updates App::lang. We also track Ctrl/Win because GetAsyncKeyState is not
    // reliable inside the hook.
    let (win_held, ctrl_held, alt_held, shift_held) = {
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
        // L-28: a hotkey whose press we swallowed must also swallow its release,
        // otherwise the focused application gets a key-up for a key that never
        // went down and treats the chord as stuck.
        if up && a.swallow_up == Some(vk) {
            a.swallow_up = None;
            return LRESULT(1);
        }
        let is_shift = vk == VK_LSHIFT.0 || vk == VK_RSHIFT.0;
        let is_alt = vk == VK_LMENU.0 || vk == VK_RMENU.0;
        let is_ctrl = vk == VK_LCONTROL.0 || vk == VK_RCONTROL.0;
        let is_win = vk == VK_LWIN.0 || vk == VK_RWIN.0;
        if is_shift || is_alt || is_ctrl || is_win {
            // L-18: the left and the right key of a pair are tracked separately.
            a.mods.set(vk, !up);
            if up {
                // The chord is only a language switch when it is released with
                // no other key pressed in between and neither half is held.
                if (is_shift || is_alt)
                    && a.alt_shift_pending
                    && !a.mods.alt()
                    && !a.mods.shift()
                    && !crate::settings_ui::is_assign_open()
                {
                    a.alt_shift_pending = false;
                    flip_tracked(&mut a);
                }
            } else {
                // Any second modifier cancels the "Alt alone" undo gesture.
                if a.mods.alt() && (is_shift || is_ctrl || is_win) {
                    a.alt_pending = false;
                }
                if is_shift {
                    if a.mods.alt() {
                        a.alt_shift_pending = true;
                    }
                } else if is_alt {
                    if a.mods.shift() {
                        a.alt_shift_pending = true;
                    }
                }
            }
        } else if a.alt_shift_pending && a.mods.alt() && a.mods.shift() {
            // Alt+Shift+<key> is an application shortcut, not a layout switch.
            a.alt_shift_pending = false;
        }
        (a.mods.win(), a.mods.ctrl(), a.mods.alt(), a.mods.shift())
    };

    // While the hotkey assignment modal is open, the settings UI owns the keys.
    // Do not run layout/word rewrite here, otherwise the modal and the main hook
    // react to the same press at once.
    if crate::settings_ui::is_assign_open() {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Win+Space cycles the input language through the OS. Re-seed the tracked
    // layout so the hook does not drift out of sync.
    if vk == VK_SPACE.0 && win_held {
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
        a.last_fg = 0;
        sync_layout(&mut a);
    }

    // Alt alone undoes the last conversion. The decision is taken on release so
    // that Alt+<key> shortcuts (Alt+3, Alt+F4) never trigger an undo.
    if (vk == VK_LMENU.0 || vk == VK_RMENU.0) && !ctrl_held {
        if !kbd.flags.contains(LLKHF_UP) {
            app().lock().unwrap_or_else(|error| error.into_inner()).alt_pending = true;
        } else {
            let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
            let was_alone = a.alt_pending;
            a.alt_pending = false;
            if was_alone && a.hotkey_undo.alt && a.hotkey_undo.vk == VK_LMENU.0 {
                undo_last(&mut a, true);
            }
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // L-40: in the Russian layout the numpad "." key types "," (RU decimal
    // separator). With the setting on (default), while Russian layout is
    // active and NumLock is on, zya sends a real "." instead. NumLock off
    // makes this key Delete: leave it to the OS unchanged.
    if vk == 0x6E {
        if up {
            if NUMPAD_DOT_SWALLOW.load(std::sync::atomic::Ordering::Relaxed) {
                NUMPAD_DOT_SWALLOW.store(false, std::sync::atomic::Ordering::Relaxed);
                return LRESULT(1);
            }
        } else if (GetKeyState(0x90) & 1) != 0 && !ctrl_held && !alt_held && !win_held {
            let go = {
                let a = app().lock().unwrap_or_else(|e| e.into_inner());
                a.lang == RU_LANGID && a.engine.cfg.numpad_comma_as_dot
            };
            if go {
                NUMPAD_DOT_SWALLOW.store(true, std::sync::atomic::Ordering::Relaxed);
                let mut items = [unicode('.', false), unicode('.', true)];
                let _ = SendInput(&mut items, std::mem::size_of::<INPUT>() as i32);
                return LRESULT(1);
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
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
        if fg != a.word_window {
            if verbose() && (!a.word.is_empty() || !a.run.is_empty()) {
                defer(Deferred::Note(format!(
                    "focus change {:#x} -> {:#x}: pending {:?} dropped",
                    a.word_window, fg, a.word
                )));
            }
            a.word.clear();
            a.run.clear();
            a.closed = None;
            a.word_window = fg;
        }
    }

    {
        let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
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
            // M-02: the switch is remembered in the config, not only in memory.
            a.auto = !a.auto;
            a.cfg.auto_enabled = a.auto;
            defer(Deferred::Persist(Box::new(a.cfg.clone())));
            defer(Deferred::Note(format!(
                "zя: автопереключение {}",
                if a.auto {
                    "включено"
                } else {
                    "выключено"
                }
            )));
            defer_tray(&a);
            a.swallow_up = Some(vk);
            // Swallow the key so the shortcut does not also reach the focused app.
            return LRESULT(1);
        }
        // Manual gestures. Each of the three has two bindings: one bound to a
        // selection, one bound to the last token. The same combination may be
        // bound to both at once (that is the default for Break): then the
        // selection wins and the token is the fallback. All of them work even
        // when automatic switching is off.
        let upper_sel = matches(a.hotkey_upper, vk, ctrl_held, alt_held, shift_held, win_held);
        let upper_word = matches(
            a.hotkey_upper_last,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        );
        let invert_sel = matches(a.hotkey_invert, vk, ctrl_held, alt_held, shift_held, win_held);
        let invert_word = matches(
            a.hotkey_invert_last,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        );
        let swap_sel = matches(
            a.hotkey_selection,
            vk,
            ctrl_held,
            alt_held,
            shift_held,
            win_held,
        );
        let swap_word = matches(a.hotkey_last, vk, ctrl_held, alt_held, shift_held, win_held);
        let add_word = matches(a.hotkey_add_word, vk, ctrl_held, alt_held, shift_held, win_held);

        if add_word {
            // Adding to the dictionary only makes sense for a highlighted word.
            spawn_selection(SelOp::AddWord, a.lang, Scope::Selection);
            a.swallow_up = Some(vk);
            return LRESULT(1);
        }

        if upper_sel || upper_word {
            spawn_selection(SelOp::Upper, a.lang, scope_of(upper_sel, upper_word));
            a.swallow_up = Some(vk);
            return LRESULT(1);
        }
        if invert_sel || invert_word {
            spawn_selection(SelOp::Invert, a.lang, scope_of(invert_sel, invert_word));
            a.swallow_up = Some(vk);
            return LRESULT(1);
        }
        if swap_sel || swap_word {
            spawn_selection(SelOp::SwapLayout, a.lang, scope_of(swap_sel, swap_word));
            a.swallow_up = Some(vk);
            return LRESULT(1);
        }
        // Undo hotkey that is not the bare-Alt gesture handled above.
        if matches(a.hotkey_undo, vk, ctrl_held, alt_held, shift_held, win_held) {
            undo_last(&mut a, ctrl_held || alt_held || shift_held || win_held);
            a.swallow_up = Some(vk);
            return LRESULT(1);
        }
    }

    // Ctrl+anything is a shortcut, not text.
    if ctrl_held || is_down(VK_LCONTROL.0) || is_down(VK_RCONTROL.0) {
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());

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
        // The text the Break undo would restore no longer exists.
        a.manual_swap = None;
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // L-26: arrows, Home/End, PageUp/PageDown, Delete, Escape and the F keys move
    // the caret. They close the pending word without any rewrite: injecting
    // backspaces after the cursor has moved eats the characters to the left of
    // the caret.
    if is_caret_movement(vk) {
        a.word.clear();
        a.run.clear();
        a.closed = None;
        a.identifier = false;
        a.last = None;
        a.last_boundary = None;
        return CallNextHookEx(None, code, wparam, lparam);
    }

    let state = state_array(&a);
    sync_layout(&mut a);
    defer_tray(&a);
    let lid = a.lang;
    if lid != RU_LANGID && lid != EN_LANGID {
        return CallNextHookEx(None, code, wparam, lparam);
    }
    let hkl = hkl_for(lid);

    let Some(mut c) = char_in_layout(vk, kbd.scanCode, &state, hkl) else {
        return CallNextHookEx(None, code, wparam, lparam);
    };
    if verbose() {
        defer(Deferred::Note(format!("key {c:?} langid {lid} word {:?}", &a.word)));
    }
    // A typed character puts the caret back at the end of the tracked token, so
    // the mouse click that made the caret position unknown no longer counts.
    CARET_SUSPECT.store(false, Ordering::Relaxed);
    // ToUnicodeEx inside a low-level hook does not always honour the shift
    // state, so normalise letter case from the modifiers we tracked ourselves.
    if a.mods.shift() && c.is_alphabetic() {
        c = c.to_uppercase().next().unwrap_or(c);
    }

    // L-31: while a selection gesture is still running its Ctrl+C / Ctrl+V
    // dance, the automatic layer keeps out of the way. Its backspace batch and
    // the gesture's paste otherwise interleave in the target application's
    // message queue, and the application pastes whatever the clipboard held at
    // that instant — the "engine pasted my clipboard" report.
    if GESTURE_BUSY.load(std::sync::atomic::Ordering::Relaxed) {
        if verbose() {
            defer(Deferred::Note(format!("key {c:?} passed through: gesture in flight")));
        }
        return CallNextHookEx(None, code, wparam, lparam);
    }

    // Requested word-end triggers: `? & / ^` behave like a space and flush the
    // pending buffer immediately. L-25: the very same `/` and `^` are also
    // identifier characters, so a run that already looks like a URL, a path or a
    // model number is protected *before* the boundary rule is applied.
    let identifier_like =
        matches!(c, '/' | '^' | '\\') && looks_like_identifier(&a.run) && !a.word.is_empty();
    let latin_boundary =
        crate::layout::is_latin_boundary_letter(c) && a.word.chars().any(crate::layout::is_latin);
    if (crate::layout::is_boundary_trigger(c) || latin_boundary) && !identifier_like {
        a.identifier = false;
        if !a.run.is_empty() {
            a.closed = Some((a.run.clone(), Some(c)));
        }
        a.run.clear();

        if !a.word.is_empty() {
            let word = std::mem::take(&mut a.word);
            let plan = plan_correction(&mut a, &word, c);
            // H-02: the App lock is released before SendInput, so a slow
            // injection never blocks the tray or the settings window.
            drop(a);
            if let Some(plan) = plan {
                if commit_correction(&plan, &word) {
                    return LRESULT(1);
                }
            }
            return CallNextHookEx(None, code, wparam, lparam);
        }
        a.last_boundary = None;
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
            let plan = plan_correction(&mut a, &word, c);
            drop(a);
            if let Some(plan) = plan {
                if commit_correction(&plan, &word) {
                    return LRESULT(1);
                }
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
    let plan = plan_correction(&mut a, &word, c);
    drop(a);
    if let Some(plan) = plan {
        if commit_correction(&plan, &word) {
            return LRESULT(1);
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

// Used by the self-test that proves the single-instance guard mutex is really
// created; the bin itself never needs it.
#[allow(dead_code)]
fn guard_mutex_exists(name: &str) -> bool {
    unsafe {
        let wide: Vec<u16> = format!("{name}\0").encode_utf16().collect();
        match CreateMutexW(None, true, PCWSTR(wide.as_ptr())) {
            Ok(_handle) => GetLastError() == ERROR_ALREADY_EXISTS,
            Err(_) => false,
        }
    }
}

/// The handle of the guard mutex *we* created. Once this process owns the mutex,
/// every later call must say "no other instance" — creating the same named mutex
/// again returns ERROR_ALREADY_EXISTS even inside the same process, which used to
/// make `zя.exe run` refuse to start on the second call (L-22).
static OWN_GUARD: AtomicUsize = AtomicUsize::new(0);

fn claim_instance_guard() -> bool {
    if OWN_GUARD.load(Ordering::Relaxed) != 0 {
        return false;
    }
    unsafe {
        let wide: Vec<u16> = "Local\\ZyaLayoutGuard\0".encode_utf16().collect();
        match CreateMutexW(None, true, PCWSTR(wide.as_ptr())) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    return true;
                }
                // The handle stays open for the lifetime of the process: that is
                // what makes the mutex visible to the next copy of zя.
                OWN_GUARD.store(handle.0 as usize, Ordering::Relaxed);
                false
            }
            Err(_) => false,
        }
    }
}

/// Hidden "ZyaTray" windows belonging to *other* processes. Diagnostic commands
/// (`trayprobe`, `menutest`, `wintest`) create such a window in this very process,
/// so the owning pid is checked (L-22).
unsafe fn foreign_tray_windows() -> Vec<HWND> {
    let class: Vec<u16> = "ZyaTray\0".encode_utf16().collect();
    let my_pid = std::process::id();
    let mut out = Vec::new();
    let mut after = None;
    loop {
        let hwnd = match FindWindowExW(None, after, PCWSTR(class.as_ptr()), PCWSTR::null()) {
            Ok(h) if !h.0.is_null() => h,
            _ => break,
        };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid != 0 && pid != my_pid {
            out.push(hwnd);
        }
        after = Some(hwnd);
    }
    out
}

/// True when another copy of zя is already resident. Two instances would both
/// hook the keyboard and rewrite the same word twice — the classic cause of
/// "double switches". The named mutex lives for the whole process.
pub fn another_instance_running() -> bool {
    if claim_instance_guard() {
        return true;
    }
    if lock_file_taken() {
        return true;
    }
    // Belt and braces: every resident copy owns a hidden tray window of class
    // "ZyaTray". We check this *before* our own tray window is created, so a
    // found window means another copy is resident even when the mutex and the
    // lock file are both unreadable (restricted tokens, sandboxed launches).
    !unsafe { foreign_tray_windows() }.is_empty()
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

fn stop_flag_path() -> std::path::PathBuf {
    crate::config::config_dir().join("stop.flag")
}

fn request_stop_other_instances() {
    if let Err(e) = std::fs::write(stop_flag_path(), "stop\n") {
        if verbose() {
            println!("zя: не удалось записать флаг остановки: {e}");
        }
    }
}

fn clear_stop_request() {
    let _ = std::fs::remove_file(stop_flag_path());
}

/// Consumes the stop flag: true means "another copy asked us to quit".
fn stop_requested() -> bool {
    let path = stop_flag_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
        return true;
    }
    false
}

/// Called once when a resident copy starts: a flag left by an earlier `stop`
/// must not kill the new instance.
pub fn clear_stop_flag() {
    clear_stop_request();
}

/// Ask every *other* resident copy of zя to quit through its hidden tray window
/// (class "ZyaTray", menu command 4 = "Выход"). Used by `zя.exe stop` and by the
/// installer, so an old build cannot keep hooking the keyboard next to the new
/// one. Returns how many copies were stopped.
///
/// PostMessage is refused by UIPI when the resident copy runs elevated (the
/// "запускать как администратор" mode) or when the caller is confined, so a
/// flag file is left as well: the focus guard in the resident process picks it
/// up and exits.
pub fn stop_other_instances() -> usize {
    unsafe {
        let targets = foreign_tray_windows();
        if targets.is_empty() {
            clear_stop_request();
            return 0;
        }
        request_stop_other_instances();
        let mut stopped = 0;
        for hwnd in targets {
            if verbose() {
                println!("zя: значок другого экземпляра: {hwnd:?}");
            }
            if let Err(e) = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(4), LPARAM(0)) {
                if verbose() {
                    println!("zя: не удалось послать команду выхода другому экземпляру: {e}");
                }
                continue;
            }
            // L-23: only a window that actually went away counts as a stopped
            // instance. Counting PostMessage calls reported "stopped 8" while
            // nothing was stopped, and the launcher then gave up because it still
            // saw another instance.
            for _ in 0..20 {
                if !IsWindow(Some(hwnd)).as_bool() {
                    stopped += 1;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        stopped
    }
}

pub fn install_hook() -> anyhow::Result<()> {
    unsafe {
        // L-21: re-installing is allowed. A stale handle is unhooked first, so
        // `install_hook` can repair a hook Windows removed on a timeout.
        remove_hook();
        ensure_worker();
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), None, 0)?;
        HOOK.store(hook.0 as usize, Ordering::Relaxed);
        // L-32: the mouse hook exists only to notice a click that moves the caret
        // out of the reach of the keyboard tracking. It never swallows anything.
        // It is optional: if the system refuses it, everything else keeps working
        // and the caret gate simply stays closed.
        match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), None, 0) {
            Ok(mouse) => MOUSE_HOOK.store(mouse.0 as usize, Ordering::Relaxed),
            Err(error) => {
                if verbose() {
                    defer(Deferred::Note(format!("mouse hook not installed: {error}")));
                }
            }
        }
        Ok(())
    }
}

pub fn remove_hook() {
    unsafe {
        let h = HOOK.swap(0, Ordering::Relaxed);
        if h != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut std::ffi::c_void));
        }
        let m = MOUSE_HOOK.swap(0, Ordering::Relaxed);
        if m != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(m as *mut std::ffi::c_void));
        }
    }
}

/// True while the low-level keyboard hook is installed. Diagnostics and tests
/// use it; the hook itself never needs it.
#[allow(dead_code)]
pub fn hook_installed() -> bool {
    HOOK.load(Ordering::Relaxed) != 0
}

pub fn start_focus_guard() {
    std::thread::spawn(|| {
        let mut last_fg = unsafe { GetForegroundWindow() }.0 as usize;
        let mut last_tick = std::time::Instant::now();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(150));
            // `zя stop` (or the installer) can be blocked by UIPI from sending a
            // message to an elevated resident copy; the flag file is the route
            // that always works.
            if stop_requested() {
                crate::tray::quit_now();
            }
            let fg = unsafe { GetForegroundWindow() }.0 as usize;

            // The tray letters must always show the layout Windows is actually
            // using, not the one we last wrote. Re-read it on every focus change
            // and once a second while nothing changes.
            if fg != last_fg || last_tick.elapsed() >= std::time::Duration::from_secs(1) {
                last_tick = std::time::Instant::now();
                let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
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
            // L-17: GetMessageW returns 0 for WM_QUIT and -1 for an error.
            // `BOOL::as_bool()` is `!= 0`, so an error used to be read as "there
            // is a message" and the stale MSG was dispatched in an endless loop.
            if r.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            let _ = DispatchMessageW(&msg);
        }
    }
}

fn auto_message(on: bool) -> String {
    format!(
        "zя: автопереключение {}",
        if on { "включено" } else { "выключено" }
    )
}

/// Change the automatic layer and remember the choice (M-02). The tray icon and
/// the journal are updated off the hook callback.
fn apply_auto(a: &mut App, on: bool) {
    a.auto = on;
    a.cfg.auto_enabled = on;
    defer(Deferred::Persist(Box::new(a.cfg.clone())));
    defer_tray(a);
}

pub fn auto_enabled() -> bool {
    app().lock().unwrap_or_else(|error| error.into_inner()).auto
}

pub fn toggle_auto() {
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let on = !a.auto;
    apply_auto(&mut a, on);
    println!("{}", auto_message(on));
}

/// Explicit version of `toggle_auto`, used by the settings window.
pub fn set_auto_enabled(on: bool) {
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    apply_auto(&mut a, on);
    println!("{}", auto_message(on));
}

pub fn switch_count() -> u32 {
    app().lock().unwrap_or_else(|error| error.into_inner()).switches
}

pub fn current_layout_langid() -> u32 {
    app().lock().unwrap_or_else(|error| error.into_inner()).lang
}

pub fn current_layout_is_russian() -> bool {
    current_layout_langid() == RU_LANGID
}

pub fn configured_toggle() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_toggle
}

pub fn configured_undo() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_undo
}

pub fn configured_last() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_last
}

pub fn configured_selection() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_selection
}

pub fn configured_upper() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_upper
}

pub fn configured_invert() -> Hotkey {
    app().lock().unwrap_or_else(|error| error.into_inner()).hotkey_invert
}

/// The Engine keeps its own copy of the config: every change to `cfg` has to be
/// pushed into it, otherwise the whitelist/exclusions of a running process stay
/// stale (L-10).
fn sync_engine(a: &mut App) {
    a.engine.cfg = a.cfg.clone();
}

pub fn set_auto(on: bool) {
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    apply_auto(&mut a, on);
}

/// Replace the process exclusions (used by the tests and by `zя exclude`).
pub fn set_exclude_processes(list: &[String]) {
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    a.cfg.exclude_processes = list.to_vec();
    sync_engine(&mut a);
}

/// Replace the folder exclusions: any foreground process whose executable lives
/// under one of these paths is left alone.
pub fn set_exclude_dirs(list: &[String]) {
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    a.cfg.exclude_dirs = list.to_vec();
    sync_engine(&mut a);
}

#[allow(dead_code)]
pub fn exclude_processes() -> Vec<String> {
    app().lock().unwrap_or_else(|error| error.into_inner()).cfg.exclude_processes.clone()
}

#[allow(dead_code)]
pub fn exclude_dirs() -> Vec<String> {
    app().lock().unwrap_or_else(|error| error.into_inner()).cfg.exclude_dirs.clone()
}

#[allow(dead_code)]
pub fn add_exclude_process(name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    let lower = name.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    if a.cfg
        .exclude_processes
        .iter()
        .any(|p| p.to_lowercase() == lower)
    {
        return false;
    }
    a.cfg.exclude_processes.push(name.to_string());
    sync_engine(&mut a);
    if let Err(error) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить настройки: {error}");
    }
    true
}

#[allow(dead_code)]
pub fn remove_exclude_process(name: &str) -> bool {
    let lower = name.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let before = a.cfg.exclude_processes.len();
    a.cfg
        .exclude_processes
        .retain(|p| p.to_lowercase() != lower);
    if a.cfg.exclude_processes.len() == before {
        return false;
    }
    sync_engine(&mut a);
    if let Err(error) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить настройки: {error}");
    }
    true
}

#[allow(dead_code)]
pub fn add_exclude_dir(dir: &str) -> bool {
    let dir = dir.trim();
    if dir.is_empty() {
        return false;
    }
    let lower = dir.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    if a.cfg.exclude_dirs.iter().any(|d| d.to_lowercase() == lower) {
        return false;
    }
    a.cfg.exclude_dirs.push(dir.to_string());
    sync_engine(&mut a);
    if let Err(error) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить настройки: {error}");
    }
    true
}

#[allow(dead_code)]
pub fn remove_exclude_dir(dir: &str) -> bool {
    let lower = dir.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let before = a.cfg.exclude_dirs.len();
    a.cfg.exclude_dirs.retain(|d| d.to_lowercase() != lower);
    if a.cfg.exclude_dirs.len() == before {
        return false;
    }
    sync_engine(&mut a);
    if let Err(error) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить настройки: {error}");
    }
    true
}

/// Process/file name of the application owning `hwnd`, e.g. "chrome.exe".
#[allow(dead_code)]
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

/// Add a short service word to the dictionary the engine trusts. The script of
/// the word decides the list: Cyrillic goes to the Russian one, Latin to the
/// English one. Returns true when the word was not there yet.
pub fn add_short_word(word: &str) -> bool {
    let clean: String = word.trim().chars().filter(|c| c.is_alphabetic()).collect();
    if clean.is_empty() || clean.chars().count() > MAX_LEXICON_WORD {
        return false;
    }
    let lower = clean.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let list = if crate::layout::is_cyrillic(lower.chars().next().unwrap()) {
        &mut a.cfg.short_ru
    } else {
        &mut a.cfg.short_en
    };
    if list.iter().any(|w| w.eq_ignore_ascii_case(&lower)) {
        return false;
    }
    list.push(lower);
    sync_engine(&mut a);
    if let Err(e) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить словарчик: {e}");
    }
    true
}

/// Remove a word the user added. Built-in words cannot be removed: they are
/// compiled into the engine.
pub fn remove_short_word(word: &str) -> bool {
    let lower = word.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let before = a.cfg.short_ru.len() + a.cfg.short_en.len();
    a.cfg.short_ru.retain(|w| !w.eq_ignore_ascii_case(&lower));
    a.cfg.short_en.retain(|w| !w.eq_ignore_ascii_case(&lower));
    if a.cfg.short_ru.len() + a.cfg.short_en.len() == before {
        return false;
    }
    sync_engine(&mut a);
    crate::config::save(&a.cfg).ok();
    true
}

pub fn short_word_lists() -> (Vec<String>, Vec<String>) {
    let a = app().lock().unwrap_or_else(|error| error.into_inner());
    (a.cfg.short_ru.clone(), a.cfg.short_en.clone())
}

/// Add a word to the user dictionary (whitelist), so the hook no longer changes it.
pub fn add_dictionary_word(word: &str) -> bool {
    let letters: String = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    let lower = letters.to_lowercase();
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    if a.cfg
        .whitelist
        .iter()
        .any(|w| w.eq_ignore_ascii_case(&lower))
    {
        return false;
    }
    a.cfg.whitelist.push(letters);
    sync_engine(&mut a);
    if let Err(e) = crate::config::save(&a.cfg) {
        println!("zя: не удалось сохранить словарь: {e}");
    }
    true
}

pub fn dictionary_words() -> Vec<String> {
    app().lock().unwrap_or_else(|error| error.into_inner()).cfg.whitelist.clone()
}

/// Remove a word from the user dictionary. Returns true when it was there.
pub fn remove_dictionary_word(word: &str) -> bool {
    let lower = word.trim().to_lowercase();
    if lower.is_empty() {
        return false;
    }
    let mut a = app().lock().unwrap_or_else(|error| error.into_inner());
    let before = a.cfg.whitelist.len();
    a.cfg.whitelist.retain(|w| !w.eq_ignore_ascii_case(&lower));
    if a.cfg.whitelist.len() == before {
        return false;
    }
    sync_engine(&mut a);
    crate::config::save(&a.cfg).ok();
    true
}

/// True when automatic correction is currently suppressed for the foreground
/// window (secret field, excluded process or excluded folder).
pub fn context_blocked_now() -> bool {
    let a = app().lock().unwrap_or_else(|error| error.into_inner());
    unsafe { context_blocked(&a) }
}

pub fn hide_console() {
    use windows::Win32::System::Console::{GetConsoleProcessList, GetConsoleWindow};
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        // L-27: hiding the console window hides *the terminal*, which is shared
        // by every process attached to it. Only hide it when we are the sole
        // process on that console — i.e. when the app was started by Explorer or
        // by the autostart key, not from PowerShell or cmd.
        let mut processes = [0u32; 8];
        let attached = GetConsoleProcessList(&mut processes) as usize;
        if attached > 1 {
            return;
        }
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
    fn the_backtick_is_a_real_key_for_the_dictionary_hotkey() {
        let h = parse_hotkey("ctrl+`").expect("Ctrl+` must parse");
        assert!(h.ctrl && !h.alt && !h.shift && !h.win);
        assert_eq!(h.vk, VK_OEM_3.0, "the key that carries ё on a Russian layout");
        assert!(matches(h, VK_OEM_3.0, true, false, false, false));
        assert!(
            !matches(h, VK_OEM_3.0, false, false, false, false),
            "the bare backtick is a character, not the gesture"
        );
    }

    #[test]
    fn a_selection_hands_its_first_word_to_the_dictionary() {
        assert_eq!(selection_word("терминология 3D-графики"), "терминология");
        assert_eq!(selection_word("  —fps—  "), "fps");
        assert_eq!(selection_word("Колю"), "Колю");
        assert_eq!(selection_word("12345"), "", "digits are not a word");
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

    #[test]
    fn foreign_tag_covers_only_our_tag_window() {
        // L-03: an unrelated injector (AutoHotkey, PowerToys, remote access)
        // must not be mistaken for a second copy of zя.
        assert!(!foreign_tag(0xC0FFEE));
        assert!(!foreign_tag(0xFFFF_FFFF));
        assert!(!foreign_tag(0x1234_5678));
        assert!(!foreign_tag(SELF_BASE - 1));
        assert!(!foreign_tag(SELF_BASE + 0x1_0000));
        assert!(foreign_tag(SELF_BASE + 7));
        assert!(!foreign_tag(self_tag()));
    }

    #[test]
    fn each_modifier_half_is_tracked_on_its_own() {
        let mut m = HeldMods::default();
        m.set(VK_LSHIFT.0, true);
        m.set(VK_RSHIFT.0, true);
        m.set(VK_RSHIFT.0, false);
        assert!(m.shift(), "the left Shift is still held");
        assert!(m.exact(VK_LSHIFT.0));
        assert!(!m.exact(VK_RSHIFT.0), "the right Shift was released");
        assert!(!m.alt() && !m.ctrl());
        assert!(!m.set(VK_P.0, true), "P is not a modifier");
    }

    #[test]
    fn caret_movement_keys_are_silent_boundaries() {
        for vk in [
            VK_LEFT.0,
            VK_RIGHT.0,
            VK_UP.0,
            VK_DOWN.0,
            VK_HOME.0,
            VK_END.0,
            VK_PRIOR.0,
            VK_NEXT.0,
            VK_DELETE.0,
            VK_F5.0,
        ] {
            assert!(is_caret_movement(vk), "{vk} must drop the buffer");
        }
        assert!(!is_caret_movement(VK_SPACE.0));
        assert!(!is_caret_movement(VK_TAB.0));
        assert!(!is_caret_movement(VK_C.0));
    }

    #[test]
    fn enter_and_tab_are_sent_as_physical_keys() {
        assert_eq!(trailing_vk('\r'), Some(VK_RETURN));
        assert_eq!(trailing_vk('\t'), Some(VK_TAB));
        assert_eq!(trailing_vk(','), None);
        assert_eq!(trailing_vk(' '), None);
    }

    #[test]
    fn es_password_is_not_ws_child() {
        assert_eq!(ES_PASSWORD, 0x0020);
        assert_eq!(ES_PASSWORD & 0x4000_0000, 0);
    }

    #[test]
    fn identifier_heuristics() {
        assert!(looks_like_identifier("http://site"));
        assert!(looks_like_identifier("c4"));
        assert!(!looks_like_identifier("ghbdtn"));
    }

    #[test]
    fn the_clipboard_round_trip_restores_what_the_user_had() {
        unsafe {
            let original = clipboard_snapshot();
            // A gesture writes its sentinel, then the transformed text, then
            // restores. Every step has to be observable, or the paste silently
            // sends the user's old contents back.
            assert!(set_clipboard_text(CLIP_SENTINEL));
            assert_eq!(clipboard_text().as_deref(), Some(CLIP_SENTINEL));
            assert!(set_clipboard_text("исправленное слово"));
            assert_eq!(clipboard_text().as_deref(), Some("исправленное слово"));
            restore_clipboard(&original);
            assert_eq!(clipboard_snapshot(), original);
        }
    }

    #[test]
    fn a_failed_clipboard_write_is_reported() {
        unsafe {
            // The clipboard can be locked by another process; the helper must not
            // claim success in that case, because the caller skips the paste.
            let ok = set_clipboard_text("проверка");
            if ok {
                assert_eq!(clipboard_text().as_deref(), Some("проверка"));
            }
        }
    }

    #[test]
    fn only_one_gesture_owns_the_clipboard_at_a_time() {
        let held = ClipboardGesture::try_acquire();
        assert!(held.is_some(), "the first gesture must get the clipboard");
        assert!(
            ClipboardGesture::try_acquire().is_none(),
            "a second gesture must not start while the first is running"
        );
        drop(held);
        assert!(
            ClipboardGesture::try_acquire().is_some(),
            "the latch must release when the guard is dropped"
        );
    }
}
