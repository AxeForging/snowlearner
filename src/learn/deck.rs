//! Phrase decks: TOML data, built-in for `en` and `es` (pt-BR cues) and
//! `pt-BR` (English cues), overridable by dropping `<lang>.toml` into the
//! config `decks/` folder.
//!
//! Phrases are real situations, not vocabulary drills: each has a topic, a
//! situation in the learner's own language, accepted variants and an
//! optional practical tip. A deck is written for one native language; a
//! translation overlay (`decks/i18n/<deck>.<native>.toml`, keyed by `say`)
//! brings it to another, and phrases it does not translate are left out.
//! A phrase may come in three tiers: `short` (the quickest way to say it),
//! `say` (the complete one) and `polished` (the most courteous one).

use super::cue::{self, Segment};
use crate::lang::{Native, T};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Built-in decks per language: hand-curated phrases, the first words and
/// chunks (`<lang>.basics.toml`), then the phrases ported from Lexicaster
/// (`scripts/port-lexicaster.mjs`). Duplicates keep the first.
const BUILTIN: &[(&str, &[&str])] = &[
    (
        "en",
        &[
            include_str!("../../decks/en.toml"),
            include_str!("../../decks/en.basics.toml"),
            include_str!("../../decks/en.lexicaster.toml"),
        ],
    ),
    (
        "es",
        &[
            include_str!("../../decks/es.toml"),
            include_str!("../../decks/es.basics.toml"),
            include_str!("../../decks/es.lexicaster.toml"),
        ],
    ),
    ("pt-BR", &[include_str!("../../decks/pt.basics.toml")]),
];

/// Answer tiers for the generated decks, hand-curated and keyed by `say`
/// (`<lang>.lexicaster.tiers.toml`), so regenerating a deck never loses them.
const TIERS: &[(&str, &str)] = &[
    ("en", include_str!("../../decks/en.lexicaster.tiers.toml")),
    ("es", include_str!("../../decks/es.lexicaster.tiers.toml")),
];

/// Built-in translations: (deck language, native, overlays keyed by `say`).
/// The Lexicaster ports have none yet, so English speakers don't get them.
const TRANSLATIONS: &[(&str, Native, &[&str])] = &[(
    "es",
    Native::En,
    &[include_str!("../../decks/i18n/es.en.toml"), include_str!("../../decks/i18n/es.basics.en.toml")],
)];

/// CEFR levels, easiest first. Pre-A1 (CEFR Companion Volume, 2020) is the
/// learner who knows nothing yet: isolated words and set expressions.
pub const LEVELS: &[&str] = &[PRE_A1, "A1", "A2", "B1", "B2", "C1", "C2"];
pub const PRE_A1: &str = "PRE-A1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFile {
    language: String,
    native: String,
    title: String,
    /// Language name in the native language ("inglês"), used in recall prompts.
    #[serde(default)]
    language_name: Option<String>,
    #[serde(default)]
    default_cue: Option<String>,
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
    /// Shortest natural way to say it ("Mute!"); `say` is the complete one.
    #[serde(default)]
    short: Option<String>,
    /// Most courteous way to say it.
    #[serde(default)]
    polished: Option<String>,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TiersFile {
    #[serde(rename = "tier", default)]
    tiers: Vec<TierFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TierFile {
    say: String,
    #[serde(default)]
    short: Option<String>,
    #[serde(default)]
    polished: Option<String>,
}

/// Same phrase once case, accents and punctuation go.
fn same_key(s: &str) -> String {
    crate::speech::matcher::normalize(s).join(" ")
}

/// Trimmed short and polished tiers; a tier equal to a lower one collapses.
fn clean_tiers(say: &str, short: Option<String>, polished: Option<String>) -> (Option<String>, Option<String>) {
    let tier = |t: Option<String>, below: &[Option<&str>]| {
        t.map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && below.iter().flatten().all(|b| same_key(b) != same_key(t)))
    };
    let short = tier(short, &[Some(say)]);
    let polished = tier(polished, &[Some(say), short.as_deref()]);
    (short, polished)
}

/// A deck's text in another native language, keyed by the phrase's `say`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverlayFile {
    language: String,
    native: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    language_name: Option<String>,
    #[serde(default)]
    default_cue: Option<String>,
    #[serde(rename = "phrase", default)]
    phrases: Vec<OverlayPhrase>,
    #[serde(rename = "tip", default)]
    tips: Vec<TipFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OverlayPhrase {
    say: String,
    meaning: String,
    #[serde(default)]
    situation: Option<String>,
    #[serde(default)]
    tip: Option<String>,
    #[serde(default)]
    cue: Option<String>,
}

/// The cue a phrase gets without its own: the situation then "Say:", or the
/// deck's default ("Repeat: {}").
fn derived_cue(native: Native, situation: Option<&str>, default_cue: Option<&str>) -> String {
    match (situation, default_cue) {
        (Some(s), _) => format!("{s} {} {{}}", T::CueSay.get(native)),
        (None, Some(d)) => d.to_string(),
        (None, None) => format!("{} {{}}", T::CueRepeat.get(native)),
    }
}

pub const DEFAULT_TOPIC: &str = "geral";

/// Which answer the learner practices: every tier shown, or a single one.
/// A tier is `Short`, `Complete` or `Polished`; `All` only picks what is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Answer {
    #[default]
    #[serde(alias = "todas")]
    All,
    #[serde(alias = "curta")]
    Short,
    #[serde(alias = "completa")]
    Complete,
    #[serde(alias = "polida")]
    Polished,
}

impl Answer {
    pub const CHOICES: [Answer; 4] = [Answer::All, Answer::Short, Answer::Complete, Answer::Polished];

    pub fn label(self, native: Native) -> &'static str {
        match self {
            Answer::All => T::AnswerAll,
            Answer::Short => T::AnswerShort,
            Answer::Complete => T::AnswerComplete,
            Answer::Polished => T::AnswerPolished,
        }
        .get(native)
    }
}

#[derive(Debug, Clone)]
pub struct Phrase {
    /// What the learner must say, in the target language.
    pub say: String,
    /// Other answers that also count ("Can you…" vs "Could you…"), as complete.
    pub accept: Vec<String>,
    /// Shorter tier; None when the deck has none (or it equals `say`).
    pub short: Option<String>,
    /// Politer tier; None when the deck has none (or it equals another tier).
    pub polished: Option<String>,
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
            short: None,
            polished: None,
            meaning: meaning.to_string(),
            situation: None,
            topic: DEFAULT_TOPIC.to_string(),
            cue: vec![Segment::Target(say.to_string())],
            tip: None,
            level: None,
        }
    }

    /// Every answer that counts with its tier, shortest tier first (so a tie
    /// in scoring goes to the higher tier). Accepted variants count as complete.
    pub fn answers(&self) -> Vec<(Answer, &str)> {
        let short = self.short.as_deref().map(|s| (Answer::Short, s));
        let polished = self.polished.as_deref().map(|s| (Answer::Polished, s));
        short
            .into_iter()
            .chain(std::iter::once((Answer::Complete, self.say.as_str())))
            .chain(self.accept.iter().map(|a| (Answer::Complete, a.as_str())))
            .chain(polished)
            .collect()
    }

    /// The distinct tiers this phrase has: short, complete, polished.
    pub fn tiers(&self) -> Vec<(Answer, &str)> {
        self.answers().into_iter().filter(|(t, s)| *t != Answer::Complete || *s == self.say).collect()
    }

    /// The tier the learner is asked for; a missing tier falls back to `say`,
    /// and `All` asks for the complete one (the others are shown beside it).
    pub fn shown(&self, answer: Answer) -> &str {
        let tier = match answer {
            Answer::Short => self.short.as_deref(),
            Answer::Polished => self.polished.as_deref(),
            Answer::All | Answer::Complete => None,
        };
        tier.unwrap_or(&self.say)
    }

    /// The repeat cue with the asked tier in place of the complete phrase.
    pub fn cue_for(&self, answer: Answer) -> Vec<Segment> {
        let shown = self.shown(answer);
        self.cue
            .iter()
            .map(|s| match s {
                Segment::Target(t) if *t == self.say => Segment::Target(shown.to_string()),
                other => other.clone(),
            })
            .collect()
    }

    /// With `All`: the other tiers, labeled ("curta: Mute!"), to show under
    /// the cue (which holds the complete one). Nothing for a single tier.
    pub fn alternatives(&self, answer: Answer, native: Native) -> Vec<String> {
        if answer != Answer::All {
            return Vec::new();
        }
        self.tiers()
            .into_iter()
            .filter(|(t, _)| *t != Answer::Complete)
            .map(|(t, s)| format!("{}: {s}", t.label(native)))
            .collect()
    }

    /// Recall-mode cue: the situation and meaning, but not the answer.
    pub fn recall_cue(&self, language_name: &str, native: Native) -> Vec<Segment> {
        let mut out = Vec::new();
        if let Some(s) = &self.situation {
            out.push(Segment::Native(s.clone()));
        }
        out.push(Segment::Native(T::RecallAsk.fill(native, &[&language_name, &self.meaning])));
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
        // Custom decks may name any native; their cue words fall back to pt-BR.
        let native = Native::parse(&file.native).unwrap_or_default();
        let mut phrases = Vec::with_capacity(file.phrases.len());
        for (i, p) in file.phrases.into_iter().enumerate() {
            let say = p.say.trim().to_string();
            if say.is_empty() {
                bail!("phrase #{} has an empty `say`", i + 1);
            }
            let cue_src =
                p.cue.unwrap_or_else(|| derived_cue(native, p.situation.as_deref(), file.default_cue.as_deref()));
            let cue = cue::parse(&cue_src, &say).with_context(|| format!("phrase #{} ({say:?})", i + 1))?;
            let (short, polished) = clean_tiers(&say, p.short, p.polished);
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
                short,
                polished,
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
        if let Some((_, overlay)) = TIERS.iter().find(|(l, _)| *l == language) {
            deck.apply_tiers(overlay).expect("built-in tier overlay must match its deck");
        }
        Some(deck)
    }

    pub fn builtin_languages() -> &'static [&'static str] {
        &["en", "es", "pt-BR"]
    }

    /// The built-in deck for `language` in the learner's `native` language:
    /// as written, or through its built-in translation. None when there is
    /// no such deck or no translation for that native.
    pub fn builtin_for(language: &str, native: Native) -> Option<Deck> {
        let deck = Deck::builtin(language)?;
        deck.localize(native, &builtin_overlays(language, native)).ok()
    }

    /// Built-in languages there is something to learn in for this native
    /// (written for it or translated), never the native itself.
    pub fn languages_for(native: Native) -> Vec<&'static str> {
        Deck::builtin_languages()
            .iter()
            .copied()
            .filter(|l| !native.is(l))
            .filter(|l| {
                let written = BUILTIN.iter().find(|(b, _)| b == l).and_then(|(_, s)| s.first());
                written.is_some_and(|src| Deck::parse(src).is_ok_and(|d| Native::parse(&d.native) == Some(native)))
                    || TRANSLATIONS.iter().any(|(b, n, _)| b == l && *n == native)
            })
            .collect()
    }

    /// This deck in `native`: unchanged when written for it; otherwise the
    /// overlays give each phrase its meaning, situation, tip and cue, and a
    /// phrase they leave out is dropped (never shown half translated). An
    /// overlay entry for a phrase the deck doesn't have is an error: it went
    /// stale when the deck changed.
    pub fn localize(mut self, native: Native, overlays: &[&str]) -> Result<Deck> {
        if Native::parse(&self.native) == Some(native) {
            return Ok(self);
        }
        if overlays.is_empty() {
            bail!("deck {:?} is for {} speakers and has no {} translation", self.language, self.native, native.code());
        }
        let deck_keys: std::collections::HashSet<String> = self.phrases.iter().map(|p| same_key(&p.say)).collect();
        let mut by_say: std::collections::HashMap<String, (OverlayPhrase, Option<String>)> = Default::default();
        let mut tips = Vec::new();
        let (mut title, mut language_name) = (None, None);
        for (n, src) in overlays.iter().enumerate() {
            let file: OverlayFile = toml::from_str(src).with_context(|| format!("invalid translation #{}", n + 1))?;
            if file.language != self.language || Native::parse(&file.native) != Some(native) {
                bail!(
                    "translation #{} is {} -> {}, the deck needs {} -> {}",
                    n + 1,
                    file.native,
                    file.language,
                    native.code(),
                    self.language
                );
            }
            title = title.or(file.title);
            language_name = language_name.or(file.language_name);
            tips.extend(file.tips.into_iter().map(|t| t.text));
            for p in file.phrases {
                let k = same_key(&p.say);
                if !deck_keys.contains(&k) {
                    bail!("translation of {:?} matches no phrase in the {} deck (stale?)", p.say, self.language);
                }
                if p.meaning.trim().is_empty() {
                    bail!("translation of {:?} has an empty `meaning`", p.say);
                }
                let say = p.say.clone();
                if by_say.insert(k, (p, file.default_cue.clone())).is_some() {
                    bail!("{say:?} is translated twice");
                }
            }
        }
        let mut phrases = Vec::new();
        for mut p in std::mem::take(&mut self.phrases) {
            let Some((t, default_cue)) = by_say.remove(&same_key(&p.say)) else { continue };
            let src = t.cue.unwrap_or_else(|| derived_cue(native, t.situation.as_deref(), default_cue.as_deref()));
            p.cue = cue::parse(&src, &p.say).with_context(|| format!("translated cue of {:?}", p.say))?;
            p.meaning = t.meaning;
            p.situation = t.situation;
            p.tip = t.tip;
            phrases.push(p);
        }
        self.phrases = phrases;
        self.tips = tips;
        self.native = native.code().to_string();
        self.title = title.unwrap_or(self.title);
        self.language_name = language_name.unwrap_or_else(|| crate::lang::text::language_name(native, &self.language));
        Ok(self)
    }

    /// Sets the short / polished tiers from an overlay (`[[tier]]` entries keyed
    /// by `say`, compared ignoring case and punctuation). An entry whose `say`
    /// is not in the deck is an error: a stale overlay must not go unnoticed.
    pub fn apply_tiers(&mut self, src: &str) -> Result<()> {
        let file: TiersFile = toml::from_str(src).context("invalid tier overlay TOML")?;
        for t in file.tiers {
            let key = same_key(&t.say);
            let Some(p) = self.phrases.iter_mut().find(|p| same_key(&p.say) == key) else {
                bail!("tier overlay entry {:?} matches no phrase in the deck (stale overlay?)", t.say);
            };
            (p.short, p.polished) = clean_tiers(&p.say, t.short, t.polished);
        }
        Ok(())
    }

    /// Appends phrases and tips from `other`, skipping phrases already present
    /// (compared ignoring case, accents and punctuation).
    pub fn merge(&mut self, other: Deck) {
        let mut seen: std::collections::HashSet<String> = self.phrases.iter().map(|p| same_key(&p.say)).collect();
        for p in other.phrases {
            if seen.insert(same_key(&p.say)) {
                self.phrases.push(p);
            }
        }
        self.tips.extend(other.tips);
    }

    /// Phrases at or below `max_level` (unleveled phrases always count) and in
    /// one of `topics` (empty = every topic). Returns indices into `phrases`.
    pub fn selection(&self, topics: &[String], max_level: &str) -> Vec<usize> {
        let max = LEVELS.iter().position(|l| *l == max_level).unwrap_or(LEVELS.len() - 1);
        (0..self.phrases.len())
            .filter(|&i| {
                let p = &self.phrases[i];
                // Unleveled phrases count at every level but pre-A1, where
                // someone who knows nothing must not get full phrases.
                let level_ok = match p.level.as_deref().and_then(|l| LEVELS.iter().position(|x| *x == l)) {
                    Some(l) => l <= max,
                    None => max_level != PRE_A1,
                };
                level_ok && (topics.is_empty() || topics.contains(&p.topic))
            })
            .collect()
    }

    /// The deck for `language` in the learner's `native` language. A custom
    /// `<decks_dir>/<language>.toml` wins over the built-in deck; one written
    /// for another native is translated by `<decks_dir>/i18n/<language>.<native>.toml`.
    pub fn load(language: &str, native: Native, decks_dir: &Path) -> Result<Deck> {
        if native.is(language) {
            bail!("{language:?} is your own language: choose another one to learn");
        }
        let custom = decks_dir.join(format!("{language}.toml"));
        let read = |p: &Path| std::fs::read_to_string(p).with_context(|| format!("reading {}", p.display()));
        if custom.exists() {
            let deck = Deck::parse(&read(&custom)?).with_context(|| format!("in {}", custom.display()))?;
            let overlay = decks_dir.join("i18n").join(format!("{language}.{}.toml", native.code()));
            let overlays = if overlay.exists() { vec![read(&overlay)?] } else { Vec::new() };
            let refs: Vec<&str> = overlays.iter().map(String::as_str).collect();
            return deck.localize(native, &refs).with_context(|| format!("in {}", custom.display()));
        }
        let Some(deck) = Deck::builtin(language) else {
            bail!(
                "no deck for language {language:?}: built-in decks are {:?}, or create {}",
                Deck::builtin_languages(),
                custom.display()
            );
        };
        deck.localize(native, &builtin_overlays(language, native))
    }

    /// Topics with something to practice at `max_level`, in deck order.
    pub fn topics_at(&self, max_level: &str) -> Vec<String> {
        self.topic_counts(max_level).into_iter().map(|(t, _)| t).collect()
    }

    /// Each topic with something at `max_level` and how many items it has
    /// there, in deck order: what the panel's topic checklist shows.
    pub fn topic_counts(&self, max_level: &str) -> Vec<(String, usize)> {
        let mut out: Vec<(String, usize)> = Vec::new();
        for i in self.selection(&[], max_level) {
            let t = &self.phrases[i].topic;
            match out.iter_mut().find(|(k, _)| k == t) {
                Some((_, n)) => *n += 1,
                None => out.push((t.clone(), 1)),
            }
        }
        out
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

fn builtin_overlays(language: &str, native: Native) -> Vec<&'static str> {
    TRANSLATIONS
        .iter()
        .filter(|(l, n, _)| *l == language && *n == native)
        .flat_map(|(_, _, o)| o.iter().copied())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "language='en'\nnative='pt-BR'\ntitle='t'\n";
    /// Built-in decks written for pt-BR speakers (the big ones, with Lexicaster).
    const PT_BR_DECKS: &[&str] = &["en", "es"];
    const ES_HEAD: &str = "language='es'\nnative='pt-BR'\ntitle='t'\n";
    const ES_EN: &str = "language='es'\nnative='en'\ntitle='Spanish'\nlanguage_name='Spanish'\n";

    #[test]
    fn an_overlay_brings_a_deck_to_english_speakers_and_drops_what_it_does_not_translate() {
        let deck = Deck::parse(&format!(
            "{ES_HEAD}language_name='espanhol'\n[[tip]]\ntext='Falso amigo: polvo.'\n\
             [[phrase]]\nsay='Hola.'\nmeaning='Olá.'\nsituation='Você entra na loja.'\ntip='O h é mudo.'\n\
             [[phrase]]\nsay='La cuenta.'\nmeaning='A conta.'\nsituation='Você terminou de jantar.'\n\
             [[phrase]]\nsay='Gracias.'\nmeaning='Obrigado.'\ntip='Dica só em português.'"
        ))
        .unwrap();
        let overlay = format!(
            "{ES_EN}[[tip]]\ntext='False friend: embarazada.'\n\
             [[phrase]]\nsay='hola'\nmeaning='Hello.'\nsituation='You walk into a shop.'\ntip='The h is silent.'\n\
             [[phrase]]\nsay='Gracias.'\nmeaning='Thank you.'"
        );
        let d = deck.localize(Native::En, &[&overlay]).unwrap();
        assert_eq!(d.native, "en");
        assert_eq!(d.language_name, "Spanish");
        assert_eq!(d.tips, vec!["False friend: embarazada."], "the deck's own tips are for pt-BR speakers");
        let says: Vec<&str> = d.phrases.iter().map(|p| p.say.as_str()).collect();
        assert_eq!(says, vec!["Hola.", "Gracias."], "no translation, no phrase");
        let hola = &d.phrases[0];
        assert_eq!((hola.meaning.as_str(), hola.tip.as_deref()), ("Hello.", Some("The h is silent.")));
        assert_eq!(
            hola.cue,
            vec![Segment::Native("You walk into a shop. Say:".into()), Segment::Target("Hola.".into())]
        );
        let gracias = &d.phrases[1];
        assert_eq!(gracias.tip, None, "a tip left untranslated is not shown in Portuguese");
        assert_eq!(gracias.cue[0], Segment::Native("Repeat:".into()));
    }

    #[test]
    fn a_stale_or_doubled_overlay_entry_is_an_error() {
        let deck = Deck::parse(&format!("{ES_HEAD}[[phrase]]\nsay='Hola.'\nmeaning='Olá.'")).unwrap();
        let stale = format!("{ES_EN}[[phrase]]\nsay='Adiós.'\nmeaning='Bye.'");
        let err = format!("{:#}", deck.clone().localize(Native::En, &[&stale]).unwrap_err());
        assert!(err.contains("Adiós") && err.contains("stale"), "{err}");
        let twice = format!("{ES_EN}[[phrase]]\nsay='Hola.'\nmeaning='Hi.'");
        assert!(deck.clone().localize(Native::En, &[&twice, &twice]).is_err());
        let wrong_pair = "language='en'\nnative='en'\n[[phrase]]\nsay='Hola.'\nmeaning='Hi.'";
        assert!(deck.clone().localize(Native::En, &[wrong_pair]).is_err(), "an overlay for another deck");
        let typo = format!("{ES_EN}[[phrase]]\nsay='Hola.'\nmeening='Hi.'");
        assert!(deck.clone().localize(Native::En, &[&typo]).is_err(), "unknown fields are not ignored");
        let err = format!("{:#}", deck.clone().localize(Native::En, &[]).unwrap_err());
        assert!(err.contains("no en translation"), "{err}");
        assert_eq!(deck.localize(Native::PtBr, &[]).unwrap().phrases[0].meaning, "Olá.", "already in pt-BR");
    }

    #[test]
    fn english_speakers_get_the_whole_curated_spanish_deck_in_english() {
        let pt = Deck::builtin("es").unwrap();
        let en = Deck::builtin_for("es", Native::En).unwrap();
        let curated = Deck::parse(include_str!("../../decks/es.toml")).unwrap().phrases.len()
            + Deck::parse(include_str!("../../decks/es.basics.toml")).unwrap().phrases.len();
        assert_eq!(en.phrases.len(), curated, "es.toml and es.basics are fully translated; Lexicaster is not");
        assert!(en.phrases.len() < pt.phrases.len());
        assert!(en.tips.len() >= 10, "tips for English speakers");
        assert_eq!(en.language_name, "Spanish");
        for p in &en.phrases {
            assert!(p.situation.is_some(), "{:?} has no situation", p.say);
            assert!(p.cue.iter().any(|s| matches!(s, Segment::Native(t) if t.ends_with("Say:"))), "{:?}", p.say);
            let text =
                format!("{} {:?} {:?} {:?}", p.meaning, p.situation, p.tip, p.alternatives(Answer::All, Native::En));
            assert!(crate::render::font::supports(&text), "{text}");
        }
        for t in &en.tips {
            assert!(crate::render::font::supports(t), "{t}");
        }
        let pre = en.selection(&[], PRE_A1);
        assert!(pre.len() >= 40, "an English beginner has {} first items", pre.len());
        let first = crate::learn::path::ordered(&en.phrases, &en.selection(&[], "C2"))[0];
        assert_eq!(crate::learn::path::Stage::of(&en.phrases[first]), crate::learn::path::Stage::Words);
    }

    #[test]
    fn english_speakers_can_learn_brazilian_portuguese_from_single_words() {
        use crate::learn::path::Stage;
        let d = Deck::builtin_for("pt-BR", Native::En).unwrap();
        assert_eq!((d.language.as_str(), d.native.as_str()), ("pt-BR", "en"));
        assert!(d.phrases.len() >= 60, "{} items", d.phrases.len());
        let count = |st: Stage| d.phrases.iter().filter(|p| Stage::of(p) == st).count();
        assert!(count(Stage::Words) >= 40, "{} first words", count(Stage::Words));
        assert!(count(Stage::Chunks) >= 20, "{} short chunks", count(Stage::Chunks));
        assert_eq!(count(Stage::Phrases), 0, "a first-steps deck has no full phrases");
        let order = crate::learn::path::ordered(&d.phrases, &d.selection(&[], "C2"));
        assert_eq!(Stage::of(&d.phrases[order[0]]), Stage::Words, "the path starts with a word");
        for p in &d.phrases {
            assert!(p.situation.is_some() && !p.meaning.is_empty(), "{:?}", p.say);
            assert_ne!(p.topic, DEFAULT_TOPIC, "{:?} has no topic", p.say);
            assert!(crate::render::font::supports(&format!("{} {:?} {:?}", p.meaning, p.situation, p.tip)));
        }
        for topic in CORE_TOPICS {
            for level in [PRE_A1, "A1"] {
                let n = d.phrases.iter().filter(|p| p.topic == *topic && p.level.as_deref() == Some(level)).count();
                assert!(n >= RUNG_MIN, "pt-BR: {topic} {level} has {n}");
            }
        }
    }

    #[test]
    fn nobody_is_offered_their_own_language_or_a_deck_they_cannot_read() {
        assert_eq!(Deck::languages_for(Native::PtBr), vec!["en", "es"]);
        assert_eq!(Deck::languages_for(Native::En), vec!["es", "pt-BR"]);
        assert!(Deck::builtin_for("pt-BR", Native::PtBr).is_none());
        assert!(Deck::builtin_for("en", Native::En).is_none());
        let dir = tempfile::tempdir().unwrap();
        let err = format!("{:#}", Deck::load("en", Native::En, dir.path()).unwrap_err());
        assert!(err.contains("own language"), "{err}");
        assert!(Deck::load("pt", Native::PtBr, dir.path()).is_err());
    }

    #[test]
    fn portuguese_speakers_keep_the_same_decks_as_before() {
        for lang in PT_BR_DECKS {
            let dir = tempfile::tempdir().unwrap();
            let loaded = Deck::load(lang, Native::PtBr, dir.path()).unwrap();
            let raw = Deck::builtin(lang).unwrap();
            assert_eq!(loaded.phrases.len(), raw.phrases.len());
            assert_eq!(loaded.tips, raw.tips);
            assert!(loaded.phrases.iter().zip(&raw.phrases).all(|(a, b)| a.meaning == b.meaning && a.cue == b.cue));
            assert!(loaded.phrases.iter().any(|p| p.cue.iter().any(|s| s.text().ends_with("Diga:"))));
        }
    }

    #[test]
    fn a_custom_deck_is_translated_by_its_own_overlay() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("es.toml"), format!("{ES_HEAD}[[phrase]]\nsay='Hola.'\nmeaning='Olá.'"))
            .unwrap();
        let err = format!("{:#}", Deck::load("es", Native::En, dir.path()).unwrap_err());
        assert!(err.contains("no en translation"), "{err}");
        std::fs::create_dir(dir.path().join("i18n")).unwrap();
        std::fs::write(dir.path().join("i18n/es.en.toml"), format!("{ES_EN}[[phrase]]\nsay='Hola.'\nmeaning='Hi.'"))
            .unwrap();
        assert_eq!(Deck::load("es", Native::En, dir.path()).unwrap().phrases[0].meaning, "Hi.");
    }

    #[test]
    fn builtin_decks_are_rich_real_life_decks() {
        for lang in PT_BR_DECKS {
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
    fn builtin_decks_give_a_pre_a1_learner_words_and_set_expressions_only() {
        for lang in PT_BR_DECKS {
            let deck = Deck::builtin(lang).unwrap();
            let pre = deck.selection(&[], PRE_A1);
            assert!(pre.len() >= 60, "{lang}: only {} pre-A1 items", pre.len());
            for &i in &pre {
                let p = &deck.phrases[i];
                assert_eq!(p.level.as_deref(), Some(PRE_A1), "{lang}: {:?} is not pre-A1", p.say);
                assert!(p.say.split_whitespace().count() <= 3, "{lang}: {:?} is a full phrase", p.say);
                assert!(crate::render::font::supports(&format!("{} {}", p.say, p.meaning)), "{lang}: {:?}", p.say);
            }
        }
    }

    /// Topics that come back at every level, harder each time, the way a
    /// language app's units recur from section to section.
    const CORE_TOPICS: &[&str] =
        &["primeiros contatos", "trabalho", "restaurante", "viagem", "compras", "saúde e social"];
    /// Levels every core topic climbs through.
    const LADDER: &[&str] = &[PRE_A1, "A1", "A2", "B1"];
    /// Fewest items that make a rung.
    const RUNG_MIN: usize = 3;
    /// Rungs still missing in the built-in decks. Filling one means removing it
    /// here: the test fails while a filled rung is still listed.
    const LADDER_GAPS: &[(&str, &str)] = &[
        ("primeiros contatos", "A2"),
        ("primeiros contatos", "B1"),
        ("restaurante", "A2"),
        ("viagem", "A2"),
        ("compras", "A2"),
        ("compras", "B1"),
        ("saúde e social", "B1"),
    ];

    #[test]
    fn core_topics_climb_every_level_from_pre_a1_to_b1() {
        for lang in PT_BR_DECKS {
            let deck = Deck::builtin(lang).unwrap();
            for topic in CORE_TOPICS {
                for level in LADDER {
                    let n =
                        deck.phrases.iter().filter(|p| p.topic == *topic && p.level.as_deref() == Some(level)).count();
                    let pending = LADDER_GAPS.contains(&(topic, level));
                    if pending {
                        assert!(n < RUNG_MIN, "{lang}: {topic} {level} has {n} items now; take it off LADDER_GAPS");
                    } else {
                        assert!(n >= RUNG_MIN, "{lang}: {topic} {level} has {n} items, a rung needs {RUNG_MIN}");
                    }
                }
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
                let all = format!(
                    "{} {} {cue} {:?} {:?} {} {:?}",
                    p.say,
                    p.meaning,
                    p.situation,
                    p.tip,
                    p.accept.join(" "),
                    p.alternatives(Answer::All, Native::parse(&deck.native).unwrap())
                );
                assert!(crate::render::font::supports(&all), "unrenderable text in {all:?}");
            }
        }
    }

    #[test]
    fn builtin_tiers_grow_from_short_to_complete_to_polished() {
        let words = |s: &str| s.split_whitespace().count();
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            let tiered: Vec<&Phrase> = deck.phrases.iter().filter(|p| p.tiers().len() > 1).collect();
            assert!(tiered.len() >= 40, "{lang}: only {} phrases with tiers", tiered.len());
            for p in tiered {
                if p.level.as_deref() == Some(PRE_A1) {
                    assert!(words(&p.say) <= 3, "{lang}: pre-A1 {:?} is asked as a word or set expression", p.say);
                }
                if let Some(s) = &p.short {
                    assert!(words(s) <= words(&p.say), "{lang}: short {s:?} longer than {:?}", p.say);
                }
                if let Some(pol) = &p.polished {
                    assert!(words(&p.say) <= words(pol), "{lang}: polished {pol:?} shorter than {:?}", p.say);
                }
                if let (Some(s), Some(pol)) = (&p.short, &p.polished) {
                    assert!(words(s) < words(pol), "{lang}: {s:?} and {pol:?} are the same size");
                }
            }
        }
    }

    /// The first-words decks, where the learning path starts.
    const BASICS: &[(&str, &str)] = &[
        ("en", include_str!("../../decks/en.basics.toml")),
        ("es", include_str!("../../decks/es.basics.toml")),
        ("pt-BR", include_str!("../../decks/pt.basics.toml")),
    ];

    #[test]
    fn first_words_keep_their_shape_and_offer_a_polished_sentence() {
        let words = |s: &str| s.split_whitespace().count();
        let key = |s: &str| crate::speech::matcher::normalize(s).join(" ");
        for (lang, src) in BASICS {
            let deck = Deck::parse(src).unwrap();
            for p in &deck.phrases {
                assert!(words(&p.say) <= 3, "{lang}: basics {:?} is asked as a word or short chunk", p.say);
                let pol = p.polished.as_deref().unwrap_or_else(|| panic!("{lang}: {:?} has no polished tier", p.say));
                assert_ne!(key(pol), key(&p.say), "{lang}: polished {pol:?} repeats the word");
                assert!(words(pol) > words(&p.say), "{lang}: polished {pol:?} is no fuller than {:?}", p.say);
                assert!(words(pol) <= 7, "{lang}: polished {pol:?} is too long for a beginner");
                assert_eq!(p.shown(Answer::Complete), p.say, "{lang}: the complete tier still asks the word");
                let native = Native::parse(&deck.native).unwrap();
                let label = Answer::Polished.label(native);
                assert_eq!(p.alternatives(Answer::All, native), vec![format!("{label}: {pol}")]);
            }
        }
    }

    #[test]
    fn every_builtin_item_offers_at_least_two_answers() {
        for lang in Deck::builtin_languages() {
            let deck = Deck::builtin(lang).unwrap();
            for p in &deck.phrases {
                assert!(p.polished.is_some(), "{lang}: {:?} has no polished answer", p.say);
                assert!(p.tiers().len() >= 2, "{lang}: {:?} offers only {:?}", p.say, p.tiers());
            }
        }
    }

    /// The generated decks and their hand-curated tier overlays.
    const GENERATED: &[(&str, &str, &str)] = &[
        ("en", include_str!("../../decks/en.lexicaster.toml"), include_str!("../../decks/en.lexicaster.tiers.toml")),
        ("es", include_str!("../../decks/es.lexicaster.toml"), include_str!("../../decks/es.lexicaster.tiers.toml")),
    ];

    #[test]
    fn every_tier_overlay_entry_names_a_phrase_of_the_generated_deck() {
        for (lang, deck, tiers) in GENERATED {
            let mut d = Deck::parse(deck).unwrap();
            if let Err(e) = d.apply_tiers(tiers) {
                panic!("{lang}: {e:#}");
            }
        }
    }

    #[test]
    fn a_stale_tier_overlay_entry_fails_loudly() {
        let mut d = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='Thank you very much!'\nmeaning='x'")).unwrap();
        let err = d.apply_tiers("[[tier]]\nsay='Thanks a ton!'\npolished='Thanks a ton, really!'").unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("Thanks a ton!") && msg.contains("stale"), "{msg}");
        assert!(d.apply_tiers("[[tier]]\nsay='Hi'\npolishd='x'").is_err(), "typos in the overlay are rejected");
    }

    #[test]
    fn a_tier_overlay_fills_tiers_by_say_ignoring_case_and_punctuation() {
        let mut d = Deck::parse(&format!("{HEAD}[[phrase]]\nsay='Thank you very much!'\nmeaning='x'")).unwrap();
        d.apply_tiers(
            "[[tier]]\nsay='thank you very much'\nshort='Thanks a lot!'\npolished='Thank you so much, really!'",
        )
        .unwrap();
        let p = &d.phrases[0];
        assert_eq!(p.say, "Thank you very much!", "say is untouched");
        assert_eq!(p.short.as_deref(), Some("Thanks a lot!"));
        assert_eq!(p.polished.as_deref(), Some("Thank you so much, really!"));
        d.apply_tiers("[[tier]]\nsay='Thank you very much!'\npolished='THANK YOU VERY MUCH'").unwrap();
        assert_eq!(d.phrases[0].polished, None, "a tier equal to say collapses, as in decks");
    }

    #[test]
    fn tiers_are_optional_and_missing_ones_fall_back_to_say() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='Can you share your screen?'\nmeaning='x'\nshort='Share your screen?'\n\
             polished='Would you mind sharing your screen?'\naccept=['Could you share your screen?']\n\
             [[phrase]]\nsay='Hi'\nmeaning='Oi'"
        ))
        .unwrap();
        let (tiered, plain) = (&d.phrases[0], &d.phrases[1]);
        assert_eq!(tiered.shown(Answer::Short), "Share your screen?");
        assert_eq!(tiered.shown(Answer::Complete), "Can you share your screen?");
        assert_eq!(tiered.shown(Answer::All), "Can you share your screen?", "all asks the complete one");
        assert_eq!(tiered.shown(Answer::Polished), "Would you mind sharing your screen?");
        assert_eq!(
            tiered.answers(),
            vec![
                (Answer::Short, "Share your screen?"),
                (Answer::Complete, "Can you share your screen?"),
                (Answer::Complete, "Could you share your screen?"),
                (Answer::Polished, "Would you mind sharing your screen?"),
            ],
            "every tier counts, shortest first"
        );
        assert_eq!(
            tiered.alternatives(Answer::All, Native::PtBr),
            vec!["curta: Share your screen?", "polida: Would you mind sharing your screen?"]
        );
        assert!(tiered.alternatives(Answer::Short, Native::PtBr).is_empty(), "one tier shows only itself");
        for a in Answer::CHOICES {
            assert_eq!(plain.shown(a), "Hi", "{a:?} falls back to say");
            assert!(plain.alternatives(a, Native::PtBr).is_empty());
        }
        assert_eq!(plain.tiers(), vec![(Answer::Complete, "Hi")]);
    }

    #[test]
    fn the_cue_asks_for_the_chosen_tier() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='Thank you so much'\nmeaning='x'\nsituation='Ajudaram você.'\nshort='Thanks!'"
        ))
        .unwrap();
        let p = &d.phrases[0];
        assert_eq!(p.cue_for(Answer::Short)[1], Segment::Target("Thanks!".into()));
        assert_eq!(p.cue_for(Answer::All), p.cue, "all reads the complete one");
        assert_eq!(p.cue_for(Answer::Polished), p.cue, "no polished tier: the complete one");
    }

    #[test]
    fn empty_and_duplicate_tiers_collapse() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='Thanks a lot.'\nmeaning='x'\nshort='  '\npolished='thanks a lot'\n\
             [[phrase]]\nsay='Thank you'\nmeaning='x'\nshort='Thanks'\npolished='THANKS!'"
        ))
        .unwrap();
        assert_eq!((d.phrases[0].short.as_deref(), d.phrases[0].polished.as_deref()), (None, None));
        assert_eq!(d.phrases[0].tiers().len(), 1);
        assert_eq!(d.phrases[1].short.as_deref(), Some("Thanks"));
        assert_eq!(d.phrases[1].polished, None, "equal to short once case and punctuation go");
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
        let recall = d.phrases[0].recall_cue(&d.language_name, Native::PtBr);
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
        let answers: Vec<&str> = d.phrases[0].answers().into_iter().map(|(_, a)| a).collect();
        assert_eq!(answers, vec!["Could you repeat that?", "Can you repeat that?"]);
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
    fn pre_a1_is_below_a1_and_keeps_beginners_to_their_own_items() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='Hello.'\nmeaning='x'\nlevel='pre-a1'\n\
             [[phrase]]\nsay='Good morning.'\nmeaning='x'\nlevel='A1'\n\
             [[phrase]]\nsay='Where is the station?'\nmeaning='x'"
        ))
        .unwrap();
        assert_eq!(LEVELS[0], PRE_A1);
        assert_eq!(d.phrases[0].level.as_deref(), Some(PRE_A1), "any case is read as PRE-A1");
        assert_eq!(d.selection(&[], PRE_A1), vec![0], "neither A1 nor unleveled phrases for someone who knows nothing");
        assert_eq!(d.selection(&[], "A1"), vec![0, 1, 2], "A1 and up still take pre-A1 and unleveled phrases");
    }

    #[test]
    fn topics_at_a_level_are_those_with_something_to_practice_there() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\ntopic='viagem'\nlevel='PRE-A1'\n\
             [[phrase]]\nsay='b'\nmeaning='x'\ntopic='trabalho'\nlevel='B1'\n\
             [[phrase]]\nsay='c'\nmeaning='x'\ntopic='compras'\nlevel='A1'"
        ))
        .unwrap();
        assert_eq!(d.topics_at(PRE_A1), vec!["viagem"]);
        assert_eq!(d.topics_at("A1"), vec!["viagem", "compras"]);
        assert_eq!(d.topics_at("C2"), d.topics(), "every topic once nothing is filtered out");
    }

    #[test]
    fn selection_filters_by_topic_and_level() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\ntopic='viagem'\nlevel='A1'\n\
             [[phrase]]\nsay='b'\nmeaning='x'\ntopic='viagem'\nlevel='B2'\n\
             [[phrase]]\nsay='c'\nmeaning='x'\ntopic='trabalho'"
        ))
        .unwrap();
        assert_eq!(d.selection(&[], "C2"), vec![0, 1, 2]);
        assert_eq!(d.selection(&[], "A2"), vec![0, 2], "unleveled phrases are always in");
        assert_eq!(d.selection(&["viagem".into()], "C2"), vec![0, 1]);
        assert!(d.selection(&["nada".into()], "C2").is_empty());
    }

    #[test]
    fn selection_takes_every_ticked_topic_at_once() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\ntopic='viagem'\nlevel='A1'\n\
             [[phrase]]\nsay='b'\nmeaning='x'\ntopic='comida'\nlevel='A1'\n\
             [[phrase]]\nsay='c'\nmeaning='x'\ntopic='trabalho'\nlevel='A1'\n\
             [[phrase]]\nsay='d'\nmeaning='x'\ntopic='trabalho'\nlevel='B2'"
        ))
        .unwrap();
        let ticked = ["trabalho".to_string(), "viagem".to_string()];
        assert_eq!(d.selection(&ticked, "C2"), vec![0, 2, 3], "work + travel, never food");
        assert_eq!(d.selection(&ticked, "A1"), vec![0, 2], "the level still applies to each topic");
        assert_eq!(d.selection(&[], "A1"), vec![0, 1, 2], "nothing ticked = every topic");
        assert_eq!(d.selection(&["nada".into(), "viagem".into()], "C2"), vec![0], "an unknown topic adds nothing");
    }

    #[test]
    fn topic_counts_say_how_many_items_each_topic_has_at_the_level() {
        let d = Deck::parse(&format!(
            "{HEAD}[[phrase]]\nsay='a'\nmeaning='x'\ntopic='viagem'\nlevel='PRE-A1'\n\
             [[phrase]]\nsay='b'\nmeaning='x'\ntopic='trabalho'\nlevel='B1'\n\
             [[phrase]]\nsay='c'\nmeaning='x'\ntopic='viagem'\nlevel='A1'\n\
             [[phrase]]\nsay='d'\nmeaning='x'\ntopic='trabalho'\nlevel='A1'"
        ))
        .unwrap();
        assert_eq!(d.topic_counts(PRE_A1), vec![("viagem".to_string(), 1)]);
        assert_eq!(d.topic_counts("A1"), vec![("viagem".to_string(), 2), ("trabalho".to_string(), 1)]);
        assert_eq!(d.topic_counts("C2"), vec![("viagem".to_string(), 2), ("trabalho".to_string(), 2)]);
    }

    #[test]
    fn builtin_decks_start_from_single_words_and_short_chunks() {
        use crate::learn::path::Stage;
        for lang in PT_BR_DECKS {
            let d = Deck::builtin(lang).unwrap();
            let count = |st: Stage| d.phrases.iter().filter(|p| Stage::of(p) == st).count();
            assert!(count(Stage::Words) >= 40, "{lang}: {} first words", count(Stage::Words));
            assert!(count(Stage::Chunks) >= 40, "{lang}: {} short chunks", count(Stage::Chunks));
            let first = crate::learn::path::ordered(&d.phrases, &d.selection(&[], "C2"))[0];
            assert_eq!(Stage::of(&d.phrases[first]), Stage::Words, "{lang} path starts with a word");
        }
    }

    #[test]
    fn builtin_decks_cover_beginner_to_advanced() {
        let d = Deck::builtin("en").unwrap();
        assert!(d.selection(&[], "A1").len() >= 40);
        assert!(d.selection(&[], "A1").len() < d.selection(&[], "B2").len());
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
        let d = Deck::load("en", Native::PtBr, dir.path()).unwrap();
        assert_eq!(d.phrases[0].say, "Custom");
        assert_eq!(Deck::load("es", Native::PtBr, dir.path()).unwrap().language, "es");
    }

    #[test]
    fn unknown_language_without_custom_file_explains_how_to_fix() {
        let dir = tempfile::tempdir().unwrap();
        let err = format!("{:#}", Deck::load("fr", Native::PtBr, dir.path()).unwrap_err());
        assert!(err.contains("fr.toml"), "{err}");
    }
}
