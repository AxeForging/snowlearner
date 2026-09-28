//! The lesson flow, independent of windows and audio devices:
//! the mage poses a challenge → cue read aloud → listen → score → melt (or
//! retry) → record; plus combos, the daily goal, nudges and the end-of-day
//! recap. It drives the scene and asks the app to run speech jobs.

use crate::learn::cue::Segment;
use crate::learn::deck::{Deck, PRE_A1, Phrase};
use crate::learn::path;
use crate::learn::picker::{self, Mode, Practice};
use crate::scene::Scene;
use crate::scene::hud::{Caption, Meter, Stats, Status, SummaryLine, SummaryPanel};
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

const MAGE_ASK_REPEAT: &[&str] = &["Repita, se for capaz!", "Hah! Diga isso!", "Vamos ver essa pronúncia!"];
const MAGE_ASK_RECALL: &[&str] = &["Duvido que lembre essa!", "Sem cola agora!", "Essa você já viu. E aí?"];
const MAGE_LAUGH: &[&str] = &["Hahaha! Errou!", "Mais neve pra você!", "Quase... mas não!"];
const MAGE_GROAN: &[&str] = &["Argh! Não!", "Impossível!", "Grrr... sorte!"];

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
    pub native: String,
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
    /// Only practice this topic (None = all).
    pub topic: Option<String>,
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
        if self.opts.topic.as_ref().is_some_and(|t| !deck.topics().contains(t)) {
            self.opts.topic = None;
        }
        self.deck = deck;
        self.last_phrase = None;
        self.combo = 0;
        scene.tips = self.tips();
        self.refresh_stats(scene);
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
        self.deck.selection(self.opts.topic.as_deref(), &self.opts.max_level)
    }

    /// Warrior tips built from the hotkeys and the deck.
    pub fn tips(&self) -> Vec<String> {
        let hk = &self.opts.hotkey;
        let mut tips = vec![
            format!("Aperte {hk} e fale uma frase pra me esquentar!"),
            format!("Se a neve subir demais eu congelo! {hk} e fale!"),
            format!("No fim do dia eu leio tudo o que você praticou ({}).", self.opts.summary_hotkey),
            format!("Acerte {SUN_COMBO} seguidas e o sol aparece!"),
        ];
        tips.extend(self.deck.tips.iter().cloned());
        for &i in self.selection().iter().take(80) {
            if let Some(t) = &self.deck.phrases[i].tip {
                tips.push(format!("Dica: {t}"));
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

    fn line(&mut self, lines: &[&str]) -> String {
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
                    Utterance { text: t.clone(), lang: self.opts.native.clone(), speed: Speed::Normal }
                }
                Segment::Target(t) => {
                    Utterance { text: t.clone(), lang: self.deck.language.clone(), speed: self.target_speed() }
                }
            })
            .collect()
    }

    fn cue_for(&self, p: &Phrase, mode: Mode) -> Vec<Segment> {
        match mode {
            Mode::Repeat => p.cue.clone(),
            Mode::Recall => p.recall_cue(&self.deck.language_name),
        }
    }

    fn tag(&self, p: &Phrase, mode: Mode) -> String {
        let mode = match mode {
            Mode::Repeat => "repita",
            Mode::Recall => "de memória",
        };
        format!("{} · {} · {mode}", path::Stage::of(p).singular_pt(), p.topic)
    }

    fn refresh_stats(&self, scene: &mut Scene) {
        scene.hud.stats = Some(Stats {
            done: self.done_today,
            goal: self.opts.daily_goal,
            combo: self.combo,
            label: format!(
                "{} · {}",
                self.deck.language.to_uppercase(),
                self.opts.topic.clone().unwrap_or_else(|| "todos os temas".into())
            ),
        });
    }

    /// Paused (black hole): no lessons, nudges or recaps until resumed.
    pub fn set_paused(&mut self, on: bool, scene: &mut Scene) {
        self.paused = on;
        if on {
            self.close(scene);
        }
        self.idle_for = 0.0;
    }

    pub fn handle(&mut self, input: Input, scene: &mut Scene) -> Vec<Job> {
        let mut jobs = Vec::new();
        if self.paused {
            match input {
                Input::Tick { dt, .. } => self.clock += dt,
                Input::Primary | Input::Summary => {
                    scene.hud.toast("Em pausa: botão direito no orbe (ou P) para voltar", 3.0)
                }
                _ => {}
            }
            return jobs;
        }
        match input {
            Input::Tick { dt, now } => self.tick(dt, now, scene, &mut jobs),
            Input::Primary => self.primary(scene, &mut jobs),
            Input::Summary => {
                if !matches!(self.state, State::Summary { .. }) {
                    self.close(scene);
                    self.start_summary(Local::now(), scene, &mut jobs);
                }
            }
            Input::Dismiss => self.close(scene),
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
                    c.footer = "Ok, analisando...".into();
                }
            }
            State::Challenge { phrase, stage: Stage::Confirm, tries, .. } => {
                let say = self.phrase(phrase).say.clone();
                self.finish_attempt(phrase, tries, &say, 1.0, true, scene);
            }
            State::Challenge { stage: Stage::Result { .. }, .. } | State::Summary { .. } => {
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
            scene.hud.toast("Nenhuma frase com esse tema/nível. Mude no menu.", 4.0);
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
        let ask = self.line(if mode == Mode::Recall { MAGE_ASK_RECALL } else { MAGE_ASK_REPEAT });
        scene.mage_say(ask, 3.0);
        scene.hud.summary = None;
        let cue = self.cue_for(&p, mode);
        scene.hud.caption = Some(Caption {
            segments: cue.clone(),
            say: p.say.clone(),
            active: None,
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
        let State::Challenge { phrase, mode, stage, job, .. } = &mut self.state else { return };
        let words = self.deck.phrases[*phrase].say.split_whitespace().count();
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
                c.footer = format!("Terminou? {} · Esc: cancelar", self.opts.hotkey);
                c.listen = Some(Meter { level: 0.0, speaking: false, think_left: plan.think, think_total: plan.think });
            }
            jobs.push(Job::Listen { id: *job, lang: self.deck.language.clone(), plan });
        } else {
            *stage = Stage::Confirm;
            if let Some(c) = cap {
                c.active = None;
                c.status = Status::Confirm;
                c.footer = format!("Fale em voz alta e aperte {} para confirmar", self.opts.hotkey);
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
                scene.hud.toast(format!("Voz indisponível: {error}"), 5.0);
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
                let (m, _) = matcher::score_any(&p.answers(), &text, slack);
                let passed = m.passed(self.opts.threshold);
                if !passed && matcher::is_hallucination(&text) {
                    // Whisper invented "Thank you for watching" out of noise: that's silence.
                    self.silence(scene);
                    return;
                }
                // Feedback is shown on the preferred phrase; an accepted variant lights it all green.
                let words = if passed {
                    p.say.split_whitespace().map(|w| WordHit { word: w.to_string(), hit: true }).collect()
                } else {
                    matcher::score_lenient(&p.say, &text, slack).words
                };
                if let Some(c) = &mut scene.hud.caption {
                    c.feedback = Some(words);
                    c.heard = Some(text.clone());
                }
                self.finish_attempt(phrase, tries, &text, m.score, passed, scene);
            }
            (State::Challenge { stage: Stage::Listening, .. }, SpeechEvent::NoSpeech { .. }) => self.silence(scene),
            (State::Challenge { stage, .. }, SpeechEvent::Failed { error, .. }) if *stage == Stage::Listening => {
                // Mic or model trouble: fall back to self-confirmation.
                *stage = Stage::Confirm;
                scene.hud.toast(format!("Microfone/reconhecimento indisponível: {error}"), 6.0);
                if let Some(c) = &mut scene.hud.caption {
                    c.listen = None;
                    c.status = Status::Confirm;
                    c.footer = format!("Fale em voz alta e aperte {} para confirmar", self.opts.hotkey);
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
        scene.mage_say("Hã? Não ouvi nada!", 2.5);
        if let Some(c) = &mut scene.hud.caption {
            c.status = Status::Failed;
            c.listen = None;
            c.heard = Some("(silêncio)".into());
            c.footer = format!("Não ouvi nada. {} ou clique no orbe para tentar de novo", self.opts.hotkey);
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
        let mut news = format!("Aprendeu: {}", p.say);
        if let Some(&i) = opened.first() {
            let next = &self.deck.phrases[i];
            let stage = path::Stage::of(next);
            let reached = open_before.iter().all(|&j| path::Stage::of(&self.deck.phrases[j]) < stage);
            news = if reached {
                format!("{} Primeira: {}", stage.welcome_pt(), next.say)
            } else {
                format!("{news} · Nova {}: {}", stage.singular_pt(), next.say)
            };
        }
        scene.hud.toast(news, 5.0);
    }

    fn finish_attempt(&mut self, phrase: usize, tries: u32, heard: &str, score: f32, passed: bool, scene: &mut Scene) {
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
        let (stage, footer) = if passed {
            (Stage::Result { until: self.clock + RESULT_SECONDS }, format!("{}: próxima frase", self.opts.hotkey))
        } else if tries < MAX_TRIES {
            let hint = if mode == Mode::Recall { "Era assim: ouça e repita..." } else { "Ouça de novo..." };
            (Stage::RetryPause { until: self.clock + RETRY_PAUSE }, format!("Tentativa {tries}/{MAX_TRIES}. {hint}"))
        } else {
            (Stage::Result { until: self.clock + RESULT_SECONDS }, "Tudo bem, vamos praticar outra depois!".into())
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
            scene.celebrate(power);
            let groan = self.line(MAGE_GROAN);
            scene.mage_say(groan, 3.5);
            if self.combo >= SUN_COMBO && self.combo % SUN_COMBO == 0 {
                scene.sun();
                scene.hud.toast(format!("COMBO x{}! O SOL APARECEU!", self.combo), 4.0);
            }
            self.path_news(&p, &open_before, scene);
            if self.done_today >= self.opts.daily_goal && !self.goal_celebrated {
                self.goal_celebrated = true;
                scene.hud.toast(format!("META DO DIA: {} frases! Mandou bem!", self.opts.daily_goal), 5.0);
                scene.warrior.say("Meta do dia batida! Tô quentinho!", 5.0);
            }
        } else {
            let laugh = self.line(MAGE_LAUGH);
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
                c.segments = p.cue.clone();
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
                    scene.mage_say("Ninguém vai me enfrentar? Hahaha!", 4.0);
                    scene.warrior.say(format!("Hora de praticar! Aperte {}", self.opts.hotkey), 6.0);
                    scene.hud.toast(format!("O mago está vencendo... {} para enfrentar!", self.opts.hotkey), 5.0);
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
        if let Some(c) = &mut scene.hud.caption {
            c.status = Status::Speaking;
            c.feedback = None;
            c.heard = None;
            c.footer = String::new();
            c.tag = tag;
            c.segments = p.cue.clone();
            c.meaning = p.meaning.clone();
            c.active = p.cue.iter().position(|s| matches!(s, Segment::Target(t) if *t == p.say));
        }
        let part = Utterance { text: p.say.clone(), lang: self.deck.language.clone(), speed: self.target_speed() };
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
        let mut parts = vec![Utterance {
            text: match learned.len() {
                0 => "Hoje você ainda não praticou nenhuma frase. O guerreiro está com frio!".to_string(),
                1 => "Resumo de hoje: você praticou uma frase.".to_string(),
                n => format!("Resumo de hoje: você praticou {n} frases."),
            },
            lang: self.opts.native.clone(),
            speed: Speed::Normal,
        }];
        parts.extend(rows.iter().map(|r| Utterance {
            text: r.say.clone(),
            lang: self.deck.language.clone(),
            speed: self.target_speed(),
        }));
        scene.hud.caption = None;
        scene.hud.summary = Some(SummaryPanel {
            title: format!("RESUMO DE HOJE · {}", day.format("%d/%m")),
            lines: rows
                .iter()
                .map(|r| SummaryLine { say: r.say.clone(), meaning: r.meaning.clone(), ok: r.successes > 0 })
                .collect(),
            active: None,
            footer: format!("{} de {} frases acertadas · meta {}", learned.len(), rows.len(), self.opts.daily_goal),
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

/// How the target language is read: slower for a pre-A1 learner.
pub fn target_speed(max_level: &str) -> Speed {
    if max_level == PRE_A1 { Speed::Slower } else { Speed::Slow }
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
            native: "pt-BR".into(),
            threshold: 0.72,
            can_listen,
            listen_seconds: 7.0,
            hotkey: "Ctrl+Alt+M".into(),
            summary_hotkey: "Ctrl+Alt+J".into(),
            summary_at: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            ask_every: 60.0,
            practice: Practice::Repeat,
            topic: None,
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
        assert!(MAGE_LAUGH.contains(&laugh.as_str()), "{laugh}");
        f.tick(RETRY_PAUSE + 0.2);
        f.send(Input::Dismiss);
        f.answer(true);
        assert!(MAGE_GROAN.contains(&f.scene.mage_bubble().unwrap()));
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
        o.topic = Some("trabalho".into());
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
        o.topic = Some("nada".into());
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
}
