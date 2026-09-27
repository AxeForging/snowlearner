//! Real speech recognition, end to end: a synthetic voice (espeak-ng) says the
//! phrase, whisper transcribes it, the matcher must accept it — and reject a
//! different phrase. Needs a model and espeak-ng, so it is opt-in:
//!   SNOWLEARNER_TEST_MODEL=~/.local/share/snowlearner/models/ggml-base.bin \
//!     cargo test --test speech_e2e -- --ignored
#![cfg(feature = "stt")]

use snowlearner::speech::matcher;
use snowlearner::speech::resample::{WHISPER_RATE, resample, to_mono};
use snowlearner::speech::stt::Recognizer;
use std::path::Path;
use std::process::Command;

fn synth(text: &str, voice: &str, dir: &Path) -> Vec<f32> {
    let wav = dir.join("say.wav");
    let ok = Command::new("espeak-ng").args(["-v", voice, "-s", "140", "-w"]).arg(&wav).arg(text).status().unwrap();
    assert!(ok.success());
    let mut r = hound::WavReader::open(&wav).unwrap();
    let spec = r.spec();
    let raw: Vec<f32> = r.samples::<i16>().map(|s| s.unwrap() as f32 / i16::MAX as f32).collect();
    resample(&to_mono(&raw, spec.channels), spec.sample_rate, WHISPER_RATE)
}

fn recognizer() -> Recognizer {
    let model = std::env::var("SNOWLEARNER_TEST_MODEL").expect("set SNOWLEARNER_TEST_MODEL");
    Recognizer::load(Path::new(&model)).unwrap()
}

#[test]
#[ignore = "needs SNOWLEARNER_TEST_MODEL and espeak-ng"]
fn spoken_english_phrase_is_recognized_and_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let heard = recognizer().transcribe(&synth("I'm hungry", "en-us", dir.path()), "en").unwrap();
    let m = matcher::score("I'm hungry", &heard);
    assert!(m.passed(0.72), "heard {heard:?} scored {}", m.score);
}

#[test]
#[ignore = "needs SNOWLEARNER_TEST_MODEL and espeak-ng"]
fn spoken_spanish_phrase_is_recognized_and_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let heard = recognizer().transcribe(&synth("Muchas gracias", "es", dir.path()), "es").unwrap();
    let m = matcher::score("Muchas gracias", &heard);
    assert!(m.passed(0.72), "heard {heard:?} scored {}", m.score);
}

#[test]
#[ignore = "needs SNOWLEARNER_TEST_MODEL and espeak-ng"]
fn saying_a_different_phrase_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let heard = recognizer().transcribe(&synth("Good night everyone", "en-us", dir.path()), "en").unwrap();
    assert!(!matcher::score("I'm hungry", &heard).passed(0.72), "heard {heard:?}");
}
