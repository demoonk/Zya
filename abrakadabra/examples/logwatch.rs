//! Does the log page follow the diary while the window is open?
//! Opens the Log tab of an already open settings window, waits longer than the
//! two-second refresh timer, and captures the page. Append a line to
//! diario.log between the two captures to see the list grow by itself.
//!
//!   cargo run --release --example logwatch
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC,
    DeleteObject, DIB_RGB_COLORS, GetDIBits, GetDC, ReleaseDC, HGDIOBJ,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClientRect, PostMessageW, WM_COMMAND,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn capture(window: HWND, path: &str) -> std::io::Result<()> {
    let mut rect = windows::Win32::Foundation::RECT::default();
    GetClientRect(window, &mut rect).ok();
    let width = rect.right.max(1);
    let height = rect.bottom.max(1);
    let screen = GetDC(Some(window));
    let bitmap = CreateCompatibleBitmap(screen, width, height);
    let memory = CreateCompatibleDC(Some(screen));
    let _ = PrintWindow(window, memory, PRINT_WINDOW_FLAGS(2));
    let stride = ((width * 4 + 3) / 4) * 4;
    let mut pixels = vec![0u8; stride as usize * height as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    let _ = GetDIBits(
        memory,
        bitmap,
        0,
        height as u32,
        Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
        &mut info,
        DIB_RGB_COLORS,
    );
    let _ = DeleteDC(memory);
    let _ = DeleteObject(HGDIOBJ(bitmap.0));
    ReleaseDC(Some(window), screen);

    // PPM: easy to open with any viewer and to diff pixel by pixel.
    let mut out = Vec::with_capacity(pixels.len() + 32);
    out.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for row in 0..height as usize {
        for column in 0..width as usize {
            let offset = row * stride as usize + column * 4;
            out.push(pixels[offset + 2]);
            out.push(pixels[offset + 1]);
            out.push(pixels[offset]);
        }
    }
    std::fs::write(path, out)
}

fn main() {
    unsafe {
        let class = wide("ZyaSettingsWindow");
        let window = FindWindowW(PCWSTR(class.as_ptr()), None)
            .unwrap_or(HWND(std::ptr::null_mut()));
        if window.0.is_null() {
            println!("окно настроек не открыто: запустите zя.exe settings");
            return;
        }
        // TAB_LOG = 102: the same WM_COMMAND the "Лог" button sends.
        PostMessageW(Some(window), WM_COMMAND, WPARAM(102), LPARAM(0)).ok();
        std::thread::sleep(std::time::Duration::from_millis(500));
        capture(window, "data/logwatch_before.ppm").ok();
        println!("снимок до: data/logwatch_before.ppm");
        // Longer than the two-second refresh timer.
        std::thread::sleep(std::time::Duration::from_millis(4000));
        capture(window, "data/logwatch_after.ppm").ok();
        println!("снимок после: data/logwatch_after.ppm");
    }
}
