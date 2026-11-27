//! Measure how Segoe UI advances the letter "m" at the sizes this app uses:
//! GDI's legacy ANTIALIASED_QUALITY versus natural ClearType. The ink profile
//! prints one column per device pixel, so a stray gap between letters is visible.
//!
//!   cargo run --release --example glyphgap
use windows::core::PCWSTR;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW,
    DeleteDC, DeleteObject, DIB_RGB_COLORS, DrawTextW, GetDC, GetDIBits, FF_MODERN,
    GetTextExtentPoint32W, HGDIOBJ, ReleaseDC, SelectObject, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET,
    ANTIALIASED_QUALITY, CLEARTYPE_QUALITY, CLEARTYPE_NATURAL_QUALITY, FONT_QUALITY,
    OUT_DEFAULT_PRECIS, DT_LEFT,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn ink_profile(pixels: &[u8], width: i32, height: i32, stride: usize) -> String {
    let mut out = String::new();
    for column in 0..width as usize {
        let mut ink = false;
        for row in 0..height as usize {
            let offset = row * stride + column * 4;
            if pixels[offset + 2] > 100 {
                ink = true;
                break;
            }
        }
        out.push(if ink { '#' } else { '.' });
    }
    out
}

fn main() {
    unsafe {
        let sample = "applicationframehost.exe";
        let mut encoded = wide(sample);
        let screen = GetDC(None);
        let width = 320;
        let height = 40;
        for (label, quality) in [
            ("ANTIALIASED    ", ANTIALIASED_QUALITY),
            ("CLEARTYPE      ", CLEARTYPE_QUALITY),
            ("CLEARTYPE NAT  ", FONT_QUALITY(CLEARTYPE_NATURAL_QUALITY as u8)),
        ] {
            for points in [10i32, 11] {
                let dpi = 146;
                let face = wide("Segoe UI");
                let font = CreateFontW(
                    -(points * dpi / 72),
                    0,
                    0,
                    0,
                    400,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET,
                    OUT_DEFAULT_PRECIS,
                    CLIP_DEFAULT_PRECIS,
                    quality,
                    FF_MODERN.0 as u32,
                    PCWSTR(face.as_ptr()),
                );
                let bitmap = CreateCompatibleBitmap(screen, width, height);
                let memory = CreateCompatibleDC(Some(screen));
                SelectObject(memory, HGDIOBJ(font.0));
                let mut rect = windows::Win32::Foundation::RECT {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                };
                DrawTextW(memory, encoded.as_mut_slice(), &mut rect, DT_LEFT);
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
                let mut extent = SIZE::default();
                let _ = GetTextExtentPoint32W(memory, &encoded, &mut extent);
                let advance = extent.cx;
                println!(
                    "{label} {points}pt: advance {advance} px\n  {}",
                    ink_profile(&pixels, width, height, stride as usize)
                );
                let _ = DeleteDC(memory);
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteObject(HGDIOBJ(font.0));
            }
        }
        ReleaseDC(None, screen);
    }
}
