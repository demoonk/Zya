//! Small WinHTTP probe used while debugging the update check:
//!   cargo run --release --example net_probe
use std::ffi::c_void;
use windows::core::PCWSTR;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpReadData,
    WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_NO_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_OPEN_REQUEST_FLAGS,
};

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn fetch(host: &str, path: &str, port: u16, secure: bool) {
    let flags = if secure {
        WINHTTP_FLAG_SECURE
    } else {
        WINHTTP_OPEN_REQUEST_FLAGS(0)
    };
    let agent = wide("ZyaUpdater/1.0");
    let host_w = wide(host);
    let path_w = wide(path);
    let verb = wide("GET");
    let session = WinHttpOpen(PCWSTR(agent.as_ptr()), WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0);
    if session.is_null() {
        println!("session null");
        return;
    }
    WinHttpSetTimeouts(session, 5000, 8000, 8000, 20000).ok();
    let conn = WinHttpConnect(session, PCWSTR(host_w.as_ptr()), port, 0);
    if conn.is_null() {
        println!("connect null");
        return;
    }
    let req = WinHttpOpenRequest(conn, PCWSTR(verb.as_ptr()), PCWSTR(path_w.as_ptr()), PCWSTR::null(), PCWSTR::null(), std::ptr::null(), flags);
    if req.is_null() {
        println!("request null");
        return;
    }
    let sent = WinHttpSendRequest(req, None, None, 0, 0, 0);
    let received = WinHttpReceiveResponse(req, std::ptr::null_mut());
    println!("{host}:{port}{path} send={sent:?} receive={received:?}");
    if received.is_ok() {
        let mut buffer = [0u8; 4096];
        let mut read = 0u32;
        let mut total = 0usize;
        while WinHttpReadData(req, buffer.as_mut_ptr() as *mut c_void, buffer.len() as u32, &mut read).is_ok() && read > 0 {
            total += read as usize;
        }
        println!("  read {total} bytes");
    }
    WinHttpCloseHandle(req).ok();
    WinHttpCloseHandle(conn).ok();
    WinHttpCloseHandle(session).ok();
}

fn main() {
    unsafe {
        fetch("example.com", "/", 80, false);
        fetch("api.github.com", "/repos/demoonk/Zya/releases/latest", 443, true);
        fetch("github.com", "/repos/demoonk/Zya/releases/latest", 80, false);
    }
}
