//! Local speech recognition with whisper.cpp. Runs fully offline on every OS;
//! the language is pinned to the one being practiced.

use super::bench::Pass;
use super::matcher::Expect;
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::sync::OnceLock;
use std::time::Instant;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Threads lessons give whisper: every core, up to six.
pub fn lesson_threads() -> i32 {
    std::thread::available_parallelism().map(|n| n.get().min(6) as i32).unwrap_or(4)
}

/// What whisper.cpp was built with and found on this CPU (AVX2, FMA…), in its own words.
///
/// Asked once per process and kept: whisper.cpp writes the text into one static
/// buffer, so two threads asking at the same moment read it half written (it
/// came back as invalid UTF-8 when the speech tests loaded models in parallel).
pub fn system_info() -> &'static str {
    static INFO: OnceLock<String> = OnceLock::new();
    INFO.get_or_init(|| whisper_rs::print_system_info().trim().to_string())
}

/// The whisper.cpp version this build carries.
pub fn engine_version() -> &'static str {
    whisper_rs::get_whisper_version()
}

pub struct Recognizer {
    ctx: WhisperContext,
}

impl Recognizer {
    pub fn load(model: &Path) -> Result<Recognizer> {
        if !model.exists() {
            bail!("speech model not found at {} — run `snowlearner model download`", model.display());
        }
        whisper_rs::install_logging_hooks();
        let path = model.to_str().context("model path is not valid UTF-8")?;
        let ctx = WhisperContext::new_with_params(path, WhisperContextParameters::default())
            .with_context(|| format!("loading speech model {}", model.display()))?;
        super::trace::line(format_args!("speech engine: {} threads, {}", lesson_threads(), system_info()));
        Ok(Recognizer { ctx })
    }

    /// Transcribes, and when `expect` says the answer was missed, tries once
    /// more with its vocabulary hint (see `matcher::Expect`).
    /// Each pass goes to the log with what it heard, its time and the verdict.
    pub fn transcribe_expecting(&self, samples: &[f32], lang: &str, expect: Option<&Expect>) -> Result<String> {
        let verdict = |text: &str| {
            expect.map(|e| {
                let m = e.judge(text);
                (m.passed(e.threshold), m.score)
            })
        };
        let started = Instant::now();
        let first = self.transcribe(samples, lang, None)?;
        let line = pass_line("first", &first, started.elapsed().as_secs_f32(), verdict(&first));
        super::trace::line(format_args!("{line}"));
        let Some(e) = expect else { return Ok(first) };
        Ok(e.pick(first, |hint| {
            let started = Instant::now();
            match self.transcribe(samples, lang, Some(hint)) {
                Ok(second) => {
                    let line = pass_line("hinted", &second, started.elapsed().as_secs_f32(), verdict(&second));
                    super::trace::line(format_args!("{line}"));
                    Some(second)
                }
                Err(err) => {
                    super::trace::line(format_args!("hinted pass failed: {err:#}"));
                    None
                }
            }
        }))
    }

    /// `samples`: 16 kHz mono. `lang`: ISO code of the language being practiced.
    /// `prompt`: words whisper should expect (an initial prompt), if any.
    pub fn transcribe(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String> {
        self.run(samples, lang, prompt, Pass { threads: lesson_threads(), audio_ctx: 0 })
    }

    /// One pass run the way `pass` says, for `snowlearner model bench`.
    pub fn transcribe_as(&self, samples: &[f32], lang: &str, pass: Pass) -> Result<String> {
        self.run(samples, lang, None, pass)
    }

    fn run(&self, samples: &[f32], lang: &str, prompt: Option<&str>, pass: Pass) -> Result<String> {
        let mut state = self.ctx.create_state().context("creating recognizer state")?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let code = lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
        params.set_language(Some(&code));
        if let Some(p) = prompt {
            params.set_initial_prompt(p);
        }
        params.set_n_threads(pass.threads);
        if pass.audio_ctx > 0 {
            params.set_audio_ctx(pass.audio_ctx);
        }
        params.set_no_context(true);
        params.set_single_segment(true);
        params.set_suppress_blank(true);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        params.set_print_timestamps(false);
        state.full(params, &prepare(samples)).context("speech recognition failed")?;
        let text: String = state
            .as_iter()
            .filter_map(|seg| seg.to_str_lossy().ok().map(|s| s.into_owned()))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(clean(&text))
    }
}

/// Silence put before the answer. A word that starts right at the first
/// sample (the learner answered instantly) is misheard without it: "Sorry."
/// came back as "I'll read.", "Good morning." as "go tomorrow again."
pub const LEAD_IN: usize = 16_000 * 3 / 10;
/// Whisper needs at least ~1 s of audio; short answers are padded with silence.
pub const MIN_SAMPLES: usize = 16_000 + 1_600;

/// The audio whisper gets: a silent lead-in, the answer, silence up to the minimum.
pub fn prepare(samples: &[f32]) -> Vec<f32> {
    let mut audio = vec![0.0; LEAD_IN];
    audio.extend_from_slice(samples);
    if audio.len() < MIN_SAMPLES {
        audio.resize(MIN_SAMPLES, 0.0);
    }
    audio
}

/// The log line for one pass: what it heard, how long it took and, in a
/// lesson, how it was judged (`(accepted, score)`).
fn pass_line(pass: &str, text: &str, seconds: f32, verdict: Option<(bool, f32)>) -> String {
    let line = format!("{pass} pass {text:?} in {seconds:.1} s");
    match verdict {
        Some((true, score)) => format!("{line}: accepted (score {score:.2})"),
        Some((false, score)) => format!("{line}: not accepted (score {score:.2})"),
        None => line,
    }
}

/// Drops whisper's non-speech annotations like "[BLANK_AUDIO]" or "(music)".
pub fn clean(text: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_are_removed_and_whitespace_collapsed() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean(" I'm hungry. (laughs)  "), "I'm hungry.");
        assert_eq!(clean("Tengo  frío"), "Tengo frío");
    }

    #[test]
    fn the_answer_gets_a_silent_lead_in_and_a_minimum_length() {
        let word = vec![0.5; 8_000];
        let audio = prepare(&word);
        assert!(audio[..LEAD_IN].iter().all(|&s| s == 0.0), "silence first");
        assert_eq!(&audio[LEAD_IN..LEAD_IN + word.len()], word.as_slice(), "then the answer, untouched");
        assert_eq!(audio.len(), MIN_SAMPLES);
        let long = vec![0.1; 48_000];
        assert_eq!(prepare(&long).len(), LEAD_IN + long.len(), "long answers are not cut");
        assert_eq!(prepare(&[]).len(), MIN_SAMPLES);
    }

    #[test]
    fn the_engine_describes_itself_the_same_way_from_every_thread() {
        let ask = || {
            let mut info = system_info();
            for _ in 0..200 {
                info = system_info();
            }
            info
        };
        let threads: Vec<_> = (0..8).map(|_| std::thread::spawn(ask)).collect();
        let here = system_info();
        assert!(!here.is_empty());
        for t in threads {
            assert_eq!(t.join().expect("asking from a thread must not panic"), here);
        }
    }

    #[test]
    fn each_pass_is_logged_with_what_it_heard_its_time_and_the_verdict() {
        let first = pass_line("first", "Oh, please.", 3.42, Some((false, 0.734)));
        assert_eq!(first, r#"first pass "Oh, please." in 3.4 s: not accepted (score 0.73)"#);
        let hinted = pass_line("hinted", "Cold.", 3.46, Some((true, 1.0)));
        assert_eq!(hinted, r#"hinted pass "Cold." in 3.5 s: accepted (score 1.00)"#);
        assert_eq!(pass_line("first", "Hi", 3.3, None), r#"first pass "Hi" in 3.3 s"#, "no lesson, no verdict");
    }

    #[test]
    fn missing_model_explains_how_to_get_one() {
        let err = format!("{:#}", Recognizer::load(Path::new("/nope/ggml-base.bin")).err().unwrap());
        assert!(err.contains("snowlearner model download"), "{err}");
    }

    /// Real end-to-end check; needs `SNOWLEARNER_TEST_MODEL=/path/ggml-*.bin`.
    #[test]
    #[ignore = "needs a whisper model; set SNOWLEARNER_TEST_MODEL and run with --ignored"]
    fn silence_transcribes_to_nothing_meaningful() {
        let model = std::env::var("SNOWLEARNER_TEST_MODEL").expect("SNOWLEARNER_TEST_MODEL");
        let r = Recognizer::load(Path::new(&model)).unwrap();
        let text = r.transcribe(&vec![0.0; 16_000 * 2], "en", None).unwrap();
        assert!(crate::speech::matcher::score("I'm hungry", &text).score < 0.5, "{text:?}");
    }
}
