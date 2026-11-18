//! Офлайн-модель хука: та же машина состояний, что в `win.rs` (токен, флаг
//! идентификатора, смена раскладки после исправления), но без окон, `SendInput`
//! и фокуса. Алгоритм перевода раскладки проверяется здесь; реальные окна
//! (`wintest`, `tgtest`) остаются только как проверка совместимости.
//!
//! В хук приходят *физические* клавиши. Кто хочет набрать «привет» при
//! английской раскладке, жмёт `g h b d t n`. Поэтому вход теста — цепочка
//! физических ключей: `keys_of("привет")` = "ghbdtn".

use crate::engine::Engine;
use crate::layout::{is_ambiguous, is_cyrillic, map_key};

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
        if !self.auto {
            self.out.push(c);
            self.prev = word;
            return;
        }
        let prev = self.prev.clone();
        match engine.decide_with(&word, &prev) {
            Some(d) => {
                self.switches += 1;
                self.log.push((word.clone(), d.converted.clone()));
                // Хук стирает исходный токен, печатает зеркальную форму и
                // перепечатывает клавишу границы: после смены раскладки она дала
                // бы другой символ.
                for _ in 0..word.chars().count() {
                    self.out.pop();
                }
                self.out.push_str(&d.converted);
                if !c.is_control() {
                    self.out.push(c);
                }
                if d.flip {
                    self.russian = !self.russian;
                }
                self.prev = d.converted.clone();
            }
            None => {
                self.out.push(c);
                self.prev = word;
            }
        }
    }

    pub fn press_text(&mut self, engine: &Engine, keys: &str) {
        for k in keys.chars() {
            self.press(engine, k);
        }
    }

    pub fn identifier_seen(&self) -> bool {
        self.identifier
    }
}

/// Физические клавиши, которыми набирается `text`: кириллическая буква — это
/// клавиша с английским знаком в том же месте, латиница — сама клавиша.
pub fn keys_of(text: &str) -> String {
    text.chars()
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
                if diffs.len() < 30 {
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
}
