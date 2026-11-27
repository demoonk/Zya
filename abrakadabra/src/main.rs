// GUI subsystem: launching zя.exe from Explorer or from the autostart key must
// not leave a black console window on the screen. Command-line output still
// works when the program is started from a terminal.
#![windows_subsystem = "windows"]

mod autostart;
mod config;
mod demo;
mod diary;
mod engine;
mod layout;
mod lexicon;
mod model;
mod probe;
mod sanity;
mod settings;
mod settings_ui;
mod sim;
mod tg;
mod theme;
mod tray;
mod update;
mod win;

use std::io::Write;

use engine::Engine;
use model::Model;

/// A CLI probe opens the settings window in a helper thread. `join()` has no
/// timeout, so a window that never closes (a control that stopped answering, a
/// wedged subprocess) would keep the command hanging forever. Close the window
/// and give the thread a bounded amount of time to finish.
fn join_window<T: 'static>(handle: std::thread::JoinHandle<T>, deadline_ms: u64) -> bool {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = sender.send(());
    });
    receiver.recv_timeout(std::time::Duration::from_millis(deadline_ms)).is_ok()
}

fn usage() {
    println!(
        "zя.exe — автопереключатель раскладки (Rust, глобальный клавиатурный хук)\n\
        \n\
        `z.exe` — установщик (запуск от администратора), `zя.exe` — само приложение.\n\
        Команды ниже работают и с `z.exe`: он передаёт параметры приложению.\n\
        Права администратора не нужны: хук и значок работают от обычного пользователя.\n\
        \n\
        команды:\n\
          z.exe (без параметров)   запустить в рабочем режиме: хук и значок в трее\n\
          z.exe run [<сек>] [headless] [restart]  то же явно: headless — без окна\n\
                                     консоли, restart — сначала остановить другие\n\
                                     экземпляры, <сек> — поработать и выйти\n\
          z.exe traytest          проверить значок/меню без клавиатурного хука\n\
          z.exe trayprobe         какая картинка значка сейчас в трее\n\
          z.exe menutest          проверить пункты меню значка\n\
          z.exe demo              проверка движка на наборе фраз (без хука)\n\
          z.exe simtest           модель хука внутри процесса: клавиши -> текст\n\
          z.exe simarticle        статья 3dnews через модель хука, без окон\n\
          z.exe simall            все собранные статьи (3dnews/ixbt) через модель хука\n\
           z.exe errtest           ошибки набора статей: уникальные ошибки с анализом\n\
           z.exe usertest          два теста на статьях: без Break и с Break\n\
          z.exe simkeys <клавиши> набрать физические клавиши в модель хука\n\
           z.exe short             словарчик коротких слов: список и конфликты\n\
           z.exe short add <слово> добавить слово в словарчик\n\
           z.exe short del <слово> убрать своё слово из словарчика\n\
           z.exe short conflicts   пары клавиш, одинаковые в обеих раскладках\n\
           z.exe wintest           совместимость: хук на реальном EDIT-окне (25 тестов)\n\
          z.exe article           реальный тест: многоязычная статья через хук\n\
          z.exe peer \"<текст>\"    набрать текст в своём окне без хука (для теста\n\
                                     с резидентным процессом, который держит хук)\n\
          z.exe sanity            сравнение с эталоном на 4000+ словах\n\
          z.exe check <текст>     показать, что движок сделал бы с этими словами\n\
          z.exe score <текст>     внутренние оценки (ru/en) для каждого токена\n\
          z.exe variants <текст>  варианты прочтения неоднозначных ключей\n\
          z.exe selftest          прогнать встроенные проверочные случаи\n\
           z.exe updatecheck       сравнить свою версию с последним релизом на GitHub\n\
          z.exe stats             статистику языковой модели\n\
          z.exe probe             раскладки + проверка Alt+Shift на своём окне\n\
          z.exe tgprobe           найти окна Telegram Desktop и снять снимок\n\
           z.exe tgshot <path>     снимок окна Telegram в BMP\n\
           z.exe tgcomposer <path> снимок полосы ввода Telegram\n\
           z.exe tgtest            тесты хука прямо в поле ввода «Избранное»\n\
           z.exe tgclear           очистить поле ввода Telegram\n\
           z.exe autostart on|off  запуск при входе в Windows (или меню значка)\n\
           z.exe install [off]      скопировать себя в %APPDATA%\\zя\\zя.exe и\n\
                                    зарегистрировать автозапуск (off — без него)\n\
           z.exe stop               остановить другие экземпляры программы\n\
           z.exe who                pid, есть ли другой экземпляр, раскладка окна\n\
          z.exe config            текущий конфигурационный JSON и его путь\n\
          z.exe log               путь к журналу переключений\n\
           z.exe dict                слова белого списка, которые движок не трогает\n\
           z.exe dict add <слово>    добавить слово в белый список\n\
           z.exe dict remove <слово> убрать слово из белого списка\n\
           z.exe log clear           очистить журнал переключений\n\
          z.exe settings          окно настроек: «Настройки», «Игнорировать», «Лог», «Словарь»\n\
          z.exe tabshot <вкладка> <файл.bmp>\n\
                                    снимок вкладки settings|ignore|log для проверки\n\
           z.exe deletetest         проверить кнопку «Удалить» в списке игнорирования\n\
            z.exe windows            список окон для кнопки «Добавить активное приложение»\n\
            z.exe deletetest multi   удалить сразу несколько выбранных строк\n\
            z.exe hotkeytest         проверить окно назначения горячей клавиши\n\
           z.exe stalltest          проверка стража: окно закрывается, если цикл\n\
                                   сообщений перестал работать (перехват клавиш\n\
                                   снимается первым, чтобы клавиатура не\n\
                                   «зависла» во всей системе)\n\
           z.exe techtest          восстановление всех терминов тех. словаря\n\
           z.exe corpus            точность движка на тексте собранных статей\n\
           z.exe layoutprobe       раскладка фокусного контрола и корневого окна\n\
           z.exe watch [<сек>]     смотреть события хука несколько секунд\n\
           прочее (passive, focustest, notepadtest, addwordtest, nphook, nphook2, hooksmoke,\n\
           npkeys, nptype, npprobe, hostedit, selprobe, settest, pickprobe, tgopen,\n\
           tgtype, tgread, tgkey) — диагностики хука и окна настроек\n\
           z.exe help              эта справка\n\
        \n\
        значок в трее: RU или EN показывает раскладку,\n\
        буквы рисуются без фона: белые в тёмной теме, чёрные в светлой; красный\n\
        косой штрих поверх букв — автоисправление выключено.\n\
        левый клик — включить/выключить, правый — меню: Автоисправление раскладки,\n\
        Автозапуск при входе, Настройки, Выход. Настройки — окно из четырёх вкладок:\n\
        «Настройки» (версия и обновление, автозапуск, горячие клавиши),\n\
        «Игнорировать» (исключения), «Лог» (журнал переключений).\n\
        \n\
        хоткеи меняются в окне «Настройки» (клик по строке — нажать новую\n\
        комбинацию, Esc или «Отмена» — закрыть, «Сбросить» — вернуть стандартные)\n\
        и в config.json:\n\
          Alt          — отменить последнее исправление\n\
          Break        — сменить раскладку выделенного текста; если выделения нет —\n\
                        последнего слова до пробела или разделителя (дефис входит\n\
                        в слово: \"wi-fi\" — одно слово). Второе нажатие на том же слове\n\
                         возвращает текст, который был до первого нажатия\n\
          Ctrl+Break   — перевести выделенный текст в верхний регистр\n\
          Shift+Break  — поменять регистр выделения на противоположный\n\
          Ctrl+Alt+P   — включить/выключить автопереключение\n\
          ЭприветЭ     — кавычки: Э на клавише \" -> \" (ЭприветЭ -> \"привет\")\n\
        новые комбинации отвергаются, если они заняты: alt сам по себе меняет\n\
        раскладку, ctrl/shift/win без обычной клавиши не считаются комбинацией,\n\
        break закреплён за undo (кроме строк, где он уже стоит), win+l, win+e,\n\
        win+r, alt+f4, alt+tab и подобные — занятые Windows.\n\
        \n\
        исключения: exclude_processes — по имени процесса, exclude_dirs — по папке,\n\
        из которой запущена программа (например \"c:\\games\"). То же самое\n\
        делается в окне настроек, вкладка «Игнорировать»: «Добавить из запущенных»,\n\
        «Добавить новый процесс», «Удалить выбранное», фильтр и правая кнопка мыши\n\
        на строке — «Скопировать строку», «Скопировать весь список», «Удалить».\n\
        exclude add <процесс>          добавить процесс в исключения\n\
        exclude add-dir <папка>        добавить папку в исключения\n\
        exclude remove <процесс>       удалить процесс из исключений\n\
        exclude remove-dir <папка>     удалить папку из исключений\n\
        exclude list                   показать исключения\n\
        exclude clear                  сбросить исключения\n\
        автозапуск: autostart в config.json или меню значка в трее. Обычный — запись\n\
        zя.exe в HKCU\\...\\Run; «Запускать как администратор» — задание ZyaAutoStart\n\
        в Планировщике заданий (/sc ONLOGON /rl HIGHEST).\n\
        меню значка: \"Автоисправление раскладки\", \"Автозапуск при входе\", \"Настройки\", \"Выход\".\n\
        Вкладка «Лог» показывает diario.log цветными строками, с фильтром, кнопками\n\
        «Обновить» и «Очистить» и копированием по правой кнопке мыши.\n\
        \n\
        переменные окружения:\n\
          ZYA_VERBOSE=1          подробный вывод решений в консоль\n\
          ZYA_THEME=light|dark   тема окна настроек и значка вместо системной\n\
          ZYA_IGNORE_FOREIGN=1   не отключать автоисправление, когда хук держит\n\
                                 другая копия программы\n"
    );
}

fn set_autostart(path: &str, on: bool) -> bool {
    autostart::set(on, path)
}

fn write_autostart_flag(on: bool) {
    // Keep config.json in sync with the registry so the tray menu and the CLI
    // always show the same state.
    let mut cfg = config::load();
    cfg.autostart = on;
    let file = config::FileConfig::from(&cfg);
    let path = config::config_dir().join("config.json");
    let _ = std::fs::write(path, serde_json::to_string_pretty(&file).unwrap());
}

/// L-10: a CLI run is a *different process* from the resident one, so saving
/// config.json here does not change the engine that is hooking the keyboard.
/// Tell every resident copy to read the file again.
fn notify_resident() {
    let notified = win::notify_resident_reload();
    if notified > 0 {
        println!("zя: изменения переданы {notified} рабочему экземпляру");
    }
}

fn startup_log(msg: &str) {
    let dir = config::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("startup.log");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let line = format!("[{}s] {}\n", stamp, msg);
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

fn main() -> anyhow::Result<()> {
    win::make_dpi_aware();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("run");

    if cmd == "config" {
        let cfg = config::load();
        let file = config::FileConfig::from(&cfg);
        println!("{}", serde_json::to_string_pretty(&file).unwrap());
        println!("файл: {}", config::config_dir().display());
        for warning in config::warnings() {
            println!("zя: {warning}");
        }
        return Ok(());
    }
    if cmd == "log" {
        if args.get(1).map(|s| s == "clear").unwrap_or(false) {
            diary::clear_recent();
            match std::fs::write(diary::path(), "") {
                Ok(_) => println!("zя: журнал переключений очищен"),
                Err(e) => println!("zя: не удалось очистить журнал: {e}"),
            }
            return Ok(());
        }
        println!("{}", diary::path().display());
        return Ok(());
    }
    if cmd == "dict" || cmd == "словарь" {
        let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
        match sub {
            "add" => {
                let word = args.get(2).cloned().unwrap_or_default();
                if word.is_empty() {
                    println!("zя dict add <слово>");
                    return Ok(());
                }
                if win::add_dictionary_word(&word) {
                    println!("zя: {word} добавлено в словарь");
                    notify_resident();
                } else {
                    println!("zя: {word} уже в словаре");
                }
            }
            "remove" | "del" => {
                let word = args.get(2).cloned().unwrap_or_default();
                if win::remove_dictionary_word(&word) {
                    println!("zя: {word} удалено из словаря");
                    notify_resident();
                } else {
                    println!("zя: {word} не найдено в словаре");
                }
            }
            _ => {
                let words = win::dictionary_words();
                if words.is_empty() {
                    println!("zя: словарь пуст");
                } else {
                    println!("zя: словарь ({}): {}", words.len(), words.join(", "));
                }
            }
        }
        return Ok(());
    }
    if cmd == "short" || cmd == "словарчик" {
        let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
        match sub {
            "add" => {
                let word = args.get(2).cloned().unwrap_or_default();
                if word.is_empty() {
                    println!("zя short add <слово>");
                    return Ok(());
                }
                if win::add_short_word(&word) {
                    println!("zя: «{word}» добавлено в словарчик коротких слов");
                    notify_resident();
                } else {
                    println!("zя: «{word}» уже в словарчике или не является словом");
                }
            }
            "remove" | "del" => {
                let word = args.get(2).cloned().unwrap_or_default();
                if win::remove_short_word(&word) {
                    println!("zя: «{word}» удалено из словарчика");
                    notify_resident();
                } else {
                    println!("zя: «{word}» не найдено — удаляются только добавленные пользователями слова");
                }
            }
            "conflicts" => {
                let (ru, en) = win::short_word_lists();
                let lex = lexicon::Lexicon::with_extra(&ru, &en);
                println!(
                    "zя: конфликтов {} (одни и те же клавиши — слово обеих раскладок)",
                    lex.conflicts().len()
                );
                for c in lex.conflicts() {
                    println!("  {:6} пишется как {:6} — обе раскладки дают настоящее слово", c.ru, c.keys);
                }
            }
            _ => {
                let (ru, en) = win::short_word_lists();
                let lex = lexicon::Lexicon::with_extra(&ru, &en);
                println!(
                    "zя: словарчик коротких слов: {} русских, {} английских (добавлено: {} + {})",
                    lex.ru_count(),
                    lex.en_count(),
                    ru.len(),
                    en.len()
                );
                println!("  русские: {}", lex.ru_words().join(", "));
                println!("  английские: {}", lex.en_words().join(", "));
                let conflicts = lex.conflicts();
                if !conflicts.is_empty() {
                    println!("  конфликты (защищено английское написание):");
                    for c in conflicts {
                        println!("    {} <-> {}", c.ru, c.keys);
                    }
                }
            }
        }
        return Ok(());
    }
    if cmd == "tabs" {
        // `zя tabs` — open the settings window, walk every page and save a
        // screenshot of each one, so the layout and the font sizes can be checked
        // at the real monitor DPI without looking at the screen.
        let _settings_thread = std::thread::spawn(|| settings::open_blocking());
        unsafe {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, PostMessageW, WM_CLOSE};
            let class: Vec<u16> = "ZyaSettingsWindow"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let hwnd = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None)
                .unwrap_or(HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                println!("zя: окно настроек не открылось");
                return Ok(());
            }
            for (index, role) in crate::settings_ui::tab_roles_probe().iter().enumerate() {
                crate::settings_ui::activate_tab_for_probe(*role);
                std::thread::sleep(std::time::Duration::from_millis(700));
                let out = format!("data/tab_{index}.bmp");
                tg::capture_window(hwnd, &out)?;
                println!("zя: страница {index} снята в {out}");
            }
            if let Some((left, top, width, height)) = crate::settings_ui::control_rect_for_probe(110) {
                println!("zя: кнопка конфигурации {left},{top} {width}x{height}");
            }
            // The assignment panel is a separate window: open it from the first row
            // of the hotkey table and take a picture of it as well.
            if crate::settings_ui::click_hotkey_row_for_probe(0) {
                std::thread::sleep(std::time::Duration::from_millis(800));
                if let Some(panel) = crate::settings_ui::modal_window_for_probe() {
                    tg::capture_window(panel, "data/modal.bmp")?;
                    println!("zя: панель назначения снята в data/modal.bmp");
                }
            }
            PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)).ok();
        }
        return Ok(());
    }
    if cmd == "shot" {
        // `zя shot <class|title> <file.bmp>` — PrintWindow any window, used to
        // check the settings window layout without looking at the screen.
        let needle = args.get(1).cloned().unwrap_or_else(|| "ZyaSettingsWindow".to_string());
        let out = args.get(2).cloned().unwrap_or_else(|| "data/shot.bmp".to_string());
        let wide: Vec<u16> = needle.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let hwnd = windows::Win32::UI::WindowsAndMessaging::FindWindowW(
                windows::core::PCWSTR(wide.as_ptr()),
                None,
            );
            match hwnd {
                Ok(window) if !window.0.is_null() => {
                    tg::capture_window(window, &out)?;
                    println!("zя: снимок окна сохранён в {out}");
                }
                _ => println!("zя: окно «{needle}» не найдено"),
            }
        }
        return Ok(());
    }
    if cmd == "cursorprobe" {
        // Diagnostics for the demo harness: what the desktop really reports about
        // the mouse, so a synthetic click can be placed correctly.
        unsafe {
            use windows::Win32::Foundation::POINT;
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                INPUT_MOUSE, INPUT_0, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
                MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT, MOUSE_EVENT_FLAGS, SendInput,
                INPUT,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN, SM_CXVIRTUALSCREEN,
                SM_CYVIRTUALSCREEN, SetCursorPos,
            };
            let mut p = POINT { x: 0, y: 0 };
            let here = GetCursorPos(&mut p);
            println!(
                "cursor {:?} at ({},{})",
                here, p.x, p.y
            );
            println!(
                "screen {}x{}, virtual {}x{}",
                GetSystemMetrics(SM_CXSCREEN),
                GetSystemMetrics(SM_CYSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN)
            );
            let set = SetCursorPos(210, 130);
            println!("SetCursorPos(210,130) -> {set:?}");
            let nx = 210 * 65535 / GetSystemMetrics(SM_CXSCREEN);
            let ny = 130 * 65535 / GetSystemMetrics(SM_CYSCREEN);
            let mk = |flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32| INPUT {
                r#type: INPUT_MOUSE,
                Anonymous: INPUT_0 {
                    mi: MOUSEINPUT {
                        dx,
                        dy,
                        mouseData: 0,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                }
            };
            let moved = SendInput(
                &[mk(MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_MOVE, nx, ny)],
                std::mem::size_of::<INPUT>() as i32,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = GetCursorPos(&mut p);
            println!(
                "SendInput absolute move to ({nx},{ny}) -> {moved} count, cursor now ({},{})",
                p.x, p.y
            );
            let rel = SendInput(
                &[mk(MOUSEEVENTF_MOVE, -150, -150)],
                std::mem::size_of::<INPUT>() as i32,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
            let _ = GetCursorPos(&mut p);
            println!("relative -150,-150 -> {rel}, cursor now ({},{})", p.x, p.y);
            let _ = SendInput(
                &[mk(MOUSEEVENTF_LEFTDOWN, 0, 0), mk(MOUSEEVENTF_LEFTUP, 0, 0)],
                std::mem::size_of::<INPUT>() as i32,
            );
        }
        return Ok(());
    }
    if cmd == "tabshot" {
        // `zя tabshot <settings|ignore|log> <file.bmp>` — open the settings
        // window, switch to the tab and capture it. A diagnostic for the layout.
        let tab = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| "settings".to_string());
        let out = args.get(2).cloned().unwrap_or_else(|| "data/tabshot.bmp".to_string());
        let role: usize = match tab.as_str() {
            "ignore" | "игнорировать" => 101,
            "log" | "лог" => 102,
            _ => 100,
        };
        let window = std::thread::spawn(|| settings::open_blocking());
        unsafe {
            use windows::Win32::Foundation::{LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowExW, FindWindowW, PostMessageW, WM_CLOSE, WM_COMMAND, WM_LBUTTONUP,
            };
            let class: Vec<u16> = "ZyaSettingsWindow"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let hwnd = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None)
                .unwrap_or(windows::Win32::Foundation::HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                println!("zя: окно настроек не открылось");
            } else {
                let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(role), LPARAM(0));
                std::thread::sleep(std::time::Duration::from_millis(700));
                if role == 100 {
                    // A click on the first hotkey row opens the assignment modal,
                    // which is the hardest part of the layout to see.
                    let list_class: Vec<u16> = "SysListView32"
                        .encode_utf16()
                        .chain(std::iter::once(0))
                        .collect();
                    if let Ok(list) = FindWindowExW(
                        Some(hwnd),
                        None,
                        windows::core::PCWSTR(list_class.as_ptr()),
                        None,
                    ) {
                        // Row coordinates are relative to the list window, and the
                        // first row starts just under the column headers.
                        let y = (30 << 16) | 20;
                        let _ = PostMessageW(Some(list), WM_LBUTTONUP, WPARAM(0), LPARAM(y));
                        // The modal is a child window, so it shows up inside the
                        // parent capture.
                        std::thread::sleep(std::time::Duration::from_millis(500));
                    }
                }
                tg::capture_window(hwnd, &out)?;
                println!("zя: снимок вкладки «{tab}» сохранён в {out}");
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let _ = join_window(window, 15_000);
        return Ok(());
    }
    if cmd == "windows" {
        // `zя windows` — what the "Добавить активное приложение" menu can offer.
        let list = settings_ui::active_windows();
        if list.is_empty() {
            println!("zя: открытых окон не найдено");
        } else {
            for (title, process) in list {
                println!("zя: {process} — {title}");
            }
        }
        return Ok(());
    }
    if cmd == "deletetest" {
        // `zя deletetest` — does "Удалить" really remove a row? Open the settings
        // window, click the top row of the ignore list, run the same code the
        // button runs and compare config.json before and after. The original file
        // is written back so the test loses nothing.
        let original = crate::config::load();
        let before = original.exclude_processes.len() + original.exclude_dirs.len();
        let window = std::thread::spawn(|| settings::open_blocking());
        unsafe {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, PostMessageW, WM_CLOSE, WM_COMMAND,
            };
            let class: Vec<u16> = "ZyaSettingsWindow"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let hwnd = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None)
                .unwrap_or(HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                println!("zя: окно настроек не открылось");
            } else {
                // Tab "Игнорировать", then a click on the very first row: rows 0
                // and 1 are exactly the ones the old lookup could not see.
                let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(101), LPARAM(0));
                std::thread::sleep(std::time::Duration::from_millis(600));
                let multi = args.get(1).map(|value| value == "multi").unwrap_or(false);
                let removed: Vec<String> = if multi {
                    crate::settings_ui::select_and_delete_ignore_rows(3)
                } else {
                    crate::settings_ui::delete_top_ignore_row().into_iter().collect()
                };
                std::thread::sleep(std::time::Duration::from_millis(400));
                // Config writes are queued on a worker thread; wait for the queue
                // to drain before comparing the file on disk.
                settings_ui::flush_config_saves(3000);
                let after = crate::config::load();
                let count = after.exclude_processes.len() + after.exclude_dirs.len();
                if removed.is_empty() {
                    println!("zя: удаление не сработало: строки не найдены");
                } else {
                    for name in &removed {
                        let gone = !after
                            .exclude_processes
                            .iter()
                            .chain(after.exclude_dirs.iter())
                            .any(|item| item.eq_ignore_ascii_case(name));
                        println!("zя: «{name}» убрана из config.json: {gone}");
                    }
                    println!("zя: записей {before} -> {count}");
                }
                tg::capture_window(hwnd, "data/after_delete.bmp")?;
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let _ = join_window(window, 15_000);
        // The probe normally puts the list back so nothing is lost. `deletetest
        // keep` leaves the deletion in place so the file can be inspected.
        if args.get(1).map(|value| value != "keep").unwrap_or(true) {
            crate::config::save(&original).ok();
            let updated = crate::win::notify_resident_reload();
            println!("zя: список восстановлен, экземпляров обновлено: {updated}");
        }
        return Ok(());
    }
    if cmd == "hotkeytest" {
        // `zя hotkeytest` — check the hotkey assignment panel: where it opens for
        // every row, whether it can be dragged, and whether a real combination is
        // accepted. The window's own "Сбросить" button restores the defaults at
        // the end, so the test leaves nothing behind.
        let _settings_thread = std::thread::spawn(|| settings::open_blocking());
        unsafe {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::UI::Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
                KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_K, VK_LCONTROL, VK_LMENU,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, PostMessageW, WM_CLOSE, WM_COMMAND,
            };
            const BTN_HOTKEY_RESET: usize = 123;
            let class: Vec<u16> = "ZyaSettingsWindow"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let hwnd = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None)
                .unwrap_or(HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                println!("zя: окно настроек не открылось");
            } else {
                let dpi = crate::settings_ui::parent_dpi_for_probe();
                println!(
                    "zя: окно dpi {dpi}, список горячих клавиш построен: {}",
                    crate::settings_ui::list_is_built()
                );
                let before = crate::settings_ui::hotkey_for_probe("hotkey_toggle");
                println!("zя: hotkey_toggle до теста: {before}");
                for row in 0..6 {
                    let (row_top, row_bottom) = crate::settings_ui::row_bounds_for_probe(row);
                    // The real click path: the message reaches the list, its
                    // subclassed procedure opens the panel on the settings thread.
                    crate::settings_ui::click_hotkey_row_for_probe(row);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    match crate::settings_ui::modal_rect_for_probe() {
                        Some((left, top, width, height)) => {
                            println!(
                                "zя: строка {row}: y {row_top}..{row_bottom} -> панель x {left}, y {top}, {width}x{height}"
                            );
                        }
                        None => println!("zя: панель для строки {row} не открылась"),
                    }
                }

                // Open one panel and photograph exactly the screen region it
                // occupies, so what is painted can be compared with the geometry.
                crate::settings_ui::click_hotkey_row_for_probe(2);
                std::thread::sleep(std::time::Duration::from_millis(400));
                let (outer, inner) = crate::settings_ui::modal_geometry_probe();
                println!("zя: панель окно {outer:?}, клиент {inner:?}");
                let (parent_outer, client_origin) = crate::settings_ui::parent_screen_rect_probe();
                println!("zя: окно на экране {parent_outer:?}, клиент старт {client_origin:?}");
                for (class, is_modal) in crate::settings_ui::modal_z_order_probe() {
                    println!(
                        "zя: порядок окон: {class}{}",
                        if is_modal { " <- панель" } else { "" }
                    );
                }
                let order = crate::settings_ui::top_level_z_order_probe();
                let steps = order.iter().position(|entry| entry.1).map(|index| index + 1);
                println!(
                    "zя: панель стоит выше окна настроек: {}, окно настроек найдено через {steps:?} шагов вниз по z-порядку",
                    crate::settings_ui::modal_above_settings_probe()
                );
                println!("zя: перерисовал панель: {}", crate::settings_ui::refresh_modal_probe());
                // Put the panel on top of the desktop and photograph the exact
                // screen rectangle it occupies: the picture is what the user sees.
                if crate::settings_ui::raise_modal_probe() {
                    if let Some(rect) = crate::settings_ui::modal_screen_rect_for_probe() {
                        tg::capture_rect(rect.0, rect.1, rect.2, rect.3, "data/modal_real.bmp")?;
                        println!("zя: экран в прямоугольнике панели: {rect:?}");
                    }
                }
                println!("zя: окно в фокусе: {}", crate::settings_ui::focus_settings_probe());
                std::thread::sleep(std::time::Duration::from_millis(300));
                let (outer_left, outer_top, outer_right, outer_bottom) =
                    crate::settings_ui::parent_screen_rect_probe().0;
                tg::capture_rect(
                    outer_left,
                    outer_top,
                    outer_right,
                    outer_bottom,
                    "data/parent_screen.bmp",
                )?;
                if let Some((left, top, right, bottom)) = crate::settings_ui::modal_screen_rect_for_probe() {
                    tg::capture_rect(left, top, right, bottom, "data/hotkey_screen.bmp")?;
                }
                if let Some(panel) = crate::settings_ui::modal_hwnd_for_probe() {
                    tg::capture_window(panel, "data/hotkey_modal.bmp")?;
                }
                tg::capture_window(hwnd, "data/hotkey_panel.bmp")?;

                // Drag the panel by its header: down to the bottom-right corner
                // and then beyond the window, where it must stop. Messages posted
                // to the modal are dispatched by the settings thread, so give it
                // time before reading the new position.
                if crate::settings_ui::drag_modal_for_probe(900, 600) {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    println!(
                        "zя: после перетаскивания: модель {:?}, на экране {:?}",
                        crate::settings_ui::modal_rect_for_probe(),
                        crate::settings_ui::modal_screen_rect_for_probe()
                    );
                }
                if crate::settings_ui::drag_modal_for_probe(5000, 5000) {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    println!(
                        "zя: после перетаскивания за край: модель {:?}",
                        crate::settings_ui::modal_rect_for_probe()
                    );
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
                if let Some(panel) = crate::settings_ui::modal_hwnd_for_probe() {
                    tg::capture_window(panel, "data/hotkey_modal.bmp")?;
                }
                tg::capture_window(hwnd, "data/hotkey_drag.bmp")?;

                // Back to a row, then a real Ctrl+Alt+K through the capture hook.
                crate::settings_ui::click_hotkey_row_for_probe(1);
                std::thread::sleep(std::time::Duration::from_millis(300));
                println!(
                    "zя: hook для перехвата установлен: {}",
                    crate::settings_ui::capture_hook_is_installed()
                );
                let key = |vk: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS| {
                    INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 {
                            ki: KEYBDINPUT {
                                wVk: vk,
                                wScan: 0,
                                dwFlags: flags,
                                time: 0,
                                dwExtraInfo: 0,
                            },
                        },
                    }
                };
                let mut injected = 0u32;
                for combo in [
                    key(VK_LCONTROL, KEYBD_EVENT_FLAGS(0)),
                    key(VK_LMENU, KEYBD_EVENT_FLAGS(0)),
                    key(VK_K, KEYBD_EVENT_FLAGS(0)),
                    key(VK_K, KEYEVENTF_KEYUP),
                    key(VK_LMENU, KEYEVENTF_KEYUP),
                    key(VK_LCONTROL, KEYEVENTF_KEYUP),
                ] {
                    injected += SendInput(&[combo], std::mem::size_of::<INPUT>() as i32);
                    std::thread::sleep(std::time::Duration::from_millis(80));
                }
                println!("zя: SendInput принял событий: {injected}");
                std::thread::sleep(std::time::Duration::from_millis(400));
                // The sandbox can swallow injected keys, so drive the hook body
                // with the same events the hook would receive from Windows.
                crate::settings_ui::click_hotkey_row_for_probe(1);
                std::thread::sleep(std::time::Duration::from_millis(300));
                for (vk, up) in [
                    (VK_LCONTROL, false),
                    (VK_LMENU, false),
                    (VK_K, false),
                    (VK_K, true),
                    (VK_LMENU, true),
                    (VK_LCONTROL, true),
                ] {
                    crate::settings_ui::capture_key_for_probe(vk, up);
                }
                println!("zя: перехватил ключей: {}", crate::settings_ui::capture_key_count());
                println!(
                    "zя: статус: {}",
                    crate::settings_ui::status_for_probe()
                );
                println!(
                    "zя: hotkey_toggle теперь: {}",
                    crate::settings_ui::hotkey_for_probe("hotkey_toggle")
                );
                if crate::settings_ui::hotkey_for_probe("hotkey_toggle") == before {
                    // Keyboard injection is blocked in this sandbox: run the same
                    // capture path the hook would, with a pressed combination.
                    crate::settings_ui::open_modal_for_probe(1);
                    crate::settings_ui::assign_combo_for_probe(VK_K, true, true, false, false);
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    println!(
                        "zя: прямое назначение: статус {}, hotkey_toggle {}",
                        crate::settings_ui::status_for_probe(),
                        crate::settings_ui::hotkey_for_probe("hotkey_toggle")
                    );
                }
                tg::capture_window(hwnd, "data/hotkey_assigned.bmp")?;
                // Put the defaults back with the button in the window itself.
                let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(BTN_HOTKEY_RESET), LPARAM(0));
                std::thread::sleep(std::time::Duration::from_millis(400));
                println!(
                    "zя: после «Сбросить»: {}",
                    crate::settings_ui::hotkey_for_probe("hotkey_toggle")
                );

                // The panel is created and destroyed on every click: measure the
                // GDI objects it leaves behind.
                use windows::Win32::System::Threading::{
                    GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS,
                };
                let before = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) as i64;
                for row in 0..100 {
                    crate::settings_ui::click_hotkey_row_for_probe(row % 6);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                    crate::settings_ui::close_modal_for_probe();
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let after = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS) as i64;
                let difference = after - before;
                println!(
                    "zя: панель открывали и закрывали 100 раз: GDI до {before}, после {after}, разница {difference}"
                );

                // The panel must also close from its own button and from Esc.
                crate::settings_ui::click_hotkey_row_for_probe(2);
                std::thread::sleep(std::time::Duration::from_millis(200));
                println!("zя: панель открыта: {}", crate::settings_ui::modal_alive_probe());
                println!(
                    "zя: нажатие «Отмена» прошло: {}",
                    crate::settings_ui::modal_cancel_probe()
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
                println!("zя: после «Отмена» панель жива: {}", crate::settings_ui::modal_alive_probe());
                crate::settings_ui::click_hotkey_row_for_probe(3);
                std::thread::sleep(std::time::Duration::from_millis(200));
                println!(
                    "zя: нажатие Esc прошло: {}",
                    crate::settings_ui::modal_escape_probe()
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
                println!("zя: после Esc панель жива: {}", crate::settings_ui::modal_alive_probe());

                // The panel must look right in both themes.
                crate::settings_ui::click_hotkey_row_for_probe(4);
                std::thread::sleep(std::time::Duration::from_millis(200));
                crate::settings_ui::set_theme_probe(false);
                println!("zя: тема после флипа: темная={}", crate::settings_ui::theme_is_dark_probe());
                crate::settings_ui::refresh_modal_probe();
                std::thread::sleep(std::time::Duration::from_millis(400));
                if let Some(panel) = crate::settings_ui::modal_hwnd_for_probe() {
                    tg::capture_window(panel, "data/hotkey_modal_light.bmp")?;
                }
                crate::settings_ui::set_theme_probe(true);
                crate::settings_ui::refresh_modal_probe();
                std::thread::sleep(std::time::Duration::from_millis(400));
                if let Some(panel) = crate::settings_ui::modal_hwnd_for_probe() {
                    tg::capture_window(panel, "data/hotkey_modal_dark.bmp")?;
                }

                // The heartbeat is what the watchdog watches: an idle but healthy
                // window must still produce a message every second.
                println!(
                    "zя: heartbeat окна за 2 секунды: {} сообщений",
                    crate::settings_ui::heartbeat_probe(2000)
                );
                // Closing the settings window must destroy the floating panel too.
                crate::settings_ui::click_hotkey_row_for_probe(0);
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                std::thread::sleep(std::time::Duration::from_millis(700));
                println!(
                    "zя: после закрытия окна панель уничтожена: {}",
                    !crate::settings_ui::modal_alive_probe()
                );
                // After WM_CLOSE the loop must be gone: no more beats, so the
                // watchdog thread also stops and cannot post anything to a dead window.
                println!(
                    "zя: после закрытия окна heartbeat остановлен: {} новых сообщений",
                    crate::settings_ui::heartbeat_probe(2000)
                );
            }
        }
        // Diagnostics only: the probe exits without keeping the window open.
        std::process::exit(0);
    }
    if cmd == "stalltest" {
        // `zя stalltest` — the watchdog test: open the window, confirm the
        // message loop ticks, then remove the heartbeat timer and check that the
        // watchdog closes the window instead of letting it (and the keyboard hook
        // installed on that thread) hang forever.
        let window = std::thread::spawn(|| settings::open_blocking());
        std::thread::sleep(std::time::Duration::from_millis(1500));
        // A second "settings" request must not nest another message loop over the
        // same window: the latch makes the second call return immediately.
        let second = std::thread::spawn(|| settings::open_blocking());
        println!(
            "zя: второй запрос настроек вернулся сразу: {} (окно не размножилось)",
            join_window(second, 3000)
        );
        let beats = settings_ui::heartbeat_probe(15000);
        println!(
            "zя: heartbeat за 15 секунд простоя: {beats} сообщений, окно живёт: {}",
            settings_ui::settings_window_open_probe()
        );
        let closed = unsafe { settings_ui::stall_watchdog_probe(15000) };
        println!("zя: страж закрыл окно после остановки heartbeat: {closed}");
        println!(
            "zя: перехват клавиш снят: {}",
            !unsafe { settings_ui::capture_hook_is_installed() }
        );
        let _ = join_window(window, 5000);
        return Ok(());
    }
    if cmd == "gditest" {
        // `zя gditest` — open the settings window, switch tabs one hundred times
        // and report how many GDI objects the process gained. A leak in the paint
        // path would show up here as a growing count.
        let window = std::thread::spawn(|| settings::open_blocking());
        unsafe {
            use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
            use windows::Win32::System::Threading::{
                GetCurrentProcess, GetGuiResources, GR_GDIOBJECTS, GR_USEROBJECTS,
            };
            use windows::Win32::UI::WindowsAndMessaging::{
                FindWindowW, PostMessageW, WM_CLOSE, WM_COMMAND,
            };
            let class: Vec<u16> = "ZyaSettingsWindow"
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let hwnd = FindWindowW(windows::core::PCWSTR(class.as_ptr()), None)
                .unwrap_or(HWND(std::ptr::null_mut()));
            if hwnd.0.is_null() {
                println!("zя: окно настроек не открылось");
            } else {
                let me = GetCurrentProcess();
                let gdi_before = GetGuiResources(me, GR_GDIOBJECTS) as i64;
                let user_before = GetGuiResources(me, GR_USEROBJECTS) as i64;
                for i in 0..100 {
                    let role = 100 + (i % 3) as usize;
                    let _ = PostMessageW(Some(hwnd), WM_COMMAND, WPARAM(role), LPARAM(0));
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                let gdi_after = GetGuiResources(me, GR_GDIOBJECTS) as i64;
                let user_after = GetGuiResources(me, GR_USEROBJECTS) as i64;
                println!(
                    "zя: GDI объектов {gdi_before} -> {gdi_after} (разница {}), пользовательских {user_before} -> {user_after}",
                    gdi_after - gdi_before
                );
                // P-01: the same measurement across one hundred light↔dark
                // switches. A leak in the palette or the brush cache would show
                // up as a second, growing difference.
                let theme_before = GetGuiResources(me, GR_GDIOBJECTS) as i64;
                crate::settings_ui::theme_flip_for_test(100);
                std::thread::sleep(std::time::Duration::from_millis(400));
                let theme_after = GetGuiResources(me, GR_GDIOBJECTS) as i64;
                println!(
                    "zя: после 100 переключений темы GDI {theme_before} -> {theme_after} (разница {})",
                    theme_after - theme_before
                );
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        let _ = join_window(window, 15_000);
        return Ok(());
    }
    if cmd == "settings" || cmd == "настройки" {
        if let Err(error) = settings::open_blocking() {
            eprintln!("zя: не удалось открыть настройки: {error}");
        }
        return Ok(());
    }
    if cmd == "settest" {
        // The stub used to always report success. `selftest` is the real one.
        println!("zя: settest — совместимость, используйте `z.exe selftest`");
        return Ok(());
    }
    if cmd == "pickprobe" {
        println!("zя: pickprobe не реализован: выбирайте процесс в окне настроек");
        return Ok(());
    }
    if cmd == "autostart" {
        let exe = std::env::current_exe()?;
        let on = args.get(1).map(|s| s == "on").unwrap_or(true);
        if on {
            if autostart::set(true, &exe.display().to_string()) {
                write_autostart_flag(true);
                println!("zя: автозапуск включён (Run\\zя, запуск скрыто)");
            } else {
                println!("zя: не удалось добавить ключ автозапуска");
            }
        } else {
            autostart::set(false, &exe.display().to_string());
            write_autostart_flag(false);
            println!("zя: автозапуск выключен");
        }
        return Ok(());
    }
    if cmd == "install" {
        // Put the binary in %APPDATA%\zя and register it for autostart,
        // so the switcher survives being moved/deleted from target\release.
        let exe = std::env::current_exe()?;
        let dir = config::config_dir();
        std::fs::create_dir_all(&dir)?;
        let dest = dir.join("zя.exe");
        std::fs::copy(&exe, &dest)?;
        println!("zя: скопирован в {}", dest.display());
        if args.get(1).map(|s| s == "off").unwrap_or(false) {
            set_autostart(&dest.display().to_string(), false);
            write_autostart_flag(false);
            println!("zя: автозапуск не включён");
        } else if set_autostart(&dest.display().to_string(), true) {
            write_autostart_flag(true);
            println!("zя: автозапуск включён (Run\\zя.exe → {})", dest.display());
            println!("запустить сейчас: {} run", dest.display());
        } else {
            println!("zя: не удалось зарегистрировать автозапуск");
        }
        return Ok(());
    }

    if cmd == "exclude" {
        let sub = args.get(1).map(|s| s.as_str()).unwrap_or("list");
        let mut cfg = config::load();
        match sub {
            "add" | "process" | "prog" | "program" => {
                let name = args.get(2).cloned().unwrap_or_default();
                if name.is_empty() {
                    println!("zя exclude add <имя процесса>");
                    return Ok(());
                }
                let lower = name.to_lowercase();
                if cfg
                    .exclude_processes
                    .iter()
                    .any(|p| p.to_lowercase() == lower)
                {
                    println!("zя: {name} уже в исключениях");
                } else {
                    cfg.exclude_processes.push(name.clone());
                    config::save(&cfg)?;
                    notify_resident();
                    println!("zя: исключён процесс {name}");
                }
            }
            "add-dir" | "dir" | "folder" => {
                let dir = args.get(2).cloned().unwrap_or_default();
                if dir.is_empty() {
                    println!("zя exclude add-dir <папка>");
                    return Ok(());
                }
                let lower = dir.to_lowercase();
                if cfg.exclude_dirs.iter().any(|d| d.to_lowercase() == lower) {
                    println!("zя: {dir} уже в исключениях");
                } else {
                    cfg.exclude_dirs.push(dir.clone());
                    config::save(&cfg)?;
                    notify_resident();
                    println!("zя: исключена папка {dir}");
                }
            }
            "remove" | "del" | "delete" => {
                let name = args.get(2).cloned().unwrap_or_default();
                if name.is_empty() {
                    println!("zя exclude remove <имя процесса>");
                    return Ok(());
                }
                let lower = name.to_lowercase();
                let before = cfg.exclude_processes.len();
                cfg.exclude_processes.retain(|p| p.to_lowercase() != lower);
                if cfg.exclude_processes.len() == before {
                    println!("zя: {name} не найдено в исключениях");
                } else {
                    config::save(&cfg)?;
                    notify_resident();
                    println!("zя: процесс {name} больше не исключён");
                }
            }
            "remove-dir" | "del-dir" => {
                let dir = args.get(2).cloned().unwrap_or_default();
                if dir.is_empty() {
                    println!("zя exclude remove-dir <папка>");
                    return Ok(());
                }
                let lower = dir.to_lowercase();
                let before = cfg.exclude_dirs.len();
                cfg.exclude_dirs.retain(|d| d.to_lowercase() != lower);
                if cfg.exclude_dirs.len() == before {
                    println!("zя: {dir} не найдено в исключениях");
                } else {
                    config::save(&cfg)?;
                    notify_resident();
                    println!("zя: папка {dir} больше не исключена");
                }
            }
            "clear" => {
                cfg.exclude_processes.clear();
                cfg.exclude_dirs.clear();
                config::save(&cfg)?;
                println!("zя: исключения очищены");
            }
            _ => {
                println!("процессы: {}", cfg.exclude_processes.join(", "));
                println!("папки:   {}", cfg.exclude_dirs.join(", "));
            }
        }
        return Ok(());
    }

    let cfg = config::load();
    let engine = Engine::new(Model::load()?, cfg.clone());

    match cmd {
        "run" => {
            let headless = args.iter().any(|a| a == "headless") || args.is_empty();
            // `zя.exe run headless restart` first asks every other copy to quit
            // through its tray window, so an old build cannot keep hooking the
            // keyboard next to the new one.
            if args.iter().any(|a| a == "restart") {
                let n = win::stop_other_instances();
                println!("zя: остановлено других экземпляров: {n}");
            }
            // Do not hide the console until the hook/tray have been checked.
            // If tray creation fails, the error remains visible to the user.
            // The `autostart` field in config.json is the single source of truth:
            // the registry entry is created or removed to match it.
            if cfg.autostart != autostart::enabled()
                || cfg.admin_autostart != autostart::admin_enabled()
            {
                autostart::sync_with_config(cfg.autostart, cfg.admin_autostart);
            }
            startup_log("zя: запуск рабочего режима");
            // A stop flag left behind by an earlier `zя stop` belongs to the copy
            // that is already gone, not to this one.
            win::clear_stop_flag();

            // Two resident copies would hook the keyboard twice and rewrite the
            // same word twice — the "double switch" the user keeps noticing.
            if win::another_instance_running() {
                startup_log("zя: другой экземпляр уже работает, второй запуск пропущен");
                println!("zя: уже запущен другой экземпляр — этот закрыт.");
                return Ok(());
            }

            let hook_ok = match win::install_hook() {
                Ok(_) => {
                    startup_log("zя: клавиатурный хук установлен");
                    true
                }
                Err(e) => {
                    startup_log(&format!("zя: не удалось установить клавиатурный хук: {e}"));
                    println!("zя: не удалось установить клавиатурный хук: {e}");
                    false
                }
            };

            let hwnd = match tray::attach() {
                Ok(h) => {
                    startup_log("zя: значок в трее создан");
                    tray::keep_alive();
                    Some(h)
                }
                Err(e) => {
                    startup_log(&format!("zя: не удалось создать значок в трее: {e}"));
                    println!("zя: не удалось создать значок в трее: {e}");
                    None
                }
            };

            if hook_ok {
                win::start_focus_guard();
                startup_log("zя: контроль фокуса окон запущен");

                println!(
                    "zя: хук установлен ({}), автопереключение {}.",
                    if hwnd.is_some() {
                        "значок в трее"
                    } else {
                        "без значка в трее"
                    },
                    if cfg.auto_enabled {
                        "включено"
                    } else {
                        "выключено"
                    }
                );
            } else {
                println!(
                    "zя: работает без клавиатурного хука ({}).",
                    if hwnd.is_some() {
                        "значок в трее"
                    } else {
                        "без значка в трее"
                    }
                );
            }
            println!(
                "раскладка активного окна: {}",
                if win::current_layout_is_russian() {
                    "русская"
                } else {
                    "английская (или другая)"
                }
            );
            if !headless {
                println!("Ctrl+C — выйти; правый клик по значку — меню.");
            }
            if headless && hook_ok && hwnd.is_some() {
                win::hide_console();
            }

            // Optional timed run: `z.exe run 10` exits cleanly after 10 seconds.
            // Used by automated tests so no process is ever left behind.
            if let Some(secs) = args.get(1).and_then(|s| s.parse::<u64>().ok()) {
                // `HWND` is not `Send`, so the handle travels as a raw pointer.
                let tray_window = hwnd.map(|window| window.0 as usize);
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    println!(
                        "zя: время вышло, переключений: {}, событий хука: {}",
                        win::switch_count(),
                        win::event_count()
                    );
                    // Close the tray window instead of calling exit(): DefWindowProc
                    // destroys it, WM_DESTROY posts WM_QUIT and the message loop ends,
                    // so detach() and remove_hook() actually run and no icon is left
                    // behind in the notification area.
                    if let Some(pointer) = tray_window {
                        unsafe {
                            windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                                Some(windows::Win32::Foundation::HWND(pointer as *mut std::ffi::c_void)),
                                windows::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                                windows::Win32::Foundation::WPARAM(0),
                                windows::Win32::Foundation::LPARAM(0),
                            )
                            .ok();
                        }
                    }
                    // Safety net for the (rare) case the loop never wakes up.
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    std::process::exit(0);
                });
            }
            win::run_message_loop();
            if let Some(h) = hwnd {
                tray::detach(h);
            }
            win::remove_hook();
        }
        "menutest" => {
            tray::menu_probe();
        }
        "trayprobe" => {
            // Attach the icon, let the focus guard read the real layout, and
            // print which of the four pictures is in the notification area.
            match tray::attach() {
                Ok(h) => {
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    tray::probe();
                    win::flip_layout_now();
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    tray::probe();
                    win::flip_layout_now();
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    tray::probe();
                    tray::detach(h);
                }
                Err(e) => println!("zя: значок не создан: {e}"),
            }
        }
        "traytest" => {
            startup_log("zя: traytest: проверка значка без хука");
            match tray::attach() {
                Ok(h) => {
                    println!("zя: значок создан; проверьте трей.");
                    startup_log("zя: traytest: значок создан");
                    std::thread::sleep(std::time::Duration::from_secs(4));
                    tray::detach(h);
                    println!("zя: traytest завершён");
                }
                Err(e) => {
                    println!("zя: не удалось создать значок: {e}");
                    startup_log(&format!("zя: traytest: значок не создан: {e}"));
                }
            }
        }
        "demo" => demo::run()?,
        "simtest" => sim::sim_test()?,
        "simarticle" => sim::sim_article()?,
        "simall" => sim::sim_all_articles()?,
        "errtest" => sim::error_report()?,
        "usertest" => sim::user_test()?,
        "breakplan" => {
            let engine =
                crate::engine::Engine::new(crate::model::Model::load()?, crate::config::load());
            if args.len() < 2 {
                println!("zя: breakplan <токен> ... — что сделает Break с каждым токеном");
            }
            for token in &args[1..] {
                let mv = crate::layout::plan_manual_move(
                    "",
                    Some((token.clone(), Some(' '))),
                    &None,
                    true,
                    engine.cfg.smart_undo,
                    &|s| engine.believable_word(s),
                );
                let mirrored = crate::layout::map_word(token);
                let letters = |s: &str| -> String {
                    s.chars().filter(|c| c.is_alphabetic()).collect()
                };
                let tails: Vec<String> = crate::layout::punctuation_tail(token)
                    .into_iter()
                    .map(|(stem, signs)| {
                        format!(
                            "{stem}+{signs} ru={:?} en={:?}",
                            engine.model.ru.score(&letters(&stem)),
                            engine.model.en.score(&letters(&stem))
                        )
                    })
                    .collect();
                println!(
                    "  {token} -> {:?}  зеркало={mirrored} ru={:?} en={:?}  хвосты: {}",
                    mv.as_ref().map(|m| m.to.clone()),
                    engine.model.ru.score(&letters(&mirrored)),
                    engine.model.en.score(&letters(&mirrored)),
                    tails.join(", ")
                );
            }
        }
        "simkeys" => {
            let text = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "привет ".to_string());
            let english = args.get(2).map(|s| s != "ru").unwrap_or(true);
            let engine =
                crate::engine::Engine::new(crate::model::Model::load()?, crate::config::load());
            let keys = sim::keys_of(&text);
            let (out, sw) = sim::type_keys(&engine, &keys, english);
            println!("{keys} -> {out}  ({sw} переключений)");
        }
        "wintest" => demo::window_test()?,
        "passive" => demo::passive_probe()?,
        "focustest" => demo::focus_test()?,
        "notepadtest" => demo::notepad_test()?,
        "addwordtest" => demo::addword_test()?,
        "inject" => demo::inject_keys(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(5))?,
        "nphook" => demo::notepad_hook_test()?,
        "nphook2" => demo::notepad_hook_loop_test()?,
        "hooksmoke" => demo::hook_smoke()?,
        "watch" => demo::hook_watch(args.get(1).and_then(|s| s.parse().ok()).unwrap_or(10))?,
        "npkeys" => demo::notepad_keys(&args[1])?,
        "nptype" => demo::notepad_type(&args[1])?,
        "npprobe" => demo::notepad_layout_probe()?,
        "layoutprobe" => {
            let (focused, root, site_class) = win::layout_probe();
            println!("zя: раскладка фокусного контрола {site_class:?} = {focused:#06x}");
            println!("zя: раскладка корневого окна = {root:#06x}");
        }
        "hostedit" => {
            let title = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "zя host".to_string());
            let secs = args
                .get(2)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            demo::host_window(&title, secs)?;
        }
        "stop" => {
            let n = win::stop_other_instances();
            println!("zя: остановлено экземпляров: {n}");
        }
        "who" => {
            println!("zя: pid {}", std::process::id());
            println!(
                "zя: другой экземпляр уже работает: {}",
                win::another_instance_running()
            );
            println!(
                "zя: раскладка активного окна: {:#06x}",
                win::active_layout_langid()
            );
        }
        "article" => demo::article_test()?,
        "selprobe" => {
            let class = args.get(1).cloned().unwrap_or_else(|| "EDIT".to_string());
            demo::selection_probe(&class)?;
        }
        "peer" => {
            let text = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "ghbdtn bqk ".to_string());
            demo::peer_test(&text)?;
        }
        "sanity" => sanity::run()?,
        "tgprobe" => tg::probe()?,
        "tgshot" => {
            let path = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "tg_shot.bmp".to_string());
            tg::snapshot(&path)?;
        }
        "tgtest" => tg::composer_test()?,
        "tgopen" => tg::open_saved_command()?,
        "tgclear" => tg::clear_probe()?,
        "tgtype" => {
            let text = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "ghbdtn ".to_string());
            tg::type_command(&text)?;
        }
        "tgread" => tg::read_command()?,
        "tgkey" => {
            let which = args.get(1).cloned().unwrap_or_else(|| "toggle".to_string());
            tg::key_command(&which)?;
        }
        "tgcomposer" => {
            let path = args
                .get(1)
                .cloned()
                .unwrap_or_else(|| "tg_composer.bmp".to_string());
            tg::composer_shot(&path)?;
        }
        "check" => {
            if args.len() < 2 {
                usage();
                return Ok(());
            }
            let text = args[1..].join(" ");
            for word in text.split_whitespace() {
                match engine.decide(word) {
                    Some(d) => println!("  {word} -> {}   {}", d.converted, d.why),
                    None => println!("  {word} -> (без изменений)"),
                }
            }
        }
        "variants" => {
            for token in args[1..].iter() {
                println!("{token}:");
                for (c, inner, edge, punct) in engine.variants_debug(token) {
                    println!(
                        "   {c:?}  kept inside={inner}, at edge={edge}, letter->punct={punct}"
                    );
                }
            }
        }
        "corpus" => {
            // Precision check over real article text: every token that a real
            // author typed must survive untouched.
            let dir = ["data/articles", "../data/articles"]
                .iter()
                .map(std::path::Path::new)
                .find(|d| d.is_dir())
                .unwrap_or_else(|| std::path::Path::new("data/articles"));
            let (mut total, mut touched) = (0usize, 0usize);
            let mut examples = Vec::new();
            let mut files = Vec::new();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "txt") {
                        files.push(p);
                    }
                }
            }
            if files.is_empty() {
                println!("нет статей в {dir:?} (запусти tools/fetch_articles.py)");
                return Ok(());
            }
            for f in &files {
                let text = std::fs::read_to_string(f).unwrap_or_default();
                for token in text.split_whitespace() {
                    let clean: String = token
                        .trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
                        .to_string();
                    if clean.chars().count() < 3 {
                        continue;
                    }
                    total += 1;
                    if let Some(d) = engine.decide(&clean) {
                        touched += 1;
                        if examples.len() < 20 {
                            examples.push(format!("{clean} -> {}", d.converted));
                        }
                    }
                }
            }
            println!(
                "текст {} статей: {total} токенов, изменено {touched} ({:.2}% ложных)",
                files.len(),
                100.0 * (touched as f64) / (total.max(1) as f64)
            );
            for e in examples {
                println!("  {e}");
            }
        }
        "techtest" => {
            // Every harvested technical term is typed on the wrong layout and
            // the engine must recover it.
            let mut terms: Vec<String> = engine.model.tech.iter().cloned().collect();
            terms.sort();
            let (mut ok, mut total, mut skipped) = (0usize, 0usize, 0usize);
            let mut misses = Vec::new();
            for term in &terms {
                let typed = layout::map_word(term);
                if engine.model.is_tech(&typed) {
                    skipped += 1;
                    continue;
                }
                total += 1;
                match engine.decide(&typed) {
                    Some(d) if d.converted == *term => ok += 1,
                    Some(d) => misses.push(format!("{typed} -> {}", d.converted)),
                    None => {
                        // Explain the miss: it is intentional whenever the
                        // Cyrillic spelling is itself a real Russian word.
                        let letters: String = typed.chars().filter(|c| c.is_alphabetic()).collect();
                        if engine.model.ru.has_word(&letters) {
                            misses.push(format!("{typed} -> (miss: {letters} — русское слово)"));
                        } else {
                            misses.push(format!("{typed} -> (miss)"));
                        }
                    }
                }
            }
            println!(
                "технический словарь: {} терминов, проверено {total}, восстановлено {ok} ({:.1}%)",
                terms.len(),
                100.0 * (ok as f64) / (total.max(1) as f64)
            );
            println!("неоднозначных (зеркало тоже термин): {skipped}");
            for m in misses.iter().take(40) {
                println!("  MISS {m}");
            }
        }
        "updatecheck" => {
            let local = update::app_version();
            match update::check_latest() {
                Some(release) => {
                    if update::is_newer(&local, &release.version) {
                        println!(
                            "доступно обновление {} (установлено {}): {}",
                            release.version, local, release.url
                        );
                    } else {
                        println!(
                            "обновлений нет: установлено {}, последняя на GitHub {}",
                            local, release.version
                        );
                    }
                }
                None => println!("не удалось связаться с api.github.com"),
            }
        }
        "selftest" => {
            let cases: &[(&str, Option<&str>)] = &[
                ("руддщ", Some("hello")),
                ("ghbdtn", Some("привет")),
                ("ьщку", Some("more")),
                ("еру", Some("the")),
                ("йгшсл", Some("quick")),
                ("фыв", None), // мусор, а не опечатка в другой раскладке
                ("bqk", None),
                ("привет", None),
                ("hello", None),
                ("щит", None),
                ("world", None),
                ("как", None),
                ("ыщт", Some("son")), // "you" и "son" дают одну и ту же раскладку
            ];
            let mut ok = 0;
            for (word, want) in cases {
                let got = engine.decide(word).map(|d| d.converted);
                if got.as_deref() == *want {
                    ok += 1;
                } else {
                    println!("  FAIL {word}: ожидалось {want:?}, получено {got:?}");
                }
            }
            println!("{ok}/{cases_len} случаев пройдено", cases_len = cases.len());
        }
        "probe" => probe::run(),
        "stats" => {
            println!("margin_unknown = {}", engine.cfg.margin_unknown);
            println!("margin_known   = {}", engine.cfg.margin_known);
            println!(
                "RU words: {}, EN words: {}",
                engine.model.ru.word_count(),
                engine.model.en.word_count()
            );
            println!("technical terms: {}", engine.model.tech_len());
            for w in ["nvme", "ddr4", "RTX 5090", "asus", "1080p"] {
                println!("  is_tech({w}) = {}", engine.model.is_tech(w));
            }
            let probe = layout::map_word("nvme");
            println!(
                "  mirror of nvme = {probe}  decide({probe}) = {:?}",
                engine.decide(&probe)
            );
            println!("top RU: {:?}", engine.model.ru.top_words(10));
            println!("top EN: {:?}", engine.model.en.top_words(10));
            for w in [
                "hello",
                "руддщ",
                "привет",
                "ghbdtn",
                "the",
                "еру",
                "more",
                "ьщку",
            ] {
                println!(
                    "  {w}: ru {:?}  en {:?}",
                    engine.model.ru.score(w),
                    engine.model.en.score(w)
                );
            }
        }
        "score" => {
            for token in args[1..].iter() {
                let letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
                let mapped = layout::map_word(token);
                let mapped_letters: String = mapped.chars().filter(|c| c.is_alphabetic()).collect();
                println!(
                    "{token}: cur ru {:?} en {:?} | alt ru {:?} en {:?} | ru_word {} en_word {} | tech {} | decision {:?}",
                    engine.model.ru.score(&letters),
                    engine.model.en.score(&letters),
                    engine.model.ru.score(&mapped_letters),
                    engine.model.en.score(&mapped_letters),
                    engine.model.ru.has_word(&mapped_letters),
                    engine.model.en.has_word(&mapped_letters),
                    engine.model.is_tech(&mapped),
                    engine.decide(token).map(|d| d.converted),
                );
            }
        }
        _ => usage(),
    }
    Ok(())
}
