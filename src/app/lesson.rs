//! The lesson flow, independent of windows and audio devices:
//! hotkey → cue read aloud → listen → score → melt (or retry) → record,
//! plus the nudges and the end-of-day recap. It drives the scene and asks the
//! app to run speech jobs through `Output`.

use crate::learn::cue::Segment;
use crate::learn::deck::{Deck, Phrase};
use crate::learn::picker;
use crate::scene::Scene;
use crate::scene::hud::{Caption, Status, SummaryLine, SummaryPanel};
use crate::speech::matcher;
use crate::speech::worker::{Job, SpeechEvent, Utterance};
use crate::store::history::{Attempt, History};
use chrono::{DateTime, Local, NaiveTime};

pub const MAX_TRIES: u32 = 3;
const RESULT_SECONDS: f32 = 3.5;
const RETRY_PAUSE: f32 = 1.6;
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

pub struct Options {
    pub native: String,
    pub threshold: f32,
    pub can_listen: bool,
    pub listen_seconds: f32,
    pub hotkey: String,
    pub summary_hotkey: String,
    pub summary_at: NaiveTime,
    /// Seconds idle before the warrior nudges you (from the commitment level).
    pub ask_every: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Stage {
    Speaking,
    Listening,
    Confirm,
    Result { passed: bool, until: f32 },
    RetryPause { until: f32 },
}

enum State {
    Idle,
    Challenge { phrase: usize, stage: Stage, tries: u32, job: u64 },
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
}

impl Lesson {
    pub fn new(deck: Deck, opts: Options, history: History) -> Lesson {
        Lesson {
            deck,
            opts,
            history,
            state: State::Idle,
            clock: 0.0,
            idle_for: 0.0,
            next_job: 1,
            last_phrase: None,
            roll: 7,
        }
    }

    /// Warrior tips built from the hotkeys and the deck.
    pub fn tips(&self) -> Vec<String> {
        let hk = &self.opts.hotkey;
        let mut tips = vec![
            format!("Aperte {hk} e fale uma frase pra me esquentar!"),
            format!("Se a neve subir demais eu congelo! {hk} e fale!"),
            format!("No fim do dia eu leio tudo o que você praticou ({}).", self.opts.summary_hotkey),
        ];
        for p in &self.deck.phrases {
            if let Some(t) = &p.tip {
                tips.push(format!("Dica: {t}"));
            }
        }
        for p in self.deck.phrases.iter().take(6) {
            tips.push(format!("Sabia? \"{}\" = {}", p.say, p.meaning));
        }
        tips
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Idle)
    }

    fn job_id(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }

    fn roll(&mut self) -> f32 {
        self.roll = self.roll.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((self.roll >> 8) % 1000) as f32 / 1000.0
    }

    fn phrase(&self, i: usize) -> &Phrase {
        &self.deck.phrases[i]
    }

    fn cue_parts(&self, p: &Phrase) -> Vec<Utterance> {
        p.cue
            .iter()
            .map(|s| match s {
                Segment::Native(t) => Utterance { text: t.clone(), lang: self.opts.native.clone(), slow: false },
                Segment::Target(t) => Utterance { text: t.clone(), lang: self.deck.language.clone(), slow: true },
            })
            .collect()
    }

    pub fn handle(&mut self, input: Input, scene: &mut Scene) -> Vec<Job> {
        let mut jobs = Vec::new();
        match input {
            Input::Tick { dt, now } => self.tick(dt, now, scene, &mut jobs),
            Input::Primary => self.primary(scene, &mut jobs),
            Input::Summary => {
                if !matches!(self.state, State::Summary { .. }) {
                    self.close(scene);
                    self.start_summary(now_or_default(), scene, &mut jobs);
                }
            }
            Input::Dismiss => self.close(scene),
            Input::Speech(ev) => self.speech(ev, scene, &mut jobs),
        }
        jobs
    }

    fn primary(&mut self, scene: &mut Scene, jobs: &mut Vec<Job>) {
        match self.state {
            State::Idle => self.start_challenge(scene, jobs),
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
        let Some(i) = picker::pick(&self.deck.phrases, &stats, self.last_phrase.as_deref(), roll) else { return };
        self.last_phrase = Some(self.phrase(i).say.clone());
        let job = self.job_id();
        let p = self.phrase(i).clone();
        self.state = State::Challenge { phrase: i, stage: Stage::Speaking, tries: 0, job };
        self.idle_for = 0.0;
        scene.set_practicing(true);
        scene.hud.summary = None;
        scene.hud.caption = Some(Caption {
            segments: p.cue.clone(),
            say: p.say.clone(),
            active: None,
            meaning: p.meaning.clone(),
            status: Status::Speaking,
            feedback: None,
            heard: None,
            footer: String::new(),
        });
        jobs.push(Job::Speak { id: job, parts: self.cue_parts(&p) });
    }

    fn after_cue(&mut self, scene: &mut Scene, jobs: &mut Vec<Job>) {
        let State::Challenge { stage, job, .. } = &mut self.state else { return };
        let cap = scene.hud.caption.as_mut();
        if self.opts.can_listen {
            *stage = Stage::Listening;
            if let Some(c) = cap {
                c.active = None;
                c.status = Status::Listening;
                c.footer = "Esc: cancelar".into();
            }
            jobs.push(Job::Listen {
                id: *job,
                lang: self.deck.language.clone(),
                max_seconds: self.opts.listen_seconds,
            });
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
        let id = match &ev {
            SpeechEvent::Part { id, .. }
            | SpeechEvent::Spoken { id }
            | SpeechEvent::Thinking { id }
            | SpeechEvent::Heard { id, .. }
            | SpeechEvent::NoSpeech { id }
            | SpeechEvent::Failed { id, .. } => *id,
        };
        if id != current {
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
            (State::Challenge { stage: Stage::Listening, .. }, SpeechEvent::Thinking { .. }) => {
                if let Some(c) = &mut scene.hud.caption {
                    c.status = Status::Thinking;
                }
            }
            (State::Challenge { phrase, stage: Stage::Listening, tries, .. }, SpeechEvent::Heard { text, .. }) => {
                let (phrase, tries) = (*phrase, *tries);
                let say = self.phrase(phrase).say.clone();
                let m = matcher::score(&say, &text);
                let passed = m.passed(self.opts.threshold);
                if let Some(c) = &mut scene.hud.caption {
                    c.feedback = Some(m.words.clone());
                    c.heard = Some(text.clone());
                }
                self.finish_attempt(phrase, tries, &text, m.score, passed, scene);
            }
            (State::Challenge { phrase, stage: Stage::Listening, tries, .. }, SpeechEvent::NoSpeech { .. }) => {
                let (phrase, tries) = (*phrase, *tries);
                if let Some(c) = &mut scene.hud.caption {
                    c.heard = Some("(silêncio)".into());
                }
                self.finish_attempt(phrase, tries, "", 0.0, false, scene);
            }
            (State::Challenge { stage, .. }, SpeechEvent::Failed { error, .. }) if *stage == Stage::Listening => {
                // Mic or model trouble: fall back to self-confirmation.
                *stage = Stage::Confirm;
                scene.hud.toast(format!("Microfone/reconhecimento indisponível: {error}"), 6.0);
                if let Some(c) = &mut scene.hud.caption {
                    c.status = Status::Confirm;
                    c.footer = format!("Fale em voz alta e aperte {} para confirmar", self.opts.hotkey);
                }
            }
            _ => {}
        }
    }

    fn finish_attempt(&mut self, phrase: usize, tries: u32, heard: &str, score: f32, passed: bool, scene: &mut Scene) {
        let p = self.phrase(phrase).clone();
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
        let State::Challenge { stage, tries: t, .. } = &mut self.state else { return };
        *t = tries;
        if passed {
            *stage = Stage::Result { passed: true, until: self.clock + RESULT_SECONDS };
            scene.celebrate();
        } else if tries < MAX_TRIES {
            *stage = Stage::RetryPause { until: self.clock + RETRY_PAUSE };
        } else {
            *stage = Stage::Result { passed: false, until: self.clock + RESULT_SECONDS };
        }
        if let Some(c) = &mut scene.hud.caption {
            c.active = None;
            c.status = if passed { Status::Passed } else { Status::Failed };
            c.footer = if passed {
                format!("{}: próxima frase", self.opts.hotkey)
            } else if tries < MAX_TRIES {
                format!("Tentativa {tries}/{MAX_TRIES}. Ouça de novo...")
            } else {
                "Tudo bem, vamos praticar outra depois!".into()
            };
        }
    }

    fn tick(&mut self, dt: f32, now: DateTime<Local>, scene: &mut Scene, jobs: &mut Vec<Job>) {
        self.clock += dt;
        match self.state {
            State::Idle => {
                self.idle_for += dt;
                if self.idle_for >= self.opts.ask_every {
                    self.idle_for = 0.0;
                    scene.warrior.say(format!("Hora de praticar! Aperte {}", self.opts.hotkey), 6.0);
                    scene
                        .hud
                        .toast(format!("O mago está vencendo... {} para lançar uma frase!", self.opts.hotkey), 5.0);
                }
                self.maybe_auto_summary(now, scene, jobs);
            }
            State::Challenge { stage: Stage::Result { until, .. }, .. } if self.clock >= until => self.close(scene),
            State::Challenge { phrase, stage: Stage::RetryPause { until }, job, tries } if self.clock >= until => {
                // Replay just the target phrase, then listen again.
                let p = self.phrase(phrase).clone();
                let job2 = self.job_id();
                let _ = job;
                self.state = State::Challenge { phrase, stage: Stage::Speaking, tries, job: job2 };
                if let Some(c) = &mut scene.hud.caption {
                    c.status = Status::Speaking;
                    c.feedback = None;
                    c.heard = None;
                    c.active = p.cue.iter().position(|s| matches!(s, Segment::Target(t) if *t == p.say));
                }
                let part = Utterance { text: p.say.clone(), lang: self.deck.language.clone(), slow: true };
                jobs.push(Job::Speak { id: job2, parts: vec![part] });
            }
            State::Summary { close_at: Some(t), .. } if self.clock >= t => self.close(scene),
            _ => {}
        }
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
            slow: false,
        }];
        parts.extend(rows.iter().map(|r| Utterance {
            text: r.say.clone(),
            lang: self.deck.language.clone(),
            slow: true,
        }));
        scene.hud.caption = None;
        scene.hud.summary = Some(SummaryPanel {
            title: format!("RESUMO DE HOJE · {}", day.format("%d/%m")),
            lines: rows
                .iter()
                .map(|r| SummaryLine { say: r.say.clone(), meaning: r.meaning.clone(), ok: r.successes > 0 })
                .collect(),
            active: None,
            footer: format!("{} de {} frases acertadas", learned.len(), rows.len()),
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

fn now_or_default() -> DateTime<Local> {
    Local::now()
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

    fn fixture(can_listen: bool) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let history = History::open(&dir.path().join("h.sqlite3")).unwrap();
        let deck = Deck::parse(
            "language='en'\nnative='pt-BR'\ntitle='t'\n\
             [[phrase]]\nsay=\"I'm hungry\"\nmeaning='Estou com fome'\ncue='Para dizer que estou com fome: {}'\n\
             [[phrase]]\nsay='Good morning'\nmeaning='Bom dia'\ncue='De manhã: {}'",
        )
        .unwrap();
        let opts = Options {
            native: "pt-BR".into(),
            threshold: 0.72,
            can_listen,
            listen_seconds: 7.0,
            hotkey: "Ctrl+Alt+M".into(),
            summary_hotkey: "Ctrl+Alt+J".into(),
            summary_at: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            ask_every: 60.0,
        };
        let mut scene = Scene::new(240, 135, 1, Commitment::Relentless.pace(), true);
        for _ in 0..(120 * 30) {
            scene.step(1.0 / 30.0); // let snow pile up so melting is observable
        }
        Fixture { _dir: dir, lesson: Lesson::new(deck, opts, history), scene }
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
    }

    #[test]
    fn challenge_reads_native_cue_in_native_voice_and_target_in_target_voice() {
        let mut f = fixture(true);
        let jobs = f.send(Input::Primary);
        let (_, parts) = Fixture::speak_job(&jobs);
        let say = f.current_say();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].lang, "pt-BR");
        assert!(!parts[0].slow);
        assert_eq!((parts[1].text.as_str(), parts[1].lang.as_str(), parts[1].slow), (say.as_str(), "en", true));
        assert_eq!(f.scene.hud.caption.as_ref().unwrap().status, Status::Speaking);
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
        assert!(f.scene.snow.fill() < snow_before);
        let today = Local::now().date_naive();
        let s = f.lesson.history.day_summary("en", today).unwrap();
        assert_eq!((s[0].say.as_str(), s[0].successes), (say.as_str(), 1));
        f.tick(RESULT_SECONDS + 0.2);
        assert!(f.lesson.is_idle());
        assert!(f.scene.hud.caption.is_none());
    }

    #[test]
    fn wrong_answer_shows_feedback_and_replays_only_the_target() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let snow_before = f.scene.snow.fill();
        f.send(Input::Speech(SpeechEvent::Heard { id, text: "banana split".into() }));
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
    fn gives_up_after_max_tries_and_records_each_attempt() {
        let mut f = fixture(true);
        let (mut id, _) = Fixture::speak_job(&f.send(Input::Primary));
        for attempt in 1..=MAX_TRIES {
            f.send(Input::Speech(SpeechEvent::Spoken { id }));
            f.send(Input::Speech(SpeechEvent::NoSpeech { id }));
            if attempt < MAX_TRIES {
                id = Fixture::speak_job(&f.tick(RETRY_PAUSE + 0.2)).0;
            }
        }
        let say = f.current_say();
        f.tick(RESULT_SECONDS + 0.2);
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
    fn summary_lists_todays_phrases_and_reads_them_back() {
        let mut f = fixture(true);
        let (id, _) = Fixture::speak_job(&f.send(Input::Primary));
        f.send(Input::Speech(SpeechEvent::Spoken { id }));
        let say = f.current_say();
        f.send(Input::Speech(SpeechEvent::Heard { id, text: say.clone() }));
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
    fn idle_learner_gets_nudged_by_the_warrior() {
        let mut f = fixture(true);
        f.scene.warrior.warm_burst(); // make sure he is not frozen and can talk
        f.tick(61.0);
        assert!(f.scene.hud.toast.is_some());
    }

    #[test]
    fn tips_mention_the_hotkey_and_deck_content() {
        let f = fixture(true);
        let tips = f.lesson.tips();
        assert!(tips.iter().any(|t| t.contains("Ctrl+Alt+M")));
        assert!(tips.iter().any(|t| t.contains("Estou com fome")));
    }
}
