//! The learning path (trilha): first words, then short chunks, then full
//! phrases. Only a few new items are open at a time; knowing them opens the
//! next ones in order. Pure — fed the deck selection and the practice stats.

use super::deck::{LEVELS, Phrase};
use super::picker::{PhraseStats, RECALL_AFTER};
use crate::lang::{Native, T};
use std::collections::HashMap;

/// New items in progress at once: enough variety, few enough to stick.
pub const LEARNING_SLOTS: usize = 4;

/// Unleveled (hand-curated) phrases sit with B1.
const UNLEVELED_LEVEL: &str = "B1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    Words,
    Chunks,
    Phrases,
}

impl Stage {
    pub const ALL: [Stage; 3] = [Stage::Words, Stage::Chunks, Stage::Phrases];

    /// 1 word → words, 2–3 → chunks ("Good morning", "The check, please"), else phrases.
    pub fn of(p: &Phrase) -> Stage {
        match p.say.split_whitespace().count() {
            0 | 1 => Stage::Words,
            2 | 3 => Stage::Chunks,
            _ => Stage::Phrases,
        }
    }

    /// One item of this stage, for the caption ("palavra · restaurante · repita").
    pub fn singular(self, native: Native) -> &'static str {
        match self {
            Stage::Words => T::StageWord,
            Stage::Chunks => T::StageChunk,
            Stage::Phrases => T::StagePhrase,
        }
        .get(native)
    }

    /// Said when the path reaches this stage.
    pub fn welcome(self, native: Native) -> &'static str {
        match self {
            Stage::Words => T::WelcomeWords,
            Stage::Chunks => T::WelcomeChunks,
            Stage::Phrases => T::WelcomePhrases,
        }
        .get(native)
    }

    pub fn label(self, native: Native) -> &'static str {
        match self {
            Stage::Words => T::StageWords,
            Stage::Chunks => T::StageChunks,
            Stage::Phrases => T::StagePhrases,
        }
        .get(native)
    }
}

pub fn stats_of(stats: &HashMap<String, PhraseStats>, p: &Phrase) -> PhraseStats {
    stats.get(&p.say).copied().unwrap_or_default()
}

/// Said right often enough to be asked from memory.
pub fn is_known(s: PhraseStats) -> bool {
    s.successes_total >= RECALL_AFTER
}

/// Tried at least once (right or wrong).
pub fn is_started(s: PhraseStats) -> bool {
    s.successes_total > 0 || s.last_failed
}

/// Stage, then level; words and chunks keep the deck's teaching order
/// (hello, bye, yes, no…), phrases go shorter first.
fn rank(p: &Phrase) -> (Stage, usize, usize) {
    let rank_of = |l: &str| LEVELS.iter().position(|x| *x == l);
    let level = p.level.as_deref().and_then(rank_of).or_else(|| rank_of(UNLEVELED_LEVEL)).unwrap_or(0);
    let stage = Stage::of(p);
    (stage, level, if stage == Stage::Phrases { p.say.chars().count() } else { 0 })
}

/// `selection` from easiest to hardest (ties keep deck order).
pub fn ordered(phrases: &[Phrase], selection: &[usize]) -> Vec<usize> {
    let mut out = selection.to_vec();
    out.sort_by_key(|&i| rank(&phrases[i]));
    out
}

/// What a lesson may ask now, in path order: everything known or started,
/// plus the next new items until `LEARNING_SLOTS` are in progress.
pub fn unlocked(phrases: &[Phrase], selection: &[usize], stats: &HashMap<String, PhraseStats>) -> Vec<usize> {
    let order = ordered(phrases, selection);
    let mut learning = order
        .iter()
        .filter(|&&i| {
            let s = stats_of(stats, &phrases[i]);
            is_started(s) && !is_known(s)
        })
        .count();
    order
        .into_iter()
        .filter(|&i| {
            if is_started(stats_of(stats, &phrases[i])) {
                return true;
            }
            let open = learning < LEARNING_SLOTS;
            learning += open as usize;
            open
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phrase(say: &str, level: Option<&str>) -> Phrase {
        Phrase { level: level.map(str::to_string), ..Phrase::new(say, "") }
    }

    /// Deliberately shuffled: a long B1 phrase, words, chunks, an unleveled phrase.
    fn deck() -> Vec<Phrase> {
        vec![
            phrase("Could you walk me through it?", Some("B1")),
            phrase("Water.", Some("A1")),
            phrase("Good morning.", Some("A1")),
            phrase("Hello.", Some("A1")),
            phrase("Sorry, you're on mute", None),
            phrase("Coffee.", Some("A2")),
            phrase("The check, please.", Some("A1")),
            phrase("Where is the station?", Some("A1")),
        ]
    }

    fn all(p: &[Phrase]) -> Vec<usize> {
        (0..p.len()).collect()
    }

    fn said(p: &[Phrase], i: usize, total: u32, last_failed: bool) -> (String, PhraseStats) {
        (p[i].say.clone(), PhraseStats { successes_today: 0, successes_total: total, last_failed })
    }

    #[test]
    fn stages_follow_word_count() {
        assert_eq!(Stage::of(&phrase("Water.", None)), Stage::Words);
        assert_eq!(Stage::of(&phrase("¿El baño?", None)), Stage::Chunks);
        assert_eq!(Stage::of(&phrase("The check, please.", None)), Stage::Chunks);
        assert_eq!(Stage::of(&phrase("Where is the station?", None)), Stage::Phrases);
        assert!(Stage::Words < Stage::Chunks && Stage::Chunks < Stage::Phrases);
    }

    #[test]
    fn the_path_goes_words_then_chunks_then_phrases_easiest_level_first() {
        let p = deck();
        let says: Vec<&str> = ordered(&p, &all(&p)).iter().map(|&i| p[i].say.as_str()).collect();
        assert_eq!(
            says,
            [
                "Water.",
                "Hello.",
                "Coffee.",
                "Good morning.",
                "The check, please.",
                "Where is the station?",
                "Sorry, you're on mute",
                "Could you walk me through it?"
            ]
        );
    }

    #[test]
    fn pre_a1_comes_first_and_unleveled_phrases_still_sit_with_b1() {
        let p = vec![
            phrase("Where is the station, please?", None),
            phrase("Could you send it tomorrow?", Some("A2")),
            phrase("Water.", Some("A1")),
            phrase("Could you walk me through the report?", Some("B2")),
            phrase("Thanks.", Some("PRE-A1")),
        ];
        let says: Vec<&str> = ordered(&p, &all(&p)).iter().map(|&i| p[i].say.as_str()).collect();
        assert_eq!(
            says,
            [
                "Thanks.",
                "Water.",
                "Could you send it tomorrow?",
                "Where is the station, please?",
                "Could you walk me through the report?"
            ]
        );
    }

    #[test]
    fn a_beginner_only_gets_the_first_few_words() {
        let p = deck();
        let open = unlocked(&p, &all(&p), &HashMap::new());
        assert_eq!(open.len(), LEARNING_SLOTS);
        assert!(open.iter().all(|&i| Stage::of(&p[i]) != Stage::Phrases), "no full phrases on day one");
        assert_eq!(p[open[0]].say, "Water.");
    }

    #[test]
    fn knowing_an_item_opens_the_next_one_and_keeps_it_for_review() {
        let p = deck();
        let before = unlocked(&p, &all(&p), &HashMap::new());
        let stats: HashMap<_, _> = [said(&p, 1, RECALL_AFTER, false)].into(); // "Water." known
        let after = unlocked(&p, &all(&p), &stats);
        assert_eq!(after.len(), LEARNING_SLOTS + 1);
        assert!(after.contains(&1), "known items stay in rotation for review");
        let newly: Vec<_> = after.iter().filter(|i| !before.contains(i)).collect();
        assert_eq!(newly.len(), 1);
        assert_eq!(p[*newly[0]].say, "The check, please.");
    }

    #[test]
    fn half_learned_and_missed_items_hold_their_slot() {
        let p = deck();
        let stats: HashMap<_, _> = [
            said(&p, 1, 1, false), // one success: still learning
            said(&p, 3, 0, true),  // tried and missed
        ]
        .into();
        let open = unlocked(&p, &all(&p), &stats);
        assert!(open.contains(&1) && open.contains(&3));
        assert_eq!(open.len(), LEARNING_SLOTS, "two started + two new");
    }

    #[test]
    fn items_already_started_stay_even_past_the_slot_limit() {
        // Someone who practiced long phrases before the path existed keeps them.
        let p = deck();
        let stats: HashMap<_, _> = [0, 4, 7, 6, 2].iter().map(|&i| said(&p, i, 1, false)).collect();
        let open = unlocked(&p, &all(&p), &stats);
        assert_eq!(open.len(), 5, "nothing new until some are learned");
        assert!([0, 4, 7, 6, 2].iter().all(|i| open.contains(i)));
    }

    #[test]
    fn filters_are_respected_and_empty_selections_stay_empty() {
        let p = deck();
        assert!(unlocked(&p, &[], &HashMap::new()).is_empty());
        assert_eq!(unlocked(&p, &[0, 4], &HashMap::new()), vec![4, 0], "only what the topic/level allows");
    }

    #[test]
    fn when_everything_is_known_everything_is_open() {
        let p = deck();
        let stats: HashMap<_, _> = (0..p.len()).map(|i| said(&p, i, 5, false)).collect();
        assert_eq!(unlocked(&p, &all(&p), &stats).len(), p.len());
    }
}
