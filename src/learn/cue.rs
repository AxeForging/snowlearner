//! Lesson cues mix the learner's native language with the target language:
//! `Para dizer que estou com fome, devo dizer: {{I'm hungry}}`.
//! `{{…}}` marks target-language text and `{}` is shorthand for the phrase itself.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Native(String),
    Target(String),
}

impl Segment {
    pub fn text(&self) -> &str {
        match self {
            Segment::Native(t) | Segment::Target(t) => t,
        }
    }

    pub fn is_target(&self) -> bool {
        matches!(self, Segment::Target(_))
    }
}

/// Parses a cue into ordered segments. A cue that never mentions the target
/// gets the phrase appended, so every lesson ends up asking for it.
pub fn parse(cue: &str, say: &str) -> Result<Vec<Segment>> {
    let mut out = Vec::new();
    let mut native = String::new();
    let mut rest = cue;

    let flush = |native: &mut String, out: &mut Vec<Segment>| {
        let t = native.trim();
        if !t.is_empty() {
            out.push(Segment::Native(t.to_string()));
        }
        native.clear();
    };

    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("{{") {
            let Some(end) = after.find("}}") else {
                bail!("unclosed '{{{{' in cue: {cue:?}");
            };
            let inner = after[..end].trim();
            if inner.is_empty() {
                bail!("empty '{{{{}}}}' in cue: {cue:?}");
            }
            flush(&mut native, &mut out);
            out.push(Segment::Target(inner.to_string()));
            rest = &after[end + 2..];
        } else if let Some(after) = rest.strip_prefix("{}") {
            flush(&mut native, &mut out);
            out.push(Segment::Target(say.trim().to_string()));
            rest = after;
        } else if rest.starts_with("}}") {
            bail!("stray '}}}}' in cue: {cue:?}");
        } else {
            let ch = rest.chars().next().unwrap();
            native.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    flush(&mut native, &mut out);

    if !out.iter().any(Segment::is_target) {
        out.push(Segment::Target(say.trim().to_string()));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Segment::*;

    #[test]
    fn splits_native_text_and_marked_target() {
        let s = parse("Para dizer que estou com fome, devo dizer: {{I'm hungry}}", "I'm hungry").unwrap();
        assert_eq!(s, vec![Native("Para dizer que estou com fome, devo dizer:".into()), Target("I'm hungry".into())]);
    }

    #[test]
    fn empty_braces_insert_the_phrase() {
        let s = parse("Repita: {} — agora você!", "Good morning").unwrap();
        assert_eq!(s, vec![Native("Repita:".into()), Target("Good morning".into()), Native("— agora você!".into())]);
    }

    #[test]
    fn multiple_target_parts_keep_their_order() {
        let s = parse("{{cold}} é frio e {{hot}} é quente", "cold").unwrap();
        assert_eq!(
            s,
            vec![Target("cold".into()), Native("é frio e".into()), Target("hot".into()), Native("é quente".into())]
        );
    }

    #[test]
    fn cue_without_target_gets_the_phrase_appended() {
        let s = parse("Como se diz obrigado?", "Thank you").unwrap();
        assert_eq!(s.last(), Some(&Target("Thank you".into())));
    }

    #[test]
    fn unbalanced_or_empty_markers_are_rejected() {
        assert!(parse("diga {{hello", "hello").is_err());
        assert!(parse("diga hello}}", "hello").is_err());
        assert!(parse("diga {{  }}", "hello").is_err());
    }

    #[test]
    fn multibyte_native_text_survives_parsing() {
        let s = parse("Ação! Não é? {}", "Action").unwrap();
        assert_eq!(s[0], Native("Ação! Não é?".into()));
    }
}
