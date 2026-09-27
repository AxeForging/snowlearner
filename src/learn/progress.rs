//! What you already know, what you're learning and what comes next — for the
//! PROGRESSO tab and `snowlearner progress`. Pure.

use super::deck::Phrase;
use super::path::{self, Stage};
use super::picker::PhraseStats;
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
    Progress { overall, stages, topics, current, learning, next_up }
}

impl Progress {
    /// Plain-text report for the terminal.
    pub fn render_text(&self, language_name: &str) -> String {
        let mut out = format!(
            "Progresso em {language_name}: {} de {} sabidas ({}%), {} aprendendo\n",
            self.overall.known,
            self.overall.total,
            self.overall.percent(),
            self.overall.learning
        );
        out += &match self.current {
            Some(s) => format!("Etapa atual: {}\n\n", s.label_pt()),
            None => "Você já sabe tudo desta seleção!\n\n".to_string(),
        };
        for (stage, t) in &self.stages {
            let mark = if Some(*stage) == self.current { ">" } else { " " };
            out += &format!("{mark} {:<11} {} {:>3}/{:<3}\n", stage.label_pt(), bar(*t, 20), t.known, t.total);
        }
        if !self.learning.is_empty() {
            out += "\nAprendendo agora:\n";
            for i in &self.learning {
                out += &format!("  {}  ({})\n", i.say, i.meaning);
            }
        }
        if !self.next_up.is_empty() {
            out += "\nDepois:\n";
            for i in &self.next_up {
                out += &format!("  {}  ({})\n", i.say, i.meaning);
            }
        }
        out += "\nPor tema:\n";
        for (topic, t) in &self.topics {
            out += &format!("  {:<22} {} {:>3}/{:<3}\n", topic, bar(*t, 12), t.known, t.total);
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
        assert!(pr.render_text("inglês").contains("Você já sabe tudo"));
    }

    #[test]
    fn empty_selection_renders_without_dividing_by_zero() {
        let pr = progress(&deck(), &[], &HashMap::new());
        assert_eq!(pr.overall.total, 0);
        assert!(pr.render_text("inglês").contains("0 de 0"));
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
        let text = progress(&p, &all(&p), &HashMap::new()).render_text("inglês");
        assert!(text.contains("> palavras"), "{text}");
        assert!(text.contains("Aprendendo agora:\n  Water.  (<Water.>)"), "{text}");
        assert!(text.contains("Depois:"), "{text}");
        assert!(text.contains("restaurante"), "{text}");
    }
}
