//! What you already know, what you're learning and what comes next — for the
//! PROGRESSO tab and `snowlearner progress`. Pure.

use super::deck::Phrase;
use super::path::{self, Stage};
use super::picker::PhraseStats;
use super::schedule::{self, CONQUERED, Card, REINFORCED_BOX};
use super::streak::Streak;
use crate::lang::{Native, T};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tally {
    pub known: usize,
    pub learning: usize,
    pub total: usize,
}

impl Tally {
    /// Share known, 0–100 (0 for an empty selection).
    pub fn percent(self) -> usize {
        (self.known * 100).checked_div(self.total).unwrap_or(0)
    }

    fn add(&mut self, s: PhraseStats) {
        self.total += 1;
        if path::is_known(s) {
            self.known += 1;
        } else if path::is_started(s) {
            self.learning += 1;
        }
    }
}

/// Questions of the question exercise by box (#228): conquered = the top
/// box, in review = from `REINFORCED_BOX` up to the one below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QuestionTally {
    pub conquered: usize,
    pub review: usize,
    pub total: usize,
}

impl QuestionTally {
    fn add(&mut self, box_index: u8) {
        self.total += 1;
        if box_index >= CONQUERED {
            self.conquered += 1;
        } else if box_index >= REINFORCED_BOX {
            self.review += 1;
        }
    }

    /// As a bar: conquered is the green part, in review the yellow one.
    pub fn as_tally(self) -> Tally {
        Tally { known: self.conquered, learning: self.review, total: self.total }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub say: String,
    pub meaning: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub overall: Tally,
    pub stages: Vec<(Stage, Tally)>,
    /// Topics in deck order.
    pub topics: Vec<(String, Tally)>,
    /// Stage of the first item not known yet (None: everything is known).
    pub current: Option<Stage>,
    /// Open now and not known yet, in path order.
    pub learning: Vec<Item>,
    /// The next items that open as you learn, in path order.
    pub next_up: Vec<Item>,
    /// Questions per topic, in deck order; empty outside the question exercise.
    pub questions: Vec<(String, QuestionTally)>,
    /// Days in a row with a right answer (spec 006 R13).
    pub streak: Streak,
}

const NEXT_UP: usize = 3;

pub fn progress(phrases: &[Phrase], selection: &[usize], stats: &HashMap<String, PhraseStats>) -> Progress {
    let order = path::ordered(phrases, selection);
    let open = path::unlocked(phrases, selection, stats);
    let item = |i: usize| Item { say: phrases[i].say.clone(), meaning: phrases[i].meaning.clone() };
    let mut overall = Tally::default();
    let mut stages: Vec<(Stage, Tally)> = Stage::ALL.iter().map(|s| (*s, Tally::default())).collect();
    let mut topics: Vec<(String, Tally)> = Vec::new();
    let (mut current, mut learning, mut next_up) = (None, Vec::new(), Vec::new());
    for &i in &order {
        let p = &phrases[i];
        let s = path::stats_of(stats, p);
        overall.add(s);
        stages.iter_mut().find(|(st, _)| *st == Stage::of(p)).unwrap().1.add(s);
        match topics.iter_mut().find(|(t, _)| *t == p.topic) {
            Some((_, t)) => t.add(s),
            None => {
                let mut t = Tally::default();
                t.add(s);
                topics.push((p.topic.clone(), t));
            }
        }
        if path::is_known(s) {
            continue;
        }
        current.get_or_insert(Stage::of(p));
        if open.contains(&i) {
            learning.push(item(i));
        } else if next_up.len() < NEXT_UP {
            next_up.push(item(i));
        }
    }
    topics.sort_by_key(|(t, _)| selection.iter().position(|&i| phrases[i].topic == *t));
    Progress { overall, stages, topics, current, learning, next_up, questions: Vec::new(), streak: Streak::default() }
}

impl Progress {
    pub fn with_streak(self, streak: Streak) -> Progress {
        Progress { streak, ..self }
    }

    /// Adds the boxes of the selection's questions, rebuilt from the history.
    pub fn with_questions(
        mut self,
        phrases: &[Phrase],
        selection: &[usize],
        cards: &HashMap<String, Card>,
    ) -> Progress {
        for &i in selection {
            let p = &phrases[i];
            if p.ask.is_none() {
                continue;
            }
            let b = schedule::card_of(cards, p).box_index;
            match self.questions.iter_mut().find(|(t, _)| *t == p.topic) {
                Some((_, t)) => t.add(b),
                None => {
                    let mut t = QuestionTally::default();
                    t.add(b);
                    self.questions.push((p.topic.clone(), t));
                }
            }
        }
        self
    }

    /// Every topic's questions together.
    pub fn questions_total(&self) -> QuestionTally {
        self.questions.iter().fold(QuestionTally::default(), |a, (_, t)| QuestionTally {
            conquered: a.conquered + t.conquered,
            review: a.review + t.review,
            total: a.total + t.total,
        })
    }

    /// Plain-text report for the terminal.
    pub fn render_text(&self, language_name: &str, native: Native) -> String {
        let o = self.overall;
        let mut out = T::ProgressReport.fill(native, &[&language_name, &o.known, &o.total, &o.percent(), &o.learning]);
        out += "\n";
        if let Some(s) = crate::lang::text::streak(native, self.streak.days, self.streak.done_today) {
            out += &format!("{s}\n");
        }
        out += &match self.current {
            Some(s) => format!("{}\n\n", T::StageNow.fill(native, &[&s.label(native)])),
            None => format!("{}\n\n", T::KnowsAll.get(native)),
        };
        for (stage, t) in &self.stages {
            let mark = if Some(*stage) == self.current { ">" } else { " " };
            out += &format!("{mark} {:<11} {} {:>3}/{:<3}\n", stage.label(native), bar(*t, 20), t.known, t.total);
        }
        if !self.learning.is_empty() {
            out += &format!("\n{}\n", T::LearningNow.get(native));
            for i in &self.learning {
                out += &format!("  {}  ({})\n", i.say, i.meaning);
            }
        }
        if !self.next_up.is_empty() {
            out += &format!("\n{}\n", T::NextUp.fill(native, &[&""]).trim_end());
            for i in &self.next_up {
                out += &format!("  {}  ({})\n", i.say, i.meaning);
            }
        }
        out += &format!("\n{}\n", T::ByTopic.get(native));
        for (topic, t) in &self.topics {
            let topic = crate::lang::text::topic(native, topic);
            out += &format!("  {:<22} {} {:>3}/{:<3}\n", topic, bar(*t, 12), t.known, t.total);
        }
        if !self.questions.is_empty() {
            out += &format!("\n{}\n", T::QuestionsByTopic.get(native));
            for (topic, t) in &self.questions {
                let topic = crate::lang::text::topic(native, topic);
                let row = T::QuestionsRow.fill(native, &[&t.conquered, &t.review, &t.total]);
                out += &format!("  {:<22} {} {row}\n", topic, bar(t.as_tally(), 12));
            }
        }
        out
    }
}

/// `#` known, `+` learning, `.` not started.
pub fn bar(t: Tally, width: usize) -> String {
    if t.total == 0 {
        return ".".repeat(width);
    }
    let known = t.known * width / t.total;
    let learning = ((t.known + t.learning) * width / t.total).saturating_sub(known);
    format!("{}{}{}", "#".repeat(known), "+".repeat(learning), ".".repeat(width - known - learning))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learn::path::LEARNING_SLOTS;

    fn phrase(say: &str, topic: &str) -> Phrase {
        Phrase { topic: topic.into(), level: Some("A1".into()), ..Phrase::new(say, &format!("<{say}>")) }
    }

    fn deck() -> Vec<Phrase> {
        vec![
            phrase("Water.", "restaurante"),
            phrase("Hello.", "primeiros contatos"),
            phrase("Coffee.", "restaurante"),
            phrase("Good morning.", "primeiros contatos"),
            phrase("The check, please.", "restaurante"),
            phrase("Where is the station?", "viagem"),
            phrase("Bye.", "primeiros contatos"),
        ]
    }

    fn all(p: &[Phrase]) -> Vec<usize> {
        (0..p.len()).collect()
    }

    fn stat(total: u32, last_failed: bool) -> PhraseStats {
        PhraseStats { successes_today: 0, successes_total: total, last_failed }
    }

    #[test]
    fn a_fresh_learner_is_on_words_with_the_first_ones_open() {
        let p = deck();
        let pr = progress(&p, &all(&p), &HashMap::new());
        assert_eq!(pr.overall, Tally { known: 0, learning: 0, total: 7 });
        assert_eq!(pr.current, Some(Stage::Words));
        assert_eq!(pr.learning.len(), LEARNING_SLOTS);
        assert_eq!(pr.learning[0], Item { say: "Water.".into(), meaning: "<Water.>".into() }, "deck order");
        assert_eq!(pr.next_up.len(), 3);
        assert_eq!(pr.stages[0], (Stage::Words, Tally { known: 0, learning: 0, total: 4 }));
    }

    #[test]
    fn counts_split_known_learning_and_new_per_stage_and_topic() {
        let p = deck();
        let stats: HashMap<String, PhraseStats> = [
            ("Water.".to_string(), stat(3, false)),
            ("Hello.".to_string(), stat(2, false)),
            ("Coffee.".to_string(), stat(1, false)),
            ("Bye.".to_string(), stat(0, true)),
        ]
        .into();
        let pr = progress(&p, &all(&p), &stats);
        assert_eq!(pr.overall, Tally { known: 2, learning: 2, total: 7 });
        assert_eq!(pr.stages[0].1, Tally { known: 2, learning: 2, total: 4 });
        assert_eq!(pr.stages[1].1, Tally { known: 0, learning: 0, total: 2 });
        let rest = pr.topics.iter().find(|(t, _)| t == "restaurante").unwrap().1;
        assert_eq!(rest, Tally { known: 1, learning: 1, total: 3 });
        assert_eq!(pr.topics[0].0, "restaurante", "topics keep deck order");
        assert!(!pr.learning.iter().any(|i| i.say == "Water."), "known items aren't 'learning'");
    }

    #[test]
    fn knowing_every_word_moves_you_to_expressions() {
        let p = deck();
        let stats: HashMap<String, PhraseStats> =
            ["Water.", "Hello.", "Coffee.", "Bye."].iter().map(|s| (s.to_string(), stat(2, false))).collect();
        assert_eq!(progress(&p, &all(&p), &stats).current, Some(Stage::Chunks));
    }

    #[test]
    fn everything_known_means_no_current_stage_and_nothing_next() {
        let p = deck();
        let stats: HashMap<String, PhraseStats> = p.iter().map(|x| (x.say.clone(), stat(4, false))).collect();
        let pr = progress(&p, &all(&p), &stats);
        assert_eq!(pr.current, None);
        assert!(pr.learning.is_empty() && pr.next_up.is_empty());
        assert!(pr.render_text("inglês", Native::PtBr).contains("Você já sabe tudo"));
    }

    #[test]
    fn empty_selection_renders_without_dividing_by_zero() {
        let pr = progress(&deck(), &[], &HashMap::new());
        assert_eq!(pr.overall.total, 0);
        assert!(pr.render_text("inglês", Native::PtBr).contains("0 de 0"));
    }

    #[test]
    fn bars_show_known_then_learning_then_new() {
        assert_eq!(bar(Tally { known: 1, learning: 1, total: 4 }, 8), "##++....");
        assert_eq!(bar(Tally { known: 4, learning: 0, total: 4 }, 4), "####");
        assert_eq!(bar(Tally::default(), 3), "...");
    }

    #[test]
    fn the_text_report_marks_the_current_stage_and_lists_items() {
        let p = deck();
        let text = progress(&p, &all(&p), &HashMap::new()).render_text("inglês", Native::PtBr);
        assert!(text.contains("> palavras"), "{text}");
        assert!(text.contains("Aprendendo agora:\n  Water.  (<Water.>)"), "{text}");
        assert!(text.contains("Depois:"), "{text}");
        assert!(text.contains("restaurante"), "{text}");
    }

    fn question(say: &str, topic: &str) -> Phrase {
        Phrase { ask: Some(format!("{say}?")), ..phrase(say, topic) }
    }

    /// One clean answer to `ask` on day `d`, from memory.
    fn clean(ask: &str, d: i64) -> schedule::QuestionAttempt {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 1).unwrap() + chrono::Duration::days(d);
        let mode = crate::learn::picker::Mode::Recall;
        schedule::QuestionAttempt { ask: ask.into(), day, success: true, tier: None, try_number: 1, mode }
    }

    #[test]
    fn question_counts_per_topic_match_the_boxes_rebuilt_from_the_log() {
        let p = vec![
            question("Water.", "restaurante"),
            question("Coffee.", "restaurante"),
            question("The check, please.", "restaurante"),
            phrase("Hello.", "primeiros contatos"),
            question("A ticket.", "viagem"),
            question("The station.", "viagem"),
        ];
        // Same-day answers climb boxes 1-2; then 1, 3 and 7 days apart.
        let days = |ask: &str, ds: &[i64]| ds.iter().map(|&d| clean(ask, d)).collect::<Vec<_>>();
        let log: Vec<_> = [
            days("Water.?", &[0, 0, 1, 4, 11]),
            days("Coffee.?", &[0, 0, 1]),
            days("The check, please.?", &[0]),
            days("A ticket.?", &[0, 0, 1, 4]),
        ]
        .concat();
        let cards = schedule::cards(&log, crate::learn::deck::Answer::Complete, 3);
        let boxes: Vec<u8> = p.iter().map(|q| schedule::card_of(&cards, q).box_index).collect();
        assert_eq!(boxes, [CONQUERED, REINFORCED_BOX, 1, 0, 4, 0], "the log the counts are read from");
        let pr = progress(&p, &all(&p), &HashMap::new()).with_questions(&p, &all(&p), &cards);
        assert_eq!(
            pr.questions,
            [
                ("restaurante".to_string(), QuestionTally { conquered: 1, review: 1, total: 3 }),
                ("viagem".to_string(), QuestionTally { conquered: 0, review: 1, total: 2 }),
            ],
            "a topic without questions is not listed"
        );
        assert_eq!(pr.questions_total(), QuestionTally { conquered: 1, review: 2, total: 5 });
        let text = pr.render_text("inglês", Native::PtBr);
        assert!(text.contains("Perguntas por tema:"), "{text}");
        assert!(text.contains("1 conquistadas · 1 em revisão · de 3"), "{text}");
        let english = pr.render_text("English", Native::En);
        assert!(english.contains("0 conquered · 1 in review · of 2"), "{english}");
    }

    #[test]
    fn without_the_question_exercise_the_report_has_no_question_lines() {
        let p = vec![question("Water.", "restaurante")];
        let text = progress(&p, &all(&p), &HashMap::new()).render_text("inglês", Native::PtBr);
        assert!(!text.contains("Perguntas"), "{text}");
    }

    #[test]
    fn the_report_shows_the_streak_and_asks_to_practice_while_today_is_missing() {
        let p = deck();
        let pr = progress(&p, &all(&p), &HashMap::new());
        assert!(!pr.render_text("inglês", Native::PtBr).contains("Sequência"), "hidden at 0");
        let done = pr.clone().with_streak(Streak { days: 3, done_today: true });
        assert!(done.render_text("inglês", Native::PtBr).contains("Sequência: 3 dias\n"));
        let waiting = pr.clone().with_streak(Streak { days: 1, done_today: false });
        assert!(waiting.render_text("English", Native::En).contains("Streak: 1 day · practice today"));
    }
}
