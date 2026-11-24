//! Character-level language model: unigrams, bigrams, trigrams and a top-word
//! dictionary. Statistics come from tools/build_data.py and are embedded in the
//! binary, so the switcher has no external data files at runtime.

use std::collections::{HashMap, HashSet};
use std::cmp::Ordering;

use serde::Deserialize;

const RAW: &[u8] = include_bytes!("../data/lang.json");
const TECH: &[u8] = include_bytes!("../data/tech.json");

#[derive(Debug, Deserialize)]
struct RawTech {
    tokens: Vec<(String, u32)>,
}

#[derive(Debug, Deserialize)]
struct RawLang {
    letters: String,
    uni_unseen: f32,
    bi_unseen: f32,
    tri_unseen: f32,
    baseline: Vec<f32>,
    unigrams: Vec<(String, f32)>,
    bigrams: Vec<(String, String, f32)>,
    trigrams: Vec<(String, String, String, f32)>,
    words: Vec<(String, f32)>,
}

#[derive(Debug, Deserialize)]
struct RawModel {
    ru: RawLang,
    en: RawLang,
}

pub struct Lang {
    n: usize,
    idx: HashMap<char, usize>,
    uni: Vec<f32>,
    bi: Vec<f32>,
    tri: HashMap<usize, f32>,
    tri_unseen: f32,
    baseline: Vec<f32>,
    words: HashMap<String, f32>,
    /// The dictionary keys sorted by frequency (ties broken alphabetically),
    /// computed once at load time so `top_words` is both correct and
    /// deterministic across runs (L-13).
    top: Vec<String>,
    /// Every three-letter tail that really closes a word of this language.
    endings: HashSet<usize>,
}

impl Lang {
    fn from_raw(raw: RawLang) -> Lang {
        let letters: Vec<char> = raw.letters.chars().collect();
        let n = letters.len();
        let idx: HashMap<char, usize> = letters.iter().copied().zip(0..).collect();

        let mut uni = vec![raw.uni_unseen; n];
        for (ch, lp) in raw.unigrams {
            if let Some(&i) = idx.get(&ch.chars().next().unwrap()) {
                uni[i] = lp;
            }
        }
        let mut bi = vec![raw.bi_unseen; n * n];
        for (a, b, lp) in raw.bigrams {
            let (x, y) = (
                idx[&a.chars().next().unwrap()],
                idx[&b.chars().next().unwrap()],
            );
            bi[x * n + y] = lp;
        }
        let mut tri: HashMap<usize, f32> = HashMap::with_capacity(raw.trigrams.len());
        for (a, b, c, lp) in raw.trigrams {
            let key = (idx[&a.chars().next().unwrap()] * n + idx[&b.chars().next().unwrap()]) * n
                + idx[&c.chars().next().unwrap()];
            tri.insert(key, lp);
        }
        // The 50k lists are scraped from corpora, so they also carry scraps that
        // are not words. Removing them would also remove rare but genuine forms
        // like "мышью", so the dictionary is kept whole and the rules decide.
        let words: HashMap<String, f32> = raw.words.into_iter().collect();
        // L-13: "top words" must be the *most frequent* ones, in a stable order.
        // Iterating a HashMap gives a seed-dependent sequence, which made the
        // accuracy tests flaky from run to run.
        let mut ranked: Vec<(String, f32)> =
            words.iter().map(|(w, &lp)| (w.clone(), lp)).collect();
        ranked.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        let top: Vec<String> = ranked.into_iter().map(|(w, _)| w).collect();
        let mut endings: HashSet<usize> = HashSet::with_capacity(words.len());
        for w in words.keys() {
            let tail: Vec<usize> = w.chars().filter_map(|c| idx.get(&c).copied()).collect();
            if tail.len() >= 3 {
                endings.insert(
                    (tail[tail.len() - 3] * n + tail[tail.len() - 2]) * n + tail[tail.len() - 1],
                );
            }
        }
        Lang {
            n,
            idx,
            uni,
            bi,
            tri,
            tri_unseen: raw.tri_unseen,
            baseline: raw.baseline,
            words,
            top,
            endings,
        }
    }

    pub fn normalize(w: &str) -> String {
        w.chars()
            .filter(|c| c.is_alphabetic())
            .map(|c| {
                if c == 'Ё' || c == 'ё' {
                    'е'
                } else {
                    c.to_lowercase().next().unwrap_or(c)
                }
            })
            .collect()
    }

    /// Raw n-gram log probability. `None` if the word uses letters from another
    /// alphabet.
    fn raw_score(&self, w: &str) -> Option<f32> {
        let w = Self::normalize(w);
        if w.is_empty() {
            return None;
        }
        let mut chars: Vec<usize> = Vec::with_capacity(w.len());
        for c in w.chars() {
            if !c.is_alphabetic() {
                return None;
            }
            chars.push(*self.idx.get(&c)?);
        }
        let mut s = 0.0;
        for &i in &chars {
            s += self.uni[i];
        }
        for pair in chars.windows(2) {
            s += self.bi[pair[0] * self.n + pair[1]];
        }
        for t in chars.windows(3) {
            let key = (t[0] * self.n + t[1]) * self.n + t[2];
            s += self.tri.get(&key).copied().unwrap_or(self.tri_unseen);
        }
        Some(s)
    }

    /// Score normalised by the average score of real words of this length in
    /// this language, so Russian and English are directly comparable.
    pub fn score(&self, w: &str) -> Option<f32> {
        let s = self.raw_score(w)?;
        let len = Self::normalize(w).chars().count();
        let base = self
            .baseline
            .get(len)
            .copied()
            .unwrap_or_else(|| self.baseline[5.min(self.baseline.len() - 1)]);
        Some(s - base)
    }

    pub fn has_word(&self, w: &str) -> bool {
        self.words.contains_key(&Self::normalize(w))
    }

    /// Can a word of this language really end with these letters? Used for the
    /// `,`/`.`/`;` keys that sit under б, ю and ж: "фактб" ends in "ктб", which
    /// no Russian word does, while "мышью" ends in "ью", which is ordinary.
    pub fn ends_like_word(&self, w: &str) -> bool {
        let w = Self::normalize(w);
        let tail: Vec<usize> = w
            .chars()
            .filter_map(|c| self.idx.get(&c).copied())
            .collect();
        if tail.len() < 3 {
            return self.words.contains_key(&w);
        }
        let key =
            (tail[tail.len() - 3] * self.n + tail[tail.len() - 2]) * self.n + tail[tail.len() - 1];
        self.endings.contains(&key)
    }

    /// Log frequency of a dictionary word, `None` if it is not in the list.
    /// Short tokens are only trusted when they are frequent: the top-50k list
    /// also contains corpus noise such as "ghb", which is not an English word.
    pub fn word_lp(&self, w: &str) -> Option<f32> {
        self.words.get(&Self::normalize(w)).copied()
    }

    /// Log probability of a letter pair in this language. `None` when either
    /// letter does not belong to the alphabet.
    pub fn bigram_lp(&self, a: char, b: char) -> Option<f32> {
        let a = a.to_lowercase().next().unwrap_or(a);
        let b = b.to_lowercase().next().unwrap_or(b);
        let &x = self.idx.get(&a)?;
        let &y = self.idx.get(&b)?;
        Some(self.bi[x * self.n + y])
    }

    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Log probability of a letter triple; unseen triples carry the model's
    /// generic penalty, so the caller can tell "rare" from "never happens".
    pub fn trigram_lp(&self, a: char, b: char, c: char) -> Option<f32> {
        let a = a.to_lowercase().next().unwrap_or(a);
        let b = b.to_lowercase().next().unwrap_or(b);
        let c = c.to_lowercase().next().unwrap_or(c);
        let a = if a == 'ё' { 'е' } else { a };
        let b = if b == 'ё' { 'е' } else { b };
        let c = if c == 'ё' { 'е' } else { c };
        let &x = self.idx.get(&a)?;
        let &y = self.idx.get(&b)?;
        let &z = self.idx.get(&c)?;
        let key = (x * self.n + y) * self.n + z;
        Some(self.tri.get(&key).copied().unwrap_or(self.tri_unseen))
    }

    pub fn top_words(&self, k: usize) -> Vec<String> {
        self.top.iter().take(k).cloned().collect()
    }
}

pub struct Model {
    pub ru: Lang,
    pub en: Lang,
    /// Technical vocabulary harvested from 3dnews/ixbt articles: brands,
    /// standards and model numbers (ddr4, nvme, rtx5060, oled, ...).
    pub tech: HashSet<String>,
}

impl Model {
    pub fn load() -> anyhow::Result<Model> {
        let raw: RawModel = serde_json::from_slice(RAW)?;
        let tech: RawTech = serde_json::from_slice(TECH)?;
        let mut set = HashSet::with_capacity(tech.tokens.len());
        for (w, _) in tech.tokens {
            set.insert(normalize_tech(&w));
        }
        Ok(Model {
            ru: Lang::from_raw(raw.ru),
            en: Lang::from_raw(raw.en),
            tech: set,
        })
    }

    /// `true` if the token is a known technical term (case and separators
    /// ignored: "DDR4", "ddr-4", "ddr4" are the same term).
    pub fn is_tech(&self, token: &str) -> bool {
        self.tech.contains(&normalize_tech(token))
    }

    pub fn tech_len(&self) -> usize {
        self.tech.len()
    }
}

fn normalize_tech(w: &str) -> String {
    w.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_words_beat_gibberish() {
        let m = Model::load().unwrap();
        assert!(m.en.score("hello").unwrap() > m.ru.score("руддщ").unwrap());
        assert!(m.ru.score("привет").unwrap() > m.en.score("ghbdtn").unwrap());
        assert!(m.en.score("more").unwrap() > m.ru.score("ьщку").unwrap());
        assert!(!m.ru.has_word("руддщ"));
        assert!(m.ru.has_word("привет"));
    }
}
