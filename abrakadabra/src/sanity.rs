//! End-to-end accuracy measurement of the engine:
//!  * recall    — correctly typed-in-the-wrong-layout words that get fixed;
//!  * precision — correctly typed words that must NOT be touched.
//!
//! Both are measured against the top 2000 words of each language in the model.

use crate::engine::Engine;
use crate::layout::map_word;
use crate::model::Model;

pub fn run() -> anyhow::Result<()> {
    let engine = Engine::new(Model::load()?, crate::config::load());

    let ru = engine.model.ru.top_words(2000);
    let en = engine.model.en.top_words(2000);

    // 1. Words typed in the wrong layout must be repaired.
    let mut total = 0usize;
    let mut misses = Vec::new();
    for w in ru.iter().chain(en.iter()) {
        if w.chars().count() < 3 {
            continue;
        }
        total += 1;
        let typed = map_word(w);
        match engine.decide(&typed) {
            Some(d) if d.converted == *w => (),
            Some(d) => misses.push(format!("{typed} -> {}", d.converted)),
            None => misses.push(format!("{typed} -> (miss)")),
        }
    }
    println!(
        "recall    {}/{} = {:.1}%  (слова, набранные не в той раскладке, исправлены)",
        total - misses.len(),
        total,
        100.0 * (total - misses.len()) as f64 / total as f64
    );
    for m in misses.iter().take(20) {
        println!("  miss: {m}");
    }

    // 2. Correctly typed words must survive untouched.
    let (mut touched, mut total2) = (0usize, 0usize);
    for w in ru.iter().chain(en.iter()) {
        if w.chars().count() < 3 {
            continue;
        }
        total2 += 1;
        if engine.decide(w).is_some() {
            touched += 1;
        }
    }
    println!(
        "precision {}/{} = {:.2}%  (ложных срабатываний: {}) ",
        total2 - touched,
        total2,
        100.0 * (total2 - touched) as f64 / total2 as f64,
        touched
    );
    Ok(())
}
