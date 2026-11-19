mod autostart;
mod config;
mod demo;
mod diary;
mod engine;
mod layout;
mod model;
mod probe;
mod sanity;
mod settings;
mod sim;
mod tg;
mod tray;
mod win;

use std::io::Write;

use engine::Engine;
use model::Model;

fn usage() {
    println!(
        "z.exe — автопереключатель раскладки (Rust, глобальный клавиатурный хук)\n\
        \n\
        Права администратора не нужны: хук и значок работают от обычного пользователя.\n\
        \n\
        команды:\n\
          z.exe (без параметров)   запустить в рабочем режиме: хук и значок в трее\n\
          z.exe run [<сек>] [headless]  то же явно; headless — без окна консоли\n\
          z.exe traytest          проверить значок/меню без клавиатурного хука\n\
          z.exe demo              проверка движка на наборе фраз (без хука)\n\
          z.exe simtest           модель хука внутри процесса: клавиши -> текст\n\
          z.exe simarticle        статья 3dnews через модель хука, без окон\n\
          z.exe simall            все собранные статьи (3dnews/ixbt) через модель хука\n\
          z.exe simkeys <клавиши> набрать физические клавиши в модель хука\n\
           z.exe wintest           совместимость: хук на реальном EDIT-окне (19 тестов)\n\
          z.exe article           реальный тест: многоязычная статья через хук\n\
          z.exe peer \"<текст>\"    набрать текст в своём окне без хука (для теста\n\
                                     с резидентным процессом, который держит хук)\n\
          z.exe sanity            сравнение с эталоном на 4000+ словах\n\
          z.exe check <текст>     показать, что движок сделал бы с этими словами\n\
          z.exe score <текст>     внутренние оценки (ru/en) для каждого токена\n\
          z.exe variants <текст>  варианты прочтения неоднозначных ключей\n\
          z.exe selftest          прогнать встроенные проверочные случаи\n\
          z.exe stats             статистику языковой модели\n\
          z.exe probe             раскладки + проверка Alt+Shift на своём окне\n\
          z.exe tgprobe           найти окна Telegram Desktop и снять снимок\n\
           z.exe tgshot <path>     снимок окна Telegram в BMP\n\
           z.exe tgcomposer <path> снимок полосы ввода Telegram\n\
           z.exe tgtest            тесты хука прямо в поле ввода «Избранное»\n\
           z.exe tgclear           очистить поле ввода Telegram\n\
           z.exe autostart on|off  запуск при входе в Windows (или install.ps1)\n\
          z.exe config            текущий конфигурационный JSON и его путь\n\
          z.exe log               путь к журналу переключений\n\
           z.exe dict                слова словаря, которые движок не трогает\n\
           z.exe log clear           очистить журнал переключений\n\
          z.exe settings          окно настроек: процессы, папки, лог переключений\n\
          z.exe techtest          восстановление всех терминов тех. словаря\n\
           z.exe corpus            точность движка на тексте собранных статей\n\
           z.exe help              эта справка\n\
        \n\
        значок в трее: RU или EN показывает раскладку,\n\
        белый текст RU/EN — автоисправление работает, контурный — выключено.\n\
        левый клик — включить/выключить, правый — меню: Автосправление раскладки,\n\
        Автозапуск при входе, Настройки, Выход. Настройки — окно с вкладками: Процессы, Папка, Лог переключений.\n\
        \n\
        хоткеи настраиваются в config.json:\n\
          Alt          — отменить последнее исправление\n\
          Break        — сменить раскладку выделенного текста; если выделения нет —\n\
                        последнего слова до пробела или разделителя (дефис входит\n\
                        в слово: \"wi-fi\" — одно слово)\n\
          Ctrl+Break   — перевести выделенный текст в верхний регистр\n\
          Shift+Break  — поменять регистр выделения на противоположный\n\
          Ctrl+Alt+P   — включить/выключить автопереключение\n\
          ЭприветЭ     — кавычки: Э на клавише \" -> \" (ЭприветЭ -> \"привет\")\n\
        \n\
        исключения: exclude_processes — по имени процесса, exclude_dirs — по папке,\n\
        из которой запущена программа (например \"c:\\games\").\n\
        exclude add <процесс>          добавить процесс в исключения\n\
        exclude add-dir <папка>        добавить папку в исключения\n\
        exclude remove <процесс>       удалить процесс из исключений\n\
        exclude remove-dir <папка>     удалить папку из исключений\n\
        exclude list                   показать исключения\n\
        exclude clear                  сбросить исключения\n\
        автозапуск: autostart в config.json или меню значка в трее.\n\
        меню значка: \"Автосправление раскладки\", \"Автозапуск при входе\", \"Настройки\", \"Выход\".\n\
        Настройки открывают окна: Процессы, Папка, Лог переключений.\n\
        \n\
        переменные окружения:\n\
          ZYA_VERBOSE=1   подробный вывод решений в консоль\n"
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
                } else {
                    println!("zя: {word} уже в словаре");
                }
            }
            "remove" | "del" => {
                let word = args.get(2).cloned().unwrap_or_default();
                if win::remove_dictionary_word(&word) {
                    println!("zя: {word} удалено из словаря");
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
    if cmd == "settings" || cmd == "настройки" {
        settings::open_blocking();
        return Ok(());
    }
    if cmd == "settest" {
        settings::self_test();
        return Ok(());
    }
    if cmd == "pickprobe" {
        settings::pick_probe();
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
            if cfg.autostart != autostart::enabled() {
                autostart::sync_with_config(cfg.autostart);
            }
            startup_log("zя: запуск рабочего режима");

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
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                    println!(
                        "zя: время вышло, переключений: {}, событий хука: {}",
                        win::switch_count(),
                        win::event_count()
                    );
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
