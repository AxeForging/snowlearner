//! The learner's own language: what every caption, line and meaning is
//! written in, and what the native-language voice reads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum Native {
    /// Brazilian Portuguese.
    #[default]
    #[serde(rename = "pt-BR", alias = "pt-br", alias = "pt")]
    PtBr,
    #[serde(rename = "en")]
    En,
}

impl Native {
    pub const ALL: [Native; 2] = [Native::PtBr, Native::En];

    /// Language code for decks, voices and the recognizer ("pt-BR", "en").
    pub fn code(self) -> &'static str {
        match self {
            Native::PtBr => "pt-BR",
            Native::En => "en",
        }
    }

    /// Reads a code in any case and with or without region ("pt", "PT-br").
    pub fn parse(code: &str) -> Option<Native> {
        Native::ALL.into_iter().find(|n| n.is(code))
    }

    /// Whether `lang` (a deck or voice code) is this language: "pt" and
    /// "pt-BR" are both Portuguese, so nobody learns their own language.
    pub fn is(self, lang: &str) -> bool {
        base(lang) == base(self.code())
    }
}

/// "pt-BR" → "pt", "en_US" → "en".
fn base(lang: &str) -> String {
    lang.trim().split(['-', '_']).next().unwrap_or_default().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_and_regions_or_case_do_not_matter() {
        for n in Native::ALL {
            assert_eq!(Native::parse(n.code()), Some(n));
        }
        assert_eq!(Native::parse("pt"), Some(Native::PtBr));
        assert_eq!(Native::parse("PT_br"), Some(Native::PtBr));
        assert_eq!(Native::parse("en-US"), Some(Native::En));
        assert_eq!(Native::parse("es"), None);
        assert_eq!(Native::parse(""), None);
    }

    #[test]
    fn a_language_is_your_own_whatever_its_region() {
        assert!(Native::PtBr.is("pt-BR") && Native::PtBr.is("pt"));
        assert!(Native::En.is("en") && Native::En.is("en-GB"));
        assert!(!Native::En.is("es") && !Native::PtBr.is("en"));
    }
}
