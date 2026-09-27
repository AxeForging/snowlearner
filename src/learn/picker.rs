//! Chooses the next phrase and how to practice it.
//!
//! Order: phrases you got wrong last time come back first (until you get them
//! right), then not-yet-said-today, then least practiced overall — never the
//! same phrase twice in a row. Mode: new phrases are heard and repeated;
//! phrases you already got right a few times must be recalled from the
//! pt-BR situation alone — that's what makes them stick.

use super::deck::Phrase;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhraseStats {
    pub successes_today: u32,
    pub successes_total: u32,
    /// The most recent attempt was wrong: due for a retry.
    pub last_failed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Practice {
    /// Repeat for new phrases, recall once you know them.
    #[default]
    Auto,
    /// Always hear it first, then repeat.
    Repeat,
    /// Always recall from the situation (hard mode).
    Recall,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Repeat,
    Recall,
}

/// Successful repeats before a phrase is asked from memory.
pub const RECALL_AFTER: u32 = 2;

pub fn mode_for(practice: Practice, stats: PhraseStats) -> Mode {
    match practice {
        Practice::Repeat => Mode::Repeat,
        Practice::Recall => Mode::Recall,
        // A phrase you just missed is heard again before being asked from memory.
        Practice::Auto if stats.successes_total >= RECALL_AFTER && !stats.last_failed => Mode::Recall,
        Practice::Auto => Mode::Repeat,
    }
}

/// Picks among `candidates` (indices into `phrases`). `roll` in [0,1) breaks ties.
pub fn pick(
    phrases: &[Phrase],
    candidates: &[usize],
    stats: &HashMap<String, PhraseStats>,
    last: Option<&str>,
    roll: f32,
) -> Option<usize> {
    let key = |i: usize| {
        let s = stats.get(&phrases[i].say).copied().unwrap_or_default();
        let tier = if s.last_failed {
            0
        } else if s.successes_today == 0 {
            1
        } else {
            2
        };
        (tier, s.successes_total)
    };
    let fresh: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|&i| candidates.len() == 1 || Some(phrases[i].say.as_str()) != last)
        .collect();
    let best = fresh.iter().map(|&i| key(i)).min()?;
    let ties: Vec<usize> = fresh.into_iter().filter(|&i| key(i) == best).collect();
    let idx = ((roll.clamp(0.0, 0.999_999)) * ties.len() as f32) as usize;
    Some(ties[idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrases(names: &[&str]) -> Vec<Phrase> {
        names.iter().map(|s| Phrase::new(s, "")).collect()
    }

    fn all(p: &[Phrase]) -> Vec<usize> {
        (0..p.len()).collect()
    }

    fn stats(entries: &[(&str, u32, u32)]) -> HashMap<String, PhraseStats> {
        entries
            .iter()
            .map(|(s, today, total)| {
                (s.to_string(), PhraseStats { successes_today: *today, successes_total: *total, last_failed: false })
            })
            .collect()
    }

    #[test]
    fn missed_phrases_come_back_first_but_not_immediately() {
        let p = phrases(&["missed", "fresh", "other"]);
        let mut s = stats(&[("fresh", 0, 0), ("other", 0, 0)]);
        s.insert("missed".into(), PhraseStats { successes_today: 0, successes_total: 3, last_failed: true });
        assert_eq!(pick(&p, &all(&p), &s, Some("fresh"), 0.9), Some(0), "retry jumps the queue");
        assert_ne!(pick(&p, &all(&p), &s, Some("missed"), 0.0), Some(0), "…but not right after the miss");
    }

    #[test]
    fn prefers_phrases_not_said_today() {
        let p = phrases(&["a", "b", "c"]);
        let s = stats(&[("a", 1, 1), ("b", 2, 9), ("c", 0, 50)]);
        assert_eq!(pick(&p, &all(&p), &s, None, 0.0), Some(2));
    }

    #[test]
    fn among_unsaid_prefers_least_practiced_overall() {
        let p = phrases(&["a", "b", "c"]);
        let s = stats(&[("a", 0, 5), ("b", 0, 1), ("c", 0, 3)]);
        assert_eq!(pick(&p, &all(&p), &s, None, 0.5), Some(1));
    }

    #[test]
    fn only_picks_from_the_candidates() {
        let p = phrases(&["a", "b", "c"]);
        for roll in [0.0, 0.5, 0.99] {
            assert_eq!(pick(&p, &[2], &HashMap::new(), None, roll), Some(2));
        }
    }

    #[test]
    fn never_repeats_the_last_phrase_when_there_is_a_choice() {
        let p = phrases(&["a", "b"]);
        let s = stats(&[("b", 1, 10)]);
        assert_eq!(pick(&p, &all(&p), &s, Some("a"), 0.0), Some(1));
    }

    #[test]
    fn single_candidate_can_repeat() {
        let p = phrases(&["a", "b"]);
        assert_eq!(pick(&p, &[0], &HashMap::new(), Some("a"), 0.3), Some(0));
    }

    #[test]
    fn roll_spreads_across_ties_and_stays_in_bounds() {
        let p = phrases(&["a", "b", "c", "d"]);
        let picks: std::collections::HashSet<_> =
            [0.0, 0.3, 0.6, 0.99, 1.0].iter().map(|r| pick(&p, &all(&p), &HashMap::new(), None, *r).unwrap()).collect();
        assert!(picks.len() >= 3);
        assert!(picks.iter().all(|&i| i < 4));
    }

    #[test]
    fn empty_selection_yields_none() {
        let p = phrases(&["a"]);
        assert_eq!(pick(&p, &[], &HashMap::new(), None, 0.1), None);
    }

    #[test]
    fn known_phrases_move_from_repeat_to_recall_in_auto_mode() {
        let new = PhraseStats::default();
        let known = PhraseStats { successes_today: 0, successes_total: RECALL_AFTER, last_failed: false };
        let slipped = PhraseStats { last_failed: true, ..known };
        assert_eq!(mode_for(Practice::Auto, new), Mode::Repeat);
        assert_eq!(mode_for(Practice::Auto, known), Mode::Recall);
        assert_eq!(mode_for(Practice::Repeat, known), Mode::Repeat);
        assert_eq!(mode_for(Practice::Recall, new), Mode::Recall);
        assert_eq!(mode_for(Practice::Auto, slipped), Mode::Repeat, "hear it again after a miss");
    }
}
