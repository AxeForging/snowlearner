//! Phrase decks: TOML data, built-in for `en` and `es` (pt-BR cues), and
//! overridable by dropping `<lang>.toml` into the config `decks/` folder.

use super::cue::{self, Segment};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

const BUILTIN_EN: &str = include_str!("../../decks/en.toml");
const BUILTIN_ES: &str = include_str!("../../decks/es.toml");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    language: String,
    native: String,
    title: String,
    #[serde(default = "default_cue")]
    default_cue: String,
    #[serde(rename = "phrase", default)]
    phrases: Vec<PhraseFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhraseFile {
    say: String,
    #[serde(default)]
    cue: Option<String>,
    #[serde(default)]
    meaning: Option<String>,
    #[serde(default)]
    tip: Option<String>,
}

fn default_cue() -> String {
    "Repita: {}".to_string()
}

#[derive(Debug, Clone)]
pub struct Phrase {
    /// What the learner must say, in the target language.
    pub say: String,
    /// Native-language meaning (shown in captions and the daily summary).
    pub meaning: String,
    /// Parsed cue: native + target segments, read aloud with matching voices.
    pub cue: Vec<Segment>,
    /// Optional pronunciation / usage tip the warrior can give.
    pub tip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Deck {
    pub language: String,
    pub native: String,
    pub title: String,
    pub phrases: Vec<Phrase>,
}

impl Deck {
    pub fn parse(src: &str) -> Result<Deck> {
        let file: DeckFile = toml::from_str(src).context("invalid deck TOML")?;
        if file.language.trim().is_empty() {
            bail!("deck `language` must not be empty");
        }
        if file.phrases.is_empty() {
            bail!("deck {:?} has no [[phrase]] entries", file.title);
        }
        let mut phrases = Vec::with_capacity(file.phrases.len());
        for (i, p) in file.phrases.into_iter().enumerate() {
            let say = p.say.trim().to_string();
            if say.is_empty() {
                bail!("phrase #{} has an empty `say`", i + 1);
            }
            let cue_src = p.cue.unwrap_or_else(|| file.default_cue.clone());
            let cue = cue::parse(&cue_src, &say).with_context(|| format!("phrase #{} ({say:?})", i + 1))?;
            phrases.push(Phrase { meaning: p.meaning.unwrap_or_default(), say, cue, tip: p.tip });
        }
        Ok(Deck { language: file.language, native: file.native, title: file.title, phrases })
    }

    pub fn builtin(language: &str) -> Option<Deck> {
        let src = match language {
            "en" => BUILTIN_EN,
            "es" => BUILTIN_ES,
            _ => return None,
        };
        Some(Deck::parse(src).expect("built-in deck must be valid"))
    }

    pub fn builtin_languages() -> &'static [&'static str] {
        &["en", "es"]
    }

    /// A custom `<decks_dir>/<language>.toml` wins over the built-in deck.
    pub fn load(language: &str, decks_dir: &Path) -> Result<Deck> {
        let custom = decks_dir.join(format!("{language}.toml"));
        if custom.exists() {
            let src = std::fs::read_to_string(&custom).with_context(|| format!("reading {}", custom.display()))?;
            return Deck::parse(&src).with_context(|| format!("in {}", custom.display()));
        }
        Deck::builtin(language).with_context(|| {
            format!(
                "no deck for language {language:?}: built-in decks are {:?}, or create {}",
                Deck::builtin_languages(),
                custom.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_decks_parse_and_every_phrase_has_a_target_segment() {
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            assert_eq!(&deck.language, lang);
            assert_eq!(deck.native, "pt-BR");
            assert!(deck.phrases.len() >= 20, "{lang} deck too small");
            for p in &deck.phrases {
                assert!(p.cue.iter().any(Segment::is_target), "{:?} has no target", p.say);
                assert!(!p.meaning.is_empty(), "{:?} has no meaning", p.say);
            }
        }
    }

    #[test]
    fn builtin_deck_text_renders_with_the_pixel_font() {
        for lang in Deck::builtin_languages() {
            for p in Deck::builtin(lang).unwrap().phrases {
                let all =
                    format!("{} {} {}", p.say, p.meaning, p.cue.iter().map(|s| s.text()).collect::<Vec<_>>().join(" "));
                assert!(crate::render::font::supports(&all), "unrenderable text in {all:?}");
            }
        }
    }

    #[test]
    fn missing_cue_uses_the_deck_default() {
        let d = Deck::parse(
            "language='en'\nnative='pt-BR'\ntitle='t'\ndefault_cue='Fale: {}'\n[[phrase]]\nsay='Hi'\nmeaning='Oi'",
        )
        .unwrap();
        assert_eq!(d.phrases[0].cue, vec![Segment::Native("Fale:".into()), Segment::Target("Hi".into())]);
    }

    #[test]
    fn invalid_decks_are_rejected_with_context() {
        let empty = Deck::parse("language='en'\nnative='pt-BR'\ntitle='t'").unwrap_err();
        assert!(format!("{empty:#}").contains("no [[phrase]]"));

        let blank_say = Deck::parse("language='en'\nnative='pt-BR'\ntitle='t'\n[[phrase]]\nsay='  '").unwrap_err();
        assert!(format!("{blank_say:#}").contains("empty `say`"));

        let bad_cue =
            Deck::parse("language='en'\nnative='pt-BR'\ntitle='t'\n[[phrase]]\nsay='Hi'\ncue='{{Hi'").unwrap_err();
        assert!(format!("{bad_cue:#}").contains("unclosed"));

        let typo = Deck::parse("language='en'\nnative='pt-BR'\ntitle='t'\n[[phrase]]\nsay='Hi'\nmeening='Oi'");
        assert!(typo.is_err(), "unknown fields must not be silently ignored");
    }

    #[test]
    fn custom_deck_file_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("en.toml"),
            "language='en'\nnative='pt-BR'\ntitle='Mine'\n[[phrase]]\nsay='Custom'\nmeaning='x'",
        )
        .unwrap();
        let d = Deck::load("en", dir.path()).unwrap();
        assert_eq!(d.title, "Mine");
        assert_eq!(Deck::load("es", dir.path()).unwrap().language, "es");
    }

    #[test]
    fn unknown_language_without_custom_file_explains_how_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let err = format!("{:#}", Deck::load("fr", dir.path()).unwrap_err());
        assert!(err.contains("fr.toml"), "{err}");
    }
}
