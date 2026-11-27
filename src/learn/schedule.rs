//! Spaced reinforcement for the question-and-answer exercise (#220): Leitner
//! boxes counted in days. Each question sits in a box (0 = never asked,
//! 5 = conquered) rebuilt from its attempts in the history, so there is no
//! state to drift. A clean answer moves it up a box and it comes back later;
//! a miss moves it down one and it comes back first. Pure: fed the history
//! rows and today's date.
//!
//! The growing gaps follow the spacing research (Cepeda et al. 2006: the best
//! gap grows with how long you must remember); a conquered question still
//! comes back, because testing again is what keeps it (Karpicke & Roediger
//! 2008). The intervals themselves are the game designer's choice.

use super::deck::{Answer, LEVELS, Phrase};
use super::path::LEARNING_SLOTS;
use super::picker::{Mode, Practice};
use chrono::NaiveDate;
use std::collections::HashMap;

/// Days before a question in each box is due again: box 0 has never been
/// asked, box 1 comes back the same day, box 5 is conquered.
pub const BOX_DAYS: [i64; 6] = [0, 0, 1, 3, 7, 21];

/// The box that counts as conquered.
pub const CONQUERED: u8 = 5;

/// First meetings a day; the rest of the day's answers are reviews. A new
/// question stays in the learning boxes (1-2) until the next day, so with
/// `LEARNING_SLOTS` (4) open at once that is the tighter limit in one day.
pub const NEW_PER_DAY: usize = 5;

/// One answer to a question, as the history keeps it.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionAttempt {
    pub ask: String,
    pub day: NaiveDate,
    pub success: bool,
    /// The tier said on a right answer.
    pub tier: Option<Answer>,
    /// 1 for the first try of a question, up to the lesson's `MAX_TRIES`.
    pub try_number: u32,
    pub mode: Mode,
}

/// How a question went, from its first try to a pass or the last try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    /// Right on the first try, at the tier asked (and recalled from box 2 up).
    Clean,
    /// Right, but on a later try, with a shorter tier, or repeated where it
    /// had to be recalled.
    Improve,
    /// Every try failed (or the learner left after a miss).
    Miss,
}

/// Where one question stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Card {
    /// 0 (never asked) to `CONQUERED`.
    pub box_index: u8,
    pub first_day: Option<NaiveDate>,
    pub last_day: Option<NaiveDate>,
    /// The last time it was asked, every try failed.
    pub last_failed: bool,
}

impl Card {
    pub fn is_new(self) -> bool {
        self.box_index == 0
    }

    /// Asked at least once and not yet out of the learning boxes (1-2).
    pub fn is_learning(self) -> bool {
        matches!(self.box_index, 1 | 2)
    }

    /// Days after its last answer before it comes back.
    pub fn interval(self) -> i64 {
        BOX_DAYS[self.box_index.min(CONQUERED) as usize]
    }

    /// Due for a review on `day`: a miss always is, otherwise once its box's
    /// interval has passed. A new question is never "due"; it is new.
    pub fn is_due(self, day: NaiveDate) -> bool {
        match self.last_day {
            _ if self.is_new() => false,
            _ if self.last_failed => true,
            Some(last) => (day - last).num_days() >= self.interval(),
            None => true,
        }
    }

    /// Days from `day` until it is due again (0 = today).
    pub fn returns_in(self, day: NaiveDate) -> i64 {
        match self.last_day {
            Some(last) if !self.last_failed => (self.interval() - (day - last).num_days()).max(0),
            _ => 0,
        }
    }

    /// The card after one graded question on `day`. Answering before it was
    /// due is extra practice: a clean answer then keeps the box.
    pub fn after(mut self, grade: Grade, day: NaiveDate) -> Card {
        let due = self.is_new() || self.is_due(day);
        self.box_index = match grade {
            Grade::Clean if due => (self.box_index + 1).min(CONQUERED),
            Grade::Clean | Grade::Improve => self.box_index.max(1),
            Grade::Miss => self.box_index.saturating_sub(1).max(1),
        };
        self.last_failed = grade == Grade::Miss;
        self.first_day = self.first_day.or(Some(day));
        self.last_day = Some(day);
        self
    }
}

/// The tier a clean answer must reach: `All` asks for the complete one.
fn rank(t: Answer) -> u8 {
    match t {
        Answer::Short => 0,
        Answer::All | Answer::Complete => 1,
        Answer::Polished => 2,
    }
}

/// Grades one question's tries, given the box it was in when asked.
pub fn grade(tries: &[&QuestionAttempt], asked: Answer, box_index: u8) -> Grade {
    let Some(pass) = tries.iter().find(|a| a.success) else { return Grade::Miss };
    let first = pass.try_number <= 1;
    let full = pass.tier.is_none_or(|t| rank(t) >= rank(asked));
    let recalled = box_index < 2 || pass.mode == Mode::Recall;
    if first && full && recalled { Grade::Clean } else { Grade::Improve }
}

/// Every question's card, replaying `log` in order. A question's tries run
/// from a first try to a pass or `max_tries`; tries cut short count as a miss.
pub fn cards(log: &[QuestionAttempt], asked: Answer, max_tries: u32) -> HashMap<String, Card> {
    let mut cards: HashMap<String, Card> = HashMap::new();
    let mut open: HashMap<&str, Vec<&QuestionAttempt>> = HashMap::new();
    let close = |cards: &mut HashMap<String, Card>, tries: Vec<&QuestionAttempt>| {
        let Some(last) = tries.last() else { return };
        let card = cards.entry(last.ask.clone()).or_default();
        *card = card.after(grade(&tries, asked, card.box_index), tries[0].day);
    };
    for a in log {
        if a.try_number <= 1
            && let Some(prev) = open.remove(a.ask.as_str())
        {
            close(&mut cards, prev);
        }
        let tries = open.entry(a.ask.as_str()).or_default();
        tries.push(a);
        if a.success || a.try_number >= max_tries {
            let done = open.remove(a.ask.as_str()).unwrap_or_default();
            close(&mut cards, done);
        }
    }
    let mut left: Vec<Vec<&QuestionAttempt>> = open.into_values().collect();
    left.sort_by_key(|t| t.first().map(|a| a.day));
    for tries in left {
        close(&mut cards, tries);
    }
    cards
}

fn card_of(cards: &HashMap<String, Card>, p: &Phrase) -> Card {
    p.ask.as_ref().and_then(|a| cards.get(a)).copied().unwrap_or_default()
}

/// First meetings so far on `day`.
pub fn new_today(cards: &HashMap<String, Card>, day: NaiveDate) -> usize {
    cards.values().filter(|c| c.first_day == Some(day)).count()
}

/// Whether a new question may be opened now: few enough are being learned
/// and today's first meetings are not used up.
fn may_open(phrases: &[Phrase], questions: &[usize], cards: &HashMap<String, Card>, day: NaiveDate) -> bool {
    let learning = questions.iter().filter(|&&i| card_of(cards, &phrases[i]).is_learning()).count();
    learning < LEARNING_SLOTS && new_today(cards, day) < NEW_PER_DAY
}

/// The next question, from `questions` (indices into `phrases`, in path
/// order, each with an `ask`): a missed one, then due reviews (lower box,
/// then the longest unseen), then a new one while `may_open`, and with
/// nothing due the one seen longest ago. Never the same one twice in a row
/// when there is another.
pub fn pick(
    phrases: &[Phrase],
    questions: &[usize],
    cards: &HashMap<String, Card>,
    day: NaiveDate,
    last: Option<&str>,
) -> Option<usize> {
    let fresh: Vec<usize> =
        questions.iter().copied().filter(|&i| questions.len() == 1 || phrases[i].ask.as_deref() != last).collect();
    let card = |i: usize| card_of(cards, &phrases[i]);
    let oldest = |i: &usize| (card(*i).box_index, card(*i).last_day);
    let missed = fresh.iter().copied().filter(|&i| card(i).last_failed && !card(i).is_new()).min_by_key(oldest);
    let due = || fresh.iter().copied().filter(|&i| !card(i).last_failed && card(i).is_due(day)).min_by_key(oldest);
    let new = || {
        let open = may_open(phrases, questions, cards, day);
        fresh.iter().copied().find(|&i| open && card(i).is_new())
    };
    let extra = || fresh.iter().copied().filter(|&i| !card(i).is_new()).min_by_key(|&i| card(i).last_day);
    missed.or_else(due).or_else(new).or_else(extra).or_else(|| fresh.first().copied())
}

/// How a question is practiced: in the first boxes the answer is heard
/// first, from box 2 it is recalled. A pre-A1 learner always hears it first
/// in boxes 0-1 only, so their questions can still climb (#220).
pub fn mode_for(practice: Practice, card: Card, beginner: bool) -> Mode {
    let learning = card.box_index < 2;
    match practice {
        _ if learning && beginner => Mode::Repeat,
        Practice::Repeat => Mode::Repeat,
        Practice::Recall => Mode::Recall,
        Practice::Auto if learning => Mode::Repeat,
        Practice::Auto => Mode::Recall,
    }
}

/// What today holds: questions due for review and new ones still to meet.
pub fn outlook(
    phrases: &[Phrase],
    questions: &[usize],
    cards: &HashMap<String, Card>,
    day: NaiveDate,
) -> (usize, usize) {
    let due = questions.iter().filter(|&&i| card_of(cards, &phrases[i]).is_due(day)).count();
    let unseen = questions.iter().filter(|&&i| card_of(cards, &phrases[i]).is_new()).count();
    (due, unseen.min(NEW_PER_DAY.saturating_sub(new_today(cards, day))))
}

/// Questions at a level that must exist before moving up is offered; a level
/// with fewer counts all of them (#224).
pub const LEVEL_UP_MIN: usize = 6;

/// The share of them that must be reinforced.
pub const LEVEL_UP_SHARE: f32 = 0.8;

/// The box that counts as reinforced: answered clean on two separate days.
pub const LEVEL_UP_BOX: u8 = 3;

/// The last level moving up is offered to (levels above B1 are out of scope).
const LEVEL_UP_TOP: &str = "B1";

/// The level to offer once the questions at `max_level`, among `questions`
/// (already narrowed to the chosen topics), are reinforced: with
/// `LEVEL_UP_MIN` or more of them, `LEVEL_UP_SHARE` in `LEVEL_UP_BOX` or
/// higher; with fewer, all of them; and every topic with a question at this
/// level has one there. The game only offers it; the
/// learner decides. The thresholds are the game designer's judgment.
pub fn level_up(
    phrases: &[Phrase],
    questions: &[usize],
    cards: &HashMap<String, Card>,
    max_level: &str,
) -> Option<&'static str> {
    let rank = LEVELS.iter().position(|l| *l == max_level)?;
    if rank >= LEVELS.iter().position(|l| *l == LEVEL_UP_TOP)? {
        return None;
    }
    let here: Vec<&Phrase> =
        questions.iter().map(|&i| &phrases[i]).filter(|p| p.level.as_deref() == Some(max_level)).collect();
    let reinforced = |p: &Phrase| card_of(cards, p).box_index >= LEVEL_UP_BOX;
    let done = here.iter().filter(|p| reinforced(p)).count();
    // With fewer than the minimum, every one of them must be reinforced.
    let needed = if here.len() >= LEVEL_UP_MIN { LEVEL_UP_SHARE * here.len() as f32 } else { here.len() as f32 };
    let enough = !here.is_empty() && done as f32 >= needed;
    let every_topic = here.iter().all(|p| here.iter().any(|q| q.topic == p.topic && reinforced(q)));
    (enough && every_topic).then(|| LEVELS[rank + 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRIES: u32 = 3;

    fn day(n: i64) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() + chrono::Duration::days(n)
    }

    fn try_(ask: &str, d: i64, n: u32, success: bool, mode: Mode) -> QuestionAttempt {
        let tier = success.then_some(Answer::Complete);
        QuestionAttempt { ask: ask.into(), day: day(d), success, tier, try_number: n, mode }
    }

    /// A clean answer on day `d`, in the mode its box asks for.
    fn clean(log: &mut Vec<QuestionAttempt>, ask: &str, d: i64) {
        let card = cards(log, Answer::All, TRIES).get(ask).copied().unwrap_or_default();
        let mode = if card.box_index < 2 { Mode::Repeat } else { Mode::Recall };
        log.push(try_(ask, d, 1, true, mode));
    }

    fn missed(log: &mut Vec<QuestionAttempt>, ask: &str, d: i64) {
        for n in 1..=TRIES {
            log.push(try_(ask, d, n, false, Mode::Recall));
        }
    }

    fn card(log: &[QuestionAttempt], ask: &str) -> Card {
        cards(log, Answer::All, TRIES)[ask]
    }

    #[test]
    fn a_clean_answer_moves_a_question_up_one_box() {
        let mut log = Vec::new();
        clean(&mut log, "Ready?", 0);
        assert_eq!(card(&log, "Ready?").box_index, 1);
        clean(&mut log, "Ready?", 0);
        assert_eq!(card(&log, "Ready?").box_index, 2, "box 1 is due again the same day");
    }

    #[test]
    fn a_question_in_box_n_is_due_after_its_box_s_days() {
        let mut log = Vec::new();
        let mut d = 0;
        for b in 1..=CONQUERED {
            clean(&mut log, "Ready?", d);
            let c = card(&log, "Ready?");
            assert_eq!(c.box_index, b);
            let gap = BOX_DAYS[b as usize];
            if gap > 0 {
                assert!(!c.is_due(day(d + gap - 1)), "box {b} is not due a day early");
            }
            assert!(c.is_due(day(d + gap)), "box {b} is due after {gap} days");
            assert_eq!(c.returns_in(day(d)), gap);
            d += gap;
        }
    }

    #[test]
    fn a_conquered_question_still_comes_back() {
        let mut log = Vec::new();
        let mut d = 0;
        for _ in 0..8 {
            clean(&mut log, "Ready?", d);
            d += card(&log, "Ready?").interval();
        }
        let c = card(&log, "Ready?");
        assert_eq!(c.box_index, CONQUERED, "the top box is the last");
        assert!(c.is_due(day(d)), "conquered, and still asked every {} days", BOX_DAYS[5]);
    }

    #[test]
    fn a_miss_moves_a_question_down_one_box_never_below_1() {
        let mut log = Vec::new();
        for d in [0, 0, 1, 4] {
            clean(&mut log, "Ready?", d);
        }
        assert_eq!(card(&log, "Ready?").box_index, 4);
        missed(&mut log, "Ready?", 11);
        let c = card(&log, "Ready?");
        assert_eq!(c.box_index, 3);
        assert!(c.last_failed && c.is_due(day(11)), "a miss comes back at once");
        let mut fresh = Vec::new();
        missed(&mut fresh, "Coffee or tea?", 0);
        missed(&mut fresh, "Coffee or tea?", 0);
        assert_eq!(card(&fresh, "Coffee or tea?").box_index, 1);
    }

    #[test]
    fn a_pass_after_a_miss_or_with_a_shorter_answer_keeps_the_box() {
        let mut log = Vec::new();
        for d in [0, 0, 1] {
            clean(&mut log, "Ready?", d);
        }
        assert_eq!(card(&log, "Ready?").box_index, 3);
        log.push(try_("Ready?", 4, 1, false, Mode::Recall));
        log.push(try_("Ready?", 4, 2, true, Mode::Repeat));
        let c = card(&log, "Ready?");
        assert_eq!((c.box_index, c.last_failed), (3, false), "right on the second try: improve");
        assert_eq!(c.returns_in(day(4)), BOX_DAYS[3], "and it comes back after the same gap");
        let mut short = try_("Ready?", 7, 1, true, Mode::Recall);
        short.tier = Some(Answer::Short);
        log.push(short);
        assert_eq!(card(&log, "Ready?").box_index, 3, "the short answer when the complete one was asked");
        let mut log = vec![try_("Ready?", 0, 1, true, Mode::Repeat)];
        log[0].tier = Some(Answer::Short);
        assert_eq!(cards(&log, Answer::Short, TRIES)["Ready?"].box_index, 1, "short is enough when short is asked");
    }

    #[test]
    fn from_box_2_a_repeated_answer_does_not_climb() {
        let mut log = Vec::new();
        clean(&mut log, "Ready?", 0);
        clean(&mut log, "Ready?", 0);
        log.push(try_("Ready?", 1, 1, true, Mode::Repeat));
        assert_eq!(card(&log, "Ready?").box_index, 2, "with the answer shown it stays in box 2");
    }

    #[test]
    fn answering_before_it_is_due_is_extra_practice_that_never_promotes() {
        let mut log = Vec::new();
        for d in [0, 0, 1] {
            clean(&mut log, "Ready?", d);
        }
        clean(&mut log, "Ready?", 2);
        assert_eq!(card(&log, "Ready?").box_index, 3, "box 3 waits 3 days; a day later it does not move");
        missed(&mut log, "Ready?", 2);
        assert_eq!(card(&log, "Ready?").box_index, 2, "but a miss still counts");
    }

    #[test]
    fn tries_cut_short_by_leaving_count_as_a_miss() {
        let log = vec![try_("Ready?", 0, 1, false, Mode::Repeat), try_("Ready?", 1, 1, true, Mode::Repeat)];
        let c = card(&log, "Ready?");
        assert_eq!((c.box_index, c.first_day), (2, Some(day(0))), "miss (box 1), then clean on day 1");
        let left = vec![try_("Ready?", 0, 1, false, Mode::Repeat)];
        assert!(card(&left, "Ready?").last_failed);
    }

    fn phrases(asks: &[&str]) -> Vec<Phrase> {
        asks.iter().map(|a| Phrase { ask: Some(a.to_string()), ..Phrase::new(&format!("answer to {a}"), "") }).collect()
    }

    fn all(p: &[Phrase]) -> Vec<usize> {
        (0..p.len()).collect()
    }

    #[test]
    fn a_missed_question_comes_first_next_session_but_never_twice_in_a_row() {
        let p = phrases(&["a", "b", "c"]);
        let mut log = Vec::new();
        clean(&mut log, "a", 0);
        missed(&mut log, "b", 0);
        let c = cards(&log, Answer::All, TRIES);
        assert_eq!(pick(&p, &all(&p), &c, day(1), None), Some(1), "the miss first");
        assert_ne!(pick(&p, &all(&p), &c, day(1), Some("b")), Some(1), "not right after itself");
    }

    #[test]
    fn due_reviews_come_before_new_questions() {
        let p = phrases(&["a", "b", "c"]);
        let mut log = Vec::new();
        clean(&mut log, "b", 0);
        let c = cards(&log, Answer::All, TRIES);
        assert_eq!(pick(&p, &all(&p), &c, day(0), None), Some(1), "b is in box 1: due today, before new a");
        assert_eq!(pick(&p, &all(&p), &c, day(0), Some("b")), Some(0), "then the first new one in path order");
    }

    #[test]
    fn a_day_meets_only_a_few_new_questions_however_many_answers_it_has() {
        let asks: Vec<String> = (0..12).map(|i| format!("q{i}")).collect();
        let p = phrases(&asks.iter().map(String::as_str).collect::<Vec<_>>());
        let mut log = Vec::new();
        let mut last: Option<String> = None;
        for _ in 0..60 {
            let c = cards(&log, Answer::All, TRIES);
            let i = pick(&p, &all(&p), &c, day(0), last.as_deref()).unwrap();
            let ask = p[i].ask.clone().unwrap();
            clean(&mut log, &ask, 0);
            last = Some(ask);
        }
        let c = cards(&log, Answer::All, TRIES);
        let limit = NEW_PER_DAY.min(LEARNING_SLOTS);
        assert_eq!(new_today(&c, day(0)), limit, "60 answers in one day meet only {limit} new questions");
        assert_eq!(c.len(), limit);
    }

    #[test]
    fn new_questions_stop_at_new_per_day_even_with_learning_slots_free() {
        let asks: Vec<String> = (0..9).map(|i| format!("q{i}")).collect();
        let p = phrases(&asks.iter().map(String::as_str).collect::<Vec<_>>());
        let met = Card { box_index: 3, first_day: Some(day(1)), last_day: Some(day(1)), last_failed: false };
        let mut c: HashMap<String, Card> = asks[..NEW_PER_DAY - 1].iter().map(|a| (a.clone(), met)).collect();
        assert!(may_open(&p, &all(&p), &c, day(1)), "one more is allowed");
        c.insert(asks[NEW_PER_DAY - 1].clone(), met);
        assert!(!may_open(&p, &all(&p), &c, day(1)), "{NEW_PER_DAY} met today: no more new ones");
        assert!(may_open(&p, &all(&p), &c, day(2)), "a new day opens again");
    }

    #[test]
    fn no_more_than_the_learning_slots_are_open_at_once() {
        let p = phrases(&["a", "b", "c", "d", "e", "f"]);
        let mut log = Vec::new();
        for (i, a) in ["a", "b", "c", "d"].iter().enumerate() {
            log.push(try_(a, 0, 1, true, Mode::Repeat));
            log.push(try_(a, 0, 1, true, Mode::Repeat));
            assert!(i < LEARNING_SLOTS);
        }
        let c = cards(&log, Answer::All, TRIES);
        assert!(c.values().all(|c| c.box_index == 2));
        let next = pick(&p, &all(&p), &c, day(0), None).unwrap();
        assert!(next < 4, "four in learning: no new question opens (got {:?})", p[next].ask);
    }

    #[test]
    fn with_nothing_due_the_learner_still_gets_a_question_and_it_does_not_promote() {
        let p = phrases(&["a"]);
        let mut log = Vec::new();
        for d in [0, 0] {
            clean(&mut log, "a", d);
        }
        let c = cards(&log, Answer::All, TRIES);
        assert!(!c["a"].is_due(day(0)));
        assert_eq!(pick(&p, &all(&p), &c, day(0), Some("a")), Some(0), "nothing due, nothing new: still asked");
        clean(&mut log, "a", 0);
        assert_eq!(card(&log, "a").box_index, 2, "extra practice keeps the box");
    }

    #[test]
    fn a_beginner_hears_the_answer_first_only_in_the_first_two_boxes() {
        let at = |b: u8| Card { box_index: b, ..Card::default() };
        for practice in [Practice::Auto, Practice::Recall] {
            assert_eq!(mode_for(practice, at(1), true), Mode::Repeat);
            assert_eq!(mode_for(practice, at(2), true), Mode::Recall, "{practice:?}: a beginner recalls from box 2");
        }
        assert_eq!(mode_for(Practice::Auto, at(0), false), Mode::Repeat);
        assert_eq!(mode_for(Practice::Auto, at(3), false), Mode::Recall);
        assert_eq!(mode_for(Practice::Repeat, at(4), false), Mode::Repeat);
        assert_eq!(mode_for(Practice::Recall, at(0), false), Mode::Recall);
    }

    #[test]
    fn the_outlook_counts_due_reviews_and_the_new_ones_left_today() {
        let p = phrases(&["a", "b", "c"]);
        let mut log = Vec::new();
        clean(&mut log, "a", 0);
        let c = cards(&log, Answer::All, TRIES);
        assert_eq!(outlook(&p, &all(&p), &c, day(0)), (1, 2));
    }

    /// Questions `(ask, topic, level)` and their boxes.
    fn leveled(items: &[(&str, &str, &str)]) -> Vec<Phrase> {
        let one = |(a, t, l): &(&str, &str, &str)| Phrase {
            ask: Some(a.to_string()),
            topic: t.to_string(),
            level: Some(l.to_string()),
            ..Phrase::new(&format!("answer to {a}"), "")
        };
        items.iter().map(one).collect()
    }

    fn boxed(boxes: &[(&str, u8)]) -> HashMap<String, Card> {
        boxes.iter().map(|(a, b)| (a.to_string(), Card { box_index: *b, ..Card::default() })).collect()
    }

    /// Six pre-A1 questions over two topics, plus one at A1.
    fn ladder() -> Vec<Phrase> {
        leveled(&[
            ("r1", "restaurante", "PRE-A1"),
            ("r2", "restaurante", "PRE-A1"),
            ("r3", "restaurante", "PRE-A1"),
            ("i1", "entrevista", "PRE-A1"),
            ("i2", "entrevista", "PRE-A1"),
            ("i3", "entrevista", "PRE-A1"),
            ("up", "restaurante", "A1"),
        ])
    }

    #[test]
    fn moving_up_is_offered_once_most_questions_of_every_topic_are_reinforced() {
        let p = ladder();
        let c = boxed(&[("r1", 3), ("r2", 4), ("r3", 5), ("i1", 3), ("i2", 3), ("i3", 2)]);
        assert_eq!(level_up(&p, &all(&p), &c, "PRE-A1"), Some("A1"), "5 of 6 (83%) in box 3+");
    }

    #[test]
    fn moving_up_waits_while_fewer_than_80_percent_are_reinforced() {
        let p = ladder();
        let c = boxed(&[("r1", 3), ("r2", 4), ("r3", 5), ("i1", 3), ("i2", 2), ("i3", 2)]);
        assert_eq!(level_up(&p, &all(&p), &c, "PRE-A1"), None, "4 of 6 (67%)");
    }

    #[test]
    fn moving_up_waits_while_a_topic_has_nothing_reinforced() {
        let mut p = ladder();
        p.extend(leveled(&[("r4", "restaurante", "PRE-A1"), ("r5", "restaurante", "PRE-A1")]));
        let c = boxed(&[("r1", 3), ("r2", 3), ("r3", 3), ("r4", 3), ("r5", 3), ("i1", 2), ("i2", 2), ("i3", 2)]);
        assert_eq!(level_up(&p, &all(&p), &c, "PRE-A1"), None, "8 of 9 would pass the share, entrevista has none");
        let c = boxed(&[("r1", 3), ("r2", 3), ("r3", 3), ("r4", 3), ("r5", 3), ("i1", 3), ("i2", 3), ("i3", 2)]);
        assert_eq!(level_up(&p, &all(&p), &c, "PRE-A1"), Some("A1"));
    }

    #[test]
    fn with_fewer_than_six_questions_every_one_must_be_reinforced() {
        let p = ladder();
        let restaurant: Vec<usize> = (0..p.len()).filter(|&i| p[i].topic == "restaurante").collect();
        let c = boxed(&[("r1", 3), ("r2", 3), ("r3", 2)]);
        assert_eq!(level_up(&p, &restaurant, &c, "PRE-A1"), None, "only restaurante chosen: 2 of 3");
        let c = boxed(&[("r1", 3), ("r2", 3), ("r3", 3)]);
        assert_eq!(level_up(&p, &restaurant, &c, "PRE-A1"), Some("A1"), "3 of 3");
        assert_eq!(level_up(&p, &[], &c, "PRE-A1"), None, "no question at this level, nothing to prove");
    }

    #[test]
    fn moving_up_counts_only_the_current_level_and_stops_at_b1() {
        let p = ladder();
        let c = boxed(&[("r1", 5), ("r2", 5), ("r3", 5), ("i1", 5), ("i2", 5), ("i3", 5)]);
        assert_eq!(level_up(&p, &all(&p), &c, "A1"), None, "the A1 question is new: pre-A1 boxes do not count");
        let b1 = leveled(&[("b", "restaurante", "B1")]);
        assert_eq!(level_up(&b1, &all(&b1), &boxed(&[("b", 5)]), "B1"), None, "nothing above B1 is offered");
        let a2 = leveled(&[("a", "restaurante", "A2")]);
        assert_eq!(level_up(&a2, &all(&a2), &boxed(&[("a", 3)]), "A2"), Some("B1"));
    }
}
