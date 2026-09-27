//! Scores what the recognizer heard against the target phrase.
//!
//! Learners often wrap the answer in their own language ("hmm, é… I'm hungry"),
//! so the score is the best-matching *window* of heard words, compared with
//! case, punctuation, apostrophes and accents folded away.

use strsim::normalized_levenshtein;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

#[derive(Debug, Clone, PartialEq)]
pub struct WordHit {
    pub word: String,
    pub hit: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchResult {
    /// 0..=1 similarity of the best window.
    pub score: f32,
    /// Target words (original spelling) with whether each was heard.
    pub words: Vec<WordHit>,
}

impl MatchResult {
    pub fn passed(&self, threshold: f32) -> bool {
        self.score >= threshold
    }
}

/// Spoken-English contractions: both sides are expanded so "I'm" == "I am".
const CONTRACTIONS: &[(&str, &str)] = &[
    ("im", "i am"),
    ("youre", "you are"),
    ("were", "we are"),
    ("theyre", "they are"),
    ("dont", "do not"),
    ("doesnt", "does not"),
    ("cant", "can not"),
    ("cannot", "can not"),
    ("wont", "will not"),
    ("isnt", "is not"),
    ("its", "it is"),
    ("whats", "what is"),
    ("thats", "that is"),
    ("lets", "let us"),
    ("id", "i would"),
    ("ill", "i will"),
];

fn fold_word(w: &str) -> String {
    w.nfd().filter(|c| !is_combining_mark(*c)).collect::<String>()
}

/// Lowercased, accent-folded, punctuation-free words with contractions expanded.
pub fn normalize(text: &str) -> Vec<String> {
    let lowered = text.to_lowercase().replace(['’', '‘', '\''], "");
    let cleaned: String = lowered.chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let mut out = Vec::new();
    for w in cleaned.split_whitespace() {
        let w = fold_word(w);
        match CONTRACTIONS.iter().find(|(k, _)| *k == w) {
            Some((_, full)) => out.extend(full.split(' ').map(str::to_string)),
            None => out.push(w),
        }
    }
    out
}

fn similar(a: &str, b: &str) -> bool {
    a == b || normalized_levenshtein(a, b) >= 0.75
}

pub fn score(target: &str, heard: &str) -> MatchResult {
    let t = normalize(target);
    let h = normalize(heard);

    // Per original target word: is it (approximately) in what was heard?
    let words = target
        .split_whitespace()
        .map(|orig| {
            let parts = normalize(orig);
            let hit = !parts.is_empty() && parts.iter().all(|p| h.iter().any(|hw| similar(p, hw)));
            WordHit { word: orig.to_string(), hit }
        })
        .collect();

    if t.is_empty() || h.is_empty() {
        return MatchResult { score: 0.0, words };
    }

    let target_joined = t.join(" ");
    let n = t.len();
    let mut best = 0.0f64;
    for len in n.saturating_sub(1).max(1)..=(n + 1) {
        let len = len.min(h.len());
        for start in 0..=(h.len() - len) {
            let window = h[start..start + len].join(" ");
            best = best.max(normalized_levenshtein(&target_joined, &window));
        }
    }
    MatchResult { score: best as f32, words }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASS: f32 = 0.72;

    #[test]
    fn exact_answer_scores_perfectly() {
        let r = score("I'm hungry", "I'm hungry.");
        assert!((r.score - 1.0).abs() < 1e-6);
        assert!(r.words.iter().all(|w| w.hit));
    }

    #[test]
    fn case_punctuation_and_contractions_do_not_matter() {
        assert!(score("I'm hungry", "i am HUNGRY!!").passed(PASS));
        assert!(score("I don't understand", "I do not understand").passed(PASS));
    }

    #[test]
    fn missing_accents_and_spanish_punctuation_still_pass() {
        assert!(score("¿Dónde está el baño?", "donde esta el bano").passed(PASS));
        assert!(score("Tengo frío", "Tengo frio.").passed(PASS));
    }

    #[test]
    fn answer_wrapped_in_portuguese_still_passes() {
        let r = score("I'm hungry", "hmm é, deixa eu ver, I'm hungry, acho");
        assert!(r.passed(PASS), "score {}", r.score);
    }

    #[test]
    fn small_recognition_slips_pass_but_wrong_phrase_fails() {
        assert!(score("Good morning", "good mourning").passed(PASS));
        assert!(!score("Good morning", "good night").passed(PASS));
        assert!(!score("Where is the bathroom?", "I like pizza very much").passed(PASS));
    }

    #[test]
    fn partial_answer_fails_and_marks_missing_words() {
        let r = score("Can I have the check, please?", "can I have");
        assert!(!r.passed(PASS), "score {}", r.score);
        let missing: Vec<_> = r.words.iter().filter(|w| !w.hit).map(|w| w.word.as_str()).collect();
        assert_eq!(missing, vec!["the", "check,", "please?"]);
    }

    #[test]
    fn empty_or_silent_input_scores_zero() {
        assert_eq!(score("Hello", "").score, 0.0);
        assert_eq!(score("Hello", " ... ").score, 0.0);
        assert!(score("Hello", "").words.iter().all(|w| !w.hit));
    }

    #[test]
    fn single_word_target_works_with_longer_answers() {
        assert!(score("Vamos", "vamos!").passed(PASS));
        assert!(score("Vamos", "bom, vamos lá").passed(PASS));
        assert!(!score("Vamos", "banana").passed(PASS));
    }
}
