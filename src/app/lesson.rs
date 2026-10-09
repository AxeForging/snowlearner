//! The lesson flow, independent of windows and audio devices:
//! the mage poses a challenge → cue read aloud → listen → score → melt (or
//! retry) → record; plus combos, the daily goal, nudges and the end-of-day
//! recap. It drives the scene and asks the app to run speech jobs.

use crate::lang::text;
use crate::lang::{Lines, Native, T};
use crate::learn::cue::Segment;
use crate::learn::deck::{Answer, Deck, PRE_A1, Phrase};
use crate::learn::path;
use crate::learn::picker::{self, Mode, Practice};
use crate::scene::Scene;
use crate::scene::hud::{Caption, Meter, Stats, Status, SummaryLine, SummaryPanel};
use crate::scene::pyro::Spell;
use crate::speech::endpoint::ListenPlan;
use crate::speech::matcher::{self, WordHit};
use crate::speech::tts::Speed;
use crate::speech::worker::{Job, SpeechEvent, Utterance};
use crate::store::history::{Attempt, History};
use chrono::{DateTime, Local, NaiveTime};

pub const MAX_TRIES: u32 = 3;
/// Correct answers in a row that call the sun.
pub const SUN_COMBO: u32 = 3;
const RESULT_SECONDS: f32 = 3.5;
const RETRY_PAUSE: f32 = 2.5;
/// Hotkey auto-repeat / double clicks within this window are ignored.
const DEBOUNCE: f32 = 0.8;
/// How long a "didn't hear you" prompt waits before closing on its own.
const WAIT_USER: f32 = 25.0;
const SUMMARY_LINGER: f32 = 8.0;
const LAST_SUMMARY_KEY: &str = "last_summary_day";

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// Main hotkey: start a challenge, confirm (no mic), or skip ahead.
    Primary,
    /// Summary hotkey.
    Summary,
    /// Esc / dismiss.
    Dismiss,
    Speech(SpeechEvent),
    Tick {
        dt: f32,
        now: DateTime<Local>,
    },
}

#[derive(Debug, Clone)]
pub struct Options {
    /// The learner's own language: every text here and the cue's native parts.
    pub native: Native,
    pub threshold: f32,
    pub can_listen: bool,
    /// Seconds you get to start answering (recall gets +4).
    pub listen_seconds: f32,
    pub hotkey: String,
    pub summary_hotkey: String,
    pub summary_at: NaiveTime,
    /// Seconds idle before the warrior nudges you (from the commitment level).
    pub ask_every: f32,
    pub practice: Practice,
    /// Which answer tier is asked (and shown); every tier always counts.
    pub answer: Answer,
    /// Only practice these topics (empty = all).
    pub topics: Vec<String>,
    /// Highest CEFR level to practice.
    pub max_level: String,
    pub daily_goal: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Speaking,
    Listening,
    Confirm,
    Result {
        until: f32,
    },
    RetryPause {
        until: f32,
    },
    /// Heard nothing: wait for the learner to try again (no auto-retry frenzy).
    WaitUser {
        until: f32,
    },
}

enum State {
    Idle,
    Challenge { phrase: usize, mode: Mode, stage: Stage, tries: u32, job: u64 },
    Summary { job: u64, lines: usize, close_at: Option<f32> },
}

pub struct Lesson {
    deck: Deck,
    opts: Options,
    history: History,
    state: State,
    clock: f32,
    idle_for: f32,
    next_job: u64,
    last_phrase: Option<String>,
    roll: u32,
    combo: u32,
    done_today: u32,
    goal_celebrated: bool,
    paused: bool,
    last_primary: f32,
}

impl Lesson {
    pub fn new(deck: Deck, opts: Options, history: History) -> Lesson {
        let today = Local::now().date_naive();
        let done_today = history
            .day_summary(&deck.language, today)
            .map(|rows| rows.iter().filter(|r| r.successes > 0).count() as u32)
            .unwrap_or(0);
        Lesson {
            goal_celebrated: done_today >= opts.daily_goal,
            deck,
            opts,
            history,
            state: State::Idle,
            clock: 0.0,
            idle_for: 0.0,
            next_job: 1,
            last_phrase: None,
            roll: 7,
            combo: 0,
            done_today,
            paused: false,
            last_primary: f32::NEG_INFINITY,
        }
    }

    /// Puts tips and the progress corner on a fresh scene.
    pub fn attach(&self, scene: &mut Scene) {
        scene.tips = self.tips();
        self.refresh_stats(scene);
    }

    pub fn deck(&self) -> &Deck {
        &self.deck
    }

    pub fn options(&self) -> &Options {
        &self.opts
    }

    /// Pre-A1: the learner knows no English yet.
    fn beginner(&self) -> bool {
        self.opts.max_level == PRE_A1
    }

    fn target_speed(&self) -> Speed {
        target_speed(&self.opts.max_level)
    }

    /// Switches language (deck) live. Cancels whatever is on screen.
    pub fn set_deck(&mut self, deck: Deck, scene: &mut Scene) {
        self.close(scene);
        let today = Local::now().date_naive();
        self.done_today = self
            .history
            .day_summary(&deck.language, today)
            .map(|rows| rows.iter().filter(|r| r.successes > 0).count() as u32)
            .unwrap_or(0);
        self.goal_celebrated = self.done_today >= self.opts.daily_goal;
        let known = deck.topics();
        self.opts.topics.retain(|t| known.contains(t)); // another deck's (or a typo'd) topics mean nothing here
        self.deck = deck;
        self.last_phrase = None;
        self.combo = 0;
        scene.tips = self.tips();
        self.refresh_stats(scene);
    }

    /// Topics with something at the current level and how many items each
    /// has there: what the panel's checklist offers.
    pub fn topics(&self) -> Vec<(String, usize)> {
        self.deck.topic_counts(&self.opts.max_level)
    }

    /// Unticks topics with nothing at the current level ("trabalho" at
    /// pre-A1) and returns them, so the caller can say so and save the
    /// setting. Topics the deck doesn't have at all are unticked quietly.
    /// Untick them all and every topic is in again.
    pub fn drop_empty_topics(&mut self, scene: &mut Scene) -> Vec<String> {
        let open: Vec<String> = self.topics().into_iter().map(|(t, _)| t).collect();
        let (keep, gone): (Vec<String>, Vec<String>) = self.opts.topics.iter().cloned().partition(|t| open.contains(t));
        if gone.is_empty() {
            return gone;
        }
        self.set_options(|o| o.topics = keep, scene);
        let known = self.deck.topics();
        gone.into_iter().filter(|t| known.contains(t)).collect()
    }

    pub fn set_options(&mut self, f: impl FnOnce(&mut Options), scene: &mut Scene) {
        f(&mut self.opts);
        self.goal_celebrated = self.done_today >= self.opts.daily_goal;
        scene.tips = self.tips();
        self.refresh_stats(scene);
    }

    /// What you know, are learning and comes next, for the current filters.
    pub fn progress(&self) -> crate::learn::progress::Progress {
        let stats = self.history.stats(&self.deck.language, Local::now().date_naive()).unwrap_or_default();
        crate::learn::progress::progress(&self.deck.phrases, &self.selection(), &stats)
    }

    /// Phrases currently in rotation (topic + level filters).
    pub fn selection(&self) -> Vec<usize> {
        self.deck.selection(&self.opts.topics, &self.opts.max_level)
    }

    /// Warrior tips built from the hotkeys and the deck.
    pub fn tips(&self) -> Vec<String> {
        let (hk, n) = (&self.opts.hotkey, self.opts.native);
        let mut tips = vec![
            T::TipHotkey.fill(n, &[hk]),
            T::TipFreeze.fill(n, &[hk]),
            T::TipRecap.fill(n, &[&self.opts.summary_hotkey]),
            T::TipSun.fill(n, &[&SUN_COMBO]),
        ];
        tips.extend(self.deck.tips.iter().cloned());
        for &i in self.selection().iter().take(80) {
            if let Some(t) = &self.deck.phrases[i].tip {
                tips.push(T::TipPrefix.fill(n, &[t]));
            }
        }
        tips
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    pub fn done_today(&self) -> u32 {
        self.done_today
    }

    fn job_id(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }

    fn roll(&mut self) -> f32 {
        self.roll = self.roll.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((self.roll >> 8) % 1000) as f32 / 1000.0
    }

    fn line(&mut self, lines: Lines) -> String {
        let lines = lines.get(self.opts.native);
        let r = self.roll();
        lines[((r * lines.len() as f32) as usize).min(lines.len() - 1)].to_string()
    }

    fn phrase(&self, i: usize) -> &Phrase {
        &self.deck.phrases[i]
    }

    fn parts(&self, segments: &[Segment]) -> Vec<Utterance> {
        segments
            .iter()
            .map(|s| match s {
                Segment::Native(t) => {
                    Utterance { text: t.clone(), lang: self.opts.native.code().into(), speed: Speed::Normal }
                }
                Segment::Target(t) => {
                    Utterance { text: t.clone(), lang: self.deck.language.clone(), speed: self.target_speed() }
                }
            })
            .collect()
    }

    fn cue_for(&self, p: &Phrase, mode: Mode) -> Vec<Segment> {
        match mode {
            Mode::Repeat => p.cue_for(self.opts.answer),
            Mode::Recall => p.recall_cue(&self.deck.language_name, self.opts.native),
        }
    }

    /// The answer the learner is asked for (the chosen tier, or the complete one).
    fn shown(&self, p: &Phrase) -> String {
        p.shown(self.opts.answer).to_string()
    }

    /// Tier of the asked answer: what "confirm" without a mic counts as.
    fn shown_tier(&self, p: &Phrase) -> Answer {
        let shown = p.shown(self.opts.answer);
        p.tiers().into_iter().find(|(_, s)| *s == shown).map_or(Answer::Complete, |(t, _)| t)
    }

    fn tag(&self, p: &Phrase, mode: Mode) -> String {
        let n = self.opts.native;
        let mode = match mode {
            Mode::Repeat => T::TagRepeat,
            Mode::Recall => T::TagRecall,
        };
        format!("{} · {} · {}", path::Stage::of(p).singular(n), text::topic(n, &p.topic), mode.get(n))
    }

    fn refresh_stats(&self, scene: &mut Scene) {
        scene.hud.stats = Some(Stats {
            done: self.done_today,
            goal: self.opts.daily_goal,
            combo: self.combo,
            label: format!(
                "{} · {}",
                self.deck.language.to_uppercase(),
                text::topics_label(self.opts.native, &self.opts.topics)
                    .unwrap_or_else(|| T::AllTopics.get(self.opts.native).into())
            ),
        });
    }

    /// Paused (black hole): no lessons, nudges or recaps until resumed.
    /// Returns the jobs to send (stopping a recap being read).
    pub fn set_paused(&mut self, on: bool, scene: &mut Scene) -> Vec<Job> {
        let mut jobs = Vec::new();
        self.paused = on;
        if on {
            self.hush(&mut jobs);
            self.close(scene);
        }
        self.idle_for = 0.0;
        jobs
    }

    /// Leaving the recap stops its reading (at the end of the current line).
    fn hush(&self, jobs: &mut Vec<Job>) {
        if matches!(self.state, State::Summary { .. }) {
            jobs.push(Job::StopSpeaking);
        }
    }

    pub fn handle(&mut self, input: Input, scene: &mut Scene) -> Vec<Job> {
        let mut jobs = Vec::new();
        if self.paused {
            match input {
                Input::Tick { dt, .. } => self.clock += dt,
                Input::Primary | Input::Summary => scene.hud.toast(T::PausedToast.get(self.opts.native), 3.0),
                _ => {}
            }
            return jobs;
        }
        match input {
            Input::Tick { dt, now } => self.tick(dt, now, scene, &mut jobs),
            Input::Primary => self.primary(scene, &mut jobs),
            Input::Summary => {
                let open = matches!(self.state, State::Summary { .. });
                self.hush(&mut jobs);
                self.close(scene);
                if !open {
                    self.start_summary(Local::now(), scene, &mut jobs);
                }
            }
            Input::Dismiss => {
                self.hush(&mut jobs);
                self.close(scene);
            }
            Input::Speech(ev) => self.speech(ev, scene, &mut jobs),
        }
        jobs
    }

    fn primary(&mut self, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let bounce = self.clock - self.last_primary < DEBOUNCE;
        self.last_primary = self.clock;
        let skipping = matches!(
            self.state,
            State::Summary { .. } | State::Challenge { stage: Stage::Result { .. } | Stage::WaitUser { .. }, .. }
        );
        if bounce && skipping {
            return; // held hotkey / double click: don't skip ahead
        }
        match self.state {
            State::Idle => self.start_challenge(scene, jobs),
            State::Challenge { phrase, stage: Stage::WaitUser { .. }, tries, .. } => {
                self.replay(phrase, tries, scene, jobs)
            }
            State::Challenge { stage: Stage::Listening, .. } => {
                // "I'm done talking": stop the mic now instead of waiting for silence.
                jobs.push(Job::StopListening);
                if let Some(c) = &mut scene.hud.caption {
                    c.footer = T::Analyzing.get(self.opts.native).into();
                }
            }
            State::Challenge { phrase, stage: Stage::Confirm, tries, .. } => {
                let p = self.phrase(phrase);
                let (say, tier) = (self.shown(p), self.shown_tier(p));
                self.finish_attempt(phrase, tries, &say, 1.0, Some(tier), scene);
            }
            State::Challenge { stage: Stage::Result { .. }, .. } | State::Summary { .. } => {
                self.hush(jobs);
                self.close(scene);
                self.start_challenge(scene, jobs);
            }
            _ => {}
        }
    }

    fn start_challenge(&mut self, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let today = Local::now().date_naive();
        let stats = self.history.stats(&self.deck.language, today).unwrap_or_default();
        let roll = self.roll();
        // The path opens words first, then chunks, then phrases.
        let open = path::unlocked(&self.deck.phrases, &self.selection(), &stats);
        let Some(i) = picker::pick(&self.deck.phrases, &open, &stats, self.last_phrase.as_deref(), roll) else {
            scene.hud.toast(T::NoPhrases.get(self.opts.native), 4.0);
            return;
        };
        let p = self.phrase(i).clone();
        // Someone who knows nothing yet always hears the answer first.
        let practice = if self.beginner() { Practice::Repeat } else { self.opts.practice };
        let mode = picker::mode_for(practice, stats.get(&p.say).copied().unwrap_or_default());
        self.last_phrase = Some(p.say.clone());
        let job = self.job_id();
        self.state = State::Challenge { phrase: i, mode, stage: Stage::Speaking, tries: 0, job };
        self.idle_for = 0.0;
        scene.set_practicing(true);
        let ask = self.line(if mode == Mode::Recall { Lines::MageAskRecall } else { Lines::MageAskRepeat });
        scene.mage_say(ask, 3.0);
        scene.hud.summary = None;
        let cue = self.cue_for(&p, mode);
        scene.hud.caption = Some(Caption {
            segments: cue.clone(),
            say: self.shown(&p),
            active: None,
            alts: if mode == Mode::Repeat { p.alternatives(self.opts.answer, self.opts.native) } else { Vec::new() },
            counted: None,
            meaning: if mode == Mode::Repeat { p.meaning.clone() } else { String::new() },
            status: Status::Speaking,
            feedback: None,
            heard: None,
            footer: String::new(),
            tag: self.tag(&p, mode),
            listen: None,
        });
        jobs.push(Job::Speak { id: job, parts: self.parts(&cue) });
    }

    fn after_cue(&mut self, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let slack = if self.beginner() { matcher::BEGINNER_SLACK } else { 0 };
        let State::Challenge { phrase, mode, stage, job, .. } = &mut self.state else { return };
        // Room for the longest tier: any of them is a right answer.
        let words = self.deck.phrases[*phrase].tiers().iter().map(|(_, s)| s.split_whitespace().count()).max();
        let words = words.unwrap_or(1);
        let recall = *mode == Mode::Recall;
        let mut plan = ListenPlan::for_phrase(words, recall);
        plan.think = self.opts.listen_seconds + if recall { 4.0 } else { 0.0 };
        if self.opts.can_listen {
            scene.set_listening(true);
        }
        let cap = scene.hud.caption.as_mut();
        if self.opts.can_listen {
            *stage = Stage::Listening;
            if let Some(c) = cap {
                c.active = None;
                c.status = Status::Listening;
                c.footer = T::FooterDone.fill(self.opts.native, &[&self.opts.hotkey]);
                c.listen = Some(Meter { level: 0.0, speaking: false, think_left: plan.think, think_total: plan.think });
            }
            let expect = expect(&self.deck, *phrase, slack, self.opts.threshold);
            jobs.push(Job::Listen { id: *job, lang: self.deck.language.clone(), plan, expect: Some(expect) });
        } else {
            *stage = Stage::Confirm;
            if let Some(c) = cap {
                c.active = None;
                c.status = Status::Confirm;
                c.footer = T::FooterConfirm.fill(self.opts.native, &[&self.opts.hotkey]);
            }
        }
    }

    fn speech(&mut self, ev: SpeechEvent, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let current = match self.state {
            State::Challenge { job, .. } | State::Summary { job, .. } => job,
            State::Idle => return,
        };
        if ev.id() != current {
            return; // stale: belongs to a cancelled challenge
        }
        match (&mut self.state, ev) {
            (State::Summary { lines, .. }, SpeechEvent::Part { index, .. }) => {
                if let Some(panel) = &mut scene.hud.summary {
                    panel.active = (index > 0 && index <= *lines).then(|| index - 1);
                }
            }
            (State::Summary { close_at, .. }, SpeechEvent::Spoken { .. } | SpeechEvent::Failed { .. }) => {
                *close_at = Some(self.clock + SUMMARY_LINGER);
                if let Some(panel) = &mut scene.hud.summary {
                    panel.active = None;
                }
            }
            (State::Challenge { stage: Stage::Speaking, .. }, SpeechEvent::Part { index, .. }) => {
                if let Some(c) = &mut scene.hud.caption {
                    c.active = Some(index);
                }
            }
            (State::Challenge { stage: Stage::Speaking, .. }, SpeechEvent::Spoken { .. }) => {
                self.after_cue(scene, jobs)
            }
            (State::Challenge { stage: Stage::Speaking, .. }, SpeechEvent::Failed { error, .. }) => {
                scene.hud.toast(T::VoiceUnavailable.fill(self.opts.native, &[&error]), 5.0);
                self.after_cue(scene, jobs);
            }
            (State::Challenge { stage: Stage::Listening, .. }, SpeechEvent::Level { level, speaking, .. }) => {
                if let Some(m) = scene.hud.caption.as_mut().and_then(|c| c.listen.as_mut()) {
                    m.level = level;
                    m.speaking |= speaking;
                }
            }
            (State::Challenge { stage: Stage::Listening, .. }, SpeechEvent::Thinking { .. }) => {
                if let Some(c) = &mut scene.hud.caption {
                    c.status = Status::Thinking;
                }
            }
            (State::Challenge { phrase, stage: Stage::Listening, tries, .. }, SpeechEvent::Heard { text, .. }) => {
                let (phrase, tries) = (*phrase, *tries);
                let p = self.phrase(phrase).clone();
                let slack = if self.beginner() { matcher::BEGINNER_SLACK } else { 0 };
                // Every tier counts. The one said is the highest whose words were
                // all heard, not the best score: the shorter tiers sit inside the
                // polished one, so a slip in the long answer must not hand the
                // reward to them; the best score decides when none is whole.
                let answers = p.answers();
                let texts: Vec<&str> = answers.iter().map(|(_, a)| *a).collect();
                let lang = &self.deck.language;
                let (m, best) = matcher::score_any(&texts, &text, slack, lang);
                let passed = m.passed(self.opts.threshold);
                let whole = answers
                    .iter()
                    .rev()
                    .find(|(_, a)| matcher::score_lenient(a, &text, slack, lang).words.iter().all(|w| w.hit));
                let said = passed.then(|| whole.unwrap_or(&answers[best]).0);
                if !passed && matcher::is_hallucination(&text) {
                    // Whisper invented "Thank you for watching" out of noise: that's silence.
                    self.silence(scene);
                    return;
                }
                // Feedback is shown on the asked phrase. When another option on
                // screen is what counted, that option lights up instead.
                let shown = self.shown(&p);
                let other =
                    said.and_then(|t| p.tiers().into_iter().find(|(tier, _)| *tier == t)).filter(|(_, s)| *s != shown);
                let words = if passed {
                    shown.split_whitespace().map(|w| WordHit { word: w.to_string(), hit: other.is_none() }).collect()
                } else {
                    matcher::score_lenient(&shown, &text, slack, &self.deck.language).words
                };
                if let Some(c) = &mut scene.hud.caption {
                    c.feedback = Some(words);
                    c.heard = Some(text.clone());
                    if passed {
                        // Now that it's said, show every other way to say it.
                        c.alts = p
                            .tiers()
                            .iter()
                            .filter(|(_, s)| *s != shown)
                            .map(|(t, s)| format!("{}: {s}", t.label(self.opts.native)))
                            .collect();
                    }
                    c.counted = other.and_then(|(_, s)| c.alts.iter().position(|a| a.ends_with(s)));
                }
                self.finish_attempt(phrase, tries, &text, m.score, said, scene);
            }
            (State::Challenge { stage: Stage::Listening, .. }, SpeechEvent::NoSpeech { .. }) => self.silence(scene),
            (State::Challenge { stage, .. }, SpeechEvent::Failed { error, .. }) if *stage == Stage::Listening => {
                // Mic or model trouble: fall back to self-confirmation.
                *stage = Stage::Confirm;
                scene.hud.toast(T::MicUnavailable.fill(self.opts.native, &[&error]), 6.0);
                if let Some(c) = &mut scene.hud.caption {
                    c.listen = None;
                    c.status = Status::Confirm;
                    c.footer = T::FooterConfirm.fill(self.opts.native, &[&self.opts.hotkey]);
                }
            }
            _ => {}
        }
    }

    /// Nothing (real) was heard: not a wrong answer — don't record it, don't
    /// auto-retry; wait for the learner.
    fn silence(&mut self, scene: &mut Scene) {
        if let State::Challenge { stage, .. } = &mut self.state {
            *stage = Stage::WaitUser { until: self.clock + WAIT_USER };
        }
        scene.set_listening(false);
        let n = self.opts.native;
        scene.mage_say(T::MageHeardNothing.get(n), 2.5);
        if let Some(c) = &mut scene.hud.caption {
            c.status = Status::Failed;
            c.listen = None;
            c.heard = Some(T::Silence.get(n).into());
            c.footer = T::FooterSilence.fill(n, &[&self.opts.hotkey]);
        }
    }

    /// Tells the learner what they just learned and what the path opened.
    fn path_news(&self, p: &Phrase, open_before: &[usize], scene: &mut Scene) {
        let Ok(stats) = self.history.stats(&self.deck.language, Local::now().date_naive()) else { return };
        if stats.get(&p.say).is_none_or(|s| s.successes_total != picker::RECALL_AFTER) {
            return; // not the moment it became known
        }
        let opened: Vec<usize> = path::unlocked(&self.deck.phrases, &self.selection(), &stats)
            .into_iter()
            .filter(|i| !open_before.contains(i))
            .collect();
        let n = self.opts.native;
        let mut news = T::Learned.fill(n, &[&p.say]);
        if let Some(&i) = opened.first() {
            let next = &self.deck.phrases[i];
            let stage = path::Stage::of(next);
            let reached = open_before.iter().all(|&j| path::Stage::of(&self.deck.phrases[j]) < stage);
            news = if reached {
                T::StageFirst.fill(n, &[&stage.welcome(n), &next.say])
            } else {
                T::NewItem.fill(n, &[&news, &stage.singular(n), &next.say])
            };
        }
        scene.hud.toast(news, 5.0);
    }

    /// Records an attempt; `said` is the tier of a right answer (None = a miss).
    fn finish_attempt(
        &mut self,
        phrase: usize,
        tries: u32,
        heard: &str,
        score: f32,
        said: Option<Answer>,
        scene: &mut Scene,
    ) {
        let passed = said.is_some();
        let p = self.phrase(phrase).clone();
        let State::Challenge { mode, .. } = self.state else { return };
        scene.set_listening(false);
        let today = Local::now().date_naive();
        let open_before = self
            .history
            .stats(&self.deck.language, today)
            .map(|s| path::unlocked(&self.deck.phrases, &self.selection(), &s))
            .unwrap_or_default();
        let _ = self.history.record(&Attempt {
            at: Local::now(),
            language: self.deck.language.clone(),
            say: p.say.clone(),
            meaning: p.meaning.clone(),
            heard: heard.to_string(),
            score,
            success: passed,
        });
        let tries = tries + 1;
        let n = self.opts.native;
        let (stage, footer) = if passed {
            // With options on screen, tell which one counted (it picks the spell).
            let counted = said.filter(|_| p.tiers().len() > 1).map(|t| T::AnswerCounted.fill(n, &[&t.label(n)]));
            let next = format!("{}{}", counted.unwrap_or_default(), T::FooterNext.fill(n, &[&self.opts.hotkey]));
            (Stage::Result { until: self.clock + RESULT_SECONDS }, next)
        } else if tries < MAX_TRIES {
            let hint = if mode == Mode::Recall { T::RetryRecall } else { T::RetryRepeat }.get(n);
            (Stage::RetryPause { until: self.clock + RETRY_PAUSE }, T::Attempt.fill(n, &[&tries, &MAX_TRIES, &hint]))
        } else {
            (Stage::Result { until: self.clock + RESULT_SECONDS }, T::GiveUp.get(n).into())
        };
        if let State::Challenge { stage: s, tries: t, .. } = &mut self.state {
            *s = stage;
            *t = tries;
        }

        if passed {
            let first_today = self
                .history
                .stats(&self.deck.language, Local::now().date_naive())
                .ok()
                .and_then(|s| s.get(&p.say).copied())
                .is_some_and(|s| s.successes_today == 1);
            if first_today {
                self.done_today += 1;
            }
            self.combo += 1;
            // Remembering from scratch is worth more than repeating.
            let power = if mode == Mode::Recall && tries == 1 { 1.5 } else { 1.0 };
            scene.celebrate(power, spell_for(said.unwrap_or(Answer::Complete)));
            let groan = self.line(Lines::MageGroan);
            scene.mage_say(groan, 3.5);
            if self.combo >= SUN_COMBO && self.combo % SUN_COMBO == 0 {
                scene.sun();
                scene.hud.toast(T::ComboSun.fill(n, &[&self.combo]), 4.0);
            }
            self.path_news(&p, &open_before, scene);
            if self.done_today >= self.opts.daily_goal && !self.goal_celebrated {
                self.goal_celebrated = true;
                scene.hud.toast(T::GoalToast.fill(n, &[&self.opts.daily_goal]), 5.0);
                scene.warrior.say(T::GoalWarrior.get(n), 5.0);
            }
        } else {
            let laugh = self.line(Lines::MageLaugh);
            scene.miss();
            scene.mage_say(laugh, 3.5);
            if tries >= MAX_TRIES {
                self.combo = 0;
            }
        }
        self.refresh_stats(scene);
        if let Some(c) = &mut scene.hud.caption {
            c.active = None;
            c.listen = None;
            c.status = if passed { Status::Passed } else { Status::Failed };
            c.footer = footer;
            if !passed && mode == Mode::Recall {
                // Reveal the answer so the retry becomes a repeat.
                c.segments = p.cue_for(self.opts.answer);
                c.alts = p.alternatives(self.opts.answer, self.opts.native);
                c.meaning = p.meaning.clone();
            }
        }
    }

    fn tick(&mut self, dt: f32, now: DateTime<Local>, scene: &mut Scene, jobs: &mut Vec<Job>) {
        self.clock += dt;
        if let State::Challenge { stage: Stage::Listening, .. } = self.state
            && let Some(m) = scene.hud.caption.as_mut().and_then(|c| c.listen.as_mut())
            && !m.speaking
        {
            m.think_left = (m.think_left - dt).max(0.0);
        }
        match self.state {
            State::Idle => {
                self.idle_for += dt;
                if self.idle_for >= self.opts.ask_every {
                    self.idle_for = 0.0;
                    let n = self.opts.native;
                    scene.mage_say(T::MageIdle.get(n), 4.0);
                    scene.warrior.say(T::WarriorIdle.fill(n, &[&self.opts.hotkey]), 6.0);
                    scene.hud.toast(T::ToastIdle.fill(n, &[&self.opts.hotkey]), 5.0);
                }
                self.maybe_auto_summary(now, scene, jobs);
            }
            State::Challenge { stage: Stage::Result { until }, .. } if self.clock >= until => self.close(scene),
            State::Challenge { phrase, stage: Stage::RetryPause { until }, tries, .. } if self.clock >= until => {
                self.replay(phrase, tries, scene, jobs)
            }
            State::Challenge { stage: Stage::WaitUser { until }, .. } if self.clock >= until => self.close(scene),
            State::Summary { close_at: Some(t), .. } if self.clock >= t => self.close(scene),
            _ => {}
        }
    }

    /// Replays just the target phrase (recall turns into repeat), then listens again.
    fn replay(&mut self, phrase: usize, tries: u32, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let p = self.phrase(phrase).clone();
        let job = self.job_id();
        self.state = State::Challenge { phrase, mode: Mode::Repeat, stage: Stage::Speaking, tries, job };
        let tag = self.tag(&p, Mode::Repeat);
        let shown = self.shown(&p);
        if let Some(c) = &mut scene.hud.caption {
            c.status = Status::Speaking;
            c.feedback = None;
            c.heard = None;
            c.footer = String::new();
            c.tag = tag;
            c.segments = p.cue_for(self.opts.answer);
            c.alts = p.alternatives(self.opts.answer, self.opts.native);
            c.meaning = p.meaning.clone();
            c.active = c.segments.iter().position(|s| matches!(s, Segment::Target(t) if *t == shown));
        }
        let part = Utterance { text: shown, lang: self.deck.language.clone(), speed: self.target_speed() };
        jobs.push(Job::Speak { id: job, parts: vec![part] });
    }

    fn maybe_auto_summary(&mut self, now: DateTime<Local>, scene: &mut Scene, jobs: &mut Vec<Job>) {
        if now.time() < self.opts.summary_at {
            return;
        }
        let today = now.date_naive().to_string();
        if self.history.meta(LAST_SUMMARY_KEY).ok().flatten().as_deref() == Some(today.as_str()) {
            return;
        }
        let _ = self.history.set_meta(LAST_SUMMARY_KEY, &today);
        self.start_summary(now, scene, jobs);
    }

    fn start_summary(&mut self, now: DateTime<Local>, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let day = now.date_naive();
        let rows = self.history.day_summary(&self.deck.language, day).unwrap_or_default();
        let learned: Vec<_> = rows.iter().filter(|r| r.successes > 0).collect();
        let job = self.job_id();
        let n = self.opts.native;
        let mut parts = vec![Utterance {
            text: match learned.len() {
                0 => T::RecapNone.get(n).to_string(),
                1 => T::RecapOne.get(n).to_string(),
                k => T::RecapMany.fill(n, &[&k]),
            },
            lang: n.code().into(),
            speed: Speed::Normal,
        }];
        parts.extend(rows.iter().map(|r| Utterance {
            text: r.say.clone(),
            lang: self.deck.language.clone(),
            speed: self.target_speed(),
        }));
        scene.hud.caption = None;
        scene.hud.summary = Some(SummaryPanel {
            title: T::RecapTitle.fill(n, &[&day.format(T::RecapDate.get(n))]),
            lines: rows
                .iter()
                .map(|r| SummaryLine { say: r.say.clone(), meaning: r.meaning.clone(), ok: r.successes > 0 })
                .collect(),
            active: None,
            footer: T::RecapFooter.fill(n, &[&learned.len(), &rows.len(), &self.opts.daily_goal]),
        });
        self.state = State::Summary { job, lines: rows.len(), close_at: None };
        scene.set_practicing(true);
        jobs.push(Job::Speak { id: job, parts });
    }

    fn close(&mut self, scene: &mut Scene) {
        self.state = State::Idle;
        self.idle_for = 0.0;
        scene.hud.caption = None;
        scene.hud.summary = None;
        scene.set_practicing(false);
    }
}

/// Says which ticked topics had nothing at the level (the HUD then shows
/// what is left, or "todos os temas"); short enough for a 320 px window:
/// names that don't fit become a count ("Sem 2 temas nesse nível.").
pub fn topic_dropped(native: Native, dropped: &[String]) -> String {
    let named = T::TopicDropped.fill(native, &[&text::topics_label(native, dropped).unwrap_or_default()]);
    if crate::render::font::text_width(&named) <= 316 {
        return named;
    }
    T::TopicDropped.fill(native, &[&T::TopicsCount.fill(native, &[&dropped.len()])])
}

/// The fire mage's spell for the tier the learner said: fuller answer, bigger spell.
pub fn spell_for(said: Answer) -> Spell {
    match said {
        Answer::Short => Spell::Spark,
        Answer::Polished => Spell::Blaze,
        Answer::All | Answer::Complete => Spell::Fireball,
    }
}

/// How the target language is read: slower for a pre-A1 learner.
pub fn target_speed(max_level: &str) -> Speed {
    if max_level == PRE_A1 { Speed::Slower } else { Speed::Slow }
}

/// What the listener should expect: every right answer, and as a hint for a
/// second pass, deck phrases around this one with the target last.
fn expect(deck: &Deck, i: usize, slack: usize, threshold: f32) -> matcher::Expect {
    let p = &deck.phrases[i];
    let around = deck.phrases.iter().enumerate().skip(i.saturating_sub(3)).take(7).filter(|(j, _)| *j != i);
    let mut hint: Vec<&str> = around.map(|(_, q)| q.say.as_str()).collect();
    hint.push(&p.say);
    matcher::Expect {
        answers: p.answers().into_iter().map(|(_, a)| a.to_string()).collect(),
        lang: deck.language.clone(),
        slack,
        threshold,
        hint: hint.join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::level::Commitment;
    use chrono::TimeZone;

    struct Fixture {
        _dir: tempfile::TempDir,
        lesson: Lesson,
        scene: Scene,
    }

    const DECK: &str = "language='en'\nnative='pt-BR'\ntitle='t'\nlanguage_name='inglês'\n\
         [[tip]]\ntext='Falso amigo: actually'\n\
         [[phrase]]\nsay=\"I'm hungry\"\nmeaning='Estou com fome'\nsituation='Na hora do almoço.'\ntopic='comida'\nlevel='A1'\n\
         [[phrase]]\nsay='Could you repeat that?'\naccept=['Can you repeat that?']\nmeaning='Pode repetir?'\nsituation='Na call.'\ntopic='trabalho'\nlevel='A2'\n\
         [[phrase]]\nsay='Let us negotiate the contract'\nmeaning='Vamos negociar o contrato'\nsituation='Reunião.'\ntopic='trabalho'\nlevel='B2'";

    fn options(can_listen: bool) -> Options {
        Options {
            native: Native::PtBr,
            threshold: 0.72,
            can_listen,
            listen_seconds: 7.0,
            hotkey: "Ctrl+Alt+M".into(),
            summary_hotkey: "Ctrl+Alt+J".into(),
            summary_at: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            ask_every: 60.0,
            practice: Practice::Repeat,
            answer: Answer::All,
            topics: Vec::new(),
            max_level: "B1".into(),
            daily_goal: 2,
        }
    }

    fn fixture_with(opts: Options) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(&dir.path().join("h.sqlite3")).unwrap();
        let deck = Deck::parse(DECK).unwrap();
        let mut scene = Scene::new(240, 135, 1, Commitment::Relentless.pace(), true);
        for _ in 0..(120 * 30) {
            scene.step(1.0 / 30.0); // let snow pile up so melting is observable
        }
        Fixture { _dir: dir, lesson: Lesson::new(deck, opts, history), scene }
    }

    fn fixture(can_listen: bool) -> Fixture {
        fixture_with(options(can_listen))
    }

    fn fixture_deck(src: &str) -> Fixture {
        let mut f = fixture(true);
        let deck = Deck::parse(src).unwrap();
        f.lesson.set_deck(deck, &mut f.scene);
        f
    }

    fn morning() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 9, 27, 10, 0, 0).unwrap()
    }

    impl Fixture {
        fn send(&mut self, i: Input) -> Vec<Job> {
            self.lesson.handle(i, &mut self.scene)
        }
        fn tick(&mut self, seconds: f32) -> Vec<Job> {
            let mut jobs = Vec::new();
            for _ in 0..(seconds * 10.0) as i32 {
                jobs.extend(self.send(Input::Tick { dt: 0.1, now: morning() }));
            }
            jobs
        }
        fn speak_job(jobs: &[Job]) -> (u64, Vec<Utterance>) {
            jobs.iter()
                .find_map(|j| match j {
                    Job::Speak { id, parts } => Some((*id, parts.clone())),
                    _ => None,
                })
                .expect("a Speak job")
        }
        fn current_say(&self) -> String {
            self.scene.hud.caption.as_ref().unwrap().say.clone()
        }
        /// Starts a challenge and answers it (correctly or not). Returns the phrase.
        fn answer(&mut self, correct: bool) -> String {
            let (id, _) = Fixture::speak_job(&self.send(Input::Primary));
            self.send(Input::Speech(SpeechEvent::Spoken { id }));
            let say = self.current_say();
            let text = if correct { say.clone() } else { "banana split".into() };
            self.send(Input::Speech(SpeechEvent::Heard { id, text }));
            self.settle();
            say
        }
        /// Lets fireballs fly and land.
        fn settle(&mut self) {
            for _ in 0..75 {
                self.scene.step(1.0 / 30.0);
            }
        }
        fn finish(&mut self) {
            self.tick(RESULT_SECONDS + 0.2);
        }
    }

    const TIER_DECK: &str = "language='en'\nnative='pt-BR'\ntitle='t'\n\
         [[phrase]]\nsay='Can you share your screen?'\nshort='Share your screen?'\n\
         polished='Could you please share your screen?'\naccept=['Could you share your screen?']\n\
         meaning='Pode compartilhar sua tela?'\nsituation='Na call.'\ntopic='trabalho'\nlevel='A2'";

    /// A lesson on the one tiered phrase, asking for `answer`.
    fn tier_fixture(answer: Answer) -> Fixture {
        let mut f = fixture_deck(TIER_DECK);
        f.lesson.set_options(|o| o.answer = answer, &mut f.scene);
        f
    }

    #[test]
    fn with_every_tier_shown_the_complete_one_is_asked_and_the_others_listed() {
        let mut f = tier_fixture(Answer::All);
        let (_, parts) = Fixture::speak_job(&f.send(Input::Primary));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(cap.say, "Can you share your screen?");
        assert_eq!(cap.alts, vec!["curta: Share your screen?", "polida: Could you please share your screen?"]);
        assert_eq!(parts.last().unwrap().text, "Can you share your screen?", "the complete one is read aloud");
    }

    #[test]
    fn a_single_tier_is_shown_read_and_replayed_alone() {
        let mut f = tier_fixture(Answer::Short);
        let (id, parts) = Fixture::speak_job(&f.send(Input::Primary));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(cap.say, "Share your screen?");
        assert!(cap.alts.is_empty(), "only the chosen tier");
        assert!(cap.segments.iter().any(|s| s.is_target() && s.text() == "Share your screen?"));
        assert_eq!(parts.last().unwrap().text, "Share your screen?");
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "banana split".into() }));
        let (_, retry) = Fixture::speak_job(&f.tick(RETRY_PAUSE + 0.2));
        assert_eq!(retry[0].text, "Share your screen?");
    }

    #[test]
    fn recall_keeps_the_other_tiers_hidden_until_a_miss_reveals_them() {
        let mut f = tier_fixture(Answer::All);
        f.lesson.set_options(|o| o.practice = Practice::Recall, &mut f.scene);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        assert!(f.scene.hud.caption.as_ref().unwrap().alts.is_empty(), "no answer while recalling");
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "no idea".into() }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().alts.len(), 2);
    }

    /// Asks the tiered phrase, answers `heard`; returns (passed, spell, snow melted).
    fn say_tier(answer: Answer, heard: &str) -> (bool, Option<Spell>, f32) {
        let mut f = tier_fixture(answer);
        f.scene.fires.clear(); // a campfire thawing nearby is not the lesson melting
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let before = f.scene.snow.fill();
        f.send(Input::Speech(SpeechEvent::Heard { id, text: heard.into() }));
        f.settle();
        let passed = f.scene.hud.caption.as_ref().unwrap().status == Status::Passed;
        (passed, f.scene.last_spell(), before - f.scene.snow.fill())
    }

    #[test]
    fn any_tier_passes_whichever_one_is_asked() {
        for answer in Answer::CHOICES {
            for heard in [
                "Share your screen?",
                "Can you share your screen?",
                "Could you share your screen?",
                "Could you please share your screen?",
            ] {
                assert!(say_tier(answer, heard).0, "{answer:?}: {heard:?} must pass");
            }
            assert!(!say_tier(answer, "banana split").0);
        }
    }

    #[test]
    fn the_tier_said_picks_the_spell_and_a_fuller_answer_melts_more() {
        let (_, short, short_melt) = say_tier(Answer::Short, "Share your screen");
        let (_, complete, complete_melt) = say_tier(Answer::Short, "Can you share your screen");
        let (_, variant, _) = say_tier(Answer::Short, "Could you share your screen");
        let (_, polished, polished_melt) = say_tier(Answer::Short, "Could you please share your screen");
        assert_eq!(short, Some(Spell::Spark));
        assert_eq!(complete, Some(Spell::Fireball), "the short tier inside it ties; the higher tier wins");
        assert_eq!(variant, Some(Spell::Fireball), "an accepted variant counts as complete");
        assert_eq!(polished, Some(Spell::Blaze));
        assert!(
            0.0 < short_melt && short_melt < complete_melt && complete_melt < polished_melt,
            "{short_melt} {complete_melt} {polished_melt}"
        );
        let (passed, spell, missed) = say_tier(Answer::Polished, "banana split");
        assert!(!passed && spell.is_none());
        assert!(missed <= 1e-6, "a miss melts nothing: {missed}");
    }

    #[test]
    fn the_longest_answer_said_counts_even_with_a_recognizer_slip() {
        // The shorter tiers sit inside the polished one and score a perfect
        // window; a slip in the long answer must not hand the reward to them.
        let (passed, spell, _) = say_tier(Answer::Short, "Could you plese share your screen");
        assert!(passed);
        assert_eq!(spell, Some(Spell::Blaze), "the learner said the polished answer");
        let (_, spell, _) = say_tier(Answer::Short, "Can yu share your screen");
        assert_eq!(spell, Some(Spell::Fireball));
    }

    #[test]
    fn the_caption_says_which_answer_counted() {
        let footer = |heard: &str| {
            let mut f = tier_fixture(Answer::All);
            let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
            f.send(Input::Speech(SpeechEvent::Spoken { id }));
            f.send(Input::Speech(SpeechEvent::Heard { id, text: heard.into() }));
            f.scene.hud.caption.as_ref().unwrap().footer.clone()
        };
        assert!(footer("Could you please share your screen").starts_with("Resposta polida!"));
        assert!(footer("Share your screen").starts_with("Resposta curta!"));
        assert!(footer("Can you share your screen").starts_with("Resposta completa!"));
        let mut f = fixture(true);
        f.answer(true);
        assert!(!f.scene.hud.caption.as_ref().unwrap().footer.starts_with("Resposta"), "no tiers, nothing to tell");
    }

    #[test]
    fn the_option_that_counted_is_the_one_lit_green() {
        let answered = |heard: &str| {
            let mut f = tier_fixture(Answer::All);
            let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
            f.send(Input::Speech(SpeechEvent::Spoken { id }));
            f.send(Input::Speech(SpeechEvent::Heard { id, text: heard.into() }));
            let c = f.scene.hud.caption.take().unwrap();
            let asked_green = c.feedback.as_ref().is_some_and(|w| w.iter().all(|w| w.hit));
            (c.counted.map(|i| c.alts[i].clone()), asked_green)
        };
        let (counted, asked_green) = answered("perdon could you please share your screen");
        assert_eq!(counted.as_deref(), Some("polida: Could you please share your screen?"));
        assert!(!asked_green, "the asked phrase is not what counted");
        assert_eq!(answered("Share your screen").0.as_deref(), Some("curta: Share your screen?"));
        assert_eq!(answered("Can you share your screen"), (None, true), "the asked one lights itself");
    }

    #[test]
    fn after_a_right_answer_every_other_option_is_shown_even_when_one_was_asked() {
        let mut f = tier_fixture(Answer::Short);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        assert!(f.scene.hud.caption.as_ref().unwrap().alts.is_empty(), "only the asked option before answering");
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "Could you please share your screen".into() }));
        let c = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(
            c.alts,
            vec!["completa: Can you share your screen?", "polida: Could you please share your screen?"],
            "the learner sees what else they could say"
        );
        assert_eq!(c.counted, Some(1), "with the one recognized lit green");
    }

    #[test]
    fn a_phrase_without_tiers_casts_the_usual_fireball() {
        let mut f = fixture(true);
        f.lesson.set_options(|o| o.answer = Answer::Polished, &mut f.scene);
        let say = f.answer(true);
        assert_eq!(f.scene.last_spell(), Some(Spell::Fireball), "{say:?} has only the complete tier");
        assert!(f.scene.hud.caption.as_ref().unwrap().alts.is_empty());
    }

    #[test]
    fn confirming_without_a_mic_counts_the_asked_tier() {
        let mut o = options(false);
        o.answer = Answer::Short;
        let mut f = fixture_with(o);
        f.lesson.set_deck(Deck::parse(TIER_DECK).unwrap(), &mut f.scene);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Primary);
        assert_eq!(f.scene.last_spell(), Some(Spell::Spark));
    }

    #[test]
    fn a_new_learner_is_asked_first_words_not_full_phrases() {
        let item = |say: &str, level: &str| {
            format!("[[phrase]]\nsay='{say}'\nmeaning='m'\nsituation='s'\ntopic='t'\nlevel='{level}'\n")
        };
        let mut src = "language='en'\nnative='pt-BR'\ntitle='t'\n".to_string();
        for (say, level) in [("Could you repeat that please?", "A1"), ("Where is the train station?", "A1")] {
            src += &item(say, level);
        }
        for w in ["Water.", "Hello.", "Coffee.", "Bye."] {
            src += &item(w, "A1");
        }
        let mut f = fixture_deck(&src);
        let mut asked = std::collections::HashSet::new();
        for _ in 0..4 {
            let say = f.answer(true);
            assert_eq!(say.split_whitespace().count(), 1, "asked {say:?} before the first words");
            asked.insert(say);
            f.finish();
        }
        assert_eq!(asked.len(), 4, "every first word gets its turn");
        // Knowing words (2 successes) opens the phrases, one per word learned.
        let later: Vec<String> = (0..8)
            .map(|_| {
                let say = f.answer(true);
                f.finish();
                say
            })
            .collect();
        assert!(later.iter().any(|s| s.split_whitespace().count() > 1), "phrases never opened: {later:?}");
    }

    /// Answers correctly until the path announces something; returns (word, toast).
    fn first_path_news(items: &[&str]) -> (String, String) {
        let mut src = "language='en'\nnative='pt-BR'\ntitle='t'\n".to_string();
        for say in items {
            src += &format!("[[phrase]]\nsay='{say}'\nmeaning='m'\nsituation='s'\ntopic='t'\nlevel='A1'\n");
        }
        let mut f = fixture_deck(&src);
        let mut news = None;
        for _ in 0..12 {
            let say = f.answer(true);
            assert!(f.scene.hud.caption.as_ref().unwrap().tag.starts_with("palavra · "), "stage in the caption");
            let toast = f.scene.hud.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
            f.finish();
            if toast.starts_with("Aprendeu") || toast.starts_with("Nova etapa") {
                news = Some((say, toast));
                break;
            }
        }
        news.expect("a word became known within 12 answers")
    }

    #[test]
    fn learning_a_word_says_so_and_names_the_word_it_opened() {
        let (say, toast) = first_path_news(&["Hello.", "Bye.", "Yes.", "No.", "Please."]);
        assert_eq!(toast, format!("Aprendeu: {say} · Nova palavra: Please."));
    }

    #[test]
    fn finishing_the_words_announces_the_expressions_stage() {
        let (_, toast) = first_path_news(&["Hello.", "Bye.", "Yes.", "No.", "Good morning."]);
        assert_eq!(toast, "Nova etapa: expressões! Agora você junta palavras. Primeira: Good morning.");
    }

    const PRE_DECK: &str = "language='en'\nnative='pt-BR'\ntitle='t'\nlanguage_name='inglês'\n\
         [[phrase]]\nsay='Thanks.'\nmeaning='Obrigado.'\nsituation='O garçom traz a água.'\ntopic='restaurante'\nlevel='PRE-A1'";

    /// A pre-A1 learner answers the only phrase with `heard`; true if it passed.
    fn beginner_answers(max_level: &str, heard: &str) -> (bool, Vec<Utterance>, String) {
        let mut o = options(true);
        o.practice = Practice::Recall; // asks from memory, unless the learner knows nothing yet
        o.max_level = max_level.into();
        let mut f = fixture_with(o);
        f.lesson.set_deck(Deck::parse(PRE_DECK).unwrap(), &mut f.scene);
        let (id, parts) = Fixture::speak_job(&f.send(Input::Primary));
        let tag = f.scene.hud.caption.as_ref().unwrap().tag.clone();
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: heard.into() }));
        (f.scene.hud.caption.as_ref().unwrap().status == Status::Passed, parts, tag)
    }

    #[test]
    fn a_pre_a1_learner_always_hears_the_answer_slower_and_gets_a_gentler_check() {
        let (passed, parts, tag) = beginner_answers(PRE_A1, "tenks");
        assert!(tag.contains("repita"), "never from memory at pre-A1: {tag}");
        let target = parts.iter().find(|p| p.lang == "en").expect("the answer is read out");
        assert_eq!(target.speed, Speed::Slower);
        assert!(passed, "one extra slip is fine for a beginner");

        let (passed, parts, tag) = beginner_answers("A1", "tenks");
        assert!(tag.contains("memória"), "{tag}");
        assert!(parts.iter().all(|p| p.lang != "en" || p.speed == Speed::Slow));
        assert!(!passed, "the usual check from A1 up");
    }

    #[test]
    fn the_dropped_topic_notice_fits_the_narrowest_screen_for_every_topic() {
        for native in Native::ALL {
            for topic in Deck::languages_for(native).iter().flat_map(|l| Deck::builtin_for(l, native).unwrap().topics())
            {
                let text = topic_dropped(native, std::slice::from_ref(&topic));
                assert!(text.contains(&crate::lang::text::topic(native, &topic)), "{text}");
                assert!(crate::render::font::text_width(&text) <= 316, "{text:?} overflows a 320 px window");
                assert!(crate::render::font::supports(&text), "{text:?}");
            }
        }
    }

    #[test]
    fn a_topic_with_nothing_at_the_level_is_dropped_and_named() {
        let mut o = options(true);
        o.topics = vec!["trabalho".into()];
        o.max_level = "B2".into();
        let mut f = fixture_with(o);
        assert!(f.lesson.drop_empty_topics(&mut f.scene).is_empty(), "trabalho has B2 phrases");
        f.lesson.set_options(|o| o.max_level = PRE_A1.into(), &mut f.scene);
        let deck = format!(
            "{PRE_DECK}\n[[phrase]]\nsay='Could you repeat that?'\nmeaning='Pode repetir?'\nsituation='Na call.'\n\
             topic='trabalho'\nlevel='A2'"
        );
        f.lesson.set_deck(Deck::parse(&deck).unwrap(), &mut f.scene);
        let ticked = ["trabalho", "restaurante", "sumido"].map(String::from).to_vec();
        f.lesson.set_options(|o| o.topics = ticked, &mut f.scene);
        assert_eq!(f.lesson.topics(), vec![("restaurante".into(), 1)], "the panel offers only what exists at pre-A1");
        assert_eq!(f.lesson.drop_empty_topics(&mut f.scene), ["trabalho"], "a topic the deck lacks goes quietly");
        assert_eq!(f.lesson.options().topics, ["restaurante"], "what still has items stays ticked");
        f.lesson.set_options(|o| o.topics = vec!["trabalho".into()], &mut f.scene);
        assert_eq!(f.lesson.drop_empty_topics(&mut f.scene), ["trabalho"]);
        assert!(f.lesson.options().topics.is_empty(), "nothing left ticked = every topic");
        let (_, parts) = Fixture::speak_job(&f.send(Input::Primary));
        assert!(!parts.is_empty(), "a lesson starts instead of 'Nenhuma frase com esse tema/nível'");
    }

    #[test]
    fn challenge_reads_native_cue_in_native_voice_and_target_in_target_voice() {
        let mut f = fixture(true);
        let jobs = f.send(Input::Primary);
        let (_, parts) = Fixture::speak_job(&jobs);
        let say = f.current_say();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].lang, "pt-BR");
        assert_eq!(parts[0].speed, Speed::Normal);
        assert_eq!((parts[1].text.as_str(), parts[1].lang.as_str(), parts[1].speed), (say.as_str(), "en", Speed::Slow));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Speaking);
    }

    #[test]
    fn the_mage_poses_the_challenge_and_reacts_to_answers() {
        let mut f = fixture(true);
        f.send(Input::Primary);
        assert!(f.scene.mage_bubble().is_some(), "the mage asks");
        f.send(Input::Dismiss);
        f.answer(false);
        let laugh = f.scene.mage_bubble().unwrap().to_string();
        assert!(Lines::MageLaugh.get(Native::PtBr).contains(&laugh.as_str()), "{laugh}");
        f.tick(RETRY_PAUSE + 0.2);
        f.send(Input::Dismiss);
        f.answer(true);
        assert!(Lines::MageGroan.get(Native::PtBr).contains(&f.scene.mage_bubble().unwrap()));
    }

    #[test]
    fn caption_highlights_the_part_being_spoken_then_listens() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Part { id, index: 1 }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().active, Some(1));
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id }));
        assert!(matches!(jobs.as_slice(), [Job::Listen { lang, .. }] if lang == "en"));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Listening);
    }

    #[test]
    fn correct_answer_melts_the_screen_records_success_and_closes() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let snow_before = f.scene.snow.fill();
        let say = f.current_say();
        f.send(Input::Speech(SpeechEvent::Heard { id, text: format!("hmm, é... {say}") }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Passed);
        f.settle();
        assert!(f.scene.snow.fill() < snow_before);
        assert!(f.scene.snow.fill() > 0.0, "one answer never clears the screen");
        let s = f.lesson.history.day_summary("en", Local::now().date_naive()).unwrap();
        assert_eq!((s[0].say.as_str(), s[0].successes), (say.as_str(), 1));
        f.finish();
        assert!(f.lesson.is_idle());
        assert!(f.scene.hud.caption.is_none());
    }

    #[test]
    fn an_accepted_variant_counts_and_lights_every_word_green() {
        let mut o = options(true);
        o.topics = vec!["trabalho".into()];
        let mut f = fixture_with(o);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        assert_eq!(f.current_say(), "Could you repeat that?");
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "Can you repeat that?".into() }));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(cap.status, Status::Passed);
        assert!(cap.feedback.as_ref().unwrap().iter().all(|w| w.hit));
    }

    #[test]
    fn wrong_answer_shows_feedback_and_replays_only_the_target() {
        let mut f = fixture(true);
        f.scene.fires.clear(); // a campfire thawing nearby is not the lesson melting
        f.scene.warrior.warm_burst(); // nor one he was about to light
        let snow_before = f.scene.snow.fill();
        f.answer(false);
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(cap.status, Status::Failed);
        assert!(cap.feedback.as_ref().unwrap().iter().all(|w| !w.hit));
        assert_eq!(cap.heard.as_deref(), Some("banana split"));
        assert!(f.scene.snow.fill() >= snow_before - 1e-6, "no melting on a miss");
        let jobs = f.tick(RETRY_PAUSE + 0.2);
        let (_, parts) = Fixture::speak_job(&jobs);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].text, f.current_say());
    }

    #[test]
    fn recall_mode_hides_the_answer_and_reveals_it_after_a_miss() {
        let mut o = options(true);
        o.practice = Practice::Recall;
        let mut f = fixture_with(o);
        let (id, parts) = Fixture::speak_job(&f.send(Input::Primary));
        let say = f.current_say();
        assert!(parts.iter().all(|p| p.lang == "pt-BR"), "recall reads only Portuguese");
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert!(cap.segments.iter().all(|s| !s.is_target()));
        assert!(!cap.segments.iter().any(|s| s.text().contains(&say)), "answer must stay hidden");
        assert!(cap.tag.contains("memória"));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "no idea".into() }));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert!(cap.segments.iter().any(|s| s.is_target() && s.text() == say), "revealed after the miss");
        let (_, retry) = Fixture::speak_job(&f.tick(RETRY_PAUSE + 0.2));
        assert_eq!(retry[0].text, say);
    }

    #[test]
    fn recall_on_first_try_melts_more_than_repeat() {
        let run = |practice: Practice| {
            let mut o = options(true);
            o.practice = practice;
            let mut f = fixture_with(o);
            let before = f.scene.snow.fill();
            f.answer(true);
            before - f.scene.snow.fill()
        };
        assert!(run(Practice::Recall) > run(Practice::Repeat));
    }

    #[test]
    fn topic_and_level_filters_limit_what_is_asked() {
        let mut o = options(true);
        o.max_level = "A1".into();
        let mut f = fixture_with(o);
        for _ in 0..4 {
            f.send(Input::Primary);
            assert_eq!(f.current_say(), "I'm hungry");
            f.send(Input::Dismiss);
        }
        let mut o = options(true);
        o.topics = vec!["nada".into()];
        let mut f = fixture_with(o);
        assert!(f.send(Input::Primary).is_empty());
        assert!(f.scene.hud.toast.as_ref().unwrap().text.contains("menu"));
    }

    #[test]
    fn a_streak_of_correct_answers_calls_the_sun_and_a_miss_streak_breaks_it() {
        let mut f = fixture(true);
        for _ in 0..SUN_COMBO {
            f.answer(true);
            f.finish();
        }
        assert_eq!(f.lesson.combo(), SUN_COMBO);
        assert!(f.scene.sun_active());
        let (mut id, _) = Fixture::speak_job(&f.send(Input::Primary));
        for attempt in 1..=MAX_TRIES {
            f.send(Input::Speech(SpeechEvent::Spoken { id }));
            f.send(Input::Speech(SpeechEvent::Heard { id, text: "banana split".into() }));
            if attempt < MAX_TRIES {
                id = Fixture::speak_job(&f.tick(RETRY_PAUSE + 0.2)).0;
            }
        }
        assert_eq!(f.lesson.combo(), 0);
    }

    #[test]
    fn daily_goal_progress_is_shown_and_celebrated_once() {
        let mut f = fixture(true);
        f.lesson.attach(&mut f.scene);
        let st = f.scene.hud.stats.as_ref().unwrap();
        assert_eq!((st.done, st.goal), (0, 2));
        f.answer(true);
        f.finish();
        assert_eq!(f.scene.hud.stats.as_ref().unwrap().done, 1);
        f.answer(true);
        assert!(f.scene.hud.toast.as_ref().unwrap().text.contains("META"));
        f.finish();
        f.scene.hud.toast = None;
        f.answer(true);
        let toast = f.scene.hud.toast.as_ref().map(|t| t.text.clone()).unwrap_or_default();
        assert!(!toast.contains("META"), "celebrated only once");
    }

    #[test]
    fn repeating_a_phrase_today_does_not_count_twice_for_the_goal() {
        let mut o = options(true);
        o.max_level = "A1".into(); // only one phrase available
        let mut f = fixture_with(o);
        f.answer(true);
        f.finish();
        f.answer(true);
        assert_eq!(f.lesson.done_today(), 1);
    }

    #[test]
    fn silence_waits_for_the_learner_instead_of_retrying_on_its_own() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::NoSpeech { id }));
        assert!(f.tick(10.0).is_empty(), "no automatic retry");
        assert!(f.scene.hud.caption.as_ref().unwrap().footer.contains("tentar de novo"));
        assert!(f.lesson.history.day_summary("en", Local::now().date_naive()).unwrap().is_empty(), "not an attempt");
        let (_, parts) = Fixture::speak_job(&f.send(Input::Primary));
        assert_eq!(parts[0].text, f.current_say(), "retries when asked");
        f.send(Input::Dismiss);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::NoSpeech { id }));
        f.tick(WAIT_USER + 0.5);
        assert!(f.lesson.is_idle(), "gives up quietly if you walk away");
    }

    #[test]
    fn listening_gets_a_plan_sized_to_the_phrase_and_the_mode() {
        let mut o = options(true);
        o.practice = Practice::Recall;
        let mut f = fixture_with(o);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let Some(Job::Listen { plan, .. }) = jobs.first() else { panic!("{jobs:?}") };
        assert_eq!(plan.think, 7.0 + 4.0, "recall gets extra thinking time");
        let words = f.current_say().split_whitespace().count() as f32;
        assert!((plan.expected - (0.45 * words + 0.6)).abs() < 1e-4);
    }

    #[test]
    fn the_caption_shows_a_live_meter_and_a_thinking_countdown() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.tick(2.0);
        let m = f.scene.hud.caption.as_ref().unwrap().listen.as_ref().unwrap();
        assert!((m.think_left - 5.0).abs() < 0.15, "counting down: {}", m.think_left);
        f.send(Input::Speech(SpeechEvent::Level { id, level: 0.2, speaking: true }));
        f.tick(2.0);
        let m = f.scene.hud.caption.as_ref().unwrap().listen.as_ref().unwrap();
        assert!(m.speaking && m.level > 0.1);
        assert!((m.think_left - 5.0).abs() < 0.15, "countdown stops once you talk");
    }

    #[test]
    fn pressing_the_hotkey_while_talking_means_done() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        assert_eq!(f.send(Input::Primary), vec![Job::StopListening]);
    }

    #[test]
    fn a_whisper_hallucination_is_treated_as_silence_not_a_miss() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "Thank you for watching!".into() }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().heard.as_deref(), Some("(silêncio)"));
        assert!(f.lesson.history.day_summary("en", Local::now().date_naive()).unwrap().is_empty());
    }

    #[test]
    fn a_held_hotkey_does_not_skip_through_phrases() {
        let mut f = fixture(true);
        f.answer(true);
        let passed = f.current_say();
        for _ in 0..5 {
            f.send(Input::Primary); // auto-repeat burst
        }
        assert_eq!(f.current_say(), passed, "still showing the result");
        f.tick(DEBOUNCE + 0.1);
        f.send(Input::Primary);
        assert_ne!(f.current_say(), passed, "a deliberate press moves on");
    }

    #[test]
    fn pause_cancels_the_lesson_and_blocks_new_ones_until_resumed() {
        let mut f = fixture(true);
        f.send(Input::Primary);
        f.lesson.set_paused(true, &mut f.scene);
        assert!(f.lesson.is_idle() && f.scene.hud.caption.is_none());
        assert!(f.send(Input::Primary).is_empty());
        assert!(f.scene.hud.toast.as_ref().unwrap().text.contains("pausa"));
        f.scene.hud.toast = None;
        f.tick(120.0);
        assert!(f.scene.hud.toast.is_none(), "no nudges while paused");
        f.lesson.set_paused(false, &mut f.scene);
        assert!(!f.send(Input::Primary).is_empty());
    }

    #[test]
    fn gives_up_after_max_tries_and_records_each_attempt() {
        let mut f = fixture(true);
        let (mut id, _) = Fixture::speak_job(&f.send(Input::Primary));
        for attempt in 1..=MAX_TRIES {
            f.send(Input::Speech(SpeechEvent::Spoken { id }));
            f.send(Input::Speech(SpeechEvent::Heard { id, text: "banana split".into() }));
            if attempt < MAX_TRIES {
                id = Fixture::speak_job(&f.tick(RETRY_PAUSE + 0.2)).0;
            }
        }
        let say = f.current_say();
        f.finish();
        assert!(f.lesson.is_idle());
        let s = f.lesson.history.day_summary("en", Local::now().date_naive()).unwrap();
        let row = s.iter().find(|r| r.say == say).unwrap();
        assert_eq!((row.attempts, row.successes), (MAX_TRIES, 0));
    }

    #[test]
    fn without_a_recognizer_the_hotkey_confirms_the_phrase() {
        let mut f = fixture(false);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id }));
        assert!(jobs.is_empty(), "must not try to listen");
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Confirm);
        f.send(Input::Primary);
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Passed);
    }

    #[test]
    fn mic_failure_falls_back_to_confirm_instead_of_getting_stuck() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        f.send(Input::Speech(SpeechEvent::Failed { id, error: "no microphone found".into() }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Confirm);
        assert!(f.scene.hud.toast.as_ref().unwrap().text.contains("no microphone"));
    }

    #[test]
    fn events_from_a_cancelled_challenge_are_ignored() {
        let mut f = fixture(true);
        let (old, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Dismiss);
        let (new, _) = Fixture::speak_job(&f.send(Input::Primary));
        assert_ne!(old, new);
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id: old }));
        assert!(jobs.is_empty());
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Speaking);
    }

    #[test]
    fn consecutive_challenges_do_not_repeat_the_same_phrase() {
        let mut f = fixture(true);
        f.send(Input::Primary);
        let first = f.current_say();
        f.send(Input::Dismiss);
        f.send(Input::Primary);
        assert_ne!(f.current_say(), first);
    }

    #[test]
    fn switching_language_live_cancels_and_uses_the_new_deck() {
        let mut f = fixture(true);
        f.send(Input::Primary);
        let mut es = Deck::builtin("es").unwrap();
        es.phrases.truncate(3);
        f.lesson.set_deck(es, &mut f.scene);
        assert!(f.scene.hud.caption.is_none(), "old challenge closed");
        let (_, parts) = Fixture::speak_job(&f.send(Input::Primary));
        assert!(parts.iter().any(|p| p.lang == "es"));
        assert!(f.scene.hud.stats.as_ref().unwrap().label.starts_with("ES"));
    }

    #[test]
    fn after_switching_to_spanish_the_listener_and_the_check_use_spanish() {
        let mut f = fixture_deck(
            "language='es'\nnative='pt-BR'\ntitle='t'\nlanguage_name='espanhol'\n\
             [[phrase]]\nsay='Pollo.'\nmeaning='Frango.'\nsituation='O garçom pergunta o prato.'\ntopic='restaurante'\nlevel='A1'",
        );
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id }));
        assert!(matches!(jobs.as_slice(), [Job::Listen { lang, .. }] if lang == "es"), "{jobs:?}");
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "¡Poyo!".into() }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Passed, "said right, spelled by sound");
    }

    #[test]
    fn the_listener_gets_the_answers_and_the_deck_vocabulary_as_a_hint() {
        let mut f = fixture_deck(
            "language='es'\nnative='pt-BR'\ntitle='t'\nlanguage_name='espanhol'\n\
             [[phrase]]\nsay='¡Pare!'\nmeaning='Pare!'\nsituation='O táxi passou do destino.'\ntopic='viagem'\nlevel='A1'\n\
             [[phrase]]\nsay='Leche.'\nmeaning='Leite.'\nsituation='No café.'\ntopic='comida'\nlevel='A1'",
        );
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        let jobs = f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let [Job::Listen { expect: Some(e), .. }] = jobs.as_slice() else { panic!("{jobs:?}") };
        let target = &f.scene.hud.caption.as_ref().unwrap().say;
        assert_eq!(&e.answers, &vec![target.clone()]);
        assert_eq!(e.lang, "es");
        assert!(e.hint.ends_with(target.as_str()), "target last, where whisper weighs it most: {:?}", e.hint);
        assert!(e.hint.contains("Leche.") || e.hint.contains("¡Pare!"), "neighbours give context: {:?}", e.hint);
        assert!(e.hint.matches(target.as_str()).count() == 1, "{:?}", e.hint);
    }

    #[test]
    fn summary_lists_todays_phrases_and_reads_them_back() {
        let mut f = fixture(true);
        let say = f.answer(true);
        let jobs = f.send(Input::Summary);
        let (sid, parts) = Fixture::speak_job(&jobs);
        assert_eq!(parts[0].lang, "pt-BR");
        assert!(parts[0].text.contains("uma frase"));
        assert_eq!(parts[1].text, say);
        let panel = f.scene.hud.summary.as_ref().unwrap();
        assert_eq!(panel.lines.len(), 1);
        assert!(panel.lines[0].ok);
        f.send(Input::Speech(SpeechEvent::Part { id: sid, index: 1 }));
        assert_eq!(f.scene.hud.summary.as_ref().unwrap().active, Some(0));
        f.send(Input::Speech(SpeechEvent::Spoken { id: sid }));
        f.tick(SUMMARY_LINGER + 0.2);
        assert!(f.scene.hud.summary.is_none());
    }

    #[test]
    fn leaving_the_summary_any_way_stops_the_reading() {
        let stops = |jobs: &[Job]| jobs.iter().any(|j| matches!(j, Job::StopSpeaking));
        let reading = || {
            let mut f = fixture(true);
            f.answer(true);
            f.send(Input::Summary);
            assert!(f.scene.hud.summary.is_some());
            f
        };
        let mut f = reading();
        assert!(stops(&f.send(Input::Dismiss)), "Esc");
        assert!(f.scene.hud.summary.is_none());
        let mut f = reading();
        let jobs = f.send(Input::Summary);
        assert!(stops(&jobs), "the summary hotkey again closes it");
        assert!(f.scene.hud.summary.is_none());
        let mut f = reading();
        f.tick(1.0); // past the held-hotkey debounce
        let jobs = f.send(Input::Primary);
        assert!(stops(&jobs) && jobs.iter().any(|j| matches!(j, Job::Speak { .. })), "skip to a challenge");
        let mut f = reading();
        assert!(stops(&f.lesson.set_paused(true, &mut f.scene)), "pausing");
        let mut f = fixture(true);
        assert!(!stops(&f.lesson.set_paused(true, &mut f.scene)), "nothing to stop outside the summary");
    }

    #[test]
    fn auto_summary_fires_once_per_day_after_the_configured_time() {
        let mut f = fixture(true);
        let evening = Local.with_ymd_and_hms(2026, 9, 27, 21, 5, 0).unwrap();
        let before = Local.with_ymd_and_hms(2026, 9, 27, 20, 55, 0).unwrap();
        assert!(f.send(Input::Tick { dt: 0.1, now: before }).is_empty());
        assert!(!f.send(Input::Tick { dt: 0.1, now: evening }).is_empty());
        f.send(Input::Dismiss);
        assert!(f.send(Input::Tick { dt: 0.1, now: evening }).is_empty(), "only once per day");
    }

    #[test]
    fn idle_learner_gets_taunted_by_the_mage_and_nudged() {
        let mut f = fixture(true);
        f.scene.warrior.warm_burst();
        f.tick(61.0);
        assert!(f.scene.hud.toast.is_some());
        assert!(f.scene.mage_bubble().is_some());
    }

    #[test]
    fn tips_include_hotkeys_general_deck_tips_and_phrase_tips() {
        let f = fixture(true);
        let tips = f.lesson.tips();
        assert!(tips.iter().any(|t| t.contains("Ctrl+Alt+M")));
        assert!(tips.iter().any(|t| t.contains("Falso amigo")));
    }

    /// An English speaker learning `language` from the built-in decks.
    fn english_fixture(language: &str, max_level: &str) -> Fixture {
        let opts = Options { native: Native::En, max_level: max_level.into(), ..options(true) };
        let mut f = fixture_with(opts);
        f.scene.set_native(Native::En);
        let dir = tempfile::tempdir().unwrap();
        let deck = Deck::load(language, Native::En, dir.path()).unwrap();
        f.lesson.set_deck(deck, &mut f.scene);
        f
    }

    #[test]
    fn an_english_speaker_learning_spanish_reads_and_hears_english() {
        let mut f = english_fixture("es", "A1");
        let (_, parts) = Fixture::speak_job(&f.send(Input::Primary));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        let p = f.lesson.deck().phrases.iter().find(|p| p.say == cap.say).unwrap().clone();
        let situation = p.situation.clone().unwrap();
        assert!(cap.meaning == p.meaning && !cap.meaning.is_empty());
        assert_eq!(parts[0].text, format!("{situation} Say:"), "the cue is English");
        assert_eq!(parts[0].lang, "en", "and read by the English voice");
        assert_eq!(parts[1].lang, "es");
        assert!(cap.tag.ends_with("· repeat"), "{}", cap.tag);
        let pt_meaning = Deck::builtin("es").unwrap().phrases.into_iter().find(|q| q.say == p.say).unwrap().meaning;
        assert_ne!(cap.meaning, pt_meaning, "not the Portuguese meaning");
        let bubble = f.scene.mage_bubble().unwrap().to_string();
        assert!(Lines::MageAskRepeat.get(Native::En).contains(&bubble.as_str()), "{bubble}");
    }

    #[test]
    fn english_feedback_footers_and_recap_follow_the_native_language() {
        let mut f = english_fixture("es", "A1");
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().footer, "Done? Ctrl+Alt+M · Esc: cancel");
        f.send(Input::Speech(SpeechEvent::NoSpeech { id }));
        let cap = f.scene.hud.caption.as_ref().unwrap();
        assert_eq!(cap.heard.as_deref(), Some("(silence)"));
        assert_eq!(f.scene.mage_bubble(), Some("Huh? I heard nothing!"));
        f.send(Input::Dismiss);
        f.send(Input::Summary);
        let summary = f.scene.hud.summary.as_ref().unwrap();
        assert!(summary.title.starts_with("TODAY'S RECAP"), "{}", summary.title);
        assert!(f.lesson.tips().iter().any(|t| t.starts_with("Press Ctrl+Alt+M")));
        for t in f.lesson.tips() {
            assert!(!t.starts_with("Dica:") && !t.contains("Aperte"), "Portuguese tip for an English speaker: {t}");
        }
    }

    #[test]
    fn an_english_beginner_starts_brazilian_portuguese_with_single_words() {
        let mut f = english_fixture("pt-BR", PRE_A1);
        for _ in 0..6 {
            let say = f.answer(true);
            assert_eq!(say.split_whitespace().count(), 1, "{say:?} is not a first word");
            f.finish();
        }
        assert!(f.lesson.deck().phrases.iter().all(|p| p.situation.is_some()));
    }

    /// A pt-BR learner of English with `topics` ticked, at `max_level`.
    fn ticked_fixture(topics: &[&str], max_level: &str) -> Fixture {
        let o = Options {
            max_level: max_level.into(),
            topics: topics.iter().map(|t| t.to_string()).collect(),
            ..options(true)
        };
        let mut f = fixture_with(o);
        let dir = tempfile::tempdir().unwrap();
        f.lesson.set_deck(Deck::load("en", Native::PtBr, dir.path()).unwrap(), &mut f.scene);
        f
    }

    fn topic_of(f: &Fixture, say: &str) -> String {
        f.lesson.deck().phrases.iter().find(|p| p.say == say).unwrap().topic.clone()
    }

    #[test]
    fn lessons_ask_only_from_the_ticked_topics() {
        let mut f = ticked_fixture(&["restaurante", "viagem"], "B1");
        assert_eq!(f.lesson.topics().len(), f.lesson.deck().topics_at("B1").len(), "the panel still lists every topic");
        let mut seen = std::collections::HashSet::new();
        for _ in 0..12 {
            let say = f.answer(true);
            seen.insert(topic_of(&f, &say));
            f.finish();
        }
        assert!(seen.iter().all(|t| t == "restaurante" || t == "viagem"), "{seen:?}");
        let sel = f.lesson.selection();
        let in_sel = |t: &str| sel.iter().any(|&i| f.lesson.deck().phrases[i].topic == t);
        assert!(in_sel("restaurante") && in_sel("viagem"), "both ticked topics are in rotation");
        let label = &f.scene.hud.stats.as_ref().unwrap().label;
        assert!(label.ends_with("restaurante + viagem"), "{label}");
    }

    #[test]
    fn a_beginner_with_ticked_topics_still_starts_from_single_words() {
        let mut f = ticked_fixture(&["restaurante", "trabalho"], PRE_A1);
        let sel = f.lesson.selection();
        assert!(!sel.is_empty());
        for &i in &sel {
            let p = &f.lesson.deck().phrases[i];
            assert!(p.topic == "restaurante" || p.topic == "trabalho", "{:?}", p.say);
            assert_eq!(p.level.as_deref(), Some(PRE_A1), "{:?}: nothing above the level", p.say);
        }
        let first = path::ordered(&f.lesson.deck().phrases, &sel)[0];
        assert_eq!(
            path::Stage::of(&f.lesson.deck().phrases[first]),
            path::Stage::Words,
            "the path still starts at words"
        );
        let say = f.answer(true);
        assert_eq!(say.split_whitespace().count(), 1, "{say:?} is not a first word");
    }

    #[test]
    fn nothing_ticked_asks_from_every_topic() {
        let f = ticked_fixture(&[], "B1");
        let all = f.lesson.deck().selection(&[], "B1");
        assert_eq!(f.lesson.selection(), all);
        let label = &f.scene.hud.stats.as_ref().unwrap().label;
        assert!(label.ends_with("todos os temas"), "{label}");
    }

    #[test]
    fn the_notice_for_two_dropped_topics_fits_the_narrowest_screen() {
        for native in Native::ALL {
            for lang in Deck::languages_for(native) {
                let topics = Deck::builtin_for(lang, native).unwrap().topics();
                for pair in topics.windows(2) {
                    let text = topic_dropped(native, pair);
                    assert!(crate::render::font::text_width(&text) <= 316, "{text:?} overflows a 320 px window");
                    assert!(crate::render::font::supports(&text), "{text:?}");
                }
            }
        }
        let two = ["social".to_string(), "viagem".to_string()];
        assert_eq!(topic_dropped(Native::PtBr, &two), "Sem social + viagem nesse nível.", "named when they fit");
        let long: Vec<String> = ["negociação e opinião", "casa e burocracia", "gírias e conversa", "vida no exterior"]
            .map(String::from)
            .to_vec();
        assert_eq!(topic_dropped(Native::PtBr, &long), "Sem 4 temas nesse nível.");
    }

    #[test]
    fn untranslated_phrases_never_reach_an_english_speakers_path() {
        let f = english_fixture("es", "C2");
        let deck = f.lesson.deck();
        let curated: std::collections::HashSet<String> = [
            Deck::parse(include_str!("../../decks/es.toml")).unwrap(),
            Deck::parse(include_str!("../../decks/es.basics.toml")).unwrap(),
        ]
        .into_iter()
        .flat_map(|d| d.phrases.into_iter().map(|p| p.say))
        .collect();
        let open = path::ordered(&deck.phrases, &f.lesson.selection());
        assert!(!open.is_empty());
        for i in open {
            assert!(curated.contains(&deck.phrases[i].say), "{:?} has no English translation", deck.phrases[i].say);
        }
    }
}
