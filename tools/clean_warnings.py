import pathlib

demo = pathlib.Path("src/demo.rs")
t = demo.read_text(encoding="utf-8")
t = t.replace("        ShowWindow(edit, SW_SHOW);", "        let _ = ShowWindow(edit, SW_SHOW);")
t = t.replace("    ShowWindow(edit, SW_SHOW);", "    let _ = ShowWindow(edit, SW_SHOW);")
t = t.replace("ShowWindow(notepad, SW_SHOW).ok();", "let _ = ShowWindow(notepad, SW_SHOW).ok();")
demo.write_text(t, encoding="utf-8")

settings = pathlib.Path("src/settings.rs")
s = settings.read_text(encoding="utf-8")
pairs = [
    ('SetWindowTextW(filter, PCWSTR(wide("notepad").as_ptr()));',
     'let _ = SetWindowTextW(filter, PCWSTR(wide("notepad").as_ptr()));'),
    ('SetWindowTextW(filter, PCWSTR(wide("").as_ptr()));',
     'let _ = SetWindowTextW(filter, PCWSTR(wide("").as_ptr()));'),
    ("SetWindowTextW(\n            get_handle(&DIR_PATH_HANDLE),",
     "let _ = SetWindowTextW(\n            get_handle(&DIR_PATH_HANDLE),"),
    ("SetWindowTextW(\n            get_handle(&FILTER_HANDLE),",
     "let _ = SetWindowTextW(\n            get_handle(&FILTER_HANDLE),"),
    ("PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0));",
     "let _ = PostMessageW(Some(dialog), WM_CLOSE, WPARAM(0), LPARAM(0));"),
    ("DestroyWindow(window);\n    }\n}",
     "let _ = DestroyWindow(window);\n    }\n}"),
]
for old, new in pairs:
    s = s.replace(old, new)
settings.write_text(s, encoding="utf-8")
print("patched")
