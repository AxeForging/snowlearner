//! Background thread that owns the (blocking) voice and microphone work and
//! reports progress as events, tagged with the job id so stale results can be
//! ignored after a cancel.

use super::endpoint::ListenPlan;
use super::remote::Refusal;
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
    /// Whether the speech server takes the key (`stt_key`), from the panel.
    CheckKey {
        id: u64,
    },
    /// End the current listen now ("I'm done"). Handled out of band.
    StopListening,
    /// Stop reading the current Speak job after the line being said. Out of band.
    StopSpeaking,
    /// Apply new voice/mic settings live (from the panel).
    Configure(Box<VoiceSettings>),
    /// Play back the right answer heard by listen job `id` (`replay_voice`, #116).
    ReplayAnswer {
        id: u64,
    },
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
    /// The speech server turned the request down for its key or its limit.
    Refused {
        id: u64,
        refusal: Refusal,
    },
    /// The speech server takes the key (`Job::CheckKey`).
    KeyWorks {
        id: u64,
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
            | SpeechEvent::Failed { id, .. }
            | SpeechEvent::Refused { id, .. }
            | SpeechEvent::KeyWorks { id } => *id,
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
    /// Where answers are kept for diagnosis (`keep_recordings`); None = not kept.
    pub recordings: Option<PathBuf>,
    /// Keep the last answer in memory to play it back (`replay_voice`).
    pub replay: bool,
    /// The Whisper server answers go to; None = whisper.cpp in the app.
    pub remote: Option<super::remote::Remote>,
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
            recordings: s.keep_recordings.then(|| paths.recordings_dir()),
            replay: s.replay_voice,
            remote: s.remote_stt(),
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
        let can_listen = cfg!(feature = "stt") && (cfg.remote.is_some() || cfg.model.exists());
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
                // The lesson voice's level, kept across voice changes (#162).
                let reference = voice.level.clone();
                #[cfg(feature = "stt")]
                let mut recognizer =
                    Resident::<super::stt::Recognizer>::new(super::resident::SPEECH_MODEL_IDLE, Instant::now());
                // The last answer's spoken part, in memory only, for `replay_voice`.
                #[cfg(feature = "stt")]
                let mut last_answer: Option<(u64, Vec<f32>)> = None;
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
                            voice.level = reference.clone();
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
                            let mut kit = Kit { recognizer: &mut recognizer, kept: &mut last_answer };
                            let ev = listen(id, &lang, expect.as_ref(), plan, &cfg, &mut kit, &stop_worker, &notify);
                            recognizer.touch(Instant::now());
                            notify(ev);
                        }
                        #[cfg(feature = "stt")]
                        Job::MicTest { id, lang } => {
                            let plan = ListenPlan { think: 6.0, expected: 3.0 };
                            let mut kit = Kit { recognizer: &mut recognizer, kept: &mut last_answer };
                            let ev = listen(id, &lang, None, plan, &cfg, &mut kit, &stop_worker, &notify);
                            recognizer.touch(Instant::now());
                            notify(ev);
                        }
                        Job::CheckKey { id } => notify(check_key(id, &cfg)),
                        #[cfg(feature = "stt")]
                        Job::ReplayAnswer { id } => replay(id, last_answer.as_ref(), &cfg, &reference),
                        #[cfg(not(feature = "stt"))]
                        Job::ReplayAnswer { .. } => {}
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

/// What a listen works with: the resident speech model, and where the last
/// answer is kept for `replay_voice` (cleared as each listen starts).
#[cfg(feature = "stt")]
struct Kit<'a> {
    recognizer: &'a mut Resident<super::stt::Recognizer>,
    kept: &'a mut Option<(u64, Vec<f32>)>,
}

/// Plays back the answer heard by listen `id`, when it is the one kept, at the
/// lesson voice's level (#162).
#[cfg(feature = "stt")]
fn replay(id: u64, kept: Option<&(u64, Vec<f32>)>, cfg: &VoiceSettings, reference: &super::loudness::Reference) {
    let Some((_, samples)) = kept.filter(|(heard, s)| *heard == id && !s.is_empty()) else {
        return;
    };
    let rate = super::resample::WHISPER_RATE;
    let target = reference.get();
    let gain = super::loudness::gain_db(samples, rate, target);
    let played = super::loudness::match_level(samples, rate, target);
    let secs = samples.len() as f32 / rate as f32;
    trace::line(format_args!("replaying the answer ({secs:.1} s, {gain:+.1} dB to {target:.1} LUFS)"));
    if let Err(e) = super::audio::play_samples(&played, rate, &cfg.speaker) {
        trace::line(format_args!("replaying the answer failed: {e:#}"));
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
    kit: &mut Kit,
    stop: &AtomicBool,
    notify: &impl Fn(SpeechEvent),
) -> SpeechEvent {
    use super::transcriber::Transcriber;
    stop.store(false, Ordering::SeqCst);
    *kit.kept = None;
    let recognizer = &mut *kit.recognizer;
    trace::line(format_args!("listen [{lang}] think {:.1} s, expected {:.1} s", plan.think, plan.expected));
    let result = (|| -> anyhow::Result<Option<String>> {
        // Open the mic right away and load the model meanwhile: after an idle
        // release the load used to keep the mic closed while the prompt
        // already said "speak now", eating the start of a quick answer.
        // A Whisper server needs no local model.
        let (rec, loaded) = std::thread::scope(|s| {
            let loading = cfg.remote.is_none().then(|| {
                s.spawn(|| {
                    let started = Instant::now();
                    let loaded = recognizer.get_or_load(&cfg.model, started, super::stt::Recognizer::load).map(|_| ());
                    trace::line(format_args!("speech model ready in {:.1} s", started.elapsed().as_secs_f32()));
                    loaded
                })
            });
            let rec = super::mic::record(plan, &cfg.mic, stop, |level, speaking| {
                notify(SpeechEvent::Level { id, level, speaking })
            });
            (rec, loading.map(|l| l.join()))
        });
        let rec = rec?;
        if let Some(loaded) = loaded {
            loaded.map_err(|_| anyhow::anyhow!("loading the speech model crashed"))??;
        }
        if !rec.heard_speech {
            return Ok(None);
        }
        notify(SpeechEvent::Thinking { id });
        if cfg.replay {
            *kit.kept = Some((id, super::replay::spoken_part(&rec.samples, super::resample::WHISPER_RATE).to_vec()));
        }
        if let Some(dir) = &cfg.recordings {
            // A failed copy is logged and never stops the lesson.
            match super::recordings::save(dir, chrono::Local::now(), &rec.samples) {
                Ok(path) => trace::line(format_args!("kept recording {}", path.display())),
                Err(e) => trace::line(format_args!("keeping the recording failed: {e:#}")),
            }
        }
        let started = Instant::now();
        let text = match &cfg.remote {
            Some(server) => {
                trace::line(format_args!("sending the answer to {}", server.endpoint()));
                server.transcribe_expecting(&rec.samples, lang, expect)?
            }
            None => {
                let model = recognizer.get_or_load(&cfg.model, Instant::now(), super::stt::Recognizer::load)?;
                model.transcribe_expecting(&rec.samples, lang, expect)?
            }
        };
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
            failure(id, e)
        }
    }
}

/// `e` as the event the app shows: a refusal by name, anything else as text.
fn failure(id: u64, e: anyhow::Error) -> SpeechEvent {
    match super::remote::refusal(&e) {
        Some(refusal) => SpeechEvent::Refused { id, refusal },
        None => SpeechEvent::Failed { id, error: format!("{e:#}") },
    }
}

/// Asks the speech server whether it takes the key, without recording anything.
fn check_key(id: u64, cfg: &VoiceSettings) -> SpeechEvent {
    let Some(server) = &cfg.remote else {
        return SpeechEvent::Failed { id, error: "recognition is on this computer: no server to check".into() };
    };
    let started = Instant::now();
    let result = server.check_key();
    let took = started.elapsed().as_secs_f32();
    match result {
        Ok(()) => {
            trace::line(format_args!("key {} works at {} ({took:.1} s)", server.key.masked(), server.url));
            SpeechEvent::KeyWorks { id }
        }
        Err(e) => {
            trace::line(format_args!("key {} check failed: {e:#}", server.key.masked()));
            failure(id, e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::paths::Paths;
    use crate::config::settings::Settings;

    #[test]
    fn replaying_your_voice_keeps_it_in_memory_and_writes_no_recordings() {
        let paths = Paths { config_dir: PathBuf::from("home/config"), data_dir: PathBuf::from("home/data") };
        assert!(!VoiceSettings::from(&Settings::default(), &paths).replay);
        let replay = VoiceSettings::from(&Settings { replay_voice: true, ..Default::default() }, &paths);
        assert!(replay.replay);
        assert_eq!(replay.recordings, None, "with keep_recordings off nothing goes to disk");
    }

    #[test]
    fn answers_stay_on_the_machine_unless_a_server_is_chosen() {
        let paths = Paths { config_dir: PathBuf::from("home/config"), data_dir: PathBuf::from("home/data") };
        let default = VoiceSettings::from(&Settings::default(), &paths);
        assert_eq!(default.remote, None, "local whisper.cpp: no server, so no connection is ever made");
        let asr = Settings {
            stt_engine: crate::speech::remote::SttEngine::Asr,
            stt_url: "http://192.168.15.4:9000/".into(),
            ..Default::default()
        };
        let server = VoiceSettings::from(&asr, &paths).remote.expect("a server");
        assert_eq!(server.endpoint(), "http://192.168.15.4:9000/asr");
    }

    #[test]
    fn a_refused_key_or_a_reached_limit_reaches_the_app_by_name() {
        let limited = anyhow::Error::new(Refusal::Limited).context("speech server at x answered HTTP 429");
        assert_eq!(failure(7, limited), SpeechEvent::Refused { id: 7, refusal: Refusal::Limited });
        let down = anyhow::anyhow!("speech server at x did not answer");
        assert_eq!(failure(7, down), SpeechEvent::Failed { id: 7, error: "speech server at x did not answer".into() });
    }

    #[test]
    fn checking_a_key_without_a_server_says_there_is_none_and_sends_nothing() {
        let paths = Paths { config_dir: PathBuf::from("home/config"), data_dir: PathBuf::from("home/data") };
        let local = VoiceSettings::from(&Settings::default(), &paths);
        assert!(matches!(check_key(3, &local), SpeechEvent::Failed { id: 3, .. }));
        let groq = Settings { stt_engine: crate::speech::remote::SttEngine::Groq, ..Default::default() };
        let groq = VoiceSettings::from(&groq, &paths);
        assert_eq!(check_key(3, &groq), SpeechEvent::Refused { id: 3, refusal: Refusal::KeyMissing });
    }

    #[test]
    fn answers_are_kept_next_to_the_log_only_when_the_config_asks() {
        let paths = Paths { config_dir: PathBuf::from("home/config"), data_dir: PathBuf::from("home/data") };
        assert_eq!(VoiceSettings::from(&Settings::default(), &paths).recordings, None);
        let on = Settings { keep_recordings: true, ..Default::default() };
        let dir = VoiceSettings::from(&on, &paths).recordings.unwrap();
        assert_eq!(dir, PathBuf::from("home/data/recordings"));
        assert_eq!(dir.parent(), paths.log_file().parent(), "same folder as snowlearner.log");
    }
}
