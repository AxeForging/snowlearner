//! Phrase decks: TOML data, built-in for `en` and `es` (pt-BR cues), and
//! overridable by dropping `<lang>.toml` into the config `decks/` folder.
//!
//! Phrases are real situations, not vocabulary drills: each has a topic, a
//! pt-BR situation, accepted variants and an optional practical tip.

use super::cue::{self, Segment};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::path::Path;

/// Built-in decks per language: hand-curated first, then the phrases ported
/// from Lexicaster (`scripts/port-lexicaster.mjs`). Duplicates keep the first.
const BUILTIN: &[(&str, &[&str])] = &[
    ("en", &[include_str!("../../decks/en.toml"), include_str!("../../decks/en.lexicaster.toml")]),
    ("es", &[include_str!("../../decks/es.toml"), include_str!("../../decks/es.lexicaster.toml")]),
];

pub const LEVELS: &[&str] = &["A1", "A2", "B1", "B2", "C1", "C2"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    language: String,
    native: String,
    title: String,
    /// Language name in the native language ("inglês"), used in recall prompts.
    #[serde(default)]
    language_name: Option<String>,
    #[serde(default = "default_cue")]
    default_cue: String,
    #[serde(rename = "phrase", default)]
    phrases: Vec<PhraseFile>,
    /// General tips (false friends, pronunciation, culture) for the warrior.
    #[serde(rename = "tip", default)]
    tips: Vec<TipFile>,
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
    situation: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    accept: Vec<String>,
    #[serde(default)]
    tip: Option<String>,
    /// CEFR level (A1..C2).
    #[serde(default)]
    level: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TipFile {
    text: String,
}

fn default_cue() -> String {
    "Repita: {}".to_string()
}

pub const DEFAULT_TOPIC: &str = "geral";

#[derive(Debug, Clone)]
pub struct Phrase {
    /// What the learner must say, in the target language.
    pub say: String,
    /// Other answers that also count ("Can you…" vs "Could you…").
    pub accept: Vec<String>,
    /// Native-language meaning (shown in captions and the daily summary).
    pub meaning: String,
    /// Real-life context in the native language ("Você entrou na call…").
    pub situation: Option<String>,
    /// Lowercase topic, e.g. "trabalho", "viagem".
    pub topic: String,
    /// Repeat-mode cue: native + target segments, read aloud with matching voices.
    pub cue: Vec<Segment>,
    /// Optional pronunciation / usage tip the warrior can give.
    pub tip: Option<String>,
    /// CEFR level, when the deck says (A1..C2).
    pub level: Option<String>,
}

impl Phrase {
    /// A minimal phrase (custom decks built in code, tests).
    pub fn new(say: &str, meaning: &str) -> Phrase {
        Phrase {
            say: say.to_string(),
            accept: Vec::new(),
            meaning: meaning.to_string(),
            situation: None,
            topic: DEFAULT_TOPIC.to_string(),
            cue: vec![Segment::Target(say.to_string())],
            tip: None,
            level: None,
        }
    }

    /// Every answer that counts, preferred one first.
    pub fn answers(&self) -> Vec<&str> {
        std::iter::once(self.say.as_str()).chain(self.accept.iter().map(String::as_str)).collect()
    }

    /// Recall-mode cue: the situation and meaning, but not the answer.
    pub fn recall_cue(&self, language_name: &str) -> Vec<Segment> {
        let mut out = Vec::new();
        if let Some(s) = &self.situation {
            out.push(Segment::Native(s.clone()));
        }
        out.push(Segment::Native(format!("Diga em {language_name}: \"{}\"", self.meaning)));
        out
    }
}

#[derive(Debug, Clone)]
pub struct Deck {
    pub language: String,
    pub native: String,
    pub title: String,
    pub language_name: String,
    pub phrases: Vec<Phrase>,
    pub tips: Vec<String>,
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
            let cue_src = match (p.cue, &p.situation) {
                (Some(c), _) => c,
                (None, Some(s)) => format!("{s} Diga: {{}}"),
                (None, None) => file.default_cue.clone(),
            };
            let cue = cue::parse(&cue_src, &say).with_context(|| format!("phrase #{} ({say:?})", i + 1))?;
            let topic = p.topic.map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
            let level = p.level.map(|l| l.trim().to_uppercase());
            if let Some(l) = &level
                && !LEVELS.contains(&l.as_str())
            {
                bail!("phrase #{} ({say:?}) has level {l:?}; use one of {LEVELS:?}", i + 1);
            }
            phrases.push(Phrase {
                meaning: p.meaning.unwrap_or_default(),
                accept: p.accept.into_iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
                situation: p.situation,
                topic: topic.unwrap_or_else(|| DEFAULT_TOPIC.to_string()),
                say,
                cue,
                tip: p.tip,
                level,
            });
        }
        let language_name = file.language_name.unwrap_or_else(|| file.language.clone());
        let tips = file.tips.into_iter().map(|t| t.text).collect();
        Ok(Deck { language: file.language, native: file.native, title: file.title, language_name, phrases, tips })
    }

    pub fn builtin(language: &str) -> Option<Deck> {
        let (_, sources) = BUILTIN.iter().find(|(l, _)| *l == language)?;
        let mut decks = sources.iter().map(|src| Deck::parse(src).expect("built-in deck must be valid"));
        let mut deck = decks.next()?;
        for more in decks {
            deck.merge(more);
        }
        Some(deck)
    }

    pub fn builtin_languages() -> &'static [&'static str] {
        &["en", "es"]
    }

    /// Appends phrases and tips from `other`, skipping phrases already present
    /// (compared ignoring case, accents and punctuation).
    pub fn merge(&mut self, other: Deck) {
        let key = |s: &str| crate::speech::matcher::normalize(s).join(" ");
        let mut seen: std::collections::HashSet<String> = self.phrases.iter().map(|p| key(&p.say)).collect();
        for p in other.phrases {
            if seen.insert(key(&p.say)) {
                self.phrases.push(p);
            }
        }
        self.tips.extend(other.tips);
    }

    /// Phrases at or below `max_level` (unleveled phrases always count) and,
    /// when given, in `topic`. Returns indices into `phrases`.
    pub fn selection(&self, topic: Option<&str>, max_level: &str) -> Vec<usize> {
        let max = LEVELS.iter().position(|l| *l == max_level).unwrap_or(LEVELS.len() - 1);
        (0..self.phrases.len())
            .filter(|&i| {
                let p = &self.phrases[i];
                let level_ok =
                    p.level.as_deref().and_then(|l| LEVELS.iter().position(|x| *x == l)).is_none_or(|l| l <= max);
                level_ok && topic.is_none_or(|t| p.topic == t)
            })
            .collect()
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

    /// Topics in first-seen order.
    pub fn topics(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for p in &self.phrases {
            if !out.contains(&p.topic) {
                out.push(p.topic.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "language='en'\nnative='pt-BR'\ntitle='t'\n";

    #[test]
    fn builtin_decks_are_rich_real_life_decks() {
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            assert_eq!(&deck.language, lang);
            assert_eq!(deck.native, "pt-BR");
            assert!(deck.phrases.len() >= 250, "{lang} deck too small: {}", deck.phrases.len());
            assert!(deck.topics().len() >= 10, "{lang} needs varied situations: {:?}", deck.topics());
            assert!(deck.tips.len() >= 10, "{lang} needs general tips");
            for p in &deck.phrases {
                assert!(p.cue.iter().any(Segment::is_target), "{:?} has no target", p.say);
                assert!(!p.meaning.is_empty(), "{:?} has no meaning", p.say);
                assert!(p.situation.is_some(), "{:?} has no real-life situation", p.say);
                assert_ne!(p.topic, DEFAULT_TOPIC, "{:?} has no topic", p.say);
            }
        }
    }

    #[test]
    fn builtin_decks_have_no_duplicate_phrases() {
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            let mut seen = std::collections::HashSet::new();
            for p in &deck.phrases {
                let key = crate::speech::matcher::normalize(&p.say).join(" ");
                assert!(seen.insert(key), "duplicate {:?} in {lang}", p.say);
            }
        }
    }

    #[test]
    fn builtin_deck_text_renders_with_the_pixel_font() {
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            for t in &deck.tips {
                assert!(crate::render::font::supports(t), "unrenderable tip {t:?}");
            }
            for p in deck.phrases {
                let cue = p.cue.iter().map(|s| s.text()).collect::<Vec<_>>().join(" ");
                let all = format!("{} {} {cue} {:?} {:?} {}", p.say, p.meaning, p.situation, p.tip, p.accept.join(" "));
                assert!(crate::render::font::supports(&all), "unrenderable text in {all:?}");
            }
        }
    }

    #[test]
    fn situation_becomes_the_default_cue() {
        let d =
            Deck::parse(&format!("{HEAD}[[phrase]]\nsay='Hi'\nmeaning='Oi'\nsituation='Você chega no escritório.'"))
                .unwrap();
        assert_eq!(
            d.phrases[0].cue,
            vec![Segment::Native("Você chega no escritório. Diga:".into()), Segment::Target("Hi".into())]
        );
    }

    #[test]
    fn missing_cue_and_situation_use_the_deck_default() {
        let d = Deck::parse(&format!("{HEAD}default_cue='Fale: {{}}'\n[[phrase]]\nsay='Hi'\nmeaning='Oi'")).unwrap();
        assert_eq!(d.phrases[0].cue, vec![Segment::Native("Fale:".into()), Segment::Target("Hi".into())]);
        assert_eq!(d.phrases[0].topic, DEFAULT_TOPIC);
    }

    #[test]
    fn recall_cue_never_contains_the_answer() {
        let d = Deck::parse(&format!(
            "{HEAD}language_name='inglês'\n[[phrase]]\nsay='Can you share your screen?'\nmeaning='Pode compartilhar sua tela?'\nsituation='Na call.'"
        ))
        .unwrap();
        let recall = d.phrases[0].recall_cue(&d.language_name);
        assert!(recall.iter().all(|s| !s.is_target()));
        let text = recall.iter().map(Segment::text).collect::<Vec<_>>().join(" ");
        assert!(!text.contains("share"), "{text}");
        assert!(text.contains("Na call.") && text.contains("inglês") && text.contains("compartilhar"));
    }

    #[test]
    fn accepted_variants_and_topics_are_normalized() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='Could you repeat that?'\nmeaning='x'\ntopic=' Trabalho '\naccept=['Can you repeat that?', '  ']"
        ))
        .unwrap();
        assert_eq!(d.phrases[0].answers(), vec!["Could you repeat that?", "Can you repeat that?"]);
        assert_eq!(d.phrases[0].topic, "trabalho");
    }

    #[test]
    fn merge_skips_phrases_that_only_differ_in_case_or_punctuation() {
        let mut a = Deck::parse(&format!("{HEAD}[[phrase]]\nsay=\"I'm hungry\"\nmeaning='x'")).unwrap();
        let b = Deck::parse(&format!(
            "{HEAD}[[tip]]\ntext='t'\n[[phrase]]\nsay=\"i'm hungry.\"\nmeaning='y'\n[[phrase]]\nsay='New one'\nmeaning='z'"
        ))
        .unwrap();
        a.merge(b);
        assert_eq!(a.phrases.iter().map(|p| p.say.as_str()).collect::<Vec<_>>(), vec!["I'm hungry", "New one"]);
        assert_eq!(a.phrases[0].meaning, "x", "the first (curated) version wins");
        assert_eq!(a.tips, vec!["t"]);
    }

    #[test]
    fn selection_filters_by_topic_and_level() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\ntopic='viagem'\nlevel='A1'\n\
             [[phrase]]\nsay='b'\nmeaning='x'\ntopic='viagem'\nlevel='B2'\n\
             [[phrase]]\nsay='c'\nmeaning='x'\ntopic='trabalho'"
        ))
        .unwrap();
        assert_eq!(d.selection(None, "C2"), vec![0, 1, 2]);
        assert_eq!(d.selection(None, "A2"), vec![0, 2], "unleveled phrases are always in");
        assert_eq!(d.selection(Some("viagem"), "C2"), vec![0, 1]);
        assert!(d.selection(Some("nada"), "C2").is_empty());
    }

    #[test]
    fn builtin_decks_cover_beginner_to_advanced() {
        let d = Deck::builtin("en").unwrap();
        assert!(d.selection(None, "A1").len() >= 40);
        assert!(d.selection(None, "A1").len() < d.selection(None, "B2").len());
        assert!(d.topics().contains(&"trabalho".to_string()));
    }

    #[test]
    fn invalid_levels_are_rejected() {
        let err = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\nlevel='Z9'")).unwrap_err();
        assert!(format!("{err:#}").contains("Z9"));
    }

    #[test]
    fn invalid_decks_are_rejected_with_context() {
        let empty = Deck::parse(HEAD).unwrap_err();
        assert!(format!("{empty:#}").contains("no [[phrase]]"));

        let blank_say = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='  '")).unwrap_err();
        assert!(format!("{blank_say:#}").contains("empty `say`"));

        let bad_cue = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='Hi'\ncue='{{{{Hi'")).unwrap_err();
        assert!(format!("{bad_cue:#}").contains("unclosed"));

        let typo = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='Hi'\nmeening='Oi'"));
        assert!(typo.is_err(), "unknown fields must not be silently ignored");
    }

    #[test]
    fn custom_deck_file_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("en.toml"), format!("{HEAD}[[phrase]]\nsay='Custom'\nmeaning='x'")).unwrap();
        let d = Deck::load("en", dir.path()).unwrap();
        assert_eq!(d.phrases[0].say, "Custom");
        assert_eq!(Deck::load("es", dir.path()).unwrap().language, "es");
    }

    #[test]
    fn unknown_language_without_custom_file_explains_how_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let err = format!("{:#}", Deck::load("fr", dir.path()).unwrap_err());
        assert!(err.contains("fr.toml"), "{err}");
    }
}
