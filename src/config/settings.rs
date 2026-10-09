//! User settings (`config.toml`). Every field has a default, unknown keys are
//! rejected so typos surface instead of being silently ignored.

use super::level::Commitment;
use crate::lang::Native;
use crate::learn::deck::{Answer, LEVELS};
use crate::learn::picker::Practice;
use crate::speech::voices::TtsEngine;
use anyhow::{Context, Result, bail};
use chrono::NaiveTime;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum WindowMode {
    /// Overlay where the platform supports it, a normal window otherwise.
    #[default]
    Auto,
    /// Regular window with the full winter scene.
    Window,
    /// Transparent, click-through, always-on-top layer over your desktop.
    Overlay,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Language you are learning (deck name): "en", "es", or a custom deck.
    pub learning: String,
    /// Your own language: every text, cue and meaning, and the voice that
    /// reads them. "pt-BR" or "en"; never the language you are learning.
    pub native: Native,
    pub commitment: Commitment,
    pub mode: WindowMode,
    /// Screen pixels per art pixel.
    pub pixel_scale: u32,
    pub hotkey_challenge: String,
    pub hotkey_summary: String,
    /// Opens the control panel (settings, pause, practice now).
    pub hotkey_menu: String,
    /// Magic hand for 15 s (holding Ctrl+Alt does it too where the OS allows).
    pub hotkey_grab: String,
    /// Opens the panel on PROGRESSO: what you know and what comes next.
    pub hotkey_progress: String,
    /// Where the magic orb sits on screen (overlay mode); negative = top-right corner.
    pub orb_x: i32,
    pub orb_y: i32,
    /// Local time ("HH:MM") for the automatic end-of-day recap.
    pub summary_time: String,
    /// 0..1 similarity needed for a phrase to count.
    pub match_threshold: f32,
    pub ipc_port: u16,
    /// Whisper model size: tiny | base | small.
    pub model: String,
    /// Seconds you get to start answering (recall mode gets +4).
    pub listen_seconds: f32,
    /// Optional TTS voice names; empty = pick by language.
    pub voice_native: String,
    pub voice_learning: String,
    /// system (OS voices) | http (OpenAI-compatible server, e.g. Kokoro) | command.
    pub tts_engine: TtsEngine,
    /// Base URL of the HTTP speech server (Kokoro-FastAPI default shown).
    pub tts_url: String,
    pub tts_model: String,
    /// Command template for `command`: {text} {lang} {voice} {out}.
    pub tts_command: String,
    /// Microphone name ("" = system default). `snowlearner audio mics` lists them.
    pub mic: String,
    /// Speaker for audio the app plays itself ("" = default).
    pub speaker: String,
    /// auto (repeat new phrases, recall known ones) | repeat | recall.
    pub practice: Practice,
    /// all (short, complete and polished shown) | short | complete | polished.
    pub answer: Answer,
    /// Only practice these topics; empty = all topics. An old config's
    /// single `topic = "x"` still loads as `["x"]`.
    #[serde(alias = "topic", deserialize_with = "one_or_many")]
    pub topics: Vec<String>,
    /// Highest CEFR level to practice: A1 A2 B1 B2 C1 C2.
    pub max_level: String,
    /// Phrases per day you aim for.
    pub daily_goal: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            learning: "en".into(),
            native: Native::PtBr,
            commitment: Commitment::Steady,
            mode: WindowMode::Auto,
            pixel_scale: 4,
            hotkey_challenge: "Ctrl+Alt+M".into(),
            hotkey_summary: "Ctrl+Alt+J".into(),
            hotkey_menu: "Ctrl+Alt+K".into(),
            hotkey_grab: "Ctrl+Alt+G".into(),
            hotkey_progress: "Ctrl+Alt+P".into(),
            orb_x: -1,
            orb_y: -1,
            summary_time: "21:00".into(),
            match_threshold: 0.72,
            ipc_port: 47821,
            model: "base".into(),
            listen_seconds: 6.0,
            voice_native: String::new(),
            voice_learning: String::new(),
            tts_engine: TtsEngine::System,
            tts_url: "http://localhost:8880/v1".into(),
            tts_model: "kokoro".into(),
            tts_command: String::new(),
            mic: String::new(),
            speaker: String::new(),
            practice: Practice::Auto,
            answer: Answer::All,
            topics: Vec::new(),
            max_level: "B2".into(),
            daily_goal: 10,
        }
    }
}

/// `topics = ["a", "b"]`, or the old single `topic = "a"` ("" = all).
fn one_or_many<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(t) => vec![t],
        OneOrMany::Many(ts) => ts,
    })
}

/// Lowercase, trimmed, no blanks and no repeats, in the order given.
pub fn normalize_topics(topics: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in topics.iter().map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()) {
        if !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

pub const MODELS: &[&str] = &["tiny", "base", "small"];

const HEADER: &str = "# snowlearner config (also editable live: `snowlearner menu`)\n\
    # learning: deck to practice (\"en\", \"es\", \"pt-BR\" or a custom decks/<name>.toml)\n\
    # native: your own language, \"pt-BR\" or \"en\" (never the one you learn)\n\
    # commitment: chill | steady | committed | relentless\n\
    # mode: auto | window | overlay    practice: auto | repeat | recall\n\
    # answer: all | short | complete | polished\n\
    # topics: [] for all, or e.g. [\"trabalho\", \"viagem\"]    max_level: PRE-A1, A1..C2\n\n";

impl Settings {
    /// Missing file → defaults. Invalid file → error naming the problem.
    pub fn load(path: &Path) -> Result<Settings> {
        if !path.exists() {
            return Ok(Settings::default());
        }
        let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut s: Settings = toml::from_str(&src).with_context(|| format!("invalid config {}", path.display()))?;
        s.max_level = s.max_level.trim().to_uppercase(); // "pre-a1", "b1" …
        s.topics = normalize_topics(&s.topics);
        s.validate().with_context(|| format!("invalid config {}", path.display()))?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<()> {
        if self.learning.trim().is_empty() {
            bail!("`learning` must name a deck, e.g. \"en\" or \"es\"");
        }
        if self.native.is(&self.learning) {
            bail!("`learning` {:?} is your own language (`native`): pick another one to learn", self.learning);
        }
        if !(self.match_threshold > 0.0 && self.match_threshold <= 1.0) {
            bail!("`match_threshold` must be in (0, 1], got {}", self.match_threshold);
        }
        if !(1..=12).contains(&self.pixel_scale) {
            bail!("`pixel_scale` must be 1..=12, got {}", self.pixel_scale);
        }
        if !(2.0..=30.0).contains(&self.listen_seconds) {
            bail!("`listen_seconds` must be 2..=30, got {}", self.listen_seconds);
        }
        if !MODELS.contains(&self.model.as_str()) {
            bail!("`model` must be one of {MODELS:?}, got {:?}", self.model);
        }
        if !LEVELS.contains(&self.max_level.as_str()) {
            bail!("`max_level` must be one of {LEVELS:?}, got {:?}", self.max_level);
        }
        if self.tts_engine == TtsEngine::Http && !self.tts_url.starts_with("http") {
            bail!("`tts_url` must be an http(s) URL, got {:?}", self.tts_url);
        }
        if self.tts_engine == TtsEngine::Command && self.tts_command.trim().is_empty() {
            bail!("`tts_command` is required when `tts_engine = \"command\"`");
        }
        if !(1..=200).contains(&self.daily_goal) {
            bail!("`daily_goal` must be 1..=200, got {}", self.daily_goal);
        }
        self.summary_at()?;
        Ok(())
    }

    /// The voice engine these settings describe.
    pub fn voice(&self) -> crate::speech::voices::Voice {
        crate::speech::voices::Voice::new(
            self.tts_engine,
            &self.tts_url,
            &self.tts_model,
            &self.tts_command,
            &self.speaker,
        )
    }

    /// Persists the current settings (used by the in-app menu).
    pub fn save(&self, path: &Path) -> Result<()> {
        self.validate()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(path, format!("{HEADER}{body}"))?;
        Ok(())
    }

    pub fn summary_at(&self) -> Result<NaiveTime> {
        NaiveTime::parse_from_str(&self.summary_time, "%H:%M")
            .with_context(|| format!("`summary_time` must be HH:MM, got {:?}", self.summary_time))
    }

    /// Writes a commented default config. Refuses to clobber unless `force`.
    pub fn write_default(path: &Path, force: bool) -> Result<()> {
        if path.exists() && !force {
            bail!("{} already exists (use --force to overwrite)", path.display());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = toml::to_string_pretty(&Settings::default())?;
        std::fs::write(path, format!("{HEADER}{body}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Settings::load(&dir.path().join("nope.toml")).unwrap(), Settings::default());
    }

    #[test]
    fn partial_file_overrides_only_given_keys() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "learning = 'es'\ncommitment = 'relentless'\n").unwrap();
        let s = Settings::load(&p).unwrap();
        assert_eq!(s.learning, "es");
        assert_eq!(s.commitment, Commitment::Relentless);
        assert_eq!(s.pixel_scale, Settings::default().pixel_scale);
    }

    #[test]
    fn invalid_values_are_rejected_with_the_key_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        for (src, key) in [
            ("match_threshold = 1.5", "match_threshold"),
            ("match_threshold = 0.0", "match_threshold"),
            ("pixel_scale = 0", "pixel_scale"),
            ("summary_time = '25:99'", "summary_time"),
            ("model = 'huge'", "model"),
            ("listen_seconds = 1.0", "listen_seconds"),
            ("learning = ''", "learning"),
            ("max_level = 'Z1'", "max_level"),
            ("max_level = 'C3'", "max_level"),
            ("daily_goal = 0", "daily_goal"),
            ("tts_engine = 'http'\ntts_url = 'localhost'", "tts_url"),
            ("tts_engine = 'command'", "tts_command"),
            ("answer = 'longest'", "answer"),
            ("native = 'fr'", "native"),
        ] {
            std::fs::write(&p, src).unwrap();
            let err = format!("{:#}", Settings::load(&p).unwrap_err());
            assert!(err.contains(key), "{src} → {err}");
        }
    }

    #[test]
    fn save_round_trips_menu_changes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        let mut s = Settings {
            learning: "es".into(),
            commitment: Commitment::Committed,
            topics: vec!["trabalho".into(), "viagem".into()],
            practice: Practice::Recall,
            answer: Answer::Polished,
            ..Default::default()
        };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), s);
        assert!(std::fs::read_to_string(&p).unwrap().contains("topics = ["), "saved under the new key");
        s.max_level = "nope".into();
        assert!(s.save(&p).is_err(), "never persist an invalid config");
    }

    #[test]
    fn unknown_keys_are_errors_not_silently_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        std::fs::write(&p, "lerning = 'es'").unwrap();
        assert!(Settings::load(&p).is_err());
    }

    #[test]
    fn written_default_loads_back_identically_and_is_not_clobbered() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("sub/config.toml");
        Settings::write_default(&p, false).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), Settings::default());
        assert!(Settings::write_default(&p, false).is_err());
        Settings::write_default(&p, true).unwrap();
    }

    #[test]
    fn the_answer_tier_defaults_to_all_and_reads_portuguese_names_too() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        assert_eq!(Settings::default().answer, Answer::All);
        for (src, want) in [("answer = 'short'", Answer::Short), ("answer = 'polida'", Answer::Polished)] {
            std::fs::write(&p, src).unwrap();
            assert_eq!(Settings::load(&p).unwrap().answer, want, "{src}");
        }
    }

    #[test]
    fn your_own_language_round_trips_and_defaults_to_portuguese() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        assert_eq!(Settings::default().native, Native::PtBr);
        let s = Settings { native: Native::En, learning: "pt-BR".into(), ..Default::default() };
        s.save(&p).unwrap();
        assert!(std::fs::read_to_string(&p).unwrap().contains("native = \"en\""));
        assert_eq!(Settings::load(&p).unwrap(), s);
        std::fs::write(&p, "native = 'en'\nlearning = 'es'\n").unwrap();
        assert_eq!(Settings::load(&p).unwrap().native, Native::En);
        std::fs::write(&p, "native = 'pt'\n").unwrap();
        assert_eq!(Settings::load(&p).unwrap().native, Native::PtBr, "pt is read as pt-BR");
    }

    #[test]
    fn an_unknown_native_language_is_rejected_with_the_key_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        for bad in ["native = 'fr'", "native = ''", "native = 'english'"] {
            std::fs::write(&p, bad).unwrap();
            let err = format!("{:#}", Settings::load(&p).unwrap_err());
            assert!(err.contains("native"), "{bad} -> {err}");
        }
    }

    #[test]
    fn you_never_learn_your_own_language() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        for src in ["native = 'en'", "native = 'en'\nlearning = 'EN'", "learning = 'pt-BR'", "learning = 'pt'"] {
            std::fs::write(&p, src).unwrap();
            let err = format!("{:#}", Settings::load(&p).unwrap_err());
            assert!(err.contains("own language"), "{src} -> {err}");
        }
        let s = Settings { native: Native::En, learning: "en".into(), ..Default::default() };
        assert!(s.save(&dir.path().join("x.toml")).is_err(), "never persisted either");
    }

    #[test]
    fn pre_a1_is_a_level_whatever_the_case_it_is_written_in() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "max_level = \"pre-a1\"\n").unwrap();
        assert_eq!(Settings::load(&path).unwrap().max_level, "PRE-A1");
    }

    #[test]
    fn an_old_single_topic_config_loads_as_a_one_topic_list() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("config.toml");
        for (src, want) in [
            ("topic = 'trabalho'", vec!["trabalho"]),
            ("topic = ' Viagem '", vec!["viagem"]),
            ("topic = ''", vec![]),
            ("topics = ['Trabalho', 'viagem', 'trabalho', ' ']", vec!["trabalho", "viagem"]),
            ("topics = []", vec![]),
            ("", vec![]),
        ] {
            std::fs::write(&p, src).unwrap();
            let s = Settings::load(&p).unwrap();
            assert_eq!(s.topics, want, "{src}");
        }
        std::fs::write(&p, "topic = 'trabalho'").unwrap();
        let mut s = Settings::load(&p).unwrap();
        s.save(&p).unwrap();
        let saved = std::fs::read_to_string(&p).unwrap();
        assert!(saved.contains("topics = [\"trabalho\"]") && !saved.contains("topic ="), "{saved}");
        assert_eq!(Settings::load(&p).unwrap(), s, "migrated once, round-trips after");
        s.topics.push("viagem".into());
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap().topics, ["trabalho", "viagem"]);
    }

    #[test]
    fn a_topic_that_is_neither_text_nor_a_list_is_rejected_with_the_key_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("c.toml");
        for bad in ["topics = 3", "topic = true", "topics = [1, 2]"] {
            std::fs::write(&p, bad).unwrap();
            assert!(Settings::load(&p).is_err(), "{bad}");
        }
    }
}
