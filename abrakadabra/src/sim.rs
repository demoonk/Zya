//! Офлайн-модель хука: та же машина состояний, что в `win.rs` (токен, флаг
//! идентификатора, смена раскладки после исправления), но без окон, `SendInput`
//! и фокуса. Алгоритм перевода раскладки проверяется здесь; реальные окна
//! (`wintest`, `tgtest`) остаются только как проверка совместимости.
//!
//! В хук приходят *физические* клавиши. Кто хочет набрать «привет» при
//! английской раскладке, жмёт `g h b d t n`. Поэтому вход теста — цепочка
//! физических ключей: `keys_of("привет")` = "ghbdtn".

use crate::engine::Engine;
use crate::layout::{
    is_ambiguous, is_boundary_trigger, is_cyrillic, is_latin, is_latin_boundary_letter, map_key,
};

#[derive(Debug, Clone)]
pub struct Sim {
    /// Раскладка, которую отслеживает хук.
    russian: bool,
    /// Автоисправление включено.
    auto: bool,
    /// Текущий незакрытый токен.
    word: String,
    /// В токене уже появилась цифра или символ идентификатора.
    identifier: bool,
    /// Токен, закрытый последним: нужен правилам про «b» и пунктуацию.
    prev: String,
    /// Текст, который оказался в поле ввода.
    pub out: String,
    pub switches: usize,
    /// (исправленное, результат) по каждому переключению.
    pub log: Vec<(String, String)>,
    /// Токен, закрытый последним, вместе с клавишей-границей: на него работает
    /// жест Break.
    closed: Option<(String, Option<char>)>,
    /// Последнее ручное переключение: (до, после, граница, раскладка до). Второй
    /// Break возвращает текст дословно, без нового зеркального прохода.
    manual: Option<(String, String, Option<char>, bool)>,
}

fn is_code_char(c: char) -> bool {
    matches!(
        c,
        '_' | '/'
            | '\\'
            | ':'
            | '#'
            | '@'
            | '='
            | '*'
            | '{'
            | '}'
            | '<'
            | '>'
            | '$'
            | '%'
            | '^'
            | '|'
            | '~'
            | '`'
    )
}

impl Sim {
    pub fn new(english: bool) -> Self {
        Sim {
            russian: !english,
            auto: true,
            word: String::new(),
            identifier: false,
            prev: String::new(),
            out: String::new(),
            switches: 0,
            log: Vec::new(),
            closed: None,
            manual: None,
        }
    }

    pub fn set_auto(&mut self, on: bool) {
        self.auto = on;
    }

    pub fn layout_is_russian(&self) -> bool {
        self.russian
    }

    /// Символ, который физическая клавиша `key` (название по английской
    /// раскладке) даёт при текущей раскладке.
    fn glyph(&self, key: char) -> char {
        if self.russian {
            map_key(key).unwrap_or(key)
        } else {
            key
        }
    }

    /// Одна физическая клавиша (Shift уже применён: `D`, `?`, `+`).
    pub fn press(&mut self, engine: &Engine, key: char) {
        let c = self.glyph(key);

        // Запрошенные границы слова: `? & / ^` закрывают буфер, как пробел.
        // `б`/`ю` закрывают его только после латинского слова: внутри русского
        // слова это обычные буквы.
        let latin_boundary = is_latin_boundary_letter(c) && self.word.chars().any(is_latin);
        if is_boundary_trigger(c) || latin_boundary {
            self.identifier = false;
            if self.word.is_empty() {
                self.out.push(c);
                return;
            }
            let word = std::mem::take(&mut self.word);
            self.close_word(engine, &word, c);
            return;
        }

        // Буквы и «двухзначные» ключи (б ю ж э х ъ) продолжают слово.
        if c.is_alphabetic() || is_ambiguous(c) {
            self.word.push(c);
            self.out.push(c);
            return;
        }

        // Дефис внутри слова не разделитель: "wi-fi" — одно слово.
        if c == '-' && !self.word.is_empty() {
            self.word.push('-');
            self.out.push(c);
            return;
        }

        // Цифра или разделитель идентификатора: токен закрывается только
        // пробелом или настоящей пунктуацией.
        if !c.is_whitespace() && (c.is_ascii_digit() || is_code_char(c)) {
            self.identifier = true;
            if !self.word.is_empty() {
                self.word.push(c);
            }
            self.out.push(c);
            return;
        }
        if c.is_whitespace() {
            self.identifier = false;
        }

        if self.word.is_empty() {
            self.out.push(c);
            return;
        }
        let word = std::mem::take(&mut self.word);
        self.close_word(engine, &word, c);
    }

    /// Закрыть текущее слово на символе-границе `c` и решить автозамену.
    fn close_word(&mut self, engine: &Engine, word: &str, c: char) {
        if !self.auto {
            self.out.push(c);
            self.prev = word.to_string();
            self.closed = Some((word.to_string(), Some(c)));
            return;
        }
        let prev = self.prev.clone();
        match engine.decide_with(word, &prev) {
            Some(d) => {
                self.switches += 1;
                self.log.push((word.to_string(), d.converted.clone()));
                // Хук стирает исходный токен, печатает зеркальную форму и
                // перепечатывает клавишу границы: после смены раскладки она дала
                // бы другой символ.
                for _ in 0..word.chars().count() {
                    self.out.pop();
                }
                self.out.push_str(&d.converted);
                if !c.is_control() {
                    // Как и хук: на границе раскладки пишется знак целевой
                    // раскладки (б/ю -> ,/. при переходе RU -> EN).
                    if d.flip && is_ambiguous(c) {
                        self.out.push(map_key(c).unwrap_or(c));
                    } else {
                        self.out.push(c);
                    }
                }
                if d.flip {
                    self.russian = !self.russian;
                }
                self.prev = d.converted.clone();
                self.closed = Some((d.converted.clone(), Some(c)));
            }
            None => {
                self.out.push(c);
                self.prev = word.to_string();
                self.closed = Some((word.to_string(), Some(c)));
            }
        }
    }

    /// Нажатие Break: переключить раскладку последнего слова. Второе нажатие на
    /// то же слово возвращает текст, который был до первого нажатия — дословно,
    /// без ещё одного прохода через зеркальную таблицу.
    pub fn press_break(&mut self, engine: &Engine) {
        let Some(mv) = crate::layout::plan_manual_move(
            &self.word,
            self.closed.clone(),
            &self.manual,
            self.russian,
            engine.cfg.smart_undo,
            &|s| engine.believable_word(s),
        ) else {
            return;
        };
        if mv.boundary.is_some() {
            self.out.pop();
        }
        for _ in 0..mv.from.chars().count() {
            self.out.pop();
        }
        self.out.push_str(&mv.to);
        if let Some(b) = mv.boundary {
            self.out.push(b);
        }
        self.russian = mv.russian_after;
        self.word.clear();
        self.closed = Some((mv.to.clone(), mv.boundary));
        self.prev = mv.to.clone();
        self.manual = mv.undo.clone();
        self.switches += 1;
        self.log.push((mv.from.clone(), mv.to.clone()));
    }

    pub fn press_text(&mut self, engine: &Engine, keys: &str) {
        for k in keys.chars() {
            self.press(engine, k);
        }
    }

    pub fn identifier_seen(&self) -> bool {
        self.identifier
    }

    /// Сброс при смене активного окна, как в `win::start_focus_guard`.
    #[allow(dead_code)]
    pub fn reset_buffer(&mut self) {
        self.word.clear();
        self.prev.clear();
        self.identifier = false;
        self.out.clear();
    }
}

/// Разбить текст на «слова + граница»: то, что пользователь видит как одно слово
/// и что закрывается одним нажатием.
fn tokens_of(text: &str) -> Vec<(String, char)> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() || c == '-' || c == '\'' || is_ambiguous(c) {
            word.push(c);
        } else if !word.is_empty() {
            out.push((std::mem::take(&mut word), c));
        } else {
            out.push((String::new(), c));
        }
    }
    if !word.is_empty() {
        out.push((word, ' '));
    }
    out
}

/// Исход теста: набор без вмешательства пользователя.
fn plain_sim(engine: &Engine, want: &str) -> Sim {
    let mut sim = Sim::new(true);
    sim.press_text(engine, &format!("{} ", keys_of(want)));
    sim
}

fn kinds_line(kinds: &std::collections::HashMap<&'static str, usize>) -> String {
    let mut list: Vec<(&'static str, usize)> = kinds.iter().map(|(k, v)| (*k, *v)).collect();
    list.sort_by_key(|x| std::cmp::Reverse(x.1));
    list.iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Тест «как печатает человек»: после каждого слова смотрим, что получилось.
/// Если слово неверное — жмём Break. Если после Break всё ещё неверно — жмём
/// Break ещё раз (переключатель должен вернуть исходный текст).
struct UserRun {
    sim: Sim,
    break_presses: usize,
    noticed: usize,
    fixed_by_one: usize,
    fixed_by_two: usize,
    returned_to_start: usize,
    still_wrong: usize,
    closer_but_wrong: usize,
    farther: usize,
    cases: Vec<(String, String, String, String, &'static str)>,
}

fn user_sim(engine: &Engine, want: &str) -> UserRun {
    let sim = Sim::new(true);
    let mut run = UserRun {
        sim,
        break_presses: 0,
        noticed: 0,
        fixed_by_one: 0,
        fixed_by_two: 0,
        returned_to_start: 0,
        still_wrong: 0,
        closer_but_wrong: 0,
        farther: 0,
        cases: Vec::new(),
    };
    for (word, boundary) in tokens_of(want) {
        if word.is_empty() {
            run.sim.press(engine, boundary);
            continue;
        }
        let start = run.sim.out.chars().count();
        run.sim.press_text(engine, &keys_of(&word));
        run.sim.press(engine, boundary);
        let expect = format!("{word}{boundary}");
        let typed: String = run.sim.out.chars().skip(start).collect();
        if typed == expect {
            continue;
        }
        run.noticed += 1;
        run.break_presses += 1;
        run.sim.press_break(engine);
        let after1: String = run.sim.out.chars().skip(start).collect();
        if after1 == expect {
            run.fixed_by_one += 1;
            run.cases.push((expect.clone(), typed, after1, String::new(), "помог один Break"));
            continue;
        }
        run.break_presses += 1;
        run.sim.press_break(engine);
        let after2: String = run.sim.out.chars().skip(start).collect();
        let before = distance(&typed, &expect);
        let now = distance(&after1, &expect);
        if now < before {
            run.closer_but_wrong += 1;
        } else if now > before {
            run.farther += 1;
        }
        if after2 == expect {
            run.fixed_by_two += 1;
            run.cases.push((expect, typed, after1, after2, "помог второй Break"));
        } else if after2 == typed {
            run.returned_to_start += 1;
            run.cases.push((expect, typed, after1, after2, "второй Break вернул исходное"));
        } else {
            run.still_wrong += 1;
            run.cases.push((expect, typed, after1, after2, "оба Break не помогли"));
        }
    }
    run
}

/// Levenshtein distance: how far the visible text is from what the user meant.
fn distance(a: &str, b: &str) -> usize {
    let x: Vec<char> = a.chars().collect();
    let y: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=y.len()).collect();
    let mut cur = vec![0usize; y.len() + 1];
    for i in 1..=x.len() {
        cur[0] = i;
        for j in 1..=y.len() {
            cur[j] = (prev[j] + 1)
                .min(cur[j - 1] + 1)
                .min(prev[j - 1] + (x[i - 1] != y[j - 1]) as usize);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[y.len()]
}

/// Счётчик совпадений с эталоном: (точные символы, ошибочные фрагменты, символы
/// в этих фрагментах).
fn score_against(want: &str, got: &str) -> (usize, usize, usize) {
    let w: Vec<char> = want.chars().collect();
    let g: Vec<char> = got.trim_end().chars().collect();
    let (mut exact, mut spans, mut chars) = (0usize, 0usize, 0usize);
    let mut i = 0usize;
    while i < w.len() {
        if g.get(i).copied() == Some(w[i]) {
            exact += 1;
            i += 1;
            continue;
        }
        let mut j = i;
        while j < w.len() && g.get(j).copied() != Some(w[j]) {
            j += 1;
        }
        spans += 1;
        chars += j - i;
        i = j;
    }
    (exact, spans, chars)
}

pub fn user_test() -> anyhow::Result<()> {
    let engine = Engine::new(crate::model::Model::load()?, crate::config::load());
    let dir = ["data/articles", "../data/articles"]
        .iter()
        .map(std::path::Path::new)
        .find(|d| d.is_dir())
        .ok_or_else(|| anyhow::anyhow!("статей нет: запусти tools/fetch_articles.py"))?;
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "txt") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(anyhow::anyhow!("no article files"));
    }

    let mut windows = 0usize;
    let mut chars = 0usize;
    let (mut p_exact, mut p_spans, mut p_chars) = (0usize, 0usize, 0usize);
    let (mut u_exact, mut u_spans, mut u_chars) = (0usize, 0usize, 0usize);
    let mut presses = 0usize;
    let mut noticed = 0usize;
    let mut fixed1 = 0usize;
    let mut fixed2 = 0usize;
    let mut returned = 0usize;
    let mut still = 0usize;
    let mut closer = 0usize;
    let mut farther = 0usize;
    let mut all_cases: Vec<(String, String, String, String, &'static str)> = Vec::new();
    let mut plain_spans: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut user_spans: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut user_kinds: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let mut per_file: std::collections::BTreeMap<String, (usize, usize, usize, usize, usize)> = std::collections::BTreeMap::new();
    let mut plain_kinds: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();

    for f in &files {
        let body = clean_article(&std::fs::read_to_string(f)?);
        for w in article_windows(&body, 700) {
            if !window_is_testable(&w) {
                continue;
            }
            windows += 1;
            chars += w.chars().count();
            let file_name = f
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "?".to_string());

            let plain = plain_sim(&engine, &w);
            let (pe, ps, pc) = score_against(&w, &plain.out);
            p_exact += pe;
            p_spans += ps;
            p_chars += pc;
            let want: Vec<char> = w.chars().collect();
            let got: Vec<char> = plain.out.trim_end().chars().collect();
            let mut i = 0usize;
            while i < want.len() {
                if got.get(i).copied() == Some(want[i]) {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < want.len() && got.get(j).copied() != Some(want[j]) {
                    j += 1;
                }
                let a: String = want[i..j].iter().collect();
                let b: String = if i < got.len() {
                    got[i..j.min(got.len())].iter().collect()
                } else {
                    String::new()
                };
                *plain_spans.entry(format!("{a} -> {b}")).or_insert(0) += 1;
                *plain_kinds.entry(mistake_kind(&a, &b)).or_insert(0) += 1;
                i = j;
            }

            let run = user_sim(&engine, &w);
            let (ue, us, uc) = score_against(&w, &run.sim.out);
            let entry = per_file
                .entry(file_name.clone())
                .or_insert((0usize, 0usize, 0usize, 0usize, 0usize));
            entry.0 += w.chars().count();
            entry.1 += pc;
            entry.2 += uc;
            entry.3 += run.noticed;
            entry.4 += run.break_presses;
            u_exact += ue;
            u_spans += us;
            u_chars += uc;
            presses += run.break_presses;
            noticed += run.noticed;
            fixed1 += run.fixed_by_one;
            fixed2 += run.fixed_by_two;
            returned += run.returned_to_start;
            still += run.still_wrong;
            closer += run.closer_but_wrong;
            farther += run.farther;
            for c in run.cases {
                all_cases.push(c);
            }
            let want: Vec<char> = w.chars().collect();
            let got: Vec<char> = run.sim.out.trim_end().chars().collect();
            let mut i = 0usize;
            while i < want.len() {
                if got.get(i).copied() == Some(want[i]) {
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < want.len() && got.get(j).copied() != Some(want[j]) {
                    j += 1;
                }
                let a: String = want[i..j].iter().collect();
                let b: String = if i < got.len() {
                    got[i..j.min(got.len())].iter().collect()
                } else {
                    String::new()
                };
                *user_spans.entry(format!("{a} -> {b}")).or_insert(0) += 1;
                *user_kinds.entry(mistake_kind(&a, &b)).or_insert(0) += 1;
                i = j;
            }
        }
    }

    println!(
        "тест: {} файлов, {} окон, {} символов",
        files.len(),
        windows,
        chars
    );
    println!(
        "1) только автоисправление: точно {}/{} = {:.1} %, ошибочных фрагментов: {} ({} символов), уникальных: {}",
        p_exact,
        chars,
        100.0 * p_exact as f64 / chars.max(1) as f64,
        p_spans,
        p_chars,
        plain_spans.len()
    );
    println!(
        "2) пользователь жмёт Break: точно {}/{} = {:.1} %, ошибочных фрагментов: {} ({} символов), уникальных: {}",
        u_exact,
        chars,
        100.0 * u_exact as f64 / chars.max(1) as f64,
        u_spans,
        u_chars,
        user_spans.len()
    );
    println!(
        "пользователь заметил ошибок: {noticed}, нажатий Break: {presses}; помог один: {fixed1}, помог второй: {fixed2}, второй вернул исходное: {returned}, не помогли: {still}"
    );
    println!(
        "после первого Break: стало ближе к правильному (но всё ещё неверно): {closer}, стало дальше: {farther}"
    );
    let dump: Vec<String> = all_cases
        .iter()
        .map(|(w, a, b, c, k)| format!("{k}\t{w}\t{a}\t{b}\t{c}"))
        .collect();
    std::fs::write("usertest_cases.txt", dump.join("\n"))?;
    println!("подробный разбор {noticed} случаев записан в usertest_cases.txt");

    println!("по типам ошибок без Break: {}", kinds_line(&plain_kinds));
    println!("по типам ошибок после Break: {}", kinds_line(&user_kinds));
    println!("\nпо статьям:");
    for (name, &(n, pc, uc, no, pr)) in &per_file {
        let saved = pc.saturating_sub(uc);
        println!(
            "  {name}: {n} симв., без Break {pc} ошиб. симв., после Break {uc} (исправлено {saved}), замечено {no}, Break нажат {pr} раз"
        );
    }

    let mut plain_list: Vec<(String, usize)> = plain_spans.into_iter().collect();
    plain_list.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("\nуникальные ошибки без Break (первые 40):");
    for (s, n) in plain_list.iter().take(40) {
        println!("  x{n} {s}");
    }
    let mut user_list: Vec<(String, usize)> = user_spans.into_iter().collect();
    user_list.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("\nуникальные ошибки после Break (первые 40):");
    for (s, n) in user_list.iter().take(40) {
        println!("  x{n} {s}");
    }

    println!("\nчто именно сделал Break:");
    for kind in [
        "помог один Break",
        "второй Break вернул исходное",
        "помог второй Break",
        "оба Break не помогли",
    ] {
        let group: Vec<(String, String, String, String)> = all_cases
            .iter()
            .filter(|c| c.4 == kind)
            .map(|c| (c.0.clone(), c.1.clone(), c.2.clone(), c.3.clone()))
            .collect();
        println!("  {kind}: {}", group.len());
        for (want, a, b, c) in group.iter().take(15) {
            if c.is_empty() {
                println!("    надо «{want}» было «{a}» -> Break -> «{b}»");
            } else {
                println!("    надо «{want}» было «{a}» -> Break -> «{b}» -> Break -> «{c}»");
            }
        }
    }
    Ok(())
}

/// Физические клавиши, которыми набирается `text`: кириллическая буква — это
/// клавиша с английским знаком в том же месте, латиница — сама клавиша.
pub fn keys_of(text: &str) -> String {    text.chars()
        .map(|c| {
            if is_cyrillic(c) {
                map_key(c).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// Набрать `keys` и вернуть (текст в поле ввода, число переключений).
pub fn type_keys(engine: &Engine, keys: &str, english: bool) -> (String, usize) {
    let mut sim = Sim::new(english);
    sim.press_text(engine, keys);
    (sim.out, sim.switches)
}

/// Текст `want` набран с раскладки `english`: проверка «нажал — получил».
fn run_case(engine: &Engine, want: &str, english: bool) -> (String, usize) {
    type_keys(engine, &keys_of(want), english)
}

pub fn sim_test() -> anyhow::Result<()> {
    let engine = Engine::new(crate::model::Model::load()?, crate::config::load());
    let mut ok = 0usize;
    let mut total = 0usize;

    // Русские слова, набранные на английской раскладке.
    for want in [
        "привет ",
        "работа ",
        "это тест ",
        "свобода ",
        "привет мир ",
        " hello ",
        "wi-fi ",
        "ddr4 nvme oled ",
    ] {
        total += 1;
        let (got, sw) = run_case(&engine, want, true);
        if got == want {
            ok += 1;
            println!(
                "OK  : {:>14} -> {want:?} ({sw} переключений)",
                keys_of(want)
            );
        } else {
            println!(
                "FAIL: {:>14} -> {got:?} (ожидалось {want:?})",
                keys_of(want)
            );
        }
    }

    // Английский текст при русской раскладке: зеркальные ключи дают кириллицу,
    // хук должен вернуть латинницу.
    for want in ["hello world ", "привет мир ", "Huawei S20 "] {
        total += 1;
        let (got, sw) = run_case(&engine, want, false);
        if got == want {
            ok += 1;
            println!(
                "OK  : {:>14} -> {want:?} ({sw} переключений, RU старт)",
                keys_of(want)
            );
        } else {
            println!(
                "FAIL: {:>14} -> {got:?} (ожидалось {want:?})",
                keys_of(want)
            );
        }
    }

    // Идентификаторы, файлы и camelCase не трогаются ни в одну сторону.
    for keys in [
        "ghbdtn.com ",
        "ghbdtn123 ",
        "my_file ",
        "ghbdTn ",
        "https://3dnews.ru/1234 ",
    ] {
        total += 1;
        let (got, _) = type_keys(&engine, keys, true);
        if got == keys {
            ok += 1;
            println!("OK  : {keys:?} защищён");
        } else {
            println!("FAIL: {keys:?} -> {got:?}");
        }
    }

    // Break — переключатель. Первое нажатие зеркалит последнее слово, второе на
    // том же слове возвращает текст, который был до первого нажатия.
    {
        total += 1;
        let mut sim = Sim::new(true);
        sim.press_text(&engine, &keys_of("привет "));
        let fixed = sim.out.clone();
        sim.press_break(&engine);
        let mirrored = sim.out.clone();
        sim.press_break(&engine);
        let restored = sim.out.clone();
        if fixed == "привет " && mirrored == "ghbdtn " && restored == "привет " {
            ok += 1;
            println!("OK  : Break дважды: {fixed:?} -> {mirrored:?} -> {restored:?}");
        } else {
            println!("FAIL: Break дважды: {fixed:?} -> {mirrored:?} -> {restored:?}");
        }
    }

    // Слово, уже исправленное автоматикой ("фактбю" -> "факт,."), при втором
    // Break должно вернуться дословно, а не конвертироваться ещё раз.
    {
        total += 1;
        let mut sim = Sim::new(false);
        sim.press_text(&engine, "afrn,. ");
        let fixed = sim.out.clone();
        sim.press_break(&engine);
        let mirrored = sim.out.clone();
        sim.press_break(&engine);
        let restored = sim.out.clone();
        if fixed == "факт,. " && mirrored == "afrnбю " && restored == "факт,. " {
            ok += 1;
            println!("OK  : Break дважды над исправленным: {fixed:?} -> {mirrored:?} -> {restored:?}");
        } else {
            println!(
                "FAIL: Break дважды над исправленным: {fixed:?} -> {mirrored:?} -> {restored:?}"
            );
        }
    }

    // Break после нового слова работает как обычный переключатель.
    {
        total += 1;
        let mut sim = Sim::new(true);
        sim.press_text(&engine, &keys_of("привет мир "));
        sim.press_break(&engine);
        let after = sim.out.clone();
        sim.press_break(&engine);
        if after == "привет vbh " && sim.out == "привет мир " {
            ok += 1;
            println!("OK  : Break по новому слову: {after:?} -> {:?}", sim.out);
        } else {
            println!("FAIL: Break по новому слову: {after:?} -> {:?}", sim.out);
        }
    }

    // Цифры: при русской раскладке "6,1" даёт токен "6б1".
    total += 1;
    let mut sim = Sim::new(false);
    sim.press_text(&engine, &keys_of("6,1"));
    let code = sim.identifier_seen();
    sim.press_text(&engine, &keys_of(" дюйма 1000; ограней, телефон"));
    if sim.out == "6,1 дюйма 1000; ограней, телефон"
        && sim.switches == 3
        && sim.layout_is_russian()
        && code
    {
        ok += 1;
        println!("OK  : цифры и пунктуация -> {:?}", sim.out);
    } else {
        println!("FAIL: {:?} -> {:?}", keys_of("6,1 дюйма"), sim.out);
    }

    // Однобуквенные слова (min_len 3) не исправляются.
    total += 1;
    let (got, _) = type_keys(&engine, "b ", true);
    if got == "b " {
        ok += 1;
        println!("OK  : одиночная клавиша не тронута");
    } else {
        println!("FAIL: \"b \" -> {got:?}");
    }

    // Пунктуация, набранная, когда раскладка ещё русская: `,` `.` `;` дают
    // б/ю/ж. Раскладка при этом не меняется.
    for want in ["факт, ", "сказка; ", "проект: ", "того, "] {
        total += 1;
        let keys = keys_of(want);
        let (got, _) = type_keys(&engine, &keys, false);
        if got == want {
            ok += 1;
            println!("OK  : {keys:?} -> {want:?}");
        } else {
            println!("FAIL: {keys:?} -> {got:?} (ожидалось {want:?})");
        }
    }

    // Термин, склеенный дефисом с русским словом: "OLED-дисплей" при русской
    // раскладке приходит как "ЩДУВ-дисплей". Исправляется только латинская
    // половина, раскладка остаётся русской.
    total += 1;
    let mut sim = Sim::new(false);
    sim.press_text(&engine, &keys_of("OLED-дисплей мини-плеер "));
    if sim.out == "OLED-дисплей мини-плеер " && sim.layout_is_russian() && sim.switches == 1
    {
        ok += 1;
        println!("OK  : склейка дефисом -> {:?}", sim.out);
    } else {
        println!(
            "FAIL: {:?} -> {:?} (ожидалось \"OLED-дисплей мини-плеер \")",
            keys_of("OLED-дисплей мини-плеер "),
            sim.out
        );
    }

    // Настоящее слово на б/ю/ж остаётся как есть.
    total += 1;
    let (got, _) = type_keys(&engine, &keys_of("клуб "), false);
    if got == "клуб " {
        ok += 1;
        println!("OK  : \"клуб\" не испорчено");
    } else {
        println!("FAIL: {:?} -> {got:?}", keys_of("клуб "));
    }

    // Одиночная `b` = союз «и», но только в русском или числовом контексте.
    total += 1;
    let (got, _) = type_keys(&engine, "p30 b p40 ", true);
    if got.starts_with("p30 и ") {
        ok += 1;
        println!("OK  : \"p30 b p40\" -> {got:?}");
    } else {
        println!("FAIL: p30 b p40 -> {got:?}");
    }
    total += 1;
    let (got, _) = type_keys(&engine, "variants a b c d ", true);
    if got == "variants a b c d " {
        ok += 1;
        println!("OK  : перечисление \"a b c d\" не тронуто");
    } else {
        println!("FAIL: variants a b c d -> {got:?}");
    }

    // Выключённое авторежим ничего не меняет.
    total += 1;
    let mut sim = Sim::new(true);
    sim.set_auto(false);
    sim.press_text(&engine, &keys_of("привет "));
    if sim.out == "ghbdtn " {
        ok += 1;
        println!("OK  : автоисправление выключено -> {:?}", sim.out);
    } else {
        println!("FAIL: автоисправление сработало при выключенном авторежиме");
    }

    println!("симуляция набора: {ok}/{total}");
    if ok < total {
        return Err(anyhow::anyhow!("layout simulation tests failed"));
    }
    Ok(())
}

pub fn sim_article() -> anyhow::Result<()> {
    let engine = Engine::new(crate::model::Model::load()?, crate::config::load());
    let dir = std::path::Path::new("data");
    let typed = std::fs::read_to_string(dir.join("article_typed.txt"))?;
    let expected = std::fs::read_to_string(dir.join("article_expected.txt"))?;
    let typed = typed.trim_end();
    let want: Vec<char> = expected.trim_end().chars().collect();

    // The saved text ends without a boundary key, so the very last token would
    // never reach the engine. A real writer always closes a word with a space,
    // a period or Enter, so the simulation adds that boundary.
    let typed = format!("{typed} ");

    let mut sim = Sim::new(true);
    sim.press_text(&engine, &typed);
    let (got, switches) = (sim.out.trim_end().to_string(), sim.switches);
    if crate::win::verbose() {
        for (w, c) in &sim.log {
            println!("  fix: {w} -> {c}");
        }
    }
    let have: Vec<char> = got.chars().collect();
    let lcs = lcs_len(&want, &have);
    let mut same = 0usize;
    let mut diffs = Vec::new();
    for (i, &a) in want.iter().enumerate() {
        let b = have.get(i).copied();
        if b == Some(a) {
            same += 1;
        } else if diffs.len() < 15 {
            diffs.push((i, a, b.unwrap_or('\0')));
        }
    }
    println!(
        "статья 3dnews: {} символов, {} исправлений, восстановлено {}/{} = {:.1} % (по позициям {}/{})",
        want.len(),
        switches,
        lcs,
        want.len(),
        lcs as f64 * 100.0 / want.len() as f64,
        same,
        want.len()
    );
    for (i, a, b) in &diffs {
        let l = i.saturating_sub(14);
        let left: String = want[l..*i].iter().collect();
        let r = (*i + 15).min(want.len());
        let right: String = want[*i + 1..r].iter().collect();
        println!("  позиция {i}: ожидалось {a:?} -> {b:?}   [{left}⟨{a}⟩{right}]");
    }
    if lcs * 100 < want.len() * 90 {
        return Err(anyhow::anyhow!("article reconstruction accuracy too low"));
    }
    println!("article simulation ok");
    Ok(())
}

/// Body text of a scraped article: navigation, ads and boilerplate lines are
/// dropped, the rest is collapsed into one paragraph.
/// Remove any remaining `&name;` / `&#123;` HTML entity the page left behind:
/// "&fnof;" is not something a person types, so it must not pollute the test.
fn strip_entities(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            let mut j = i + 1;
            while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '#') {
                j += 1;
            }
            if j > i + 1 && j < chars.len() && chars[j] == ';' {
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn clean_article(raw: &str) -> String {
    const JUNK: [&str; 16] = [
        "реклама",
        "вакансии",
        "опрос",
        "о сайте",
        "рассылка",
        "контакты",
        "новости hardware",
        "самое интересное",
        "материалы по теме",
        "источник",
        "постоянный url",
        "теги",
        "авторизация",
        "личный кабинет",
        "читайте также",
        "если вы заметили ошибку",
    ];
    const ENTITIES: [(&str, &str); 10] = [
        ("&mdash;", "—"),
        ("&ndash;", "–"),
        ("&laquo;", "«"),
        ("&raquo;", "»"),
        ("&nbsp;", " "),
        ("&amp;", "&"),
        ("&quot;", "\""),
        ("&ldquo;", "«"),
        ("&rdquo;", "»"),
        ("&#8211;", "–"),
    ];
    let mut out = String::new();
    for line in raw.lines() {
        let mut l = line.trim().to_string();
        for (from, to) in ENTITIES {
            l = l.replace(from, to);
        }
        // Whatever else the page left as "&something;" is markup, not prose.
        l = strip_entities(&l);
        let low = l.to_lowercase();
        if l.chars().count() < 90 {
            continue;
        }
        if l.contains("//") || low.contains("javascript") || low.contains("cookie") {
            continue;
        }
        if low.contains("18+") || low.contains('©') || low.contains("выйти") {
            continue;
        }
        if JUNK.iter().any(|j| low.contains(j)) {
            continue;
        }
        // A line that is mostly digits is a table, not prose.
        let digits = l.chars().filter(|c| c.is_ascii_digit()).count();
        if digits * 3 > l.chars().count() {
            continue;
        }
        out.push_str(&l);
        out.push(' ');
    }
    let mut res = String::new();
    for c in out.chars() {
        if c.is_whitespace() {
            if !res.ends_with(' ') {
                res.push(' ');
            }
        } else {
            res.push(c);
        }
    }
    res.trim().to_string()
}

/// Cut the body into windows of about `size` characters on word boundaries: one
/// window is one "typing session" of the simulation.
fn article_windows(text: &str, size: usize) -> Vec<String> {
    let mut windows = Vec::new();
    let mut cur = String::new();
    for tok in text.split_whitespace() {
        if cur.chars().count() + tok.chars().count() + 1 > size && !cur.is_empty() {
            windows.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(tok);
    }
    if !cur.is_empty() {
        windows.push(cur);
    }
    windows
}

/// A window is worth simulating only when it really mixes two languages and
/// every character has a physical key in the mirrored layout. Capital Б/Ю/Ж/Э/
/// Х/Ъ/Ё come from the shifted layer of `,` `.` `;` `'` `[` `]` `` ` ``; the
/// mirror table has no entry for them, so such windows are counted separately.
fn window_is_testable(w: &str) -> bool {
    let (mut latin, mut cyr) = (0usize, 0usize);
    for tok in w.split_whitespace() {
        if tok.chars().any(crate::layout::is_cyrillic) {
            cyr += 1;
        } else if tok.chars().any(|c| c.is_ascii_alphabetic()) {
            latin += 1;
        }
    }
    if latin < 3 || cyr < 8 {
        return false;
    }
    !w.chars()
        .any(|c| matches!(c, 'Б' | 'Ю' | 'Ж' | 'Э' | 'Х' | 'Ъ' | 'Ё'))
}

/// The real algorithm test: every downloaded technical article is "typed" with
/// the layout in the wrong state, one physical key at a time, and the engine
/// has to reconstruct the original text.
pub fn sim_all_articles() -> anyhow::Result<()> {
    let engine = Engine::new(crate::model::Model::load()?, crate::config::load());
    let dir = ["data/articles", "../data/articles"]
        .iter()
        .map(std::path::Path::new)
        .find(|d| d.is_dir())
        .ok_or_else(|| anyhow::anyhow!("статей нет: запусти tools/fetch_articles.py"))?;

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "txt") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(anyhow::anyhow!("no article files in {dir:?}"));
    }

    let (mut chars, mut exact, mut fixes, mut wins, mut skipped) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut per_file: Vec<(String, usize, usize, usize, f64)> = Vec::new();
    let mut diffs: Vec<String> = Vec::new();
    let mut flips: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    let mut broken_spans: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for f in &files {
        let name = f
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let body = clean_article(&std::fs::read_to_string(f)?);
        let (mut fc, mut fe, mut ff, mut fw) = (0usize, 0usize, 0usize, 0usize);
        for w in article_windows(&body, 700) {
            if !window_is_testable(&w) {
                skipped += 1;
                continue;
            }
            fw += 1;
            wins += 1;
            let want: Vec<char> = w.chars().collect();
            let mut sim = Sim::new(true);
            sim.press_text(&engine, &format!("{} ", keys_of(&w)));
            let got: Vec<char> = sim.out.trim_end().chars().collect();
            chars += want.len();
            fc += want.len();
            let mut i = 0usize;
            while i < want.len() {
                if got.get(i).copied() == Some(want[i]) {
                    exact += 1;
                    fe += 1;
                    i += 1;
                    continue;
                }
                // One run of broken characters = one readable case.
                let mut j = i;
                while j < want.len() && got.get(j).copied() != Some(want[j]) {
                    j += 1;
                }
                let broken: String = want[i..j].iter().collect();
                let typed: String = if i < got.len() {
                    got[i..j.min(got.len())].iter().collect()
                } else {
                    String::new()
                };
                *broken_spans
                    .entry(format!("{broken} -> {typed}"))
                    .or_insert(0) += 1;
                if diffs.len() < 120 {
                    let l = i.saturating_sub(16);
                    let left: String = want[l..i].iter().collect();
                    let right: String = want[j..(j + 16).min(want.len())].iter().collect();
                    let gl = i.saturating_sub(16).min(got.len());
                    let got_left: String = got[gl..i.min(got.len())].iter().collect();
                    let got_right: String = got[j..(j + 16).min(got.len())].iter().collect();
                    diffs.push(format!(
                        "{name}: {left}⟨{broken}⟩{right} -> {got_left}⟨{typed}⟩{got_right}"
                    ));
                }
                i = j;
            }
            fixes += sim.switches;
            ff += sim.switches;
            for (wrd, conv) in &sim.log {
                *flips.entry(format!("{wrd} -> {conv}")).or_insert(0) += 1;
            }
        }
        let acc = 100.0 * fe as f64 / fc.max(1) as f64;
        per_file.push((name, fw, fc, ff, acc));
    }

    for (name, w, c, f, acc) in &per_file {
        println!("{name}: {w} окон, {c} символов, {f} исправлений, точно {acc:.1} %");
    }
    let acc = 100.0 * exact as f64 / chars.max(1) as f64;
    println!(
        "итого: {} файлов, {} окон, {} символов, {} исправлений, восстановлено {}/{} = {:.1} % (пропущено окон: {skipped})",
        files.len(),
        wins,
        chars,
        fixes,
        exact,
        chars,
        acc
    );
    for d in &diffs {
        println!("  DIFF {d}");
    }
    let mut span_list: Vec<(String, usize)> = broken_spans.into_iter().collect();
    span_list.sort_by_key(|x| std::cmp::Reverse(x.1));
    let lost: usize = span_list.iter().map(|(_, n)| n).sum();
    println!("не восстановлено фрагментов: {lost}");
    for (s, n) in span_list.iter().take(40) {
        println!("  MISS {s}  x{n}");
    }
    let mut flip_list: Vec<(String, usize)> = flips.into_iter().collect();
    flip_list.sort_by_key(|x| std::cmp::Reverse(x.1));
    println!("исправлений всего: {}", flip_list.len());
    for (f, n) in flip_list.iter().take(20) {
        println!("  {f}  x{n}");
    }
    if acc < 98.0 {
        return Err(anyhow::anyhow!("article typing accuracy below 98%"));
    }
    println!("прогон всех статей ok");
    Ok(())
}

/// What kind of mistake a broken fragment is: eaten punctuation, a letter that
/// the rules turned into punctuation, a missed or an extra layout flip.
fn mistake_kind(want: &str, got: &str) -> &'static str {
    let w: Vec<char> = want.chars().collect();
    let g: Vec<char> = got.chars().collect();
    if w.len() == 1 && g.len() == 1 {
        let (a, b) = (w[0], g[0]);
        if matches!(a, '.' | ',' | ';' | ':') && matches!(b, 'б' | 'ю' | 'ж' | 'Б' | 'Ю' | 'Ж') {
            return "пунктуация съедена буквой";
        }
        if matches!(b, '.' | ',' | ';' | ':') && matches!(a, 'б' | 'ю' | 'ж' | 'Б' | 'Ю' | 'Ж') {
            return "буква слова заменена знаком";
        }
        if is_cyrillic(a) && is_latin(b) {
            return "слово осталось латинским (не зеркалось)";
        }
        if is_latin(a) && is_cyrillic(b) {
            return "лишнее зеркальное исправление";
        }
    }
    if w.iter().all(|&c| is_cyrillic(c)) && g.iter().all(|&c| is_latin(c)) {
        return "пропущенное переключение раскладки";
    }
    if w.iter().all(|&c| is_latin(c)) && g.iter().all(|&c| is_cyrillic(c)) {
        return "излишнее переключение раскладки";
    }
    "другое"
}

/// Report on the saved technical corpus: every article is typed with the layout
/// in the wrong state, every mistake is grouped, each distinct one is explained
/// by the rule that let it through, and repeats are folded into a count.
pub fn error_report() -> anyhow::Result<()> {
    let engine = Engine::new(crate::model::Model::load()?, crate::config::load());
    let dir = ["data/articles", "../data/articles"]
        .iter()
        .map(std::path::Path::new)
        .find(|d| d.is_dir())
        .ok_or_else(|| anyhow::anyhow!("статей нет: запусти tools/fetch_articles.py"))?;

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "txt") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(anyhow::anyhow!("no article files in {dir:?}"));
    }

    struct Mistake {
        want: String,
        got: String,
        count: usize,
        kind: &'static str,
        place: String,
        places: Vec<String>,
        verdict: String,
        causes: std::collections::HashMap<&'static str, usize>,
        tokens: Vec<String>,
    }

    let mut seen: std::collections::HashMap<String, Mistake> = std::collections::HashMap::new();
    let mut kinds: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let mut causes: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let (mut chars, mut exact, mut fixes, mut wins, mut spans) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut lost_chars = 0usize;

    for f in &files {
        let name = f
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let body = clean_article(&std::fs::read_to_string(f)?);
        for w in article_windows(&body, 700) {
            if !window_is_testable(&w) {
                continue;
            }
            wins += 1;
            let want: Vec<char> = w.chars().collect();
            let mut sim = Sim::new(true);
            sim.press_text(&engine, &format!("{} ", keys_of(&w)));
            let got: Vec<char> = sim.out.trim_end().chars().collect();
            engine.take_notes();
            chars += want.len();
            fixes += sim.switches;

            let mut i = 0usize;
            while i < want.len() {
                if got.get(i).copied() == Some(want[i]) {
                    exact += 1;
                    i += 1;
                    continue;
                }
                let mut j = i;
                while j < want.len() && got.get(j).copied() != Some(want[j]) {
                    j += 1;
                }
                spans += 1;
                lost_chars += j - i;
                let broken: String = want[i..j].iter().collect();
                let typed: String = if i < got.len() {
                    got[i..j.min(got.len())].iter().collect()
                } else {
                    String::new()
                };

                // The word the engine really saw: the run of output characters
                // that ends where the text first went wrong.
                let mut s = i;
                while s > 0 && got[s - 1] != ' ' {
                    s -= 1;
                }
                let mut e = i;
                while e < got.len() && got[e] != ' ' {
                    e += 1;
                }
                let token: String = got[s..e].iter().collect();
                let decision = engine.decide_with(&token, "");
                let notes = engine.take_notes();
                let verdict = match &decision {
                    Some(d) => format!("исправлено: «{token}» -> «{}»", d.converted),
                    None if !notes.is_empty() => notes.join(" + "),
                    None => format!("«{token}» не распознан ни одним правилом"),
                };

                let kind = mistake_kind(&broken, &typed);
                let cause = cause_of(&verdict);
                *kinds.entry(kind).or_insert(0) += 1;
                *causes.entry(cause).or_insert(0) += 1;
                let entry = seen
                    .entry(format!("{broken} -> {typed}"))
                    .or_insert_with(|| Mistake {
                        want: broken.clone(),
                        got: typed.clone(),
                        count: 0,
                        kind,
                        place: String::new(),
                        places: Vec::new(),
                        verdict,
                        causes: std::collections::HashMap::new(),
                        tokens: Vec::new(),
                    });
                *entry.causes.entry(cause).or_insert(0) += 1;
                if entry.tokens.len() < 8 && !entry.tokens.iter().any(|t| t == &token) {
                    entry.tokens.push(token.clone());
                }
                entry.count += 1;
                let l = i.saturating_sub(14);
                let left: String = want[l..i].iter().collect();
                let right: String = want[j..(j + 14).min(want.len())].iter().collect();
                let place = format!("{name}:{i} «{left}[{broken}]{right}»");
                if entry.place.is_empty() {
                    entry.place = place.clone();
                } else if entry.place != place && entry.places.len() < 3 {
                    entry.places.push(place);
                }
                i = j;
            }
        }
    }

    let acc = 100.0 * exact as f64 / chars.max(1) as f64;
    println!(
        "тест набора технического текста: {} файлов, {} окон, {} символов",
        files.len(),
        wins,
        chars
    );
    println!(
        "исправлений: {fixes}, точно: {exact}/{chars} = {acc:.1} %, ошибок: {spans} фрагментов ({lost_chars} символов), уникальных: {}",
        seen.len()
    );

    let mut kind_list: Vec<(&'static str, usize)> = kinds.into_iter().collect();
    kind_list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("по типам:");
    for (k, n) in &kind_list {
        println!("  {k}: {n}");
    }

    let mut cause_list: Vec<(&'static str, usize)> = causes.into_iter().collect();
    cause_list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("по причинам:");
    for (c, n) in &cause_list {
        println!("  {c}: {n}");
    }

    let mut list: Vec<&Mistake> = seen.values().collect();
    list.sort_by_key(|m| (std::cmp::Reverse(m.count), m.kind, m.want.clone()));
    println!("уникальные ошибки (повторяющиеся объединены в счётчик):");
    for (n, m) in list.iter().enumerate() {
        println!(
            "  {}. x{} {want} -> {got}   [{kind}]",
            n + 1,
            m.count,
            want = m.want,
            got = m.got,
            kind = m.kind
        );
        println!("     где: {}", m.place);
        for p in &m.places {
            println!("       также: {p}");
        }
        let mut cs: Vec<(&'static str, usize)> = m
            .causes
            .iter()
            .map(|(c, n)| (*c, *n))
            .collect();
        cs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let why: Vec<String> = cs
            .iter()
            .map(|(c, n)| format!("{c} ×{n}"))
            .collect();
        println!("     причина: {}", why.join("; "));
        println!("     токены: {}", m.tokens.join(", "));
        println!("     пример разбора движком: {}", m.verdict);
    }
    if acc < 98.0 {
        return Err(anyhow::anyhow!("article typing accuracy below 98%"));
    }
    println!("прогон ok");
    Ok(())
}

/// The rule that let a mistake through, in words a human can act on.
fn cause_of(verdict: &str) -> &'static str {
    if verdict.starts_with("исправлено") {
        return "исправлено, но не до конца (каскад из-за предыдущей правки)";
    }
    if verdict.contains("не распознан") {
        return "токен не распознан ни одним правилом";
    }
    for (needle, cause) in [
        ("too short", "токен короче min_len (3) — движок его не судит"),
        ("looks like an identifier", "защита идентификаторов (protect_code)"),
        (
            "a real word of the layout in use",
            "форма есть в словаре той раскладки, что уже включена",
        ),
        (
            "the ending closes like a real Russian word",
            "окончание выглядит как окончание русского слова",
        ),
        (
            "is already a complete word",
            "основа до ключа — целое слово словаря",
        ),
        (
            "no mirror candidate survived",
            "ни один зеркальный кандидат не прошёл проверку",
        ),
        (
            "not believable as",
            "зеркальная форма неправдоподобна как слово языка",
        ),
        (
            "keeps a letter that is punctuation",
            "кандидат сохраняет букву там, где клавиша даёт пунктуацию",
        ),
        (
            "reads better as",
            "основа читается как слово другой раскладки",
        ),
        ("is only", "переход не уверенный: не хватает margin"),
        (
            "no context around a lone letter",
            "вокруг одной буквы нет слова — контекст неясен",
        ),
        (
            "is an English abbreviation and",
            "конфликт словарей: одни клавиши — слово обеих раскладок",
        ),
        (
            "a repeated letter is not a word",
            "повтор одной буквы, а не слово",
        ),
        (
            "the mirror keeps a key that is punctuation",
            "зеркальное чтение содержит клавишу пунктуации",
        ),
        ("known technical term", "термин из технического словаря"),
        ("on the whitelist", "слово в списке защиты пользователя"),
    ] {
        if verdict.contains(needle) {
            return cause;
        }
    }
    "причина не определена"
}

fn lcs_len(a: &[char], b: &[char]) -> usize {
    let mut prev = vec![0usize; b.len() + 1];
    let mut cur = vec![0usize; b.len() + 1];
    for &x in a {
        for j in 1..=b.len() {
            cur[j] = if x == b[j - 1] {
                prev[j - 1] + 1
            } else {
                prev[j].max(cur[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(
            crate::model::Model::load().expect("language model"),
            crate::engine::Config::default(),
        )
    }

    #[test]
    fn the_classic_break_mirrors_back_when_smart_undo_is_off() {
        let mut cfg = crate::engine::Config::default();
        cfg.smart_undo = false;
        let engine = Engine::new(crate::model::Model::load().expect("language model"), cfg);
        let mut sim = Sim::new(true);
        sim.press_text(&engine, "привет ");
        let fixed = sim.out.clone();
        sim.press_break(&engine);
        let mirrored = sim.out.clone();
        sim.press_break(&engine);
        let back = sim.out.clone();
        assert_eq!(fixed, "привет ");
        assert_eq!(mirrored, "ghbdtn ");
        // Without the smart mode the second press mirrors the token again, which
        // is the classic behaviour and still returns the original text.
        assert_eq!(back, "привет ");
    }

    #[test]
    fn mistake_categories_are_named() {
        assert_eq!(mistake_kind(".", "ю"), "пунктуация съедена буквой");
        assert_eq!(mistake_kind("ю", "."), "буква слова заменена знаком");
        assert_eq!(
            mistake_kind("факт", "rfvjt"),
            "пропущенное переключение раскладки"
        );
        assert_eq!(
            mistake_kind("OLED", "щдув"),
            "излишнее переключение раскладки"
        );
        assert_eq!(mistake_kind("12", "13"), "другое");
    }

    #[test]
    fn the_error_report_runs_when_articles_are_saved() {
        if !std::path::Path::new("data/articles").is_dir()
            && !std::path::Path::new("../data/articles").is_dir()
        {
            return;
        }
        error_report().expect("the mistake report must run on the saved corpus");
    }

    #[test]
    fn the_user_test_runs_when_articles_are_saved() {
        if !std::path::Path::new("data/articles").is_dir()
            && !std::path::Path::new("../data/articles").is_dir()
        {
            return;
        }
        user_test().expect("the user test must run on the saved corpus");
    }

    #[test]
    fn a_word_is_split_from_its_boundary() {
        let units = tokens_of("привет мир,");
        assert_eq!(units[0], ("привет".to_string(), ' '));
        assert_eq!(units[1], ("мир,".to_string(), ' '));
    }

    #[test]
    fn the_user_test_notices_what_the_engine_missed_and_break_fixes_it() {
        let e = engine();
        let run = user_sim(&e, "мы ");
        assert_eq!(run.noticed, 1, "`мы` stayed Latin because `vs` is an English word");
        assert_eq!(run.fixed_by_one, 1, "one Break is enough");
        assert_eq!(run.returned_to_start, 0);
        assert_eq!(run.sim.out, "мы ");
    }

    #[test]
    fn a_second_break_never_creates_a_third_state() {
        let e = engine();
        let run = user_sim(&e, "коронавируса. Да, лицо. тройную");
        assert!(!run.cases.is_empty(), "the sample must contain mistakes");
        for (want, before, after1, after2, _kind) in &run.cases {
            // One press fixed it (after2 is empty); otherwise the second press
            // must either repair the token or give back exactly what was there.
            let outcome = if after2.is_empty() { after1 } else { after2 };
            assert!(
                outcome == want || outcome == before,
                "{want}: the second press must restore or fix, not invent: {before} / {outcome}"
            );
        }
    }

    #[test]
    fn wrong_layout_word_is_fixed() {
        let (got, sw) = type_keys(&engine(), "ghbdtn ", true);
        assert_eq!(got, "привет ", "the mistyped word must be rewritten");
        assert_eq!(sw, 1, "exactly one switch");
    }

    #[test]
    fn layout_flips_after_a_fix() {
        let e = engine();
        let mut sim = Sim::new(true);
        sim.press_text(&e, "ghbdtn ");
        assert!(sim.layout_is_russian(), "layout flipped after the fix");
        sim.press(&e, 'b');
        assert_eq!(sim.out, "привет и", "the same key now types Cyrillic");
    }

    #[test]
    fn correct_text_is_untouched() {
        let (got, sw) = run_case(&engine(), "привет мир ", true);
        assert_eq!(got, "привет мир ");
        assert_eq!(sw, 1, "only the first word needed a switch");
    }

    #[test]
    fn code_tokens_are_protected() {
        for keys in ["ghbdtn.com ", "ghbdtn123 ", "my_file ", "ghbdTn "] {
            let (got, _) = type_keys(&engine(), keys, true);
            assert_eq!(got, keys, "{keys} must survive untouched");
        }
    }

    #[test]
    fn hyphen_is_part_of_the_word() {
        // "wi-fi" typed while the layout was Russian: the hyphen must not split
        // the token, so the whole thing is mirrored back.
        let (got, _) = type_keys(&engine(), "wi-fi ", false);
        assert_eq!(got, "wi-fi ");
    }

    #[test]
    fn mixed_harness_text_is_not_fixed_twice() {
        // The sentence the user actually types into the chat: Russian words with
        // Latin abbreviations and `!`, `,`, `.` mixed in.
        let e = engine();
        let want = "задавай вопросы по компам, выбору железа и сборкам. можно даже по серверам. кидать сборку на оценку. стрим идёт на YT, VK и Twitch!";
        let mut sim = Sim::new(true);
        sim.press_text(&e, &keys_of(want));

        let mut seen = std::collections::HashSet::new();
        for (token, _) in &sim.log {
            assert!(
                seen.insert(token.clone()),
                "token {token} was rewritten twice in one run"
            );
        }
        println!("mixed out: {}", sim.out);
        for (a, b) in &sim.log {
            println!("  switch: {a} -> {b}");
        }
    }

    #[test]
    fn article_round_trip_is_reproducible() {
        let e = engine();
        let typed = std::fs::read_to_string("data/article_typed.txt").unwrap();
        let typed = typed.trim_end();
        let first = type_keys(&e, typed, true);
        let second = type_keys(&e, typed, true);
        assert_eq!(first, second, "the simulation must be deterministic");
        assert!(first.1 > 10, "the article must exercise the engine");
    }

    /// `,` `.` `;` typed while the layout is still Russian come out as б/ю/ж.
    /// The fix restores the punctuation and does not touch the layout.
    #[test]
    fn trailing_punctuation_is_restored() {
        let e = engine();
        for want in ["факт, ", "сказка; ", "проект: ", "того, "] {
            let (got, _) = type_keys(&e, &keys_of(want), false);
            assert_eq!(got, want, "{want} typed with , . ; keys");
        }
    }

    #[test]
    fn technical_term_glued_by_hyphen_is_fixed_without_flipping() {
        let e = Engine::new(
            crate::model::Model::load().unwrap(),
            crate::engine::Config::default(),
        );
        let mut s = Sim::new(false);
        s.press_text(&e, &keys_of("OLED-дисплей мини-плеер "));
        assert_eq!(s.out, "OLED-дисплей мини-плеер ");
        assert!(s.layout_is_russian(), "the layout must stay Russian");
        assert_eq!(s.switches, 1, "only the glued technical term was fixed");
    }

    #[test]
    fn punctuation_fix_does_not_flip_the_layout() {
        let e = engine();
        let mut sim = Sim::new(false); // Russian layout
        sim.press_text(&e, &keys_of("факт, "));
        assert!(
            sim.layout_is_russian(),
            "a comma does not switch the layout"
        );
        assert_eq!(sim.out, "факт, ");
    }

    #[test]
    fn real_words_ending_in_those_letters_survive() {
        let e = engine();
        let (got, sw) = type_keys(&e, &keys_of("клуб "), false);
        assert_eq!(got, "клуб ", "клуб is a word, the letter stays");
        assert_eq!(sw, 0);
    }

    /// Запрошенные символы-границы `? & / б ю ^` закрывают слово, как пробел.
    #[test]
    fn asked_boundaries_trigger_the_fix() {
        let e = engine();
        for (typed, want) in [
            ("pfghtn&", "запрет&"),
            ("pfghtn?", "запрет?"),
            ("pfghtn/", "запрет/"),
            ("pfghtn^", "запрет^"),
        ] {
            let (got, sw) = type_keys(&e, typed, true);
            assert_eq!(got, want, "{typed}");
            assert_eq!(sw, 1, "{typed} must be fixed");
        }

        // `б` и `ю` — это знаки `,` и `.` при русской раскладке: сразу после
        // латинского слова они закрывают буфер и дают место запятой/точке.
        let (got, sw) = type_keys(&e, "rfrjq&nj", true);
        assert_eq!(got, "какой&то", "& closes the word like a space");
        assert_eq!(sw, 1, "никто keeps the buffer open");

        let mut sim = Sim::new(false);
        sim.press_text(&e, &keys_of("контекст"));
        assert_eq!(sim.out, "контекст");
        sim.press(&e, 'б');
        assert_eq!(sim.out, "контекст,", "б is the comma key");
        assert!(
            sim.layout_is_russian(),
            "a punctuation fix keeps the layout"
        );
    }

    /// Смена активного окна сбрасывает буфер: недописанное слово не склеивается
    /// с первым словом нового окна.
    #[test]
    fn focus_change_drops_the_pending_buffer() {
        let mut sim = Sim::new(true);
        sim.word.push_str("pfgh");
        assert!(!sim.word.is_empty());
        sim.reset_buffer();
        assert!(sim.word.is_empty(), "focus change must clear the buffer");
        assert!(sim.prev.is_empty());
        assert!(sim.out.is_empty());
    }

    #[test]
    fn lone_b_is_the_conjunction_only_in_context() {
        let e = engine();
        let (got, _) = type_keys(&e, "p30 b p40 ", true);
        assert!(got.starts_with("p30 и "), "got {got}");
        let (got, _) = type_keys(&e, "привет b тест ", true);
        assert_eq!(got, "привет и тест ", "got {got}");
        let (got, _) = type_keys(&e, "variants a b c d ", true);
        assert_eq!(got, "variants a b c d ", "a list of letters is not a typo");
    }

    /// Punctuation typed while the layout is still Russian arrives as a
    /// letter: "6б1" is "6,1", a lone ж after a number is ";", and "огнутыхб"
    /// is "огнутых,". None of these may switch the layout.
    #[test]
    fn punctuation_inside_numbers_and_after_words_is_restored() {
        let e = engine();
        let mut sim = Sim::new(false); // the layout is Russian
        sim.press_text(&e, &keys_of("6,1"));
        assert!(sim.identifier_seen(), "digits mark the token as code");
        sim.press_text(&e, &keys_of(" дюйма 1000; огнутых, тройную "));
        assert_eq!(
            sim.out, "6,1 дюйма 1000; огнутых, тройную ",
            "got {:?}",
            sim.out
        );
        assert_eq!(sim.switches, 3, "three punctuation fixes, no layout switch");
        assert!(
            sim.layout_is_russian(),
            "punctuation never switches the layout"
        );
    }

    /// «ё» lives on the backtick key: to type it in the Russian layout the user
    /// physically presses `, and the hook must read that as `ё`.
    #[test]
    fn physical_keys_of_yo_is_the_backtick() {
        assert_eq!(keys_of("запрещённый"), "pfghto`yysq");
        assert_eq!(keys_of("привет"), "ghbdtn");
    }

    #[test]
    fn backtick_key_restores_yo_without_a_switch() {
        let e = engine();
        let (got, sw) = type_keys(&e, "pfghto`yysq ", true);
        assert_eq!(got, "запрещённый ", "got {got}");
        assert_eq!(sw, 1);
    }

    /// While the layout is Russian the `.` key really writes `ю`. It becomes a
    /// dot only when the ending cannot belong to the word: "kjdl." is "Колю"
    /// and stays "Колю", "kjrd." is "клуб" + the dot key and becomes "клуб.".
    #[test]
    fn the_dot_key_is_a_letter_inside_a_russian_word() {
        let e = engine();
        let (got, sw) = run_case(&e, "Колю ", false);
        assert_eq!(got, "Колю ", "got {got}");
        assert_eq!(sw, 0, "the layout did not change, so nothing is fixed");
        let (got, sw) = run_case(&e, "клуб. ", false);
        assert_eq!(got, "клуб. ", "got {got}");
        assert_eq!(sw, 1);
        let (got, _) = run_case(&e, "факт. ", false);
        assert_eq!(got, "факт. ", "got {got}");
    }
}
