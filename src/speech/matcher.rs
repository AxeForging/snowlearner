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
    /// Passes on overall similarity, or when every target word was heard
    /// (each allowed a small slip) — recognition noise must not fail a learner
    /// who said it right.
    ///
    /// A phrase that is merely *similar* (score ≥ threshold) must still have at
    /// least 3 of every 4 words right, so "I'm angry" never passes for
    /// "I'm hungry" just because the letters look alike.
    pub fn passed(&self, threshold: f32) -> bool {
        if self.words.is_empty() {
            return false;
        }
        let hits = self.words.iter().filter(|w| w.hit).count() as f32;
        let ratio = hits / self.words.len() as f32;
        ratio >= 1.0 || (self.score >= threshold && ratio >= 0.75)
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

/// Same word, allowing 1 slip in words of 3–6 letters and 2 in longer ones
/// (recognizer slips like "hungre", "repeet", "tomorow") — but not enough to
/// turn "hungry" into "angry".
fn similar(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let len = a.chars().count().max(b.chars().count());
    let allowed = if len <= 2 {
        0
    } else if len <= 6 {
        1
    } else {
        2
    };
    strsim::levenshtein(a, b) <= allowed
}

/// Best score over several accepted answers; returns it with the answer's index.
pub fn score_any(answers: &[&str], heard: &str) -> (MatchResult, usize) {
    answers
        .iter()
        .enumerate()
        .map(|(i, a)| (score(a, heard), i))
        .max_by(|a, b| a.0.score.total_cmp(&b.0.score))
        .unwrap_or_else(|| (score("", heard), 0))
}

/// Phrases whisper is known to invent on silence/noise (trained on subtitles).
const HALLUCINATIONS: &[&str] = &[
    "thank you for watching",
    "thanks for watching",
    "thank you",
    "subtitles by the amara org community",
    "please subscribe",
    "you",
    "obrigado por assistir",
    "legendas pela comunidade amara org",
    "gracias por ver",
    "subtitulos realizados por la comunidad de amara org",
];

/// True when the whole transcript is one of whisper's stock hallucinations.
pub fn is_hallucination(text: &str) -> bool {
    let norm = normalize(text).join(" ");
    norm.is_empty() || HALLUCINATIONS.iter().any(|h| normalize(h).join(" ") == norm)
}

/// Number names (accent-folded) for the digits whisper writes instead of the
/// word the learner said: English, Spanish, Portuguese.
const NUMBER_WORDS: &[(&str, &[&str])] = &[
    ("0", &["zero", "cero"]),
    ("1", &["one", "uno", "una", "un", "um", "uma"]),
    ("2", &["two", "dos", "dois", "duas"]),
    ("3", &["three", "tres"]),
    ("4", &["four", "cuatro", "quatro"]),
    ("5", &["five", "cinco"]),
    ("6", &["six", "seis"]),
    ("7", &["seven", "siete", "sete"]),
    ("8", &["eight", "ocho", "oito"]),
    ("9", &["nine", "nueve", "nove"]),
    ("10", &["ten", "diez", "dez"]),
    ("11", &["eleven", "once", "onze"]),
    ("12", &["twelve", "doce", "doze"]),
    ("13", &["thirteen", "trece", "treze"]),
    ("14", &["fourteen", "catorce", "catorze", "quatorze"]),
    ("15", &["fifteen", "quince", "quinze"]),
    ("16", &["sixteen", "dieciseis", "dezesseis"]),
    ("17", &["seventeen", "diecisiete", "dezessete"]),
    ("18", &["eighteen", "dieciocho", "dezoito"]),
    ("19", &["nineteen", "diecinueve", "dezenove"]),
    ("20", &["twenty", "veinte", "vinte"]),
    ("30", &["thirty", "treinta", "trinta"]),
    ("40", &["forty", "cuarenta", "quarenta"]),
    ("50", &["fifty", "cincuenta", "cinquenta"]),
    ("60", &["sixty", "sesenta", "sessenta"]),
    ("70", &["seventy", "setenta"]),
    ("80", &["eighty", "ochenta", "oitenta"]),
    ("90", &["ninety", "noventa"]),
];

/// Reads the heard text the way the target is written: a digit becomes the
/// target's number word ("3" → "three" when "three" is expected), and a word
/// the recognizer split ("yes ter day") is joined back when that spells a
/// target word exactly.
fn align_to_target(target: &[String], heard: Vec<String>) -> Vec<String> {
    let heard: Vec<String> = heard
        .into_iter()
        .map(|w| {
            NUMBER_WORDS
                .iter()
                .find(|(digits, _)| *digits == w)
                .and_then(|(_, names)| target.iter().find(|t| names.contains(&t.as_str())))
                .cloned()
                .unwrap_or(w)
        })
        .collect();
    let mut out = Vec::with_capacity(heard.len());
    let mut i = 0;
    while i < heard.len() {
        let joined = (2..=3).rev().filter(|n| i + n <= heard.len()).find_map(|n| {
            let word = heard[i..i + n].concat();
            target.contains(&word).then_some((word, n))
        });
        match joined {
            Some((word, n)) if !target.contains(&heard[i]) => {
                out.push(word);
                i += n;
            }
            _ => {
                out.push(heard[i].clone());
                i += 1;
            }
        }
    }
    out
}

pub fn score(target: &str, heard: &str) -> MatchResult {
    let t = normalize(target);
    let h = align_to_target(&t, normalize(heard));

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
    fn any_accepted_variant_can_win() {
        let (m, i) = score_any(&["Could you repeat that?", "Can you repeat that?"], "can you repeat that");
        assert!((m.score - 1.0).abs() < 1e-6);
        assert_eq!(i, 1);
        let (m, _) = score_any(&["Could you repeat that?"], "pizza");
        assert!(!m.passed(PASS));
    }

    #[test]
    fn stock_hallucinations_are_recognized_as_non_answers() {
        assert!(is_hallucination("Thank you for watching!"));
        assert!(is_hallucination(" Legendas pela comunidade Amara.org "));
        assert!(is_hallucination(""));
        assert!(!is_hallucination("Thank you very much"));
        assert!(!is_hallucination("I'm hungry"));
    }

    #[test]
    fn empty_or_silent_input_scores_zero() {
        assert_eq!(score("Hello", "").score, 0.0);
        assert_eq!(score("Hello", " ... ").score, 0.0);
        assert!(score("Hello", "").words.iter().all(|w| !w.hit));
    }

    #[test]
    fn one_or_two_letter_slips_per_word_still_count() {
        for heard in ["I'm hungre", "Could you repeet that", "see you tomorow", "cant you repeat that"] {
            let target = match heard {
                "I'm hungre" => "I'm hungry",
                "see you tomorow" => "See you tomorrow",
                _ => "Could you repeat that?",
            };
            let r = score(target, heard);
            assert!(r.passed(PASS), "{heard:?} vs {target:?}: {}", r.score);
        }
    }

    #[test]
    fn slips_do_not_turn_different_words_into_matches() {
        let r = score("Good morning", "good night");
        assert!(!r.passed(PASS));
        assert!(!r.words[1].hit, "night is not morning");
        assert!(!score("I'm hungry", "I'm angry").passed(PASS), "angry is a different word");
        // Two-letter words must match exactly: "no" is not "so".
        assert!(!score("no", "so").passed(PASS));
    }

    #[test]
    fn single_word_target_works_with_longer_answers() {
        assert!(score("Vamos", "vamos!").passed(PASS));
        assert!(score("Vamos", "bom, vamos lá").passed(PASS));
        assert!(!score("Vamos", "banana").passed(PASS));
    }

    #[test]
    fn numbers_written_as_digits_count_as_the_spoken_word() {
        // Whisper writes "3" / "20" for a number the learner said perfectly.
        assert!(score("Three.", "3").passed(PASS));
        assert!(score("Two coffees, please.", "2 coffees, please.").passed(PASS));
        assert!(score("Tengo veinte años.", "tengo 20 años").passed(PASS));
        assert!(score("¿Cinco?", "¿5?").passed(PASS));
        assert!(score("Dois cafés.", "2 cafés").passed(PASS));
        assert!(!score("Three.", "4").passed(PASS), "the wrong number is still wrong");
        assert!(!score("Tengo veinte años.", "tengo 30 años").passed(PASS));
    }

    #[test]
    fn a_word_the_recognizer_split_in_two_still_counts() {
        // Seen from whisper on a clean "Yesterday.": "Yes,ter day."
        assert!(score("Yesterday.", "Yes,ter day.").passed(PASS));
        assert!(score("Good morning", "good mor ning").passed(PASS));
        assert!(!score("Yesterday.", "Yes, today.").passed(PASS), "joining must not invent a match");
    }
}
