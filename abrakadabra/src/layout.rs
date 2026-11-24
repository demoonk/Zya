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

/// The shifted layer of the keys that carry a Cyrillic letter in the Russian
/// layout. `; , . [ ] \`` are `ж б ю х ъ ё` without Shift, and `: < > { } ~`
/// with it — so the capitals `Ж Б Ю Х Ъ Ё` live on those symbols. The generic
/// case-pairing loop above cannot create these pairs: the "latin" side of the
/// key is punctuation, not an ASCII letter.
const SHIFTED_QWERTY_TO_RU: [(char, char); 6] = [
    (':', 'Ж'),
    ('<', 'Б'),
    ('>', 'Ю'),
    ('{', 'Х'),
    ('}', 'Ъ'),
    ('~', 'Ё'),
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
        // Shifted punctuation keys: `;`/`:` both carry ж/Ж, `,`/`<` carry
        // б/Б, and so on. Both directions, so a capital Ё/Ж/Б/Ю/Х/Ъ written in
        // the Russian layout maps back to `~ : < > { }` in the English one.
        for (shifted, cyr) in SHIFTED_QWERTY_TO_RU {
            m.insert(shifted, cyr);
            m.insert(cyr, shifted);
        }
        // Shifted quote key: the same physical quote produces " in the English
        // layout and Э in the Russian layout.
        m.insert('"', 'Э');
        m.insert('Э', '"');

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
/// (`, . ; ' [ ] \`` and their shifted `: < > { } ~` vs `б ю ж э х ъ ё`). The
/// switcher keeps them in the word buffer until the real boundary arrives.
pub fn is_ambiguous(c: char) -> bool {
    match_key_letter(c)
}

/// Characters that close the pending word buffer, just like a space.
/// These are the punctuation/word-end keys the user asked for: `? & / ^`.
/// `б`/`ю` only close the buffer when they cannot be a letter of the word.
pub fn is_boundary_trigger(c: char) -> bool {
    matches!(c, '?' | '&' | '/' | '^')
}

/// What one press of the manual switch (Break) should do to the last token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualMove {
    /// The text that is in the document now.
    pub from: String,
    /// The text to write in its place.
    pub to: String,
    /// The separator that closed the token, re-injected after the rewrite.
    pub boundary: Option<char>,
    /// Whether the keyboard should be in the Russian layout afterwards.
    pub russian_after: bool,
    /// Recorded so the next press gives the text back. `None` for an undo press.
    pub undo: Option<(String, String, Option<char>, bool)>,
    pub why: &'static str,
}

/// The last letter of a Cyrillic token sits on a punctuation key: `,`->`б`,
/// `.`->`ю`, `;`->`ж`. Returns the stem and the signs it stands for, shortest
/// run first, so "фактбю" is tried as "фактб." before "факт,.".
pub fn punctuation_tail(token: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = token.chars().collect();
    let punct = |c: char| -> Option<char> {
        match c {
            'ю' => Some('.'),
            'Ю' => Some('.'),
            'б' => Some(','),
            'Б' => Some(','),
            'ж' => Some(';'),
            'Ж' => Some(':'),
            _ => None,
        }
    };
    if !chars.iter().any(|c| is_cyrillic(*c)) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for k in 1..=3 {
        if chars.len() <= k {
            break;
        }
        let tail: Vec<char> = chars[chars.len() - k..].to_vec();
        if tail.iter().any(|&c| punct(c).is_none()) {
            break;
        }
        let stem: String = chars[..chars.len() - k].iter().collect();
        if !stem.chars().any(|c| c.is_alphanumeric()) {
            continue;
        }
        if stem
            .chars()
            .any(|c| !c.is_alphanumeric() && c != '-' && c != '\'' && c != '.' && c != ',')
        {
            continue;
        }
        let signs: String = tail.iter().map(|&c| punct(c).unwrap()).collect();
        out.push((stem, signs));
    }
    out
}

/// Mirror only the letters of a token. A sign that is already punctuation stays
/// a sign: "vtyztncz:" is "меняется:", not "меняетсяЖ".
pub fn mirror_token(token: &str) -> String {
    token
        .chars()
        .map(|c| {
            if c.is_alphabetic() {
                map_key(c).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// Break is a toggle. The first press mirrors the last token; a second press on
/// the *same* token hands back the text that was there before the first press,
/// taken verbatim from the record. Mirroring it again is the dangerous part: a
/// token the automatic layer has already half-corrected ("факт,." -> "фактб;")
/// loses or gains a character on every pass.
pub fn plan_manual_move(
    word: &str,
    closed: Option<(String, Option<char>)>,
    manual: &Option<(String, String, Option<char>, bool)>,
    russian: bool,
    smart: bool,
    believable: &dyn Fn(&str) -> bool,
) -> Option<ManualMove> {
    let (token, boundary) = if !word.is_empty() {
        (word.to_string(), None)
    } else if let Some((t, b)) = closed {
        (t, b)
    } else {
        return None;
    };

    // "Умная отмена" on: the second press on the same token writes back the text
    // recorded by the first one, verbatim. With it off the token is mirrored
    // again, which is its own inverse in the classic scheme.
    if smart {
        if let Some((before, after, b, russian_before)) = manual.clone() {
            if token == after && boundary == b {
                return Some(ManualMove {
                    from: after,
                    to: before,
                    boundary: b,
                    russian_after: russian_before,
                    undo: None,
                    why: "вручную: Break ещё раз — возврат последнего слова",
                });
            }
        }
    }

    // Two readings of the same token. The classic one: the whole token was
    // typed with the layout still in the other language. The smart one: only the
    // last key was wrong — `,` `.` `;` were pressed while the layout was Russian
    // and came out as `б` `ю` `ж`.
    let mirrored = mirror_token(&token);
    let mirror_ok = mirrored != token && believable(&mirrored);

    if smart && !mirror_ok {
        // Longest run first: in "фактбю" the tail is `,.` (the user pressed the
        // comma and the dot), not just the last `ю`. Trying the shortest run first
        // would leave the `б` in the word and produce "фактб.".
        for (stem, signs) in punctuation_tail(&token).into_iter().rev() {
            if believable(&stem) {
                let to = format!("{stem}{signs}");
                return Some(ManualMove {
                    undo: Some((token.clone(), to.clone(), boundary, russian)),
                    from: token,
                    to,
                    boundary,
                    russian_after: true,
                    why: "вручную: буквы на месте знаков препинания",
                });
            }
        }
    }

    if mirrored != token {
        let russian_after = mirrored.chars().any(is_cyrillic);
        return Some(ManualMove {
            undo: Some((token.clone(), mirrored.clone(), boundary, russian)),
            from: token,
            to: mirrored,
            boundary,
            russian_after,
            why: "вручную: смена раскладки последнего слова",
        });
    }
    None
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

    /// A miniature dictionary for the Break tests: enough to tell a real stem
    /// from a mirror of gibberish.
    fn believable(s: &str) -> bool {
        let letters: String = s
            .chars()
            .filter(|c| c.is_alphabetic())
            .map(|c| c.to_lowercase().next().unwrap_or(c))
            .collect();
        const WORDS: [&str; 10] = [
            "привет", "мир", "факт", "да", "по", "iface", "коронавируса", "колю", "gps", "it",
        ];
        WORDS.contains(&letters.as_str())
    }

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

    /// L-07: the shifted layer of `, . ; [ ] \`` carries the capitals Б Ю Ж Х
    /// Ё Ъ, so `~ : < > { }` must map in both directions.
    #[test]
    fn shifted_keys_map_to_their_capitals_both_ways() {
        for (latin, cyr) in [
            ('~', 'Ё'),
            (':', 'Ж'),
            ('<', 'Б'),
            ('>', 'Ю'),
            ('{', 'Х'),
            ('}', 'Ъ'),
        ] {
            assert_eq!(map_key(latin), Some(cyr), "{latin} should map to {cyr}");
            assert_eq!(map_key(cyr), Some(latin), "{cyr} should map back to {latin}");
            assert!(is_ambiguous(latin), "{latin} is an ambiguous key");
        }
        // The unshifted keys keep their lowercase letters.
        for (latin, cyr) in [('`', 'ё'), (',', 'б'), ('.', 'ю'), (';', 'ж'), ('[', 'х'), (']', 'ъ')] {
            assert_eq!(map_key(latin), Some(cyr), "{latin} -> {cyr}");
            assert_eq!(map_key(cyr), Some(latin), "{cyr} -> {latin}");
        }
    }

    #[test]
    fn capitalized_words_on_punctuation_keys_round_trip() {
        for (latin, cyr) in [
            ("~krf", "Ёлка"),
            (":er", "Жук"),
            ("<tlf", "Беда"),
            (">kf", "Юла"),
            ("{kt,", "Хлеб"),
            ("}tplf", "Ъезда"),
        ] {
            assert_eq!(map_word(latin), cyr, "{latin}");
            assert_eq!(map_word(cyr), latin, "{cyr}");
        }
        // A whole phrase made of keys that are punctuation in the other layout.
        assert_eq!(map_word(":er ~krf"), "Жук Ёлка");
        assert_eq!(map_word("Жук Ёлка"), ":er ~krf");
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

    #[test]
    fn break_presses_are_a_toggle_not_a_chain_of_conversions() {
        // "привет " is in the document and the keyboard is Russian. One press
        // mirrors the token, the second gives back the exact original text.
        let first = plan_manual_move(
            "",
            Some(("привет".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("Break has a token to switch");
        assert_eq!(first.from, "привет");
        assert_eq!(first.to, "ghbdtn");
        assert_eq!(first.boundary, Some(' '));
        assert!(!first.russian_after);

        let second = plan_manual_move(
            "",
            Some((first.to.clone(), Some(' '))),
            &first.undo,
            false,
            true,
            &believable,
        )
        .expect("the second Break is an undo");
        assert_eq!(second.to, "привет", "the undo must be the original text");
        assert!(second.russian_after, "the layout goes back too");
        assert!(second.undo.is_none(), "the undo is spent after one press");
    }

    #[test]
    fn the_undo_writes_the_recorded_text_not_another_mirror() {
        // The automatic layer had already changed "фактбю" into "факт,."; Break
        // mirrored that into "afrtбю". The second Break must put back "факт,."
        // exactly as it was — the record, not a fresh pass over the mapping.
        let manual = Some(("факт,.".to_string(), "afrtбю".to_string(), None, true));
        let mv = plan_manual_move(
            "",
            Some(("afrtбю".to_string(), None)),
            &manual,
            false,
            true,
            &believable,
        )
        .expect("the second press is an undo");
        assert_eq!(mv.to, "факт,.");
        assert!(mv.russian_after);
    }

    #[test]
    fn the_undo_only_applies_to_the_token_it_was_recorded_for() {
        let manual = Some(("привет".to_string(), "ghbdtn".to_string(), None, true));
        // The user carried on typing: Break now works on the new token.
        let mv = plan_manual_move("yjkyf", None, &manual, false, true, &believable).expect("a new token");
        assert_eq!(mv.from, "yjkyf");
        assert_eq!(mv.to, "нолна");
        assert!(mv.russian_after);
    }

    #[test]
    fn a_token_with_no_other_reading_is_left_alone() {
        assert!(
            plan_manual_move(
                "",
                Some(("12345".to_string(), None)),
                &None,
                false,
                true,
                &believable
            )
            .is_none(),
            "digits have no other layout"
        );
    }

    /// Smart Break: "коронавирусаю" is not a word, "коронавируса" is, so only the
    /// last key — the `.` eaten by `ю` — has to be put back.
    #[test]
    fn the_smart_break_restores_eaten_punctuation() {
        let mv = plan_manual_move(
            "",
            Some(("коронавирусаю".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("Break has a token to switch");
        assert_eq!(mv.from, "коронавирусаю");
        assert_eq!(mv.to, "коронавируса.");
        assert_eq!(mv.boundary, Some(' '));
        assert!(mv.russian_after, "the word stays Russian, the layout does not flip");

        let back = plan_manual_move(
            "",
            Some((mv.to.clone(), Some(' '))),
            &mv.undo,
            true,
            true,
            &believable,
        )
        .expect("the second press is an undo");
        assert_eq!(back.to, "коронавирусаю", "the undo is verbatim, not another mirror");
    }

    /// A real Russian word ending in `ю` is never cut down to a stem plus a dot:
    /// "колю" has no mirror worth mirroring to, and "кол" is not what was typed.
    #[test]
    fn a_word_that_is_a_word_is_not_cut_at_the_punctuation_key() {
        let mv = plan_manual_move(
            "",
            Some(("колю".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("Break still mirrors the whole word");
        assert_eq!(mv.to, "rjk.", "the classic mirror of «колю», not «кол.»");
        assert_eq!(mv.from, "колю");
    }

    /// A short word keeps its comma: "да," came out as "Даб", and the mirror of
    /// the whole token ("Lf,") is not a word, so only the last key is wrong.
    #[test]
    fn a_short_word_reclaims_its_comma() {
        let mv = plan_manual_move(
            "",
            Some(("Даб".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("the comma key can be read back");
        assert_eq!(mv.to, "Да,", "the capital stays, the eaten comma comes back");
    }

    /// A Latin token with the punctuation letter glued to it is repaired by the
    /// key, not by mirroring the whole word into Russian gibberish.
    #[test]
    fn a_latin_token_reclaims_its_dot() {
        let mv = plan_manual_move(
            "",
            Some(("ifaceб".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("iface is a word, so the trailing letter is the dot");
        assert_eq!(mv.to, "iface,");
    }

    /// The same key pressed several times: "по..." typed with the layout in the
    /// wrong place comes out as "поююю".
    #[test]
    fn a_run_of_punctuation_keys_is_read_as_several_signs() {
        let mv = plan_manual_move(
            "",
            Some(("поююю".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("the run of ю is a run of dots");
        assert_eq!(mv.to, "по...");
    }

    /// "фактбю" is "факт,." typed while the layout was Russian. The longest run of
    /// punctuation keys is the right reading: the shortest one would leave a stray
    /// "б" at the end of the word.
    #[test]
    fn a_comma_and_a_dot_are_read_together() {
        let mv = plan_manual_move("фактбю", None, &None, true, true, &believable)
            .expect("the tail is punctuation");
        assert_eq!(mv.to, "факт,.");
    }

    /// When the mirror really is a word, the classic reading wins.
    #[test]
    fn a_real_mirror_still_wins_over_the_punctuation_reading() {
        let mv = plan_manual_move(
            "",
            Some(("ПЗЫб".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("GPS is the only reading that makes sense");
        assert_eq!(mv.to, "GPS,");
    }

    /// «Умная отмена» off: the eaten punctuation key is not read, the whole token
    /// is mirrored, exactly as in the classic version.
    #[test]
    fn the_punctuation_reading_is_off_when_smart_undo_is_off() {
        let smart = plan_manual_move(
            "",
            Some(("коронавирусаю".to_string(), Some(' '))),
            &None,
            true,
            true,
            &believable,
        )
        .expect("the smart reading has a tail to cut");
        assert_eq!(smart.to, "коронавируса.");

        let classic = plan_manual_move(
            "",
            Some(("коронавирусаю".to_string(), Some(' '))),
            &None,
            true,
            false,
            &believable,
        )
        .expect("the classic Break mirrors the whole token");
        assert_eq!(classic.to, "rjhjyfdbhecf.");
    }

    /// With the smart reading off the second press mirrors the token back: the
    /// recorded undo is not used, so nothing is written from it.
    #[test]
    fn the_classic_break_mirrors_back_and_not_from_the_record() {
        let first = plan_manual_move(
            "",
            Some(("привет".to_string(), Some(' '))),
            &None,
            true,
            false,
            &believable,
        )
        .expect("the classic Break mirrors");
        assert_eq!(first.to, "ghbdtn");

        // A record left by an earlier smart press must not resurrect the old text
        // while the smart mode is off: the mirror of "ghbdtn" is "привет" anyway,
        // and the undo field of a classic move is empty.
        let stale = Some(("факт,.".to_string(), "afrtбю".to_string(), None, true));
        let second = plan_manual_move(
            "",
            Some((first.to.clone(), Some(' '))),
            &stale,
            false,
            false,
            &believable,
        )
        .expect("the second classic press mirrors back");
        assert_eq!(second.to, "привет");
        assert_eq!(second.from, "ghbdtn");
    }
}
