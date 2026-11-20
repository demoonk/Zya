//! Update check for ZЯ.
//!
//! The app talks to the GitHub Releases API of `demoonk/Zya` with WinHTTP — no
//! third-party HTTP crate is linked into the 6 MB binary. When a newer release
//! exists, the settings window shows an "Обновить" button: it downloads the
//! release asset `z.exe` (the installer) into the config folder and starts it.
//! `z.exe` is the installer and is marked `requireAdministrator` in its
//! manifest, so Windows asks for UAC approval.

use std::ffi::c_void;
use std::path::PathBuf;

use serde::Deserialize;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_OPEN_REQUEST_FLAGS,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

const REPO: &str = "demoonk/Zya";
const MAX_BODY: usize = 40 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
    size: u64,
}

pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

unsafe fn request(host: &str, path: &str) -> Option<Vec<u8>> {
    let agent = wide("ZyaUpdater/1.0");
    let session = WinHttpOpen(
        PCWSTR(agent.as_ptr()),
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
        PCWSTR::null(),
        PCWSTR::null(),
        0,
    );
    if session.is_null() {
        return None;
    }
    let _ = WinHttpSetTimeouts(session, 5000, 8000, 8000, 20000);

    let host_wide = wide(host);
    let connect = WinHttpConnect(session, PCWSTR(host_wide.as_ptr()), 443, 0);
    if connect.is_null() {
        WinHttpCloseHandle(session).ok();
        return None;
    }

    let verb = wide("GET");
    let path_wide = wide(path);
    let request = WinHttpOpenRequest(
        connect,
        PCWSTR(verb.as_ptr()),
        PCWSTR(path_wide.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        std::ptr::null(),
        WINHTTP_OPEN_REQUEST_FLAGS(0),
    );
    if request.is_null() {
        WinHttpCloseHandle(connect).ok();
        WinHttpCloseHandle(session).ok();
        return None;
    }

    let sent = WinHttpSendRequest(request, None, None, 0, 0, 0);
    if sent.is_err() || WinHttpReceiveResponse(request, std::ptr::null_mut()).is_err() {
        WinHttpCloseHandle(request).ok();
        WinHttpCloseHandle(connect).ok();
        WinHttpCloseHandle(session).ok();
        return None;
    }

    let mut body = Vec::new();
    let mut buffer = [0u8; 16384];
    loop {
        let mut read = 0u32;
        if WinHttpReadData(request, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read)
            .is_err()
            || read == 0
        {
            break;
        }
        body.extend_from_slice(&buffer[..read as usize]);
        if body.len() > MAX_BODY {
            break;
        }
    }

    WinHttpCloseHandle(request).ok();
    WinHttpCloseHandle(connect).ok();
    WinHttpCloseHandle(session).ok();
    Some(body)
}

/// The newest release published on GitHub. `None` means "no connection".
pub fn check_latest() -> Option<Release> {
    let body = unsafe { request("api.github.com", &format!("/repos/{REPO}/releases/latest")) }?;
    let parsed: ReleaseJson = serde_json::from_slice(&body).ok()?;
    let asset = parsed.assets.iter().find(|a| a.name.eq_ignore_ascii_case("z.exe"))?;
    Some(Release {
        version: parsed.tag_name.trim_start_matches('v').trim().to_string(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
    })
}

/// Numeric compare of dotted versions: 0.10.1 > 0.9.9.
pub fn is_newer(local: &str, remote: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.trim().parse::<u64>().unwrap_or(0))
            .collect()
    };
    let a = parts(local);
    let b = parts(remote);
    for i in 0..a.len().max(b.len()) {
        let left = a.get(i).copied().unwrap_or(0);
        let right = b.get(i).copied().unwrap_or(0);
        if right > left {
            return true;
        }
        if right < left {
            return false;
        }
    }
    false
}

/// Download the installer `z.exe` into the config folder and return its path.
pub fn download_installer(release: &Release) -> Option<PathBuf> {
    let rest = release
        .url
        .strip_prefix("https://")
        .unwrap_or(&release.url);
    let (host, path) = match rest.split_once('/') {
        Some((host, path)) => (host, format!("/{path}")),
        None => (rest, "/".to_string()),
    };
    let bytes = unsafe { request(host, &path) }?;
    if bytes.len() < 1024 {
        return None;
    }
    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir).ok();
    let file = dir.join("z_update.exe");
    std::fs::write(&file, bytes).ok()?;
    Some(file)
}

/// Start the downloaded installer. It requires administrator rights, so the
/// UAC prompt is expected here.
pub fn run_installer(path: &PathBuf) -> bool {
    unsafe {
        let file = wide(&path.display().to_string());
        let action = wide("open");
        let result = ShellExecuteW(
            None,
            PCWSTR(action.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOW,
        );
        // ShellExecuteW returns an HINSTANCE; values above 32 mean success.
        (result.0 as isize) > 32
    }
}
