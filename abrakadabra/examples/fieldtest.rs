//! Diagnostics: put a sample string into every input field of an already open
//! settings window and dump the window pixels to a PPM image, so the text and
//! caret placement inside the fields can be inspected without looking at the screen.
//!
//!   cargo run --release --example fieldtest
use std::io::Write;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, DIB_RGB_COLORS,
    GetDIBits, GetDC, ReleaseDC, HGDIOBJ, BITMAPINFO, BITMAPINFOHEADER, SelectObject,
};
use windows::Win32::Storage::Xps::{PrintWindow, PRINT_WINDOW_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, FindWindowW, GetClientRect, GetWindowRect, PostMessageW,
    SetWindowTextW, WM_COMMAND,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[link(name = "user32")]
extern "system" {
    fn SetProcessDPIAware() -> i32;
}

unsafe fn make_dpi_aware() {
    SetProcessDPIAware();
}

unsafe fn edit_children(window: HWND) -> Vec<HWND> {
    let class = wide("Edit");
    let mut found = Vec::new();
    let mut child = FindWindowExW(Some(window), None, PCWSTR(class.as_ptr()), None)
        .unwrap_or(HWND(std::ptr::null_mut()));
    while !child.0.is_null() {
        found.push(child);
        child = FindWindowExW(Some(window), Some(child), PCWSTR(class.as_ptr()), None)
            .unwrap_or(HWND(std::ptr::null_mut()));
    }
    found
}

fn main() {
    unsafe {
        make_dpi_aware();
        let class = wide("ZyaSettingsWindow");
        let window = FindWindowW(PCWSTR(class.as_ptr()), None)
            .unwrap_or(HWND(std::ptr::null_mut()));
        if window.0.is_null() {
            println!("окно настроек не открыто: запустите zя.exe settings");
            return;
        }
        let tab = std::env::args()
            .nth(1)
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(101);
        PostMessageW(Some(window), WM_COMMAND, WPARAM(tab), LPARAM(0)).ok();
        std::thread::sleep(std::time::Duration::from_millis(400));
        let sample = wide("mm mm mm");
        let fields = edit_children(window);
        let mut outer = RECT::default();
        GetWindowRect(window, &mut outer).ok();
        for field in &fields {
            let mut inner = RECT::default();
            GetWindowRect(*field, &mut inner).ok();
            println!(
                "поле {}: left {} top {} {}x{} | окно left {} top {} {}x{}",
                field.0 as usize,
                inner.left,
                inner.top,
                inner.right - inner.left,
                inner.bottom - inner.top,
                outer.left,
                outer.top,
                outer.right - outer.left,
                outer.bottom - outer.top
            );
        }
        for field in &fields {
            SetWindowTextW(*field, PCWSTR(sample.as_ptr())).ok();
        }
        if let Some(&first) = fields.first() {
            SetFocus(Some(first)).ok();
        }
        println!("полей ввода: {}", fields.len());

        let mut rect = RECT::default();
        GetClientRect(window, &mut rect).ok();
        let width = rect.right.max(1);
        let height = rect.bottom.max(1);
        let screen = GetDC(Some(window));
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let memory = CreateCompatibleDC(Some(screen));
        SelectObject(memory, HGDIOBJ(bitmap.0));
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
                biCompression: 0,
                ..Default::default()
            },
            ..Default::default()
        };
        GetDIBits(
            memory,
            bitmap,
            0,
            height as u32,
            Some(pixels.as_mut_ptr() as *mut std::ffi::c_void),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(memory);
        ReleaseDC(Some(window), screen);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));

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
        std::fs::File::create("data/fieldtest.ppm")
            .expect("ppm")
            .write_all(&out)
            .expect("write");
        println!("снимок: data/fieldtest.ppm {width}x{height}");
    }
}
