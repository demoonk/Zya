//! Decision layer: given the token that was just typed, decide whether it was
//! produced with the wrong keyboard layout.

use crate::layout::{is_ambiguous, map_key, script_of, Script};
use crate::model::Model;

pub const INVALID: f32 = -1000.0;

/// Tokens that look like code, URLs or file names are never rewritten: a real
/// identifier keeps its capitals, digits, slashes and extensions.
fn looks_like_code(token: &str) -> bool {
    let low = token.to_lowercase();
    if low.contains("http") || low.contains("www.") || low.contains("://") {
        return true;
    }
    if token.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    if token
        .chars()
        .any(|c| c == '/' || c == '\\' || c == ':' || c == '@' || c == '#' || c == '_')
    {
        return true;
    }
    const EXTENSIONS: [&str; 22] = [
        ".com", ".net", ".org", ".info", ".html", ".htm", ".pdf", ".png", ".jpg", ".jpeg", ".gif",
        ".svg", ".zip", ".rar", ".txt", ".csv", ".json", ".xml", ".exe", ".dll", ".doc", ".xls",
    ];
    EXTENSIONS.iter().any(|e| low.contains(e))
}

/// camelCase / PascalCase / ALL-CAPS: an identifier, not a mistyped word.
/// Legitimate all-caps Cyrillic acronyms ("кВт", "Гц") are protected here; the
/// English terms typed with the layout still Russian ("КЧ" = "RX") are caught
/// earlier by the technical lexicon.
fn has_inner_capital(token: &str) -> bool {
    token
        .chars()
        .skip(1)
        .any(|c| c.is_alphabetic() && c.is_uppercase())
}

#[derive(Debug, Clone)]
pub struct Config {
    /// Margin of log-probability needed to rewrite a word whose mirror form is
    /// *not* in the target dictionary.
    pub margin_unknown: f32,
    /// Margin when the mirror form *is* a known word.
    pub margin_known: f32,
    /// Extra bonus for an exact dictionary hit.
    pub word_bonus: f32,
    /// A mirror form that is not in the dictionary must still be a plausible
    /// sequence of letters for its language: its score, normalised against the
    /// average real word of the same length, has to be above this floor. This
    /// is what keeps `http` from becoming `реез`.
    pub plausibility: f32,
    pub min_len: usize,
    /// Never touch tokens that look like code, URLs, filenames or identifiers:
    /// anything with a digit, an internal capital, `/`, `:` or a file extension.
    pub protect_code: bool,
    /// Words the user never wants touched.
    pub whitelist: Vec<String>,
    /// Window/process substrings where the switcher must not act.
    pub exclude_processes: Vec<String>,
    /// Folder prefixes: if the foreground program lives under one of these
    /// directories, automatic switching is off for it.
    pub exclude_dirs: Vec<String>,
    pub auto_enabled: bool,
    /// Register the executable in the Windows "Run" key at start-up.
    pub autostart: bool,
    pub log_enabled: bool,
    /// Hotkey that undoes the last correction. "alt" keeps the classic behaviour;
    /// anything else ("f2", "ctrl+alt+z") triggers on key-down.
    pub hotkey_undo: String,
    /// Hotkey that turns auto-switching on/off ("pause", "ctrl+alt+p", ...).
    pub hotkey_toggle: String,
    /// Switch the layout of the last typed token (up to a space or separator;
    /// `-` stays part of the token). Default: Break.
    pub hotkey_last: String,
    /// Switch the layout of the selected text. Default: Break.
    pub hotkey_selection: String,
    /// Force the selected text to UPPER CASE. Default: Ctrl+Break.
    pub hotkey_upper: String,
    /// Swap the case of the selected text. Default: Shift+Break.
    pub hotkey_invert: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            margin_unknown: 2.5,
            margin_known: 1.0,
            word_bonus: 20.0,
            plausibility: -12.0,
            min_len: 3,
            protect_code: true,
            hotkey_undo: "alt".into(),
            hotkey_toggle: "ctrl+alt+p".into(),
            hotkey_last: "break".into(),
            hotkey_selection: "break".into(),
            hotkey_upper: "ctrl+break".into(),
            hotkey_invert: "shift+break".into(),
            whitelist: vec![
                "ok".into(),
                "git".into(),
                "vim".into(),
                "cd".into(),
                "js".into(),
                "sudo".into(),
                "bash".into(),
                "cmd".into(),
                "ctrl".into(),
                "alt".into(),
                "shift".into(),
                "enter".into(),
                "http".into(),
                "https".into(),
                "www".into(),
                "com".into(),
                "org".into(),
                "ru".into(),
                "net".into(),
                "rust".into(),
                "python".into(),
                "php".into(),
                "sql".into(),
                "html".into(),
                "json".into(),
                "url".into(),
                "pdf".into(),
                "exe".into(),
                "dll".into(),
                "file".into(),
            ],
            exclude_processes: vec![
                "cmd.exe".into(),
                "powershell".into(),
                "conhost".into(),
                "terminal".into(),
                "password".into(),
                "пароль".into(),
            ],
            exclude_dirs: vec![],
            auto_enabled: true,
            autostart: false,
            log_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub original: String,
    pub converted: String,
    /// Layout the keyboard should be switched to.
    pub target: TargetLayout,
    /// Human readable explanation, goes into the diary.
    pub why: String,
    /// Inject Alt+Shift after the rewrite. Punctuation fixes keep the layout
    /// untouched: the letters around them were typed correctly.
    pub flip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetLayout {
    Russian,
    English,
}

pub struct Engine {
    pub model: Model,
    pub cfg: Config,
}

/// Diagnostic output: with ZYA_VERBOSE every reason a token is *not* touched is
/// printed, which is how a miss in the article test becomes a findable rule.
static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn traced() -> bool {
    *TRACE.get_or_init(|| std::env::var("ZYA_VERBOSE").is_ok())
}

impl Engine {
    pub fn new(model: Model, cfg: Config) -> Engine {
        Engine { model, cfg }
    }

    /// Keys such as `,` `.` `;` `'` produce punctuation in one layout and a
    /// letter in the other, so the same token can be read in several ways.
    /// Letters are always mirrored; each ambiguous key is tried both ways.
    /// Every variant carries how many ambiguous keys were kept as punctuation
    /// inside the word (`inner`) and at its edges (`edge`), plus how many
    /// letters had to *become* punctuation because their physical key is
    /// `,` `.` `;` `'` `[` `]` `` ` `` (`punct` — the б/ю/ж/э/х/ъ/ё case).
    pub fn variants_debug(&self, token: &str) -> Vec<(String, usize, usize, usize)> {
        self.variants(token)
    }

    fn variants(&self, token: &str) -> Vec<(String, usize, usize, usize)> {
        let chars: Vec<char> = token.chars().collect();
        let mut slots: Vec<usize> = Vec::new();
        for (i, &c) in chars.iter().enumerate() {
            if !c.is_alphabetic() && is_ambiguous(c) {
                slots.push(i);
            }
        }
        let n = slots.len().min(4);
        let mut slot_of = std::collections::HashMap::new();
        for (s, &p) in slots.iter().take(n).enumerate() {
            slot_of.insert(p, s);
        }

        let mut out = Vec::with_capacity(1 << n);
        for mask in 0u32..(1u32 << n) {
            // `edge` counts ambiguous keys kept at the start/end of the token
            // (a trailing comma is real punctuation), `inner` counts the ones
            // inside the word, which are almost always the wrong layout.
            // `punct` counts letters that had to *turn into* punctuation: only
            // б ю ж э х ъ ё sit on `,` `.` `;` `'` `[` `]` `` ` ``, so a token
            // containing them was not an ordinary word of the other language.
            let mut inner = 0usize;
            let mut edge = 0usize;
            let mut punct = 0usize;
            let s: String = chars
                .iter()
                .enumerate()
                .map(|(i, &c)| {
                    if c.is_alphabetic() {
                        let m = map_key(c).unwrap_or(c);
                        if !m.is_alphabetic() {
                            punct += 1;
                            if i == 0 || i + 1 == chars.len() {
                                edge += 1;
                            } else {
                                inner += 1;
                            }
                        }
                        m
                    } else if let Some(&slot) = slot_of.get(&i) {
                        if (mask >> slot) & 1 == 1 {
                            map_key(c).unwrap_or(c)
                        } else {
                            if i == 0 || i + 1 == chars.len() {
                                edge += 1;
                            } else {
                                inner += 1;
                            }
                            c
                        }
                    } else {
                        c
                    }
                })
                .collect();
            out.push((s, inner, edge, punct));
        }
        out
    }

    fn tech_decision(&self, token: &str) -> Option<Decision> {
        let mirror: String = token.chars().map(|c| map_key(c).unwrap_or(c)).collect();
        if mirror == token {
            return None;
        }
        let cur_letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
        let alt_letters: String = mirror.chars().filter(|c| c.is_alphabetic()).collect();
        let cur = script_of(&cur_letters)?;
        let alt = match cur {
            Script::Cyrillic => Script::Latin,
            Script::Latin => Script::Cyrillic,
        };
        if script_of(&alt_letters) != Some(alt) {
            return None;
        }
        // A model number is only believable when it is at least two
        // characters long and has a real letter in it.
        if token.chars().count() < 2 || alt_letters.is_empty() {
            return None;
        }
        // If the run of letters is a real word of the layout in use, it is
        // not a mistyped model number. A single letter glued to digits
        // ("з40", "ь2") is never a word, so short runs are still checked.
        let cur_lang = match cur {
            Script::Cyrillic => &self.model.ru,
            Script::Latin => &self.model.en,
        };
        if cur_letters.chars().count() >= 2 && cur_lang.has_word(&cur_letters) {
            return None;
        }
        if !self.model.is_tech(&mirror) {
            // A digit glued to a known technical root ("16gb", "rtx5060") is
            // still a technical term even if the exact string is not in the
            // lexicon.
            if !token.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            if alt_letters.chars().count() < 2 || !self.model.is_tech(&alt_letters) {
                return None;
            }
        }
        Some(Decision {
            original: token.to_string(),
            converted: mirror,
            target: if alt == Script::Cyrillic {
                TargetLayout::Russian
            } else {
                TargetLayout::English
            },
            why: format!("{cur_letters} -> {alt_letters}  (technical term)"),
            flip: true,
        })
    }

    /// A hyphen glues a technical term to a Russian word, so the whole token is
    /// neither a term nor a word: "OLED-дисплей" typed with the layout still
    /// Russian arrives as "ЩДУВ-дисплей". Only the parts that are Cyrillic,
    /// are not Russian words and mirror to a known technical term are rewritten;
    /// the rest of the token stays as typed. The layout is not flipped: the two
    /// halves really were typed in different layouts.
    fn hyphen_decision(&self, token: &str) -> Option<Decision> {
        if !token.contains('-') {
            return None;
        }
        let parts: Vec<&str> = token.split('-').collect();
        let mut converted = String::new();
        let mut fixed = 0;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                converted.push('-');
            }
            let mirror: String = part.chars().map(|c| map_key(c).unwrap_or(c)).collect();
            // The part must be Cyrillic letters (digits allowed: "ввк4" is DDR4),
            // not a Russian word, and its mirror must be a known technical term.
            let cyr: String = part
                .chars()
                .filter(|&c| crate::layout::is_cyrillic(c))
                .collect();
            if part.chars().count() >= 2
                && mirror != *part
                && cyr.chars().count() >= 2
                && part
                    .chars()
                    .all(|c| crate::layout::is_cyrillic(c) || c.is_ascii_digit())
                && !self.model.ru.has_word(&cyr)
                && self.model.is_tech(&mirror)
            {
                converted.push_str(&mirror);
                fixed += 1;
            } else {
                converted.push_str(part);
            }
        }
        if fixed == 0 {
            return None;
        }
        Some(Decision {
            original: token.to_string(),
            converted: converted.clone(),
            target: TargetLayout::English,
            why: format!("{token} -> {converted}  (technical term inside a hyphenated token)"),
            flip: false,
        })
    }

    /// Shifted quote key in the Russian layout produces Э. Restore it to a real
    /// double quote without mirroring the word inside: ЭприветЭ -> "привет".
    fn quote_decision(&self, token: &str) -> Option<Decision> {
        let chars: Vec<char> = token.chars().collect();
        if chars.is_empty() {
            return None;
        }

        if chars.iter().all(|c| *c == 'Э') {
            return Some(Decision {
                original: token.to_string(),
                converted: "\"".repeat(chars.len()),
                target: TargetLayout::Russian,
                why: format!(
                    "{token} -> {}  (shifted quote key)",
                    "\"".repeat(chars.len())
                ),
                flip: false,
            });
        }

        let leading = chars[0] == 'Э';
        let trailing = chars[chars.len() - 1] == 'Э';

        if !leading && !trailing {
            return None;
        }
        // A leading Э alone is too risky: it can be the first letter of a real
        // word. Opening quotes are restored only when the matching closing Э is
        // present.
        if leading && !trailing {
            return None;
        }

        let inner: String = if leading {
            chars[1..chars.len() - 1].iter().collect()
        } else {
            chars[..chars.len() - 1].iter().collect()
        };

        let inner_cyrillic: String = inner
            .chars()
            .filter(|c| crate::layout::is_cyrillic(c))
            .collect();

        if inner_cyrillic.chars().count() < 2 || inner.chars().any(|c| c == 'Э') {
            return None;
        }

        // The inner text is still subject to the normal layout rules:
        // ЭруддщЭ becomes "hello", but ЭприветЭ stays "привет".
        let inner_fix = if self.model.ru.has_word(&inner_cyrillic) {
            None
        } else {
            self.decide_with(&inner, "")
        };
        let fixed_inner = match &inner_fix {
            Some(d) => d.converted.clone(),
            None => inner.clone(),
        };

        let converted = if leading {
            format!("\"{}\"", fixed_inner)
        } else {
            format!("{}\"", fixed_inner)
        };

        Some(Decision {
            original: token.to_string(),
            converted,
            target: inner_fix
                .as_ref()
                .map(|d| d.target.clone())
                .unwrap_or(TargetLayout::Russian),
            why: "Э -> \"  (shifted quote key typed in Russian layout)".into(),
            flip: inner_fix.map(|d| d.flip).unwrap_or(false),
        })
    }

    /// A Cyrillic word ending in б/ю/ж that does not exist in the dictionary is
    /// punctuation typed while the layout was still Russian: those letters sit
    /// on `,` `.` `;`. The layout stays as it is — nothing else in the run was
    /// typed wrong. Capital Б/Ю/Ж come from the shifted layer of the same keys,
    /// where the English layout has `.` `,` `:`.
    fn punctuation_decision(&self, token: &str) -> Option<Decision> {
        let chars: Vec<char> = token.chars().collect();
        // How many of the last keys are punctuation typed in the Russian layout.
        let mut run = 0;
        while run < chars.len() {
            match chars[chars.len() - 1 - run] {
                'б' | 'ю' | 'ж' | 'Б' | 'Ю' | 'Ж' => run += 1,
                _ => break,
            }
        }
        if run == 0 {
            return None;
        }
        let last = chars[chars.len() - 1];
        let punct = match last {
            'б' => ',',
            'ю' => '.',
            'ж' => ';',
            'Б' => '.',
            'Ю' => ',',
            _ => ':',
        };
        let head: String = chars[..chars.len() - run].iter().collect();
        // "ююю" is "..." : the same key pressed several times, no Russian word
        // is made only of these letters.
        if head.is_empty() {
            let converted: String = punct.to_string().repeat(chars.len());
            return Some(Decision {
                original: token.to_string(),
                converted: converted.clone(),
                target: TargetLayout::Russian,
                why: format!("{token} -> {converted}  (punctuation typed in Russian)"),
                flip: false,
            });
        }
        // The word before the key is the trailing run of Cyrillic letters:
        // "гироскоп/магнитометрб" is a URL-like run whose last word still ended
        // with a comma key.
        let head_chars: Vec<char> = head.chars().collect();
        let mut start = head_chars.len();
        while start > 0 && crate::layout::is_cyrillic(head_chars[start - 1]) {
            start -= 1;
        }
        let stem: String = head_chars[start..].iter().collect();
        // A stem shorter than three letters is too easy to guess: "чиж", "веб"
        // and "100руб" all end in a real letter, not in a punctuation key.
        if stem.chars().count() < 3 {
            return None;
        }
        // "клуб", "свобода" ... a real word keeps its letter.
        if self.model.ru.has_word(token) {
            if traced() {
                eprintln!("skip {token}: the ending is a real Russian word");
            }
            return None;
        }
        // The stem must read as Russian at all: "щдув" (the mirror of OLED)
        // is letter soup, "огнутых" and "факт" are not. An abbreviation like
        // "ГГц" scores badly too, but its mirror ("uuw") is no better, so the
        // only sensible reading left is the punctuation key.
        let ru_head = self.model.ru.score(&stem).unwrap_or(f32::MIN);
        if ru_head < -20.0 {
            let mirror: String = stem.chars().map(|c| map_key(c).unwrap_or(c)).collect();
            if mirror.chars().all(|c| c.is_alphabetic()) {
                let other = self.model.en.score(&mirror).unwrap_or(f32::MIN);
                if other > ru_head + 10.0 {
                    if traced() {
                        eprintln!(
                            "skip {token}: stem {stem} reads better as {mirror} ({other:.1} vs {ru_head:.1})"
                        );
                    }
                    return None;
                }
            }
        }
        // The decisive question is whether the letter can belong to the word at
        // all. If the stem on its own is a Russian word ("зумом", "факт"), the
        // key that follows it was `,` `.` or `;`. Otherwise the whole token has
        // to close like a real Russian word: "голуб" ends in "луб", which real
        // words do, while "ктб" closes nothing.
        let v: Vec<char> = stem.chars().collect();
        let pair = self
            .model
            .ru
            .bigram_lp(v[v.len() - 1], last)
            .unwrap_or(f32::MIN);
        let triple = if v.len() >= 2 {
            self.model
                .ru
                .trigram_lp(v[v.len() - 2], v[v.len() - 1], last)
                .unwrap_or(f32::MIN)
        } else {
            f32::MAX
        };
        if !self.model.ru.has_word(&stem)
            && self.model.ru.ends_like_word(&format!("{stem}{last}"))
            && pair >= -11.0
            && triple >= -17.0
        {
            if traced() {
                eprintln!(
                    "skip {token}: {stem}{last} closes like a real Russian word (pair {pair:.1}, triple {triple:.1})"
                );
            }
            return None;
        }
        let converted = format!("{head}{}", punct.to_string().repeat(run));
        Some(Decision {
            original: token.to_string(),
            converted: converted.clone(),
            target: TargetLayout::Russian,
            why: format!("{token} -> {converted}  (punctuation typed in Russian)"),
            flip: false,
        })
    }

    /// The same keys sit under `,` `.` `;` next to digits: "6б1" is "6,1",
    /// "53ю" is "53.". Fires only when those letters are the only Cyrillic
    /// letters in the token, so "100руб" and "5мб" keep their letters.
    /// The mirror of the previous rule: `,` `.` `;` `'` `[` `]` are `б` `ю` `ж`
    /// `э` `х` `ъ` in the Russian layout. A Latin token that contains one of
    /// them was typed with the layout still in English. The key can be either
    /// real punctuation or the Russian letter under it: "le[t" is "духе", while
    /// "ghbdtn," is "привет," — the comma really was a comma. Both readings are
    /// scored and the better one wins.
    fn punct_letter_decision(&self, token: &str) -> Option<Decision> {
        if !token.chars().any(|c| !c.is_alphabetic() && is_ambiguous(c)) {
            return None;
        }
        // "ghbdtn.txt" and "C:/Users" are identifiers, not mistyped words.
        if self.cfg.protect_code && (looks_like_code(token) || has_inner_capital(token)) {
            return None;
        }
        let full: String = token.chars().map(|c| map_key(c).unwrap_or(c)).collect();
        // The "keep the punctuation" reading is only possible when the token's
        // punctuation is one run of `,` `.` `;` at the very end: "ghbdtn," is
        // "привет,". A medial `[` in "clj[kb" or a leading `;` in ";enrb[" is
        // the Russian "х"/"ж", and `[` `]` `'` are never word-final punctuation
        // in normal text — they only ever mean "х", "ъ", "э".
        let trailing_punct = token
            .chars()
            .rev()
            .take_while(|c| matches!(c, ',' | '.' | ';' | ':'))
            .count();
        let all_punct = token.chars().filter(|c| !c.is_alphabetic()).count();
        let keep: String = if trailing_punct == 0 || trailing_punct != all_punct {
            token.to_string()
        } else {
            let head_len = token.chars().count() - trailing_punct;
            token
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if i < head_len && c.is_alphabetic() {
                        map_key(c).unwrap_or(c)
                    } else {
                        c
                    }
                })
                .collect()
        };
        if full == token && keep == token {
            return None;
        }
        let cyr_letters: String = full
            .chars()
            .filter(|&c| crate::layout::is_cyrillic(c))
            .collect();
        let keep_letters: String = keep
            .chars()
            .filter(|&c| crate::layout::is_cyrillic(c))
            .collect();
        let score_of = |letters: &str, whole: &str| -> Option<f32> {
            let known = self.model.ru.has_word(letters);
            // Two-letter words are below what the statistics can judge, but a
            // dictionary hit is enough: "yf," is "на," and not "наб".
            if letters.chars().count() < if known { 2 } else { 3 } {
                return None;
            }
            if whole
                .chars()
                .any(|c| c.is_alphabetic() && !crate::layout::is_cyrillic(c))
            {
                return None;
            }
            if !known {
                let len = letters.chars().count() as f32;
                let floor = self.cfg.plausibility * (len / 6.0).max(1.0);
                if self.model.ru.score(letters).unwrap_or(f32::MIN) < floor {
                    return None;
                }
            }
            Some(
                self.model.ru.score(letters).unwrap_or(f32::MIN)
                    + if known { self.cfg.word_bonus } else { 0.0 },
            )
        };
        let a = score_of(&cyr_letters, &full);
        let b = score_of(&keep_letters, &keep);
        let (converted, why) = match (a, b) {
            (Some(x), Some(y)) if x >= y => (full.clone(), "punctuation key is a Russian letter"),
            (Some(_), Some(_)) => (keep.clone(), "letters typed in English, punctuation kept"),
            (Some(x), None) => {
                let _ = x;
                (full.clone(), "punctuation key is a Russian letter")
            }
            (None, Some(_)) => (keep.clone(), "letters typed in English, punctuation kept"),
            (None, None) => return None,
        };
        if converted == token {
            return None;
        }
        Some(Decision {
            original: token.to_string(),
            converted: converted.clone(),
            target: TargetLayout::Russian,
            why: format!("{token} -> {converted}  ({why})"),
            flip: true,
        })
    }

    fn number_punct_decision(&self, token: &str) -> Option<Decision> {
        let chars: Vec<char> = token.chars().collect();
        if chars.iter().any(|c| crate::layout::is_cyrillic(*c))
            && !chars
                .iter()
                .any(|c| matches!(c, 'б' | 'ю' | 'ж' | 'Б' | 'Ю' | 'Ж'))
        {
            return None;
        }
        for (i, &c) in chars.iter().enumerate() {
            let punct = match c {
                'б' => ',',
                'ю' => '.',
                'ж' => ';',
                'Б' => '.',
                'Ю' => ',',
                'Ж' => ':',
                _ => continue,
            };
            let left = i > 0 && chars[i - 1].is_ascii_digit();
            let right = chars.get(i + 1).is_some_and(|d| d.is_ascii_digit());
            if !left && !right {
                continue;
            }
            let converted: String = chars
                .iter()
                .enumerate()
                .map(|(j, &ch)| if j == i { punct } else { ch })
                .collect();
            return Some(Decision {
                original: token.to_string(),
                converted: converted.clone(),
                target: TargetLayout::Russian,
                why: format!("{token} -> {converted}  (punctuation typed in Russian)"),
                flip: false,
            });
        }
        None
    }

    /// A lone Cyrillic letter typed in the wrong layout is almost always a
    /// one-letter Russian word: "d" is "в", "c" is "с", "b" is "и", "r" is "к",
    /// "e" is "у", "j" is "о", "f" is "а", "z" is "я". The corpus has almost no
    /// legitimate standalone Latin letters (they all sit inside model numbers
    /// such as "802.11a/b/g"), so the rule only fires when the previous token is
    /// Russian text: a Latin letter inside English text keeps its key.
    fn lone_letter_decision(&self, token: &str, prev: &str) -> Option<Decision> {
        let letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
        if letters != token || letters.chars().count() != 1 {
            return None;
        }
        // Only the letters that are real Russian one-letter words.
        const WORDS: [&str; 8] = ["в", "и", "с", "к", "у", "о", "а", "я"];
        let mirror: String = letters.chars().map(|c| map_key(c).unwrap_or(c)).collect();
        if script_of(&mirror) != Some(Script::Cyrillic) || !WORDS.contains(&mirror.as_str()) {
            return None;
        }
        if script_of(&letters) != Some(Script::Latin) {
            return None;
        }
        // The context has to be Russian: the previous token must contain a
        // Cyrillic letter and be a word, not a single character or a number.
        if !prev.chars().any(crate::layout::is_cyrillic) || prev.chars().count() < 2 {
            return None;
        }
        if self.model.is_tech(&letters) || self.model.is_tech(&mirror) {
            return None;
        }
        Some(Decision {
            original: token.to_string(),
            converted: mirror.clone(),
            target: TargetLayout::Russian,
            why: format!("{token} -> {mirror}  (one-letter word after {prev})"),
            flip: true,
        })
    }

    /// A standalone Latin `b` is the Russian `и` (the same physical key). It is
    /// turned into `и` when the previous token is a real word or a number —
    /// "P30 b P40", "свобода b мир" — and never inside a list of single
    /// letters, where "a b c d" really is a list of variants.
    fn conjunction_decision(&self, token: &str, prev: &str) -> Option<Decision> {
        if token != "b" || self.model.is_tech("b") {
            return None;
        }
        // A previous token of one character ("a", "X", "1") means a list of
        // variants, not a conjunction.
        if prev.chars().count() < 2 {
            return None;
        }
        Some(Decision {
            original: token.to_string(),
            converted: "и".to_string(),
            target: TargetLayout::Russian,
            why: format!("b -> и  (conjunction after {prev})"),
            flip: true,
        })
    }

    /// Two-letter Russian function words ("на", "не", "от") are below the length
    /// the statistics can judge. Here an exact hit in the dictionary of the
    /// other layout is enough, provided the letters really typed are not a
    /// common word of the layout in use ("a", "I", "the").
    fn short_word_decision(&self, token: &str) -> Option<Decision> {
        let letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
        let len = letters.chars().count();
        if letters != token || len != 2 {
            return None;
        }
        let cur = script_of(&letters)?;
        if self.model.is_tech(&letters)
            || self
                .cfg
                .whitelist
                .iter()
                .any(|w| w.eq_ignore_ascii_case(&letters))
        {
            return None;
        }
        let mirror: String = letters.chars().map(|c| map_key(c).unwrap_or(c)).collect();
        let other = match cur {
            Script::Cyrillic => Script::Latin,
            Script::Latin => Script::Cyrillic,
        };
        if script_of(&mirror) != Some(other) || self.model.is_tech(&mirror) {
            return None;
        }
        let typed_lp = match cur {
            Script::Cyrillic => self.model.ru.word_lp(&letters),
            Script::Latin => self.model.en.word_lp(&letters),
        };
        if typed_lp.is_some_and(|lp| lp > -8.0) {
            return None;
        }
        let alt_lang = match cur {
            Script::Cyrillic => &self.model.en,
            Script::Latin => &self.model.ru,
        };
        let wanted_lp = alt_lang.word_lp(&mirror)?;
        if wanted_lp < -11.0 {
            return None;
        }
        let target = match other {
            Script::Cyrillic => TargetLayout::Russian,
            Script::Latin => TargetLayout::English,
        };
        Some(Decision {
            original: token.to_string(),
            converted: mirror.clone(),
            target,
            why: format!("{token} -> {mirror}  (short word, {wanted_lp:.1} in the other layout)"),
            flip: true,
        })
    }

    pub fn decide(&self, token: &str) -> Option<Decision> {
        self.decide_with(token, "")
    }

    /// `prev` is the token that was typed immediately before this one: the
    /// single-letter rules need that context, the others do not.
    pub fn decide_with(&self, token: &str, prev: &str) -> Option<Decision> {
        // A technical term typed correctly is never touched, and a mistyped
        // model number ("ввк4" = DDR4) is caught before the identifier rule,
        // which would otherwise throw away anything containing a digit.
        if self.model.is_tech(token) {
            if traced() {
                eprintln!("skip {token}: known technical term");
            }
            return None;
        }
        if let Some(d) = self.tech_decision(token) {
            return Some(d);
        }
        // "OLED-дисплей" typed with the layout still Russian: the term part is
        // mirrored, the Russian part stays.
        if let Some(d) = self.hyphen_decision(token) {
            return Some(d);
        }
        // Shifted quote key in the Russian layout: ЭприветЭ -> "привет".
        if let Some(d) = self.quote_decision(token) {
            return Some(d);
        }

        // `,` `.` `;` are `б` `ю` `ж` in the Russian layout: a Cyrillic token
        // that ends in one of them and is not a word was really punctuation.
        if let Some(d) = self.punctuation_decision(token) {
            return Some(d);
        }
        // The same keys seen from the English side: "j,vfyekf" is "обманула".
        if let Some(d) = self.punct_letter_decision(token) {
            return Some(d);
        }
        // The same keys next to digits: "6б1" is "6,1", "53ю" is "53.".
        if let Some(d) = self.number_punct_decision(token) {
            return Some(d);
        }
        // A lone Latin "b" in Russian text is the conjunction "и" typed with
        // the layout still in English.
        if let Some(d) = self.conjunction_decision(token, prev) {
            return Some(d);
        }
        // The other one-letter Russian words: "d" is "в", "c" is "с" ...
        if let Some(d) = self.lone_letter_decision(token, prev) {
            return Some(d);
        }
        // Short function words are judged by the dictionary, not by the model.
        if let Some(d) = self.short_word_decision(token) {
            return Some(d);
        }

        let letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
        // A token made only of ambiguous punctuation (`,her`, `yb[`) still has
        // enough letters once the ambiguous keys are read as letters, so we do
        // not bail out on the raw letter count here.
        let has_ambiguous = token.chars().any(|c| !c.is_alphabetic() && is_ambiguous(c));
        if letters.chars().count() < self.cfg.min_len && !has_ambiguous {
            if traced() {
                eprintln!("skip {token}: too short for the model to judge");
            }
            return None;
        }
        let cur = script_of(&letters)?;

        // Identifiers, URLs and filenames are left alone even if the mirror
        // form looks like a word of the other language.
        if self.cfg.protect_code && (looks_like_code(token) || has_inner_capital(token)) {
            // An all-caps or mixed-case Cyrillic token is usually a real
            // abbreviation ("кВт", "Гц"), but "ФТЕ" is "ANT" typed with the
            // layout still Russian. The mirror decides — but only when the code
            // signal is the capitalisation. Digits, slashes and extensions are a
            // stronger signal: "ghbdtn123" is an identifier, not "привет123".
            let mirror_letters: String = letters.chars().map(|c| map_key(c).unwrap_or(c)).collect();
            let other_lang = match cur {
                Script::Cyrillic => &self.model.en,
                Script::Latin => &self.model.ru,
            };
            let hard_code = token.chars().any(|c| c.is_ascii_digit())
                || token
                    .chars()
                    .any(|c| c == '/' || c == '\\' || c == ':' || c == '@' || c == '#' || c == '_');
            // Only an all-caps token ("ФТЕ") can be an acronym that was really
            // typed in the wrong layout. Mixed case is camelCase: "ghbdTn".
            let all_caps = token
                .chars()
                .filter(|c| c.is_alphabetic())
                .all(|c| c.is_uppercase());
            if hard_code
                || !all_caps
                || (!other_lang.has_word(&mirror_letters) && !self.model.is_tech(&mirror_letters))
            {
                if traced() {
                    eprintln!("skip {token}: looks like an identifier");
                }
                return None;
            }
        }

        if self
            .cfg
            .whitelist
            .iter()
            .any(|w| w.eq_ignore_ascii_case(&letters))
        {
            if traced() {
                eprintln!("skip {token}: on the whitelist");
            }
            return None;
        }
        // A word that is a real word of the layout actually in use is untouched.
        // The dictionary is a raw frequency list, so it also contains corpus
        // scraps; keeping them is deliberate: dropping rare but genuine forms
        // costs more than tolerating a few junk entries.
        let cur_lang = match cur {
            Script::Cyrillic => &self.model.ru,
            Script::Latin => &self.model.en,
        };
        if cur_lang.has_word(&letters) {
            if traced() {
                eprintln!("skip {token}: a real word of the layout in use");
            }
            return None;
        }
        let s_cur = cur_lang.score(&letters).unwrap_or(INVALID);

        let alt_lang = match cur {
            Script::Cyrillic => &self.model.en,
            Script::Latin => &self.model.ru,
        };

        let mut best: Option<(String, f32, bool)> = None;
        for (cand, inner, edge, punct) in self.variants(token) {
            let cand_letters: String = cand.chars().filter(|c| c.is_alphabetic()).collect();
            if cand_letters.chars().count() < self.cfg.min_len && !self.model.is_tech(&cand_letters)
            {
                continue;
            }
            if script_of(&cand_letters)
                != Some(match cur {
                    Script::Cyrillic => Script::Latin,
                    Script::Latin => Script::Cyrillic,
                })
            {
                continue;
            }
            // A candidate that keeps an ambiguous key is not the dictionary word
            // its letters spell: "с]ела" is not "села" and "цель." is not "цель".
            let known = inner == 0 && edge == 0 && punct == 0 && alt_lang.has_word(&cand_letters);
            // A letter that mirrors into punctuation (`б`->`,`, `ю`->`.`, `ж`->`;`,
            // `э`->`'`, `х`->`[`, `ъ`->`]`, `ё`->`` ` ``) can only be a mistyped
            // key if the user really wanted that punctuation. Such a reading is
            // believable only when the remaining letters are a real word or a
            // known technical term; otherwise the Russian word stays as it is.
            if punct > 0 && !alt_lang.has_word(&cand_letters) && !self.model.is_tech(&cand_letters)
            {
                if traced() {
                    eprintln!("  candidate {cand}: keeps a letter that is punctuation");
                }
                continue;
            }
            // A mirror form that is not in the dictionary still has to look like
            // a real word of that language, otherwise `http` -> `реез` happens.
            // The floor grows with length: an 11-letter Russian word that is not
            // in the top-50k list scores about -23 even when it is perfectly
            // real, while a 3-letter junk mirror is still caught by -12.
            let len = cand_letters.chars().count() as f32;
            let floor = self.cfg.plausibility * (len / 6.0).max(1.0);
            // A term from the technical lexicon is real even if the general
            // corpus never saw it ("Li", "SoC", "FP").
            if !known
                && !self.model.is_tech(&cand_letters)
                && alt_lang.score(&cand_letters).unwrap_or(INVALID) < floor
            {
                if traced() {
                    let other = if cur == Script::Cyrillic {
                        "English"
                    } else {
                        "Russian"
                    };
                    eprintln!("  candidate {cand} of {token}: not believable as a {other} word");
                }
                continue;
            }
            // Keeping an ambiguous key is the less likely reading; inside the word
            // it is far less likely than at its edge (a trailing comma).
            let score = alt_lang.score(&cand_letters).unwrap_or(INVALID)
                + if known { self.cfg.word_bonus } else { 0.0 }
                - 18.0 * inner as f32
                - 4.0 * edge as f32
                - 12.0 * punct as f32;
            if best.as_ref().is_none_or(|(_, s, _)| score > *s) {
                best = Some((cand, score, known));
            }
        }

        let (converted, s_alt, known) = match best {
            Some(b) => b,
            None => {
                if traced() {
                    eprintln!("skip {token}: no mirror candidate survived the checks");
                }
                return None;
            }
        };
        if converted == token {
            return None;
        }
        let margin = if known {
            self.cfg.margin_known
        } else {
            self.cfg.margin_unknown
        };
        let diff = s_alt - s_cur;
        if diff > margin {
            let why = format!(
                "{letters} -> {converted}  (cur {s_cur:.2}, alt {s_alt:.2}, margin {diff:.2})"
            );
            Some(Decision {
                original: token.to_string(),
                converted,
                target: match alt_lang_has_cyrillic(cur) {
                    true => TargetLayout::Russian,
                    false => TargetLayout::English,
                },
                why,
                flip: true,
            })
        } else {
            if traced() {
                eprintln!(
                    "skip {token}: mirror {converted} is only {diff:.2} better, need {margin:.2}"
                );
            }
            None
        }
    }
}

fn alt_lang_has_cyrillic(cur: Script) -> bool {
    matches!(cur, Script::Latin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::map_word;

    fn engine() -> Engine {
        Engine::new(Model::load().unwrap(), Config::default())
    }

    #[test]
    fn fixes_wrong_layout_words() {
        let e = engine();
        let cases = [
            ("руддщ", "hello"),
            ("ghbdtn", "привет"),
            ("ьщку", "more"),
            ("еру", "the"),
            ("йгшсл", "quick"),
        ];
        for (typed, want) in cases {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("no decision for {typed}"));
            assert_eq!(d.converted, want, "{typed}");
        }
    }

    /// Tokens that are not words in either language are junk keystrokes, not a
    /// layout mistake, so the hook leaves them alone.
    #[test]
    fn junk_tokens_are_not_converted() {
        let e = engine();
        for word in ["фыв", "bqk", "asd", "ийл", "зщ"] {
            if let Some(d) = e.decide(word) {
                panic!("junk {word} converted to {}", d.converted);
            }
        }
    }

    #[test]
    fn leaves_correct_words_alone() {
        let e = engine();
        for word in [
            "привет",
            "как",
            "дела",
            "это",
            "красивый",
            "щит",
            "работа",
            "спасибо",
            "привет.",
        ] {
            if let Some(d) = e.decide(word) {
                panic!("should not switch {word} -> {}", d.converted);
            }
        }
        for word in [
            "hello",
            "world",
            "the",
            "quick",
            "brown",
            "fox",
            "javascript",
            "hello,",
        ] {
            if let Some(d) = e.decide(word) {
                panic!("should not switch {word} -> {}", d.converted);
            }
        }
    }

    /// Every technical term harvested from 3dnews/ixbt is typed on the wrong
    /// layout and must come back. Terms whose mirror is a real word of the
    /// other language are excluded (there is no way to tell them apart).
    #[test]
    fn technical_terms_round_trip() {
        let e = engine();
        let (mut ok, mut total) = (0usize, 0usize);
        let mut bad = Vec::new();
        for term in e.model.tech.iter() {
            let typed = map_word(term);
            if e.model.is_tech(&typed) {
                continue; // ambiguous: both spellings are known terms
            }
            total += 1;
            match e.decide(&typed) {
                Some(d) if d.converted == *term => ok += 1,
                Some(d) => bad.push(format!("{typed} -> {}", d.converted)),
                None => bad.push(format!("{typed} -> (miss)")),
            }
        }
        println!(
            "technical recall {}/{} = {:.1}%; misses: {:?}",
            ok,
            total,
            100.0 * (ok as f64) / (total as f64),
            &bad[..bad.len().min(20)]
        );
        assert!(
            (ok as f64) / (total as f64) > 0.90,
            "technical recall too low"
        );
    }

    #[test]
    fn model_numbers_are_not_identifiers() {
        let e = engine();
        for (typed, want) in [
            ("ввк4", "ddr4"),
            ("щдув", "oled"),
            ("тмьу", "nvme"),
            ("кеч", "rtx"),
            ("ыыв", "ssd"),
            ("сзг", "cpu"),
        ] {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("no decision for {typed}"));
            assert_eq!(d.converted, want, "{typed}");
        }
        // correctly spelled model numbers stay as they are
        for token in ["ddr4", "OLED", "rtx5060", "NVMe", "1080p", "Wi-Fi"] {
            assert!(e.decide(token).is_none(), "{token} must not be rewritten");
        }
    }

    #[test]
    fn punctuation_is_preserved() {
        let e = engine();
        // "привет" typed while the layout was English, followed by a comma.
        let d = e.decide("ghbdtn,").expect("should fix");
        assert_eq!(d.converted, "привет,");
    }

    /// URLs, file names, identifiers and words with an inner capital are never
    /// touched, even when the mirror image looks like a real word.
    #[test]
    fn code_tokens_are_protected() {
        let e = engine();
        for token in [
            "ghbdtn.txt",
            "ghbdtn123",
            "ghbdtn.com",
            "GhbdtnBdtn",
            "my_file",
            "C:/Users/demOOn",
        ] {
            assert!(e.decide(token).is_none(), "{token} must not be rewritten");
        }
        // A single leading capital is still an ordinary word start.
        assert!(e.decide("Ghbdtn").is_some());
    }

    /// Take the most frequent real words, type them in the *wrong* layout and
    /// check that the engine recovers them.
    /// A Cyrillic token ending in б/ю/ж that is not a word was really `,` `.`
    /// `;` typed while the layout was still Russian.
    #[test]
    fn punctuation_after_a_russian_stem_is_restored() {
        let e = engine();
        for (typed, want) in [
            ("фактб", "факт,"),
            ("огнутыхб", "огнутых,"),
            ("сказкаж", "сказка;"),
            ("проектЖ", "проект:"),
            ("б", ","),
            ("ю", "."),
            ("ж", ";"),
        ] {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("{typed} should become punctuation"));
            assert_eq!(d.converted, want, "{typed}");
            assert!(!d.flip, "punctuation must not switch the layout");
        }
    }

    #[test]
    fn technical_term_glued_to_a_russian_word_is_split() {
        let e = engine();
        let d = e.decide("ЩДУВ-дисплей").expect("OLED-дисплей");
        assert_eq!(d.converted, "OLED-дисплей");
        assert!(!d.flip, "the layout must not be flipped");
        let d = e.decide("ввк4-карта").expect("DDR4-карта");
        assert_eq!(d.converted, "ddr4-карта");
        // Correctly typed or genuinely Russian hyphenated tokens stay as typed.
        for ok in [
            "OLED-дисплей",
            "мини-плеер",
            "wi-fi",
            "две-комнатный",
            "Full-HD",
        ] {
            assert!(e.decide(ok).is_none(), "{ok} must not be rewritten");
        }
    }

    #[test]
    fn real_words_ending_in_those_letters_survive() {
        let e = engine();
        // Dictionary words and real inflections that are not in the top-50k
        // list: all of them keep their last letter.
        for word in [
            "клуб",
            "чиж",
            "багаж",
            "машину",
            "компанию",
            "батарею",
            "версию",
            "тройную",
            "груд",
            "муж",
        ] {
            assert!(e.decide(word).is_none(), "{word} is a real word");
        }
        // Letter soup is not a stem either ("щдув" is handled by the tech rule).
        for word in [
            "клуб",
            "чиж",
            "багаж",
            "машину",
            "компанию",
            "батарею",
            "версию",
            "тройную",
            "груд",
            "муж",
        ] {
            assert!(e.decide(word).is_none(), "{word} is a real word");
        }
        for junk in ["сиж", "щдб"] {
            assert!(e.decide(junk).is_none(), "{junk} has no Russian reading");
        }
    }

    #[test]
    fn punctuation_typed_in_russian_is_restored() {
        let e = engine();
        for (typed, want) in [
            ("фактб", "факт,"),
            ("сказкаж", "сказка;"),
            ("проектЖ", "проект:"),
        ] {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("{typed} should become punctuation"));
            assert_eq!(d.converted, want, "{typed}");
            assert!(!d.flip, "punctuation must not switch the layout");
        }
        // Real words keep their last letter.
        for word in ["клуб", "свобода", "чиж", "багаж", "груд"] {
            assert!(e.decide(word).is_none(), "{word} is a real word");
        }
        // A stem that is not a word is just noise, not punctuation.
        assert!(e.decide("сиж").is_none(), "сиж has no Russian reading");
    }

    /// A standalone Latin "b" is the Russian "и" (same key), but only when the
    /// previous token makes it a conjunction, not a list of letters.
    #[test]
    fn lone_b_is_a_conjunction_only_in_context() {
        let e = engine();
        let d = e.decide_with("b", "P30").expect("P30 b P40 must be fixed");
        assert_eq!(d.converted, "и");
        assert!(d.flip, "the letter really came from the English layout");
        for prev in ["привет", "Pro", "100"] {
            assert!(
                e.decide_with("b", prev).is_some(),
                "b after {prev:?} is a conjunction"
            );
        }
        // "variants a b c d": the token before "b" is a single letter.
        for prev in ["a", "X", "1", ""] {
            assert!(
                e.decide_with("b", prev).is_none(),
                "b after {prev:?} is a list"
            );
        }
    }

    /// `,` `.` `;` squeezed between digits come out as б/ю/ж in the Russian
    /// layout; real words glued to numbers are left alone.
    #[test]
    fn punctuation_inside_numbers_is_restored() {
        let e = engine();
        for (typed, want) in [("6б1", "6,1"), ("53ю", "53."), ("1000ж", "1000;")] {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("{typed} should become punctuation"));
            assert_eq!(d.converted, want, "{typed}");
            assert!(!d.flip, "punctuation must not switch the layout");
        }
        for word in ["100руб", "16ГБ", "2000р"] {
            assert!(e.decide(word).is_none(), "{word} keeps its letters");
        }
    }

    /// Only б ю ж э х ъ ё sit on keys whose other layout is punctuation
    /// (`,` `.` `;` `'` `[` `]` `` ` ``). A Russian technical word like "буфер"
    /// mirrored gives ",eath": a real English word never contains a comma, so
    /// the Russian spelling has to survive.
    #[test]
    fn russian_words_on_punctuation_keys_survive() {
        let e = engine();
        for word in [
            "буфер",
            "буфера",
            "буферная",
            "буферизация",
            "объект",
            "объекты",
            "объём",
            "язык",
            "часть",
            "части",
            "человек",
            "съезд",
            "техника",
            "машина",
            "гипер",
        ] {
            if let Some(d) = e.decide(word) {
                panic!("{word} must stay Russian, got {}", d.converted);
            }
        }
        // The same keys really typed as punctuation are still restored.
        let d = e
            .decide("щдувб")
            .unwrap_or_else(|| panic!("oled, was not recognised"));
        assert_eq!(d.converted, "oled,");
    }

    #[test]
    fn shifted_quote_keys_are_restored() {
        let e = engine();

        for (typed, want) in [
            ("ЭприветЭ", "\"привет\""),
            ("ЭтекстЭ", "\"текст\""),
            ("ЭруддщЭ", "\"hello\""),
            ("приветЭ", "привет\""),
            ("ЭЭЭ", "\"\"\""),
        ] {
            let d = e
                .decide(typed)
                .unwrap_or_else(|| panic!("{typed} should become quotes"));
            assert_eq!(d.converted, want, "{typed}");
            assert!(!d.flip, "{typed}: quote fix must not switch layout");
        }

        // A real Russian word starting with Э must not be eaten by an opening quote.
        for word in ["Эта", "Эра", "Эхо"] {
            assert!(e.decide(word).is_none(), "{word} must stay as typed");
        }
    }

    #[test]
    fn round_trip_accuracy() {
        let e = engine();
        let ru = e.model.ru.top_words(2000);
        let en = e.model.en.top_words(2000);
        let (mut ok, mut total) = (0usize, 0usize);
        let mut bad = Vec::new();
        for w in ru.iter().chain(en.iter()) {
            if w.chars().count() < 3 {
                continue;
            }
            total += 1;
            let typed = map_word(w);
            match e.decide(&typed) {
                Some(d) if d.converted == *w => ok += 1,
                Some(d) => bad.push(format!("{typed} -> {}", d.converted)),
                None => bad.push(format!("{typed} -> (miss)")),
            }
        }
        println!(
            "recall {}/{} = {:.1}%; first failures: {:?}",
            ok,
            total,
            100.0 * (ok as f64) / (total as f64),
            &bad[..bad.len().min(15)]
        );
        assert!(
            (ok as f64) / (total as f64) > 0.90,
            "round trip recall too low"
        );
    }

    /// Correctly typed words must never be rewritten.
    #[test]
    fn false_positives_on_correct_words() {
        let e = engine();
        let ru = e.model.ru.top_words(2000);
        let en = e.model.en.top_words(2000);
        let (mut fp, mut total) = (0usize, 0usize);
        for w in ru.iter().chain(en.iter()) {
            if w.chars().count() < 3 {
                continue;
            }
            total += 1;
            if let Some(d) = e.decide(w) {
                fp += 1;
                if fp < 10 {
                    println!("false positive: {w} -> {}", d.converted);
                }
            }
        }
        println!(
            "false positives {fp}/{total} = {:.2}%",
            100.0 * (fp as f64) / (total as f64)
        );
        assert!(
            (fp as f64) / (total as f64) < 0.01,
            "too many false positives"
        );
    }
}
