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

/// The recording as the mic hands it over when the learner answers the instant
/// listening starts: the word right at sample 0, then the room until the pause
/// ends the turn.
fn answered_instantly(text: &str, dir: &Path) -> Vec<f32> {
    let voice = synth(text, "en-us", dir);
    let start = voice.iter().position(|s| s.abs() > 0.01).unwrap_or(0);
    let end = voice.iter().rposition(|s| s.abs() > 0.01).unwrap_or(voice.len() - 1);
    let mut x: u32 = 0x2545_f491;
    let mut room = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.005
    };
    let mut rec: Vec<f32> = voice[start..=end].iter().map(|s| s * 0.3 + room()).collect();
    rec.extend((0..WHISPER_RATE as usize * 18 / 10).map(|_| room()));
    rec
}

#[test]
#[ignore = "needs SNOWLEARNER_TEST_MODEL and espeak-ng"]
fn a_single_word_said_the_instant_listening_starts_is_recognized() {
    // Without a lead-in whisper heard "Sorry." as "I'll read." and
    // "Good morning." as "go tomorrow again."
    let dir = tempfile::tempdir().unwrap();
    let r = recognizer();
    for phrase in ["Sorry.", "Good morning."] {
        let heard = r.transcribe(&answered_instantly(phrase, dir.path()), "en").unwrap();
        assert!(matcher::score(phrase, &heard).passed(0.72), "{phrase:?} was heard as {heard:?}");
    }
}

#[test]
#[ignore = "needs SNOWLEARNER_TEST_MODEL and espeak-ng"]
fn a_word_through_a_hissy_48khz_mic_is_recognized() {
    // Mic hiss above 8 kHz folded into the speech band by a plain linear
    // resampler turned "Please." into "Clean.".
    let dir = tempfile::tempdir().unwrap();
    let voice = synth("Please.", "en-us", dir.path());
    let level = (voice.iter().map(|v| v * v).sum::<f32>() / voice.len() as f32).sqrt();
    let at_48k = resample(&voice, WHISPER_RATE, 48_000);
    let mut x: u32 = 0x1234_5679;
    let mut hiss = move || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        (x as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.0173 // ≈ 0.01 RMS
    };
    let mut mic: Vec<f32> = (0..24_000).map(|_| hiss()).collect();
    mic.extend(at_48k.iter().map(|v| v * 0.05 / level + hiss()));
    mic.extend((0..48_000).map(|_| hiss()));
    let heard = recognizer().transcribe(&resample(&mic, 48_000, WHISPER_RATE), "en").unwrap();
    assert!(matcher::score("Please.", &heard).passed(0.72), "heard {heard:?}");
}
