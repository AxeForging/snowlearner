//! Which engine reads the lessons aloud:
//! - `system`: the OS voices (see `tts.rs`),
//! - `http`: any OpenAI-compatible speech server — Kokoro-FastAPI, etc. —
//!   `POST {url}/audio/speech` returning WAV, played on the chosen speaker,
//! - `command`: your own command (Piper, a script…), run without a shell.

use super::tts::{Tts, command_no_window, locale};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::process::Stdio;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum TtsEngine {
    #[default]
    System,
    Http,
    Command,
}

/// Kokoro voice ids start with a language letter: a/b English, e Spanish, p pt-BR…
const KOKORO_PREFIX: &[(&str, &[char])] =
    &[("en", &['a', 'b']), ("es", &['e']), ("pt", &['p']), ("fr", &['f']), ("it", &['i']), ("ja", &['j'])];

/// Sensible Kokoro voice when none is configured.
pub fn kokoro_default(lang: &str) -> &'static str {
    match locale(lang).split('-').next().unwrap_or("") {
        "pt" => "pf_dora",
        "es" => "ef_dora",
        "fr" => "ff_siwis",
        "it" => "if_sara",
        _ => "af_heart",
    }
}

/// Keeps voices whose Kokoro prefix matches `lang`; lists without the Kokoro
/// naming scheme are returned untouched.
pub fn filter_kokoro(voices: Vec<String>, lang: &str) -> Vec<String> {
    let base = locale(lang).split('-').next().unwrap_or("").to_string();
    let kokoro_style = voices.iter().all(|v| v.len() > 3 && v.as_bytes()[2] == b'_');
    let Some((_, letters)) = KOKORO_PREFIX.iter().find(|(l, _)| *l == base) else { return voices };
    if !kokoro_style {
        return voices;
    }
    voices.into_iter().filter(|v| v.chars().next().is_some_and(|c| letters.contains(&c))).collect()
}

/// Parses `GET /audio/voices`: `{"voices": [...]}` or a plain array.
pub fn parse_voice_list(body: &str) -> Result<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(body).context("voices response is not JSON")?;
    let arr = v.get("voices").unwrap_or(&v).as_array().context("voices response has no list")?;
    Ok(arr
        .iter()
        .filter_map(|x| {
            x.as_str().map(str::to_string).or_else(|| x.get("id").and_then(|i| i.as_str()).map(str::to_string))
        })
        .collect())
}

/// Splits a command template into argv, honouring quotes — never via a shell.
pub fn split_template(t: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in t.chars() {
        match (quote, c) {
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (_, c) => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}

/// Fills `{text}` `{lang}` `{voice}` `{out}` in each argument (as whole values, so
/// the text can never inject extra arguments).
pub fn fill(args: &[String], text: &str, lang: &str, voice: &str, out: &str) -> Vec<String> {
    args.iter()
        .map(|a| a.replace("{text}", text).replace("{lang}", lang).replace("{voice}", voice).replace("{out}", out))
        .collect()
}

pub struct Http {
    pub url: String,
    pub model: String,
}

impl Http {
    fn base(&self) -> String {
        self.url.trim_end_matches('/').to_string()
    }

    /// Asks the server for WAV audio of `text`.
    pub fn synthesize(&self, text: &str, lang: &str, voice: &str, slow: bool) -> Result<Vec<u8>> {
        let voice = if voice.is_empty() { kokoro_default(lang) } else { voice };
        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": voice,
            "response_format": "wav",
            "speed": if slow { 0.85 } else { 1.0 },
        });
        let url = format!("{}/audio/speech", self.base());
        let mut resp =
            with_agents(&url, |a| a.post(&url).header("Content-Type", "application/json").send(body.to_string()))
                .with_context(|| format!("TTS server at {url} did not answer — is it running?"))?;
        let bytes = resp.body_mut().with_config().limit(50_000_000).read_to_vec()?;
        if bytes.is_empty() {
            bail!("TTS server at {url} answered with no audio for voice {voice:?} — its log says why");
        }
        Ok(bytes)
    }

    pub fn voices(&self, lang: &str) -> Result<Vec<String>> {
        let url = format!("{}/audio/voices", self.base());
        let mut resp = with_agents(&url, |a| a.get(&url).call()).with_context(|| format!("listing voices at {url}"))?;
        let body = resp.body_mut().read_to_string()?;
        Ok(filter_kokoro(parse_voice_list(&body)?, lang))
    }
}

/// Longest wait for a TTS server to accept the connection; ureq alone waits
/// for the OS timeout (21 s on Windows) on each address that never answers.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// `localhost` resolves to `::1` first, but local TTS servers (Docker, WSL)
/// often listen on IPv4 only, and on Windows `::1` then hangs for 21 s
/// instead of refusing. Such URLs try IPv4 first.
fn ipv4_first(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
    host.eq_ignore_ascii_case("localhost")
}

/// Sends through shared agents (they keep connections open between lines),
/// moving to the next one only when connecting failed, so a server on IPv6
/// loopback alone still answers after the IPv4 try.
fn with_agents<T>(url: &str, mut send: impl FnMut(&ureq::Agent) -> Result<T, ureq::Error>) -> Result<T, ureq::Error> {
    use std::sync::OnceLock;
    use ureq::config::IpFamily;
    static ANY: OnceLock<ureq::Agent> = OnceLock::new();
    static IPV4: OnceLock<ureq::Agent> = OnceLock::new();
    let make =
        |family| ureq::Agent::config_builder().timeout_connect(Some(CONNECT_TIMEOUT)).ip_family(family).build().into();
    let any = ANY.get_or_init(|| make(IpFamily::Any));
    let agents = if ipv4_first(url) { vec![IPV4.get_or_init(|| make(IpFamily::Ipv4Only)), any] } else { vec![any] };
    let mut last = None;
    for agent in agents {
        match send(agent) {
            Err(
                e @ (ureq::Error::Io(_)
                | ureq::Error::ConnectionFailed
                | ureq::Error::HostNotFound
                | ureq::Error::Timeout(_)),
            ) => last = Some(e),
            done => return done,
        }
    }
    Err(last.unwrap_or(ureq::Error::ConnectionFailed))
}

/// What an engine produced for one utterance.
pub enum Audio {
    /// The engine already played it (e.g. OS voices speak by themselves).
    Played,
    /// WAV bytes for the app to play on the chosen speaker.
    Wav(Vec<u8>),
}

/// A text-to-speech engine. Implement this to add a new one (Piper, Coqui,
/// a cloud API…) and register it in [`Voice::new`]; playback, device choice and
/// the panel/CLI voice pickers come for free.
pub trait SpeechEngine: Send {
    /// Human-readable description for `doctor` and the panel.
    fn describe(&self) -> String;
    /// Whether it can speak at all on this machine/config.
    fn available(&self) -> bool;
    /// Voice names for a language (empty = the engine picks by language).
    fn voices(&self, lang: &str) -> Result<Vec<String>>;
    /// Synthesize (or directly speak) `text`. `slow` = the learner's target language.
    fn speak(&self, text: &str, lang: &str, voice: &str, slow: bool) -> Result<Audio>;
}

impl SpeechEngine for Tts {
    fn describe(&self) -> String {
        self.engine().map(|e| e.name().to_string()).unwrap_or_else(|| "none found".into())
    }
    fn available(&self) -> bool {
        self.engine().is_some()
    }
    fn voices(&self, lang: &str) -> Result<Vec<String>> {
        Ok(Tts::voices(self, lang))
    }
    fn speak(&self, text: &str, lang: &str, voice: &str, slow: bool) -> Result<Audio> {
        Tts::speak(self, text, lang, voice, slow)?;
        Ok(Audio::Played)
    }
}

impl SpeechEngine for Http {
    fn describe(&self) -> String {
        format!("HTTP {} ({})", self.url, self.model)
    }
    fn available(&self) -> bool {
        !self.url.is_empty()
    }
    fn voices(&self, lang: &str) -> Result<Vec<String>> {
        Http::voices(self, lang)
    }
    fn speak(&self, text: &str, lang: &str, voice: &str, slow: bool) -> Result<Audio> {
        Ok(Audio::Wav(self.synthesize(text, lang, voice, slow)?))
    }
}

/// Runs a user command per utterance (see `split_template` / `fill`).
pub struct CommandEngine {
    pub args: Vec<String>,
}

impl SpeechEngine for CommandEngine {
    fn describe(&self) -> String {
        format!("command: {}", self.args.join(" "))
    }
    fn available(&self) -> bool {
        !self.args.is_empty()
    }
    fn voices(&self, _lang: &str) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    fn speak(&self, text: &str, lang: &str, voice: &str, _slow: bool) -> Result<Audio> {
        let Some((prog, rest)) = self.args.split_first() else { bail!("`tts_command` is empty") };
        let out = tempfile_dir().join(format!("snowlearner-tts-{}.wav", std::process::id()));
        let out_s = out.to_string_lossy().to_string();
        let wants_out = self.args.iter().any(|a| a.contains("{out}"));
        let text_in_args = self.args.iter().any(|a| a.contains("{text}"));
        let mut cmd = command_no_window(fill(std::slice::from_ref(prog), text, lang, voice, &out_s).remove(0));
        cmd.args(fill(rest, text, lang, voice, &out_s)).stdout(Stdio::null()).stderr(Stdio::null());
        if !text_in_args {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().with_context(|| format!("running `{prog}`"))?;
        if !text_in_args && let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(text.as_bytes());
        }
        let status = child.wait()?;
        if !status.success() {
            bail!("`{prog}` exited with {status}");
        }
        if wants_out {
            let wav = std::fs::read(&out).with_context(|| format!("`{prog}` did not write {out_s}"))?;
            let _ = std::fs::remove_file(&out);
            return Ok(Audio::Wav(wav));
        }
        Ok(Audio::Played)
    }
}

/// The configured engine plus where its audio goes.
pub struct Voice {
    pub engine: Box<dyn SpeechEngine>,
    /// Output device for audio the app plays itself ("" = default).
    pub speaker: String,
}

impl Voice {
    /// Engine factory: add new engines here.
    pub fn new(engine: TtsEngine, url: &str, model: &str, command: &str, speaker: &str) -> Voice {
        let engine: Box<dyn SpeechEngine> = match engine {
            TtsEngine::System => Box::new(Tts::detect()),
            TtsEngine::Http => Box::new(Http { url: url.to_string(), model: model.to_string() }),
            TtsEngine::Command => Box::new(CommandEngine { args: split_template(command) }),
        };
        Voice { engine, speaker: speaker.to_string() }
    }

    pub fn describe(&self) -> String {
        self.engine.describe()
    }

    pub fn available(&self) -> bool {
        self.engine.available()
    }

    pub fn voices(&self, lang: &str) -> Result<Vec<String>> {
        self.engine.voices(lang)
    }

    /// Speaks and blocks until done.
    pub fn speak(&self, text: &str, lang: &str, voice: &str, slow: bool) -> Result<()> {
        match self.engine.speak(text, lang, voice, slow)? {
            Audio::Played => Ok(()),
            Audio::Wav(wav) => self.play(&wav),
        }
    }

    #[cfg(feature = "audio")]
    fn play(&self, wav: &[u8]) -> Result<()> {
        super::audio::play_wav(wav, &self.speaker)
    }

    #[cfg(not(feature = "audio"))]
    fn play(&self, _wav: &[u8]) -> Result<()> {
        bail!("this build has no audio playback (enable the `audio` feature)")
    }
}

fn tempfile_dir() -> std::path::PathBuf {
    std::env::temp_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read};
    use std::net::TcpListener;

    #[test]
    fn kokoro_voices_are_filtered_by_language_prefix() {
        let all: Vec<String> =
            ["af_heart", "bm_george", "ef_dora", "pf_dora", "pm_alex"].iter().map(|s| s.to_string()).collect();
        assert_eq!(filter_kokoro(all.clone(), "pt-BR"), vec!["pf_dora", "pm_alex"]);
        assert_eq!(filter_kokoro(all.clone(), "en"), vec!["af_heart", "bm_george"]);
        assert_eq!(filter_kokoro(all, "es"), vec!["ef_dora"]);
        let other = vec!["alloy".to_string(), "nova".to_string()];
        assert_eq!(filter_kokoro(other.clone(), "pt-BR"), other, "non-Kokoro servers keep every voice");
    }

    #[test]
    fn default_kokoro_voices_speak_the_right_language() {
        assert_eq!(kokoro_default("pt-BR"), "pf_dora");
        assert_eq!(kokoro_default("es"), "ef_dora");
        assert_eq!(kokoro_default("en"), "af_heart");
    }

    #[test]
    fn voice_lists_parse_in_both_shapes() {
        assert_eq!(parse_voice_list(r#"{"voices":["af_heart","pf_dora"]}"#).unwrap(), vec!["af_heart", "pf_dora"]);
        assert_eq!(parse_voice_list(r#"[{"id":"x"},{"id":"y"}]"#).unwrap(), vec!["x", "y"]);
        assert!(parse_voice_list("<html>").is_err());
    }

    #[test]
    fn templates_split_with_quotes_and_fill_without_injection() {
        let args = split_template(r#"piper --model "/models/pt BR.onnx" --output_file {out}"#);
        assert_eq!(args, vec!["piper", "--model", "/models/pt BR.onnx", "--output_file", "{out}"]);
        let evil = "\"; rm -rf ~ #";
        let filled = fill(&split_template("say-it --text {text}"), evil, "en", "", "/tmp/x.wav");
        assert_eq!(filled, vec!["say-it", "--text", evil], "the text stays one argument");
    }

    /// Any type can be an engine: this is the whole extension contract.
    struct Fake;
    impl SpeechEngine for Fake {
        fn describe(&self) -> String {
            "fake".into()
        }
        fn available(&self) -> bool {
            true
        }
        fn voices(&self, lang: &str) -> Result<Vec<String>> {
            Ok(vec![format!("{lang}-voice")])
        }
        fn speak(&self, _: &str, _: &str, _: &str, _: bool) -> Result<Audio> {
            Ok(Audio::Played)
        }
    }

    #[test]
    fn custom_engines_plug_in_through_the_trait() {
        let v = Voice { engine: Box::new(Fake), speaker: String::new() };
        assert_eq!(v.describe(), "fake");
        assert_eq!(v.voices("es").unwrap(), vec!["es-voice"]);
        v.speak("hola", "es", "", false).unwrap();
    }

    #[test]
    fn command_engine_passes_text_on_stdin_when_not_in_the_template() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("got.txt");
        let v = Voice::new(TtsEngine::Command, "", "", &format!("tee {}", out.display()), "");
        v.speak("Olá mundo", "pt-BR", "", false).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "Olá mundo");
    }

    /// A tiny local stand-in for Kokoro-FastAPI.
    fn fake_server(reply_voices: &'static str) -> (String, std::thread::JoinHandle<String>) {
        fake_server_with(reply_voices, b"RIFFfake")
    }

    fn fake_server_with(
        reply_voices: &'static str,
        reply_audio: &'static [u8],
    ) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line.to_lowercase().starts_with("content-length:") {
                    len = line[15..].trim().parse().unwrap();
                }
                if line == "\r\n" {
                    break;
                }
                head.push_str(&line);
            }
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let payload: Vec<u8> =
                if head.starts_with("GET") { reply_voices.as_bytes().to_vec() } else { reply_audio.to_vec() };
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", payload.len());
            s.write_all(resp.as_bytes()).unwrap();
            s.write_all(&payload).unwrap();
            format!("{head}\n{}", String::from_utf8_lossy(&body))
        });
        (url, handle)
    }

    #[test]
    fn http_engine_posts_an_openai_style_request_with_the_kokoro_voice() {
        let (url, server) = fake_server("");
        let h = Http { url, model: "kokoro".into() };
        let wav = h.synthesize("Olá", "pt-BR", "", true).unwrap();
        assert_eq!(wav, b"RIFFfake");
        let req = server.join().unwrap();
        assert!(req.starts_with("POST /v1/audio/speech"), "{req}");
        assert!(req.contains(r#""voice":"pf_dora""#) && req.contains(r#""input":"Olá""#), "{req}");
        assert!(req.contains(r#""response_format":"wav""#));
        // Kokoro wants one-letter codes ("p", "a"): "pt"/"en" made it answer
        // 200 with no audio. Left out, it takes the language from the voice.
        assert!(!req.contains("lang_code"), "{req}");
    }

    #[test]
    fn a_tts_server_answering_without_audio_says_so() {
        let (url, server) = fake_server_with("", b"");
        let h = Http { url, model: "kokoro".into() };
        let err = format!("{:#}", h.synthesize("oi", "en", "af_heart", false).unwrap_err());
        server.join().unwrap();
        assert!(err.contains("no audio") && err.contains("log"), "{err}");
    }

    #[test]
    fn http_engine_lists_voices_for_a_language() {
        let (url, server) = fake_server(r#"{"voices":["af_heart","pf_dora","ef_dora"]}"#);
        let h = Http { url, model: "kokoro".into() };
        assert_eq!(h.voices("es").unwrap(), vec!["ef_dora"]);
        server.join().unwrap();
    }

    #[test]
    fn localhost_tries_ipv4_first_and_literal_addresses_are_left_alone() {
        assert!(ipv4_first("http://localhost:8880/v1"));
        assert!(ipv4_first("https://LocalHost/v1"));
        assert!(ipv4_first("http://localhost"));
        assert!(!ipv4_first("http://127.0.0.1:8880/v1"));
        assert!(!ipv4_first("http://[::1]:8880/v1"));
        assert!(!ipv4_first("http://kokoro.lan:8880/v1"));
        assert!(!ipv4_first("http://localhost.example.com/v1"));
    }

    #[test]
    fn a_server_on_ipv4_loopback_answers_under_localhost() {
        let (url, server) = fake_server(r#"{"voices":["af_heart","pf_dora"]}"#);
        let h = Http { url: url.replace("127.0.0.1", "localhost"), model: "kokoro".into() };
        assert_eq!(h.voices("pt-BR").unwrap(), vec!["pf_dora"]);
        server.join().unwrap();
    }

    #[test]
    fn a_server_only_on_ipv6_loopback_still_answers_under_localhost() {
        use std::net::ToSocketAddrs;
        let v6_localhost = ("localhost", 1).to_socket_addrs().is_ok_and(|mut a| a.any(|a| a.is_ipv6()));
        let Ok(listener) = TcpListener::bind("[::1]:0") else { return };
        if !v6_localhost {
            return; // this host's `localhost` never means ::1
        }
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            while reader.read_line(&mut line).unwrap() > 0 && !line.ends_with("\r\n\r\n") {}
            let body = r#"{"voices":["ef_dora"]}"#;
            write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        let h = Http { url: format!("http://localhost:{port}/v1"), model: "kokoro".into() };
        assert_eq!(h.voices("es").unwrap(), vec!["ef_dora"]);
        server.join().unwrap();
    }

    #[test]
    fn a_dead_tts_server_gives_an_actionable_error() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let h = Http { url: format!("http://127.0.0.1:{port}/v1"), model: "kokoro".into() };
        let err = format!("{:#}", h.synthesize("oi", "pt-BR", "", false).unwrap_err());
        assert!(err.contains("is it running"), "{err}");
    }
}
