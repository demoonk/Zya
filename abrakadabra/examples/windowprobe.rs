//! Diagnostics for the two new "add exception" buttons:
//!   1. enumerate the visible top-level windows the "Добавить активное
//!      приложение" menu would offer (GetWindow(NULL, ...) returns nothing, so
//!      the app uses EnumWindows — this probe checks the same thing);
//!   2. open the "Указать на диске" dialog and close it automatically, so the
//!      OPENFILENAMEW structure is checked without a human clicking.
//!
//!   cargo run --release --example windowprobe
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, WPARAM};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_EXPLORER, OFN_NOVALIDATE, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowThreadProcessId, GetWindowTextW, IsWindowVisible,
    PostMessageW, WM_CLOSE,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn process_name(window: HWND) -> String {
    let mut pid = 0u32;
    GetWindowThreadProcessId(window, Some(&mut pid));
    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return String::new();
    };
    let mut buffer = vec![0u16; 1024];
    let mut size = 1024u32;
    let result = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(buffer.as_mut_ptr()),
        &mut size,
    );
    CloseHandle(handle).ok();
    if result.is_err() {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..size as usize])
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_lowercase()
}

unsafe extern "system" fn each_window(hwnd: HWND, _param: LPARAM) -> windows::core::BOOL {
    let mut buffer = vec![0u16; 200];
    let length = GetWindowTextW(hwnd, &mut buffer).max(0) as usize;
    let title = String::from_utf16_lossy(&buffer[..length]).trim().to_string();
    if IsWindowVisible(hwnd).as_bool() && !title.is_empty() {
        println!("окно: {title}  ({})", process_name(hwnd));
    }
    windows::core::BOOL(1)
}

fn main() {
    unsafe {
        EnumWindows(Some(each_window), LPARAM(0)).ok();

        // Close the dialog on its own so the probe does not hang on the screen.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let class = wide("#32770");
            if let Ok(dialog) = FindWindowW(PCWSTR(class.as_ptr()), None) {
                if !dialog.0.is_null() {
                    let _ = PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0));
                }
            }
        });

        let mut name = [0u16; 512];
        let filter: Vec<u16> = "Процессы (*.exe)\0*.exe\0Папки и файлы\0*.*\0\0"
            .encode_utf16()
            .collect();
        let title = wide("Указать на диске: папка или процесс");
        let initial = wide("C:\\Users\\demOOn");
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: HWND(std::ptr::null_mut()),
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: PWSTR(name.as_mut_ptr()),
            nMaxFile: name.len() as u32,
            lpstrTitle: PCWSTR(title.as_ptr()),
            lpstrInitialDir: PCWSTR(initial.as_ptr()),
            Flags: OFN_EXPLORER | OFN_NOVALIDATE,
            ..Default::default()
        };
        let chosen = GetOpenFileNameW(&mut ofn).as_bool();
        println!("диалог открыт и закрыт автоматически, выбор: {chosen}");
    }
}
