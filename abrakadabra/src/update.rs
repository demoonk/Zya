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
use std::time::{Duration, Instant};

use serde::Deserialize;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_FLAG_NUMBER64,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOW;

const REPO: &str = "demoonk/Zya";
const MAX_BODY: usize = 40 * 1024 * 1024;
// H-01: WinHTTP's per-phase timeouts are per attempt, and the read loop had no
// deadline at all - a slow server could hold the settings window (and the tray,
// which shares the thread) for minutes.
const READ_DEADLINE: Duration = Duration::from_secs(30);

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
    // PCWSTR needs a terminating NUL; without it WinHTTP reads past the buffer.
    let mut units = text.encode_utf16().collect::<Vec<u16>>();
    units.push(0);
    units
}

/// HTTP status code of a received response. `None` means the header was absent.
unsafe fn query_status(request: *mut c_void) -> Option<u32> {
    let mut status = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
        PCWSTR::null(),
        Some(&mut status as *mut u32 as *mut c_void),
        &mut size,
        &mut index,
    )
    .ok()
    .map(|()| status)
}

/// Content-Length as reported by the server, when it reports one.
unsafe fn query_content_length(request: *mut c_void) -> Option<u64> {
    let mut length = 0u64;
    let mut size = std::mem::size_of::<u64>() as u32;
    let mut index = 0u32;
    WinHttpQueryHeaders(
        request,
        WINHTTP_QUERY_CONTENT_LENGTH | WINHTTP_QUERY_FLAG_NUMBER64,
        PCWSTR::null(),
        Some(&mut length as *mut u64 as *mut c_void),
        &mut size,
        &mut index,
    )
    .ok()
    .map(|()| length)
}

unsafe fn close_all(request: *mut c_void, connect: *mut c_void, session: *mut c_void) {
    WinHttpCloseHandle(request).ok();
    WinHttpCloseHandle(connect).ok();
    WinHttpCloseHandle(session).ok();
}

unsafe fn request_with_proxy(
    host: &str,
    path: &str,
    proxy: windows::Win32::Networking::WinHttp::WINHTTP_ACCESS_TYPE,
) -> Option<(u32, Vec<u8>)> {
    let debug = std::env::var("ZYA_DEBUG_UPDATE").unwrap_or_default() == "1";
    let deadline = Instant::now() + READ_DEADLINE;
    let agent = wide("ZyaUpdater/1.0");
    let session = WinHttpOpen(
        PCWSTR(agent.as_ptr()),
        proxy,
        PCWSTR::null(),
        PCWSTR::null(),
        0,
    );
    if session.is_null() {
        if debug {
            eprintln!("update: WinHttpOpen returned null");
        }
        return None;
    }
    let _ = WinHttpSetTimeouts(session, 5000, 8000, 8000, 20000);

    let host_wide = wide(host);
    let connect = WinHttpConnect(session, PCWSTR(host_wide.as_ptr()), 443, 0);
    if connect.is_null() {
        if debug {
            eprintln!("update: WinHttpConnect to {host} failed");
        }
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
        WINHTTP_FLAG_SECURE,
    );
    if request.is_null() {
        WinHttpCloseHandle(connect).ok();
        WinHttpCloseHandle(session).ok();
        return None;
    }

    let sent = WinHttpSendRequest(request, None, None, 0, 0, 0);
    let received = WinHttpReceiveResponse(request, std::ptr::null_mut());
    if sent.is_err() || received.is_err() {
        if debug {
            eprintln!("update: send/receive failed: {:?} / {:?}", sent, received);
        }
        close_all(request, connect, session);
        return None;
    }

    // H-01: a 403 rate-limit page or a proxy error page is bigger than 1 KB and
    // used to be saved as `z_update.exe` and handed to ShellExecuteW. Only a
    // 200 response is accepted, and the body must match Content-Length.
    let status = query_status(request).unwrap_or(0);
    if status != 200 {
        if debug {
            eprintln!("update: {host}{path} ответил статусом {status}");
        }
        close_all(request, connect, session);
        return None;
    }
    let expected = query_content_length(request);

    let mut body = Vec::new();
    let mut buffer = [0u8; 16384];
    loop {
        if Instant::now() >= deadline {
            eprintln!(
                "update: чтение тела превысило {} с (получено {} байт) — загрузка отменена",
                READ_DEADLINE.as_secs(),
                body.len()
            );
            close_all(request, connect, session);
            return None;
        }
        let mut read = 0u32;
        if WinHttpReadData(request, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read)
            .is_err()
            || read == 0
        {
            break;
        }
        body.extend_from_slice(&buffer[..read as usize]);
        if body.len() > MAX_BODY {
            if debug {
                eprintln!("update: тело больше {MAX_BODY} байт — ответ отброшен");
            }
            close_all(request, connect, session);
            return None;
        }
        if let Some(length) = expected {
            if (body.len() as u64) > length {
                if debug {
                    eprintln!("update: тело длиннее Content-Length — ответ отброшен");
                }
                close_all(request, connect, session);
                return None;
            }
        }
    }

    // A stream that stopped early is a truncated installer, not a finished one.
    if let Some(length) = expected {
        if (body.len() as u64) != length {
            eprintln!(
                "update: получено {} байт вместо обещанных {length} — ответ отброшен",
                body.len()
            );
            close_all(request, connect, session);
            return None;
        }
    }

    close_all(request, connect, session);
    Some((status, body))
}

/// Try the system proxy first, then a direct connection: some machines have a
/// broken proxy entry in the registry and only the direct route works.
unsafe fn request(host: &str, path: &str) -> Option<(u32, Vec<u8>)> {
    match request_with_proxy(host, path, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY) {
        Some(result) => Some(result),
        None => request_with_proxy(host, path, WINHTTP_ACCESS_TYPE_NO_PROXY),
    }
}

/// The newest release published on GitHub. `None` means "no connection".
pub fn check_latest() -> Option<Release> {
    let debug = std::env::var("ZYA_DEBUG_UPDATE").unwrap_or_default() == "1";
    let (_status, body) = unsafe { request("api.github.com", &format!("/repos/{REPO}/releases/latest")) }?;
    if debug {
        let head = String::from_utf8_lossy(&body[..body.len().min(160)]);
        eprintln!("update: body {} bytes: {head}", body.len());
    }
    let parsed: ReleaseJson = serde_json::from_slice(&body).ok()?;
    let asset = parsed
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case("z.exe"))
        .or_else(|| parsed.assets.iter().find(|a| a.name.eq_ignore_ascii_case("z_single_launcher.exe")))
        .or_else(||
            parsed.assets.iter().find(|a| a.name.to_lowercase().starts_with("z")
                && a.name.to_lowercase().ends_with(".exe")
                && a.size > 1024 * 1024),
        )
        ?;
    Some(Release {
        version: parsed.tag_name.trim_start_matches('v').trim().to_string(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
    })
}

/// Split "0.1.5-beta.2+build7" into the numeric core [0,1,5] and the pre-release
/// segments ["beta","2"]. Build metadata ("+...") never affects precedence.
fn split_version(text: &str) -> (Vec<u64>, Vec<String>) {
    let text = text.trim().trim_start_matches('v').replace(' ', "");
    let text = text.split('+').next().unwrap_or("");
    let (base, pre) = match text.split_once('-') {
        Some((base, pre)) => (base, Some(pre)),
        None => (text, None),
    };

    let segments: Vec<&str> = base.split('.').collect();
    let mut core = Vec::new();
    let mut first_non_numeric = None;
    for (index, segment) in segments.iter().enumerate() {
        match segment.trim().parse::<u64>() {
            Ok(number) => core.push(number),
            Err(_) => {
                first_non_numeric = Some(index);
                break;
            }
        }
    }
    let mut pre_release: Vec<String> = Vec::new();
    if let Some(index) = first_non_numeric {
        pre_release.extend(segments[index..].iter().map(|s| s.trim().to_string()));
    }
    if let Some(pre) = pre {
        pre_release.extend(pre.split('.').map(|s| s.trim().to_string()));
    }
    pre_release.retain(|s| !s.is_empty());
    (core, pre_release)
}

/// Semver-style precedence between two pre-release tag lists: numeric identifiers
/// compare numerically, alphanumeric ones lexically, alphanumeric outranks numeric,
/// and a longer list outranks its own prefix. True if `remote` is later.
fn pre_release_is_later(local: &[String], remote: &[String]) -> bool {
    for index in 0..local.len().max(remote.len()) {
        let left = local.get(index);
        let right = remote.get(index);
        match (left, right) {
            (None, Some(_)) => return true,
            (Some(_), None) => return false,
            (None, None) => continue,
            (Some(left), Some(right)) => {
                let left_number = left.parse::<u64>().ok();
                let right_number = right.parse::<u64>().ok();
                match (left_number, right_number) {
                    (Some(a), Some(b)) => {
                        if b > a {
                            return true;
                        }
                        if b < a {
                            return false;
                        }
                    }
                    (Some(_), None) => return false, // numeric ranks below alphanumeric
                    (None, Some(_)) => return true,
                    (None, None) => {
                        let (a, b) = (left.to_lowercase(), right.to_lowercase());
                        if b > a {
                            return true;
                        }
                        if b < a {
                            return false;
                        }
                    }
                }
            }
        }
    }
    false
}

/// `remote` is a newer version than `local`. L-14: non-numeric segments used to be
/// read as 0, so "0.1.5-beta.1" compared as "0.1.0.1". The numeric core is compared
/// first, and a release always outranks the pre-release of the same core.
pub fn is_newer(local: &str, remote: &str) -> bool {
    let (local_core, local_pre) = split_version(local);
    let (remote_core, remote_pre) = split_version(remote);

    for index in 0..local_core.len().max(remote_core.len()) {
        let left = local_core.get(index).copied().unwrap_or(0);
        let right = remote_core.get(index).copied().unwrap_or(0);
        if right > left {
            return true;
        }
        if right < left {
            return false;
        }
    }

    // Same numeric core: release > pre-release, otherwise compare the tags.
    match (local_pre.is_empty(), remote_pre.is_empty()) {
        (true, true) => false,
        (false, true) => true,
        (true, false) => false,
        (false, false) => pre_release_is_later(&local_pre, &remote_pre),
    }
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
    let (_status, bytes) = unsafe { request(host, &path) }?;

    // H-01: never hand a truncated or foreign file to ShellExecuteW. The status is
    // already known to be 200 (see `request_with_proxy`); now the size has to agree
    // with Content-Length and with the size the release API advertised.
    if bytes.len() < 1024 {
        eprintln!("update: загрузка меньше 1 КБ — это не установщик");
        return None;
    }
    if &bytes[..2] != b"MZ" {
        eprintln!("update: файл не начинается с сигнатуры PE (MZ) — загрузка отменена");
        return None;
    }
    if release.size > 0 && (bytes.len() as u64) != release.size {
        eprintln!(
            "update: скачано {} байт, релиз объявляет {} — файл не сохранён",
            bytes.len(),
            release.size
        );
        return None;
    }

    let dir = crate::config::config_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join("z_update.exe");
    if let Err(e) = std::fs::write(&file, &bytes) {
        eprintln!("update: не удалось записать {}: {e}", file.display());
        return None;
    }

    // Re-check the size on disk: a partial write (disk full, antivirus cutting the
    // stream) is deleted instead of being run as an administrator.
    let written = std::fs::metadata(&file).map(|meta| meta.len()).unwrap_or(0);
    if written != bytes.len() as u64 || (release.size > 0 && written != release.size) {
        eprintln!(
            "update: на диске {} байт вместо {} — установщик удалён",
            written,
            bytes.len()
        );
        let _ = std::fs::remove_file(&file);
        return None;
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn wide_strings_are_null_terminated() {
        // WinHTTP reads PCWSTR until a NUL; without one it reports a bogus host.
        assert_eq!(super::wide("GET"), vec![71, 69, 84, 0]);
        assert_eq!(super::wide("zя"), vec![122, 0x044F, 0]);
    }

    #[test]
    fn version_compare_is_numeric() {
        assert!(super::is_newer("0.1.1", "0.1.2"));
        assert!(super::is_newer("0.9.9", "0.10.0"));
        assert!(!super::is_newer("0.1.2", "0.1.1"));
        assert!(!super::is_newer("0.1.2", "0.1.2"));
    }

    #[test]
    fn version_compare_understands_pre_release() {
        // L-14: "0.1.5-beta.1" used to turn into [0, 1, 0, 1].
        assert!(super::is_newer("0.1.4", "0.1.5-beta.1"));
        assert!(!super::is_newer("0.1.5", "0.1.5-beta.1"));
        assert!(super::is_newer("0.1.5-beta.1", "0.1.5"));
        assert!(super::is_newer("0.1.5-beta.1", "0.1.5-beta.2"));
        assert!(!super::is_newer("0.1.5-beta.2", "0.1.5-beta.1"));
        assert!(super::is_newer("0.1.5-beta", "0.1.5-rc"));
        assert!(!super::is_newer("0.1.5", "0.1.4+build9"));
        assert!(super::is_newer("v0.1.4", "v0.1.5"));
        assert_eq!(super::split_version("0.1.5-beta.2+build7"), (vec![0, 1, 5], vec!["beta".to_string(), "2".to_string()]));
    }

    #[test]
    fn latest_release_is_reachable_with_network() {
        // Guarded so the offline test suite stays green.
        if std::env::var("ZYA_NETWORK_TEST").unwrap_or_default() != "1" {
            return;
        }
        let release = super::check_latest();
        println!("latest release: {release:?}");
        assert!(release.is_some(), "GitHub Releases API должен отвечать");
    }
}
