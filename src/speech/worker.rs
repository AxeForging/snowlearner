//! Background thread that owns the (blocking) voice and microphone work and
//! reports progress as events, tagged with the job id so stale results can be
//! ignored after a cancel.

use super::tts::{Engine, Tts};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};

#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub text: String,
    pub lang: String,
    /// Target-language parts are read a little slower.
    pub slow: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Speak { id: u64, parts: Vec<Utterance> },
    Listen { id: u64, lang: String, max_seconds: f32 },
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

pub struct VoiceSettings {
    pub native_voice: String,
    pub learning_voice: String,
    pub native_lang: String,
    pub model: PathBuf,
}

pub struct Speech {
    tx: Sender<Job>,
    pub tts: Option<Engine>,
    pub can_listen: bool,
}

impl Speech {
    /// `can_listen` is true when this build has the recognizer and a model on disk.
    pub fn start(voices: VoiceSettings, notify: impl Fn(SpeechEvent) + Send + 'static) -> Speech {
        let tts = Tts::detect();
        let engine = tts.engine();
        let can_listen = cfg!(feature = "stt") && voices.model.exists();
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("speech".into())
            .spawn(move || {
                #[cfg(feature = "stt")]
                let mut recognizer: Option<super::stt::Recognizer> = None;
                for job in rx {
                    match job {
                        Job::Speak { id, parts } => {
                            for (index, p) in parts.iter().enumerate() {
                                notify(SpeechEvent::Part { id, index });
                                let voice = if p.lang == voices.native_lang {
                                    &voices.native_voice
                                } else {
                                    &voices.learning_voice
                                };
                                if let Err(e) = tts.speak(&p.text, &p.lang, voice, p.slow) {
                                    notify(SpeechEvent::Failed { id, error: format!("{e:#}") });
                                    break;
                                }
                            }
                            notify(SpeechEvent::Spoken { id });
                        }
                        #[cfg(feature = "stt")]
                        Job::Listen { id, lang, max_seconds } => {
                            let result = (|| -> anyhow::Result<Option<String>> {
                                if recognizer.is_none() {
                                    recognizer = Some(super::stt::Recognizer::load(&voices.model)?);
                                }
                                let rec = super::mic::record(max_seconds)?;
                                if !rec.heard_speech {
                                    return Ok(None);
                                }
                                notify(SpeechEvent::Thinking { id });
                                Ok(Some(recognizer.as_ref().unwrap().transcribe(&rec.samples, &lang)?))
                            })();
                            notify(match result {
                                Ok(Some(text)) => SpeechEvent::Heard { id, text },
                                Ok(None) => SpeechEvent::NoSpeech { id },
                                Err(e) => SpeechEvent::Failed { id, error: format!("{e:#}") },
                            });
                        }
                        #[cfg(not(feature = "stt"))]
                        Job::Listen { id, .. } => {
                            notify(SpeechEvent::Failed { id, error: "built without speech recognition".into() });
                        }
                    }
                }
            })
            .expect("spawning speech thread");
        Speech { tx, tts: engine, can_listen }
    }

    pub fn send(&self, job: Job) {
        let _ = self.tx.send(job);
    }
}
