use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Shell::{
    SHBrowseForFolderW, SHGetPathFromIDListEx, SHParseDisplayName, BROWSEINFOW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn main() {
    unsafe {
        extern "system" {
            fn FindWindowW(class: *const u16, title: *const u16) -> usize;
            fn FindWindowExW(
                parent: usize,
                child: usize,
                class: *const u16,
                name: *const u16,
            ) -> usize;
            fn SendMessageW(window: usize, msg: u32, w: usize, l: usize) -> usize;
        }
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(4));
            let cls = wide("#32770");
            let tree_name = wide("SysTreeView32");
            let button = wide("Button");
            let ok = wide("OK");
            let d = FindWindowW(cls.as_ptr(), std::ptr::null());
            println!("probe: dialog {d}");
            if d == 0 {
                return;
            }
            let tree = FindWindowExW(d, 0, tree_name.as_ptr(), std::ptr::null());
            let root = SendMessageW(tree, 4365, 0, 0); // TVM_GETNEXTITEM, TVGN_ROOT
            println!("probe: tree {tree} root {root}");
            if root != 0 {
                SendMessageW(tree, 4362, 9, root); // TVM_SELECTITEM, TVGN_CARET
            }
            let b = FindWindowExW(d, 0, button.as_ptr(), ok.as_ptr());
            SendMessageW(b, 0x00F5, 0, 0);
        });

        let start = wide("C:\\Users\\demOOn\\Documents\\Abrakadra");
        let mut root_pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST =
            std::ptr::null_mut();
        let parsed = SHParseDisplayName(PCWSTR(start.as_ptr()), None, &mut root_pidl, 0, None);
        println!("probe: parse {parsed:?} pidl {}", root_pidl as usize);

        let title = wide("Выберите папку");
        let info = BROWSEINFOW {
            hwndOwner: HWND(std::ptr::null_mut()),
            pidlRoot: root_pidl,
            lpszTitle: PCWSTR(title.as_ptr()),
            ..Default::default()
        };
        let id = SHBrowseForFolderW(&info);
        println!("probe: pidl {}", id as usize);
        if !id.is_null() {
            let mut path = [0u16; 260];
            let ok =
                SHGetPathFromIDListEx(id, &mut path, windows::Win32::UI::Shell::GPFIDL_FLAGS(0))
                    .as_bool();
            let text = String::from_utf16_lossy(&path);
            let end = text.find('\0').unwrap_or(text.len());
            println!("probe: ok {ok} path {}", &text[..end]);
        }
    }
}
