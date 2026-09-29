//! Text-to-speech through each OS's own voices, by invoking the platform's
//! speech command — no model, no network, nothing to link:
//! Linux `spd-say` (speech-dispatcher) or `espeak-ng`, macOS `say`,
//! Windows PowerShell + System.Speech (SAPI).

use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    SpdSay,
    EspeakNg,
    Say,
    PowerShell,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::SpdSay => "speech-dispatcher (spd-say)",
            Engine::EspeakNg => "espeak-ng",
            Engine::Say => "macOS say",
            Engine::PowerShell => "Windows SAPI (PowerShell)",
        }
    }
}

/// A fully resolved command line, separate from running it so it is testable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// Region-qualified locale for a deck language code ("en" → "en-US").
pub fn locale(lang: &str) -> String {
    match lang.to_ascii_lowercase().as_str() {
        "en" => "en-US".into(),
        "es" => "es-ES".into(),
        "pt" | "pt-br" => "pt-BR".into(),
        "fr" => "fr-FR".into(),
        "de" => "de-DE".into(),
        "it" => "it-IT".into(),
        _ => lang.to_string(),
    }
}

const PS_SCRIPT: &str = "Add-Type -AssemblyName System.Speech; \
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
if ($env:SL_VOICE) { $s.SelectVoice($env:SL_VOICE) } else { \
$v = $s.GetInstalledVoices() | Where-Object { $_.VoiceInfo.Culture.Name -like ($env:SL_LANG + '*') } | Select-Object -First 1; \
if ($v) { $s.SelectVoice($v.VoiceInfo.Name) } }; \
$s.Rate = [int]$env:SL_RATE; $s.Speak($env:SL_TEXT)";

/// How fast a line is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    /// Portuguese cues, recaps.
    #[default]
    Normal,
    /// The learner's target language: a little slower.
    Slow,
    /// The target language for a pre-A1 learner, who knows no English yet.
    Slower,
}

pub fn command(
    engine: Engine,
    text: &str,
    lang: &str,
    voice: &str,
    speed: Speed,
    mac_voices: &[(String, String)],
) -> Cmd {
    let loc = locale(lang);
    let mut args: Vec<String> = Vec::new();
    let mut env = Vec::new();
    let program = match engine {
        Engine::SpdSay => {
            args.extend(["-w".into(), "-l".into(), loc]);
            match speed {
                Speed::Normal => {}
                Speed::Slow => args.extend(["-r".into(), "-20".into()]),
                Speed::Slower => args.extend(["-r".into(), "-45".into()]),
            }
            if !voice.is_empty() {
                args.extend(["-y".into(), voice.into()]);
            }
            args.extend(["--".into(), text.into()]);
            "spd-say"
        }
        Engine::EspeakNg => {
            let v = if voice.is_empty() { loc.to_ascii_lowercase() } else { voice.to_string() };
            let v = if v == "es-es" { "es".to_string() } else { v };
            args.extend([
                "-v".into(),
                v,
                "-s".into(),
                match speed {
                    Speed::Normal => "165",
                    Speed::Slow => "135",
                    Speed::Slower => "110",
                }
                .into(),
                "--".into(),
                text.into(),
            ]);
            "espeak-ng"
        }
        Engine::Say => {
            let chosen = if voice.is_empty() { mac_voice_for(&loc, mac_voices) } else { Some(voice.to_string()) };
            if let Some(v) = chosen {
                args.extend(["-v".into(), v]);
            }
            match speed {
                Speed::Normal => {}
                Speed::Slow => args.extend(["-r".into(), "150".into()]),
                Speed::Slower => args.extend(["-r".into(), "120".into()]),
            }
            args.extend(["--".into(), text.into()]);
            "say"
        }
        Engine::PowerShell => {
            args.extend(["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), PS_SCRIPT.into()]);
            env.push(("SL_TEXT".into(), text.into()));
            env.push(("SL_LANG".into(), loc));
            env.push(("SL_VOICE".into(), voice.into()));
            let rate = match speed {
                Speed::Normal => "0",
                Speed::Slow => "-2",
                Speed::Slower => "-4",
            };
            env.push(("SL_RATE".into(), rate.into()));
            "powershell"
        }
    };
    Cmd { program: program.into(), args, env }
}

/// Parses `say -v '?'` lines: "Luciana   pt_BR   # Olá, meu nome é Luciana".
pub fn parse_mac_voices(listing: &str) -> Vec<(String, String)> {
    listing
        .lines()
        .filter_map(|l| {
            let head = l.split('#').next()?.trim_end();
            let mut parts = head.rsplitn(2, char::is_whitespace);
            let loc = parts.next()?.trim().replace('_', "-");
            let name = parts.next()?.trim().to_string();
            (!name.is_empty() && loc.contains('-')).then_some((name, loc))
        })
        .collect()
}

fn mac_voice_for(loc: &str, voices: &[(String, String)]) -> Option<String> {
    let lang = loc.split('-').next().unwrap_or(loc).to_ascii_lowercase();
    voices
        .iter()
        .find(|(_, l)| l.eq_ignore_ascii_case(loc))
        .or_else(|| voices.iter().find(|(_, l)| l.to_ascii_lowercase().starts_with(&lang)))
        .map(|(n, _)| n.clone())
}

/// `program` as a child with no console window of its own. Without it,
/// Windows pops a terminal up for every line read aloud once the app has
/// let go of its console (started from the Start menu).
pub fn command_no_window(program: impl AsRef<std::ffi::OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub fn in_path(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    std::env::split_paths(&path)
        .find_map(|dir| exts.iter().map(|e| dir.join(format!("{program}{e}"))).find(|p| p.is_file()))
}

pub struct Tts {
    engine: Option<Engine>,
    mac_voices: Vec<(String, String)>,
}

impl Tts {
    pub fn detect() -> Tts {
        let engine = if cfg!(windows) {
            in_path("powershell").map(|_| Engine::PowerShell)
        } else if cfg!(target_os = "macos") {
            in_path("say").map(|_| Engine::Say)
        } else if in_path("spd-say").is_some() {
            Some(Engine::SpdSay)
        } else {
            in_path("espeak-ng").map(|_| Engine::EspeakNg)
        };
        let mac_voices = if engine == Some(Engine::Say) {
            Command::new("say")
                .args(["-v", "?"])
                .output()
                .map(|o| parse_mac_voices(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Tts { engine, mac_voices }
    }

    /// Voices this engine offers for `lang` (names to put in `voice_*`).
    pub fn voices(&self, lang: &str) -> Vec<String> {
        let loc = locale(lang);
        let run = |prog: &str, args: &[&str]| {
            command_no_window(prog)
                .args(args)
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                .unwrap_or_default()
        };
        match self.engine {
            Some(Engine::SpdSay) => parse_spd_voices(&run("spd-say", &["-L"]), &loc),
            Some(Engine::EspeakNg) => parse_espeak_voices(&run("espeak-ng", &["--voices"]), &loc),
            Some(Engine::Say) => voices_for_locale(&self.mac_voices, &loc),
            Some(Engine::PowerShell) => {
                let script = "Add-Type -AssemblyName System.Speech; \
                    (New-Object System.Speech.Synthesis.SpeechSynthesizer).GetInstalledVoices() | \
                    ForEach-Object { $_.VoiceInfo.Name + '|' + $_.VoiceInfo.Culture.Name }";
                let out = run("powershell", &["-NoProfile", "-NonInteractive", "-Command", script]);
                let pairs: Vec<(String, String)> = out
                    .lines()
                    .filter_map(|l| l.split_once('|').map(|(n, c)| (n.trim().to_string(), c.trim().to_string())))
                    .collect();
                voices_for_locale(&pairs, &loc)
            }
            None => Vec::new(),
        }
    }

    pub fn engine(&self) -> Option<Engine> {
        self.engine
    }

    /// Speaks and blocks until done.
    pub fn speak(&self, text: &str, lang: &str, voice: &str, speed: Speed) -> Result<()> {
        let Some(engine) = self.engine else {
            bail!("no text-to-speech engine found (install speech-dispatcher or espeak-ng)");
        };
        let cmd = command(engine, text, lang, voice, speed, &self.mac_voices);
        let status = command_no_window(&cmd.program)
            .args(&cmd.args)
            .envs(cmd.env.iter().map(|(k, v)| (k, v)))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .with_context(|| format!("running {}", cmd.program))?;
        if !status.success() {
            bail!("{} exited with {status}", cmd.program);
        }
        Ok(())
    }
}

fn lang_matches(candidate: &str, loc: &str) -> bool {
    let (c, l) = (candidate.to_ascii_lowercase().replace('_', "-"), loc.to_ascii_lowercase());
    let base = l.split('-').next().unwrap_or(&l).to_string();
    c == l || c == base || c.starts_with(&format!("{base}-"))
}

/// Prefers voices of the exact region (pt-BR over pt-PT) when there are any.
fn prefer_exact(rows: Vec<(String, String)>, loc: &str) -> Vec<String> {
    let exact: Vec<String> = rows
        .iter()
        .filter(|(_, l)| l.eq_ignore_ascii_case(loc) || l.replace('_', "-").eq_ignore_ascii_case(loc))
        .map(|(n, _)| n.clone())
        .collect();
    if !exact.is_empty() { exact } else { rows.into_iter().map(|(n, _)| n).collect() }
}

/// `spd-say -L` rows: "  English (America)      en-US      none" (columns split by 2+ spaces).
pub fn parse_spd_voices(listing: &str, loc: &str) -> Vec<String> {
    let rows = listing
        .lines()
        .filter_map(|l| {
            let cols: Vec<&str> = l.split("  ").map(str::trim).filter(|c| !c.is_empty()).collect();
            (cols.len() >= 2 && lang_matches(cols[1], loc)).then(|| (cols[0].to_string(), cols[1].to_string()))
        })
        .collect();
    prefer_exact(rows, loc)
}

/// `espeak-ng --voices` rows: "Pty Language Age/Gender VoiceName File Other".
pub fn parse_espeak_voices(listing: &str, loc: &str) -> Vec<String> {
    let mut out: Vec<String> = listing
        .lines()
        .skip(1)
        .filter_map(|l| {
            let cols: Vec<&str> = l.split_whitespace().collect();
            (cols.len() >= 4 && lang_matches(cols[1], loc)).then(|| cols[1].to_string())
        })
        .collect();
    out.dedup();
    out
}

fn voices_for_locale(pairs: &[(String, String)], loc: &str) -> Vec<String> {
    prefer_exact(pairs.iter().filter(|(_, l)| lang_matches(l, loc)).cloned().collect(), loc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spd_voice_list_is_filtered_by_language() {
        let listing = "NAME                                LANGUAGE                  VARIANT\n\
                       English (America)                   en-US                     none\n\
                       English (America)+Alex              en-US                     Alex\n\
                       Portuguese (Portugal)               pt-PT                     none\n\
                       Portuguese (Brazil)                 pt-BR                     none\n\
                       Spanish (Spain)                     es                        none\n";
        assert_eq!(parse_spd_voices(listing, "pt-BR"), vec!["Portuguese (Brazil)"]);
        assert_eq!(parse_spd_voices(listing, "en-US"), vec!["English (America)", "English (America)+Alex"]);
        assert_eq!(parse_spd_voices(listing, "es-ES"), vec!["Spanish (Spain)"]);
    }

    #[test]
    fn espeak_voice_list_is_filtered_by_language() {
        let listing = "Pty Language       Age/Gender VoiceName          File                 Other Languages\n \
                       2  en-us           --/M      English_(America)  gmw/en-US            (en 3)\n \
                       5  pt-br           --/M      Portuguese_(Brazil) roa/pt-BR           (pt 6)\n \
                       5  es              --/M      Spanish_(Spain)    roa/es\n";
        assert_eq!(parse_espeak_voices(listing, "pt-BR"), vec!["pt-br"]);
        assert_eq!(parse_espeak_voices(listing, "es-ES"), vec!["es"]);
    }

    #[test]
    fn deck_codes_become_region_locales() {
        assert_eq!(locale("en"), "en-US");
        assert_eq!(locale("pt-BR"), "pt-BR");
        assert_eq!(locale("es"), "es-ES");
        assert_eq!(locale("nl-BE"), "nl-BE");
    }

    #[test]
    fn spd_say_waits_and_slows_the_target_language() {
        let c = command(Engine::SpdSay, "I'm hungry", "en", "", Speed::Slow, &[]);
        assert_eq!(c.program, "spd-say");
        assert_eq!(c.args, vec!["-w", "-l", "en-US", "-r", "-20", "--", "I'm hungry"]);
    }

    #[test]
    fn every_engine_has_a_slower_pace_for_beginners() {
        let rate = |e, speed| command(e, "Hello.", "en", "", speed, &[]);
        assert_eq!(rate(Engine::SpdSay, Speed::Slower).args[3..5], ["-r", "-45"]);
        assert_eq!(rate(Engine::EspeakNg, Speed::Slower).args[3], "110");
        assert_eq!(rate(Engine::EspeakNg, Speed::Normal).args[3], "165");
        assert_eq!(rate(Engine::Say, Speed::Slower).args[0..2], ["-r", "120"]);
        let sapi = |speed| rate(Engine::PowerShell, speed).env.into_iter().find(|(k, _)| k == "SL_RATE").unwrap().1;
        assert_eq!(
            (sapi(Speed::Normal), sapi(Speed::Slow), sapi(Speed::Slower)),
            ("0".into(), "-2".into(), "-4".into())
        );
    }

    #[test]
    fn text_is_passed_as_one_argument_never_through_a_shell() {
        let evil = "\"; rm -rf ~; echo \"";
        for e in [Engine::SpdSay, Engine::EspeakNg, Engine::Say] {
            let c = command(e, evil, "en", "", Speed::Normal, &[]);
            assert_eq!(c.args.last().unwrap(), evil, "{e:?}");
        }
        let ps = command(Engine::PowerShell, evil, "en", "", Speed::Normal, &[]);
        assert!(!ps.args.iter().any(|a| a.contains(evil)), "PowerShell text must go through env");
        assert!(ps.env.contains(&("SL_TEXT".into(), evil.into())));
    }

    #[test]
    fn espeak_uses_its_own_voice_names() {
        assert_eq!(command(Engine::EspeakNg, "oi", "pt-BR", "", Speed::Normal, &[]).args[1], "pt-br");
        assert_eq!(command(Engine::EspeakNg, "hola", "es", "", Speed::Normal, &[]).args[1], "es");
    }

    #[test]
    fn mac_voice_listing_is_parsed_and_matched_by_locale() {
        let listing = "Alex                en_US    # Most people recognize me by my voice.\n\
                       Luciana             pt_BR    # Olá, meu nome é Luciana.\n\
                       Mónica              es_ES    # Hola, me llamo Mónica.\n\
                       Bad Line\n";
        let v = parse_mac_voices(listing);
        assert_eq!(v.len(), 3);
        let c = command(Engine::Say, "Oi", "pt-BR", "", Speed::Normal, &v);
        assert_eq!(&c.args[..2], &["-v", "Luciana"]);
        let c = command(Engine::Say, "Hola", "es", "", Speed::Normal, &v);
        assert_eq!(&c.args[..2], &["-v", "Mónica"]);
    }

    #[test]
    fn explicit_voice_wins() {
        let c = command(Engine::SpdSay, "hi", "en", "Alex", Speed::Normal, &[]);
        assert!(c.args.windows(2).any(|w| w == ["-y", "Alex"]));
    }
}
