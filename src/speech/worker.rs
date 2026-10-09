//! Background thread that owns the (blocking) voice and microphone work and
//! reports progress as events, tagged with the job id so stale results can be
//! ignored after a cancel.

use super::endpoint::ListenPlan;
#[cfg(feature = "stt")]
use super::resident::Resident;
use super::trace;
use super::voices::{TtsEngine, Voice};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub text: String,
    pub lang: String,
    /// Target-language parts are read slower (slower still at pre-A1).
    pub speed: super::tts::Speed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Speak {
        id: u64,
        parts: Vec<Utterance>,
    },
    Listen {
        id: u64,
        lang: String,
        plan: ListenPlan,
        /// The answers, for a hinted second pass when the first one misses.
        expect: Option<super::matcher::Expect>,
    },
    /// Microphone check from the panel/CLI: record, then transcribe.
    MicTest {
        id: u64,
        lang: String,
    },
    /// End the current listen now ("I'm done"). Handled out of band.
    StopListening,
    /// Stop reading the current Speak job after the line being said. Out of band.
    StopSpeaking,
    /// Apply new voice/mic settings live (from the panel).
    Configure(Box<VoiceSettings>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpeechEvent {
    /// Part `index` of a Speak job started.
    Part {
        id: u64,
        index: usize,
    },
    Spoken {
        id: u64,
    },
    /// Live microphone level (RMS) and whether speech has started.
    Level {
        id: u64,
        level: f32,
        speaking: bool,
    },
    Thinking {
        id: u64,
    },
    Heard {
        id: u64,
        text: String,
    },
    NoSpeech {
        id: u64,
    },
    Failed {
        id: u64,
        error: String,
    },
}

impl SpeechEvent {
    pub fn id(&self) -> u64 {
        match self {
            SpeechEvent::Part { id, .. }
            | SpeechEvent::Spoken { id }
            | SpeechEvent::Level { id, .. }
            | SpeechEvent::Thinking { id }
            | SpeechEvent::Heard { id, .. }
            | SpeechEvent::NoSpeech { id }
            | SpeechEvent::Failed { id, .. } => *id,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceSettings {
    pub native_voice: String,
    pub learning_voice: String,
    pub native_lang: String,
    pub model: PathBuf,
    pub engine: TtsEngine,
    pub url: String,
    pub tts_model: String,
    pub command: String,
    pub speaker: String,
    pub mic: String,
}

impl VoiceSettings {
    pub fn from(s: &crate::config::settings::Settings, paths: &crate::config::paths::Paths) -> VoiceSettings {
        VoiceSettings {
            native_voice: s.voice_native.clone(),
            learning_voice: s.voice_learning.clone(),
            native_lang: s.native.code().to_string(),
            model: paths.model_file(&s.model),
            engine: s.tts_engine,
            url: s.tts_url.clone(),
            tts_model: s.tts_model.clone(),
            command: s.tts_command.clone(),
            speaker: s.speaker.clone(),
            mic: s.mic.clone(),
        }
    }

    fn voice(&self) -> Voice {
        Voice::new(self.engine, &self.url, &self.tts_model, &self.command, &self.speaker)
    }
}

pub struct Speech {
    /// Jobs with their send order, so a stop can reach queued ones too.
    tx: Sender<(u64, Job)>,
    stop: Arc<AtomicBool>,
    /// Speak jobs sent up to this order number are hushed.
    hush: Arc<AtomicU64>,
    sent: AtomicU64,
    /// Engine description, e.g. "speech-dispatcher (spd-say)".
    pub tts: String,
    pub tts_ok: bool,
    pub can_listen: bool,
}

impl Speech {
    /// `can_listen` is true when this build has the recognizer and a model on disk.
    pub fn start(cfg: VoiceSettings, notify: impl Fn(SpeechEvent) + Send + 'static) -> Speech {
        let voice = cfg.voice();
        let (tts, tts_ok) = (voice.describe(), voice.available());
        let can_listen = cfg!(feature = "stt") && cfg.model.exists();
        let (tx, rx) = mpsc::channel::<(u64, Job)>();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_worker = stop.clone();
        let hush = Arc::new(AtomicU64::new(0));
        let hush_worker = hush.clone();
        std::thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                let mut cfg = cfg;
                let mut voice = voice;
                #[cfg(feature = "stt")]
                let mut recognizer =
                    Resident::<super::stt::Recognizer>::new(super::resident::SPEECH_MODEL_IDLE, Instant::now());
                loop {
                    // Sleep until the next job, waking only to free an idle speech model.
                    #[cfg(feature = "stt")]
                    let deadline = recognizer.deadline();
                    #[cfg(not(feature = "stt"))]
                    let deadline: Option<Instant> = None;
                    let (seq, job) = match deadline {
                        None => match rx.recv() {
                            Ok(job) => job,
                            Err(_) => break,
                        },
                        Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                            Ok(job) => job,
                            Err(RecvTimeoutError::Timeout) => {
                                #[cfg(feature = "stt")]
                                recognizer.release_if_idle(Instant::now());
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => break,
                        },
                    };
                    match job {
                        Job::Configure(new) => {
                            cfg = *new;
                            voice = cfg.voice();
                        }
                        Job::StopListening | Job::StopSpeaking => {}
                        Job::Speak { id, parts } => {
                            let started = Instant::now();
                            for (index, p) in parts.iter().enumerate() {
                                // shortcut: stops between lines, not mid-line; threading the
                                // flag into audio playback would make it instant.
                                if seq <= hush_worker.load(Ordering::SeqCst) {
                                    trace::line(format_args!("speech stopped"));
                                    break;
                                }
                                notify(SpeechEvent::Part { id, index });
                                trace::line(format_args!("speak [{}] {:?}", p.lang, p.text));
                                let v = if p.lang == cfg.native_lang { &cfg.native_voice } else { &cfg.learning_voice };
                                if let Err(e) = voice.speak(&p.text, &p.lang, v, p.speed) {
                                    trace::line(format_args!("speak failed: {e:#}"));
                                    notify(SpeechEvent::Failed { id, error: format!("{e:#}") });
                                    break;
                                }
                            }
                            trace::line(format_args!("spoken in {:.1} s", started.elapsed().as_secs_f32()));
                            notify(SpeechEvent::Spoken { id });
                        }
                        #[cfg(feature = "stt")]
                        Job::Listen { id, lang, plan, expect } => {
                            let ev =
                                listen(id, &lang, expect.as_ref(), plan, &cfg, &mut recognizer, &stop_worker, &notify);
                            recognizer.touch(Instant::now());
                            notify(ev);
                        }
                        #[cfg(feature = "stt")]
                        Job::MicTest { id, lang } => {
                            let plan = ListenPlan { think: 6.0, expected: 3.0 };
                            let ev = listen(id, &lang, None, plan, &cfg, &mut recognizer, &stop_worker, &notify);
                            recognizer.touch(Instant::now());
                            notify(ev);
                        }
                        #[cfg(not(feature = "stt"))]
                        Job::Listen { id, .. } | Job::MicTest { id, .. } => {
                            let _ = &stop_worker;
                            notify(SpeechEvent::Failed { id, error: "built without speech recognition".into() });
                        }
                    }
                }
            })
            .expect("spawning speech thread");
        Speech { tx, stop, hush, sent: AtomicU64::new(0), tts, tts_ok, can_listen }
    }

    pub fn send(&self, job: Job) {
        match job {
            Job::StopListening => self.stop.store(true, Ordering::SeqCst),
            Job::StopSpeaking => self.hush.store(self.sent.load(Ordering::SeqCst), Ordering::SeqCst),
            other => {
                let seq = self.sent.fetch_add(1, Ordering::SeqCst) + 1;
                let _ = self.tx.send((seq, other));
            }
        }
    }
}

#[cfg(feature = "stt")]
/// `expect`: what the lesson expects to hear, for a hinted second pass.
#[allow(clippy::too_many_arguments)]
fn listen(
    id: u64,
    lang: &str,
    expect: Option<&super::matcher::Expect>,
    plan: ListenPlan,
    cfg: &VoiceSettings,
    recognizer: &mut Resident<super::stt::Recognizer>,
    stop: &AtomicBool,
    notify: &impl Fn(SpeechEvent),
) -> SpeechEvent {
    stop.store(false, Ordering::SeqCst);
    trace::line(format_args!("listen [{lang}] think {:.1} s, expected {:.1} s", plan.think, plan.expected));
    let result = (|| -> anyhow::Result<Option<String>> {
        // Open the mic right away and load the model meanwhile: after an idle
        // release the load used to keep the mic closed while the prompt
        // already said "speak now", eating the start of a quick answer.
        let (rec, loaded) = std::thread::scope(|s| {
            let loading = s.spawn(|| {
                let started = Instant::now();
                let loaded = recognizer.get_or_load(&cfg.model, started, super::stt::Recognizer::load).map(|_| ());
                trace::line(format_args!("speech model ready in {:.1} s", started.elapsed().as_secs_f32()));
                loaded
            });
            let rec = super::mic::record(plan, &cfg.mic, stop, |level, speaking| {
                notify(SpeechEvent::Level { id, level, speaking })
            });
            (rec, loading.join())
        });
        let rec = rec?;
        loaded.map_err(|_| anyhow::anyhow!("loading the speech model crashed"))??;
        if !rec.heard_speech {
            return Ok(None);
        }
        notify(SpeechEvent::Thinking { id });
        let started = Instant::now();
        let model = recognizer.get_or_load(&cfg.model, Instant::now(), super::stt::Recognizer::load)?;
        let text = model.transcribe_expecting(&rec.samples, lang, expect)?;
        trace::line(format_args!("heard {text:?} (transcribed in {:.1} s)", started.elapsed().as_secs_f32()));
        Ok(Some(text))
    })();
    match result {
        Ok(Some(text)) => SpeechEvent::Heard { id, text },
        Ok(None) => {
            trace::line(format_args!("no speech detected"));
            SpeechEvent::NoSpeech { id }
        }
        Err(e) => {
            trace::line(format_args!("listen failed: {e:#}"));
            SpeechEvent::Failed { id, error: format!("{e:#}") }
        }
    }
}
