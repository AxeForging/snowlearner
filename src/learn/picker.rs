//! Chooses the next phrase: not-yet-said-today first, then least practiced
//! overall, never the same phrase twice in a row when there is a choice.

use super::deck::Phrase;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhraseStats {
    pub successes_today: u32,
    pub successes_total: u32,
}

/// `roll` in [0,1) breaks ties so the order does not feel scripted.
pub fn pick(phrases: &[Phrase], stats: &HashMap<String, PhraseStats>, last: Option<&str>, roll: f32) -> Option<usize> {
    let key = |i: usize| {
        let s = stats.get(&phrases[i].say).copied().unwrap_or_default();
        (s.successes_today.min(1), s.successes_total)
    };
    let candidates: Vec<usize> =
        (0..phrases.len()).filter(|&i| phrases.len() == 1 || Some(phrases[i].say.as_str()) != last).collect();
    let best = candidates.iter().map(|&i| key(i)).min()?;
    let ties: Vec<usize> = candidates.into_iter().filter(|&i| key(i) == best).collect();
    let idx = ((roll.clamp(0.0, 0.999_999)) * ties.len() as f32) as usize;
    Some(ties[idx])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::cue::Segment;

    fn phrases(names: &[&str]) -> Vec<Phrase> {
        names
            .iter()
            .map(|s| Phrase {
                say: s.to_string(),
                meaning: String::new(),
                cue: vec![Segment::Target(s.to_string())],
                tip: None,
            })
            .collect()
    }

    fn stats(entries: &[(&str, u32, u32)]) -> HashMap<String, PhraseStats> {
        entries
            .iter()
            .map(|(s, today, total)| (s.to_string(), PhraseStats { successes_today: *today, successes_total: *total }))
            .collect()
    }

    #[test]
    fn prefers_phrases_not_said_today() {
        let p = phrases(&["a", "b", "c"]);
        let s = stats(&[("a", 1, 1), ("b", 2, 9), ("c", 0, 50)]);
        assert_eq!(pick(&p, &s, None, 0.0), Some(2));
    }

    #[test]
    fn among_unsaid_prefers_least_practiced_overall() {
        let p = phrases(&["a", "b", "c"]);
        let s = stats(&[("a", 0, 5), ("b", 0, 1), ("c", 0, 3)]);
        assert_eq!(pick(&p, &s, None, 0.5), Some(1));
    }

    #[test]
    fn never_repeats_the_last_phrase_when_there_is_a_choice() {
        let p = phrases(&["a", "b"]);
        let s = stats(&[("b", 1, 10)]);
        assert_eq!(pick(&p, &s, Some("a"), 0.0), Some(1));
    }

    #[test]
    fn single_phrase_deck_can_repeat() {
        let p = phrases(&["a"]);
        assert_eq!(pick(&p, &HashMap::new(), Some("a"), 0.3), Some(0));
    }

    #[test]
    fn roll_spreads_across_ties_and_stays_in_bounds() {
        let p = phrases(&["a", "b", "c", "d"]);
        let picks: std::collections::HashSet<_> =
            [0.0, 0.3, 0.6, 0.99, 1.0].iter().map(|r| pick(&p, &HashMap::new(), None, *r).unwrap()).collect();
        assert!(picks.len() >= 3);
        assert!(picks.iter().all(|&i| i < 4));
    }

    #[test]
    fn empty_deck_yields_none() {
        assert_eq!(pick(&[], &HashMap::new(), None, 0.1), None);
    }
}
