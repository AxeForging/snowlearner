//! User settings (`config.toml`). Every field has a default, unknown keys are
//! rejected so typos surface instead of being silently ignored.

use super::level::Commitment;
use crate::learn::deck::LEVELS;
use crate::learn::picker::Practice;
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
    /// Your language, used for cues and the voice that reads them.
    pub native: String,
    pub commitment: Commitment,
    pub mode: WindowMode,
    /// Screen pixels per art pixel.
    pub pixel_scale: u32,
    pub hotkey_challenge: String,
    pub hotkey_summary: String,
    /// Opens the control panel (settings, pause, practice now).
    pub hotkey_menu: String,
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
    pub listen_seconds: f32,
    /// Optional TTS voice names; empty = pick by language.
    pub voice_native: String,
    pub voice_learning: String,
    /// auto (repeat new phrases, recall known ones) | repeat | recall.
    pub practice: Practice,
    /// Only practice this topic; empty = all topics.
    pub topic: String,
    /// Highest CEFR level to practice: A1 A2 B1 B2 C1 C2.
    pub max_level: String,
    /// Phrases per day you aim for.
    pub daily_goal: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            learning: "en".into(),
            native: "pt-BR".into(),
            commitment: Commitment::Steady,
            mode: WindowMode::Auto,
            pixel_scale: 4,
            hotkey_challenge: "Ctrl+Alt+M".into(),
            hotkey_summary: "Ctrl+Alt+J".into(),
            hotkey_menu: "Ctrl+Alt+K".into(),
            orb_x: -1,
            orb_y: -1,
            summary_time: "21:00".into(),
            match_threshold: 0.72,
            ipc_port: 47821,
            model: "base".into(),
            listen_seconds: 7.0,
            voice_native: String::new(),
            voice_learning: String::new(),
            practice: Practice::Auto,
            topic: String::new(),
            max_level: "B2".into(),
            daily_goal: 10,
        }
    }
}

pub const MODELS: &[&str] = &["tiny", "base", "small"];

const HEADER: &str = "# snowlearner config (also editable live: `snowlearner menu`)\n\
    # learning: deck to practice (\"en\", \"es\" or a custom decks/<name>.toml)\n\
    # commitment: chill | steady | committed | relentless\n\
    # mode: auto | window | overlay    practice: auto | repeat | recall\n\
    # topic: \"\" for all, or e.g. \"trabalho\"    max_level: A1..C2\n\n";

impl Settings {
    /// Missing file → defaults. Invalid file → error naming the problem.
    pub fn load(path: &Path) -> Result<Settings> {
        if !path.exists() {
            return Ok(Settings::default());
        }
        let src = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let s: Settings = toml::from_str(&src).with_context(|| format!("invalid config {}", path.display()))?;
        s.validate().with_context(|| format!("invalid config {}", path.display()))?;
        Ok(s)
    }

    pub fn validate(&self) -> Result<()> {
        if self.learning.trim().is_empty() {
            bail!("`learning` must name a deck, e.g. \"en\" or \"es\"");
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
        if !(1..=200).contains(&self.daily_goal) {
            bail!("`daily_goal` must be 1..=200, got {}", self.daily_goal);
        }
        self.summary_at()?;
        Ok(())
    }

    /// Topic filter as an option (empty string = every topic).
    pub fn topic_filter(&self) -> Option<String> {
        let t = self.topic.trim().to_lowercase();
        (!t.is_empty()).then_some(t)
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
            ("daily_goal = 0", "daily_goal"),
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
            topic: "trabalho".into(),
            practice: Practice::Recall,
            ..Default::default()
        };
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p).unwrap(), s);
        assert_eq!(s.topic_filter().as_deref(), Some("trabalho"));
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
}
