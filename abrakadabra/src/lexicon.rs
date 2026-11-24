//! Curated dictionary of short service words in both layouts.
//!
//! Words of one to three letters are the hardest for the engine: the character
//! model cannot judge them, the general dictionaries are full of corpus junk of
//! exactly that length, and `min_len` would otherwise bail out before any rule
//! runs. So the engine keeps a hand-picked list of function words for each
//! language and treats it as ground truth.
//!
//! Two lists are needed, and they collide:
//!   * `RU_SHORT` - Russian prepositions, pronouns, conjunctions, particles.
//!     A Latin token whose key mirror is in this list was typed with the layout
//!     still in English and must be flipped.
//!   * `EN_SHORT` - English articles, prepositions, pronouns, auxiliaries and
//!     the abbreviations that live in technical text (`kb`, `GB`, `fps`). These
//!     are never flipped, because their mirror often looks like a Russian word.
//!
//! `conflicts()` reports the pairs where the same physical keys spell a word of
//! both languages (`vs` / `мы`, `kb` / `ли`). Such tokens are decided by the
//! layout actually in use, never by the mirror.

use std::collections::HashSet;

use crate::layout::{is_cyrillic, map_key};

/// Russian function words of one to three letters.
pub const RU_SHORT: &[&str] = &[
    "в", "и", "с", "к", "у", "о", "а", "я", "й",
    "не", "но", "да", "на", "мы", "вы", "он", "из", "до", "по", "за", "от", "со", "во", "те",
    "их", "ты", "то", "та", "ту", "ее", "ей", "им", "ли", "бы", "ни", "уж", "же", "мой",
    "наш", "ваш", "тот", "нас", "вас", "вам", "нам", "мне", "нее", "ней", "нем", "них", "был",
    "кто", "как", "чем", "чей", "чьи", "при", "что", "где", "кем", "еще", "тут", "там", "без",
    "над", "под", "сам", "нет", "его", "все", "всю", "той", "уже", "три", "две", "оно", "они",
    "так", "ну", "обо", "эти", "вот", "или", "для", "про", "сей", "чем", "как", "что", "где",
    "её", "ёй", "кто", "неё", "нее", "неё", "ней", "чем", "как",
];

/// Russian single-letter abbreviations that stay Cyrillic in technical text.
pub const RU_ABBREV: &[&str] = &[
    "Гц", "МГц", "кГц", "Вт", "кВт", "МВт", "см", "мм", "кг", "км", "ч", "г", "с", "шт", "руб",
    "дол", "ИИ", "ТЗ", "ЛК", "ПК", "ОС", "ЦП", "ГПУ", "КПК", "НМ", "УТ", "ДВС", "ЭУ",
];

/// English function words and abbreviations of one to three letters. Anything
/// here is protected: mirroring `vs` into `мы` is the mistake this list prevents.
pub const EN_SHORT: &[&str] = &[
    "a", "an", "the", "and", "of", "to", "in", "is", "it", "be", "as", "at", "by", "on", "or",
    "no", "so", "we", "he", "she", "my", "his", "her", "its", "our", "for", "not", "but", "if",
    "all", "one", "two", "up", "out", "use", "get", "had", "has", "was", "are", "you", "did",
    "say", "new", "old", "top", "key", "per", "via", "who", "how", "why", "can", "may", "do",
    "me", "us", "him", "any", "own", "see", "far", "set", "end", "due", "low", "bit", "log",
    "map", "run", "dev", "i", "vs", "ii", "iv", "yes", "now", "let", "add", "fit", "off", "etc",
    "api", "cpu", "gpu", "ram", "hdd", "ssd", "usb", "ios", "mac", "win", "web", "app", "lab",
    "mod", "pro", "max", "min", "url", "ftp", "nfc", "hdr", "lcd", "led", "ips", "dpi", "hz",
    "gb", "tb", "mb", "kb", "ms", "sec", "avg", "sum", "dll", "exe", "src", "bin", "dir", "tmp",
    "net", "int", "str", "var", "fps", "nm", "pci", "aac", "svg", "png", "pdf", "isp", "llm",
    "dsp", "gms", "npu", "eod", "tof", "fs", "acl", "gdp", "qt", "xl", "icc", "dcs", "pst",
    "ods", "bt", "tv", "uk", "us", "eu", "cn", "jp", "kr", "de", "fr",
];

/// The physical keys that produce a word when the layout is the other one.
pub fn key_spelling(word: &str) -> String {
    word.chars()
        .map(|c| map_key(c).unwrap_or(c))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The Russian service word.
    pub ru: String,
    /// The Latin letters that produce it on a Russian keyboard.
    pub keys: String,
}

pub struct Lexicon {
    ru: HashSet<String>,
    en: HashSet<String>,
    abbrev: HashSet<String>,
}

impl Lexicon {
    /// Built-in lists plus the words the user added in the settings window.
    pub fn with_extra(extra_ru: &[String], extra_en: &[String]) -> Lexicon {
        let mut ru: HashSet<String> = RU_SHORT.iter().map(|w| w.to_lowercase()).collect();
        let mut en: HashSet<String> = EN_SHORT.iter().map(|w| w.to_lowercase()).collect();
        let abbrev: HashSet<String> = RU_ABBREV.iter().map(|w| w.to_lowercase()).collect();
        for w in extra_ru {
            let clean: String = w.chars().filter(|c| is_cyrillic(*c)).collect();
            if !clean.is_empty() {
                ru.insert(clean.to_lowercase());
            }
        }
        for w in extra_en {
            let clean: String = w
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .collect();
            if !clean.is_empty() {
                en.insert(clean.to_lowercase());
            }
        }
        Lexicon { ru, en, abbrev }
    }

    pub fn is_ru(&self, word: &str) -> bool {
        self.ru.contains(&word.to_lowercase())
    }

    pub fn is_en(&self, en_word: &str) -> bool {
        self.en.contains(&en_word.to_lowercase())
    }

    pub fn is_ru_abbrev(&self, word: &str) -> bool {
        self.abbrev.contains(&word.to_lowercase())
    }

    pub fn ru_count(&self) -> usize {
        self.ru.len()
    }

    pub fn en_count(&self) -> usize {
        self.en.len()
    }

    pub fn ru_words(&self) -> Vec<String> {
        let mut v: Vec<String> = self.ru.iter().cloned().collect();
        v.sort();
        v
    }

    pub fn en_words(&self) -> Vec<String> {
        let mut v: Vec<String> = self.en.iter().cloned().collect();
        v.sort();
        v
    }

    /// Russian service words whose key spelling is also an English word the
    /// engine is asked to protect. These are the tokens the engine cannot
    /// resolve from the letters alone.
    pub fn conflicts(&self) -> Vec<Conflict> {
        let mut out = Vec::new();
        for w in &self.ru {
            let keys = key_spelling(w);
            if self.en.contains(&keys) {
                out.push(Conflict { ru: w.clone(), keys });
            }
        }
        out.sort_by(|a, b| a.ru.cmp(&b.ru));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_short_russian_word_is_short_and_cyrillic() {
        for w in RU_SHORT {
            assert!(w.chars().count() <= 3, "{w} is too long for the short list");
            assert!(is_cyrillic(w.chars().next().unwrap()), "{w} is not Cyrillic");
        }
    }

    #[test]
    fn every_short_english_word_is_ascii() {
        let l = Lexicon::with_extra(&[], &[]);
        for w in l.en_words() {
            assert!(w.chars().all(|c| c.is_ascii_alphabetic()), "{w} is not English");
        }
    }

    #[test]
    fn key_spelling_round_trips() {
        assert_eq!(key_spelling("при"), "ghb");
        assert_eq!(key_spelling("еще"), "tot");
        assert_eq!(key_spelling("в"), "d");
        assert_eq!(key_spelling("что"), "xnj");
    }

    #[test]
    fn the_known_collisions_are_reported() {
        let l = Lexicon::with_extra(&[], &[]);
        let c = l.conflicts();
        assert!(!c.is_empty(), "the dictionary must report its own conflicts");
        assert!(c.iter().any(|x| x.ru == "мы" && x.keys == "vs"));
        assert!(c.iter().any(|x| x.ru == "ли" && x.keys == "kb"));
    }

    #[test]
    fn user_words_join_the_dictionary() {
        let base = Lexicon::with_extra(&[], &[]);
        let l = Lexicon::with_extra(&["кт".into()], &["qps".into()]);
        assert!(l.is_ru("кт"));
        assert!(l.is_en("qps"));
        assert_eq!(l.ru_count(), base.ru_count() + 1);
        assert_eq!(l.en_count(), base.en_count() + 1);
    }

    #[test]
    fn a_word_is_neither_in_the_ru_nor_in_the_en_list_by_spelling() {
        let l = Lexicon::with_extra(&[], &[]);
        for w in l.ru_words() {
            assert!(!l.en.contains(&w), "{w} cannot be both Russian and English");
        }
    }
}
