//! Physical-key mapping between the QWERTY and ЙЦУКЕН (Windows "Russian") layouts.
//!
//! The mapping is keyed by *physical key*, i.e. "what character does this key
//! produce in layout X" — the way a layout switcher thinks about it.
//!
//!   ЙЦУКЕН: й ц у к е н г ш щ з х ъ   (top row, q..p [ ])
//!           ф ы в а п р о л д ж э     (home row, a..l ;)
//!           ч с м б т ь ю             (bottom row, z..m)

use std::collections::HashMap;

/// Latin physical key -> Cyrillic letter produced by the Russian layout.
const QWERTY_TO_RU: [(char, char); 33] = [
    ('q', 'й'),
    ('w', 'ц'),
    ('e', 'у'),
    ('r', 'к'),
    ('t', 'е'),
    ('y', 'н'),
    ('u', 'г'),
    ('i', 'ш'),
    ('o', 'щ'),
    ('p', 'з'),
    ('a', 'ф'),
    ('s', 'ы'),
    ('d', 'в'),
    ('f', 'а'),
    ('g', 'п'),
    ('h', 'р'),
    ('j', 'о'),
    ('k', 'л'),
    ('l', 'д'),
    ('z', 'я'),
    ('x', 'ч'),
    ('c', 'с'),
    ('v', 'м'),
    ('b', 'и'),
    ('n', 'т'),
    ('m', 'ь'),
    (',', 'б'),
    ('.', 'ю'),
    (';', 'ж'),
    ('\'', 'э'),
    ('`', 'ё'),
    ('[', 'х'),
    (']', 'ъ'),
];

fn mapping() -> &'static HashMap<char, char> {
    static MAP: std::sync::OnceLock<HashMap<char, char>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        for (latin, cyr) in QWERTY_TO_RU {
            m.insert(latin, cyr);
            m.insert(cyr, latin);
            if latin.is_ascii_alphabetic() {
                m.insert(
                    latin.to_ascii_uppercase(),
                    cyr.to_uppercase().next().unwrap(),
                );
                m.insert(
                    cyr.to_uppercase().next().unwrap(),
                    latin.to_ascii_uppercase(),
                );
            }
        }
        // Shifted quote key: the same physical quote produces " in the English
        // layout and Э in the Russian layout.
        m.insert('"', 'Э');
        m.insert('Э', '"');

        // ё/Ё and the case variants are handled above.
        m
    })
}

/// Convert one character as if the same physical key had been pressed in the
/// other layout. Returns `None` for keys that are identical in both layouts.
pub fn map_key(c: char) -> Option<char> {
    mapping().get(&c).copied()
}

pub fn map_word(s: &str) -> String {
    s.chars().map(|c| map_key(c).unwrap_or(c)).collect()
}

pub fn is_cyrillic(c: char) -> bool {
    // «Ё»/«ё» (U+0401/U+0451) sit just outside А..я, so they need their own arm.
    matches!(c, 'А'..='я' | 'Ё' | 'ё')
}

pub fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Cyrillic,
    Latin,
}

pub fn script_of(s: &str) -> Option<Script> {
    let (mut cyr, mut lat) = (false, false);
    for c in s.chars() {
        if is_cyrillic(c) {
            cyr = true;
        } else if is_latin(c) {
            lat = true;
        }
    }
    match (cyr, lat) {
        (true, false) => Some(Script::Cyrillic),
        (false, true) => Some(Script::Latin),
        _ => None,
    }
}

/// Keys that produce a letter in one layout and punctuation in the other
/// (`, . / ; '` vs `б ю . ж э`). The switcher keeps them in the word buffer until the
/// real boundary arrives.
pub fn is_ambiguous(c: char) -> bool {
    match_key_letter(c)
}

/// Characters that close the pending word buffer, just like a space.
/// These are the punctuation/word-end keys the user asked for: `? & / ^`.
/// `б`/`ю` only close the buffer when they cannot be a letter of the word.
pub fn is_boundary_trigger(c: char) -> bool {
    matches!(c, '?' | '&' | '/' | '^')
}

/// `б` and `ю` live on the `,` and `.` keys: after a Latin word they close it.
pub fn is_latin_boundary_letter(c: char) -> bool {
    matches!(c, 'б' | 'ю' | 'Б' | 'Ю')
}

fn match_key_letter(c: char) -> bool {
    map_key(c).is_some_and(|m| m.is_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_is_ruddsh() {
        assert_eq!(map_word("hello"), "руддщ");
        assert_eq!(map_word("руддщ"), "hello");
        assert_eq!(map_word("More"), "Ьщку");
    }

    #[test]
    fn yo_key_is_the_backtick() {
        assert_eq!(map_key('`'), Some('ё'));
        assert_eq!(map_key('ё'), Some('`'));
        assert!(
            is_cyrillic('ё'),
            "ё={} я={} ё-range={}",
            is_cyrillic('ё'),
            is_cyrillic('я'),
            'Ё' <= 'ё' && 'ё' <= 'ё'
        );
    }

    #[test]
    fn script_detection() {
        assert_eq!(script_of("руддщ"), Some(Script::Cyrillic));
        assert_eq!(script_of("hello"), Some(Script::Latin));
        assert_eq!(script_of("привет123"), Some(Script::Cyrillic));
        assert_eq!(script_of("abcдеф"), None);
    }

    #[test]
    fn asked_boundary_chars_close_the_buffer() {
        for c in ['?', '&', '/', '^'] {
            assert!(is_boundary_trigger(c), "{c} must close the buffer");
        }
        for c in ['б', 'ю', 'Б', 'Ю'] {
            assert!(is_latin_boundary_letter(c), "{c} closes a Latin word");
            assert!(!is_boundary_trigger(c), "{c} still may be a letter");
        }
        assert!(!is_boundary_trigger('a'));
        assert!(!is_boundary_trigger('к'));
        assert!(!is_boundary_trigger('-'));
    }
}
