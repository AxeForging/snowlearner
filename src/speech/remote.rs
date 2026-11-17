//! Speech recognition on a Whisper server the learner runs, instead of
//! whisper.cpp inside the app. Opt-in: with `stt_engine = "local"` (the
//! default) nothing leaves the machine; otherwise each answer's audio goes to
//! `stt_url` and nowhere else.

use super::transcriber::{Transcriber, clean, prepare};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Which recognizer turns the answer into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SttEngine {
    /// whisper.cpp inside the app, offline.
    #[default]
    Local,
    /// `POST {stt_url}/audio/transcriptions`: the OpenAI-compatible shape.
    Openai,
    /// `POST {stt_url}/asr`: Whisper ASR Webservice's own shape.
    Asr,
}

impl SttEngine {
    /// The name the config uses.
    pub fn name(self) -> &'static str {
        match self {
            SttEngine::Local => "local",
            SttEngine::Openai => "openai",
            SttEngine::Asr => "asr",
        }
    }
}

/// Longest wait for the server to accept the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Longest wait for a transcription. A server loading a model for its first
/// answer took 18 s (`large-v3-turbo`, measured in #59); answers then take under 1 s.
pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The 16 kHz the recorded answer is in.
const RATE: u32 = 16_000;

/// A Whisper server, as the settings describe it.
#[derive(Debug, Clone, PartialEq)]
pub struct Remote {
    pub engine: SttEngine,
    pub url: String,
    /// Model name sent to an OpenAI-compatible server; empty = the server's default.
    pub model: String,
    pub timeout: Duration,
}

/// One request to send: where, its content type and its body.
#[derive(Debug, PartialEq)]
struct Request {
    url: String,
    content_type: String,
    body: Vec<u8>,
}

const BOUNDARY: &str = "snowlearner-answer-7d1f3c";

impl Remote {
    /// The server these settings point at, or None for the local recognizer.
    pub fn new(engine: SttEngine, url: &str, model: &str) -> Option<Remote> {
        (engine != SttEngine::Local).then(|| Remote {
            engine,
            url: url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            timeout: ANSWER_TIMEOUT,
        })
    }

    /// Where answers are sent, for the log and `doctor`.
    pub fn endpoint(&self) -> String {
        match self.engine {
            SttEngine::Openai => format!("{}/audio/transcriptions", self.url),
            _ => format!("{}/asr", self.url),
        }
    }

    /// Whether anything answers at `stt_url`: any HTTP status counts, only a
    /// connection that fails or hangs does not.
    pub fn reachable(&self) -> Result<()> {
        let url = self.url.clone();
        match send(&url, self.timeout, |a| a.get(&url).call()) {
            Ok(_) | Err(ureq::Error::StatusCode(_)) => Ok(()),
            Err(e) => Err(explain(e, &url, self.timeout)),
        }
    }

    fn request(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Request {
        let code = lang.split(['-', '_']).next().unwrap_or(lang).to_ascii_lowercase();
        let audio = wav(&prepare(samples));
        let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
        match self.engine {
            SttEngine::Openai => {
                let mut fields = vec![("language", code.as_str()), ("response_format", "json")];
                if !self.model.is_empty() {
                    fields.push(("model", self.model.as_str()));
                }
                if let Some(p) = prompt {
                    fields.push(("prompt", p));
                }
                Request { url: self.endpoint(), content_type, body: multipart(&fields, "file", &audio) }
            }
            _ => {
                let mut url = format!("{}?language={code}&output=json", self.endpoint());
                if let Some(p) = prompt {
                    url.push_str(&format!("&initial_prompt={}", percent(p)));
                }
                Request { url, content_type, body: multipart(&[], "audio_file", &audio) }
            }
        }
    }
}

impl Transcriber for Remote {
    fn pass(&self, samples: &[f32], lang: &str, prompt: Option<&str>) -> Result<String> {
        let req = self.request(samples, lang, prompt);
        let reply = send(&req.url, self.timeout, |a| {
            a.post(&req.url).header("Content-Type", &req.content_type).send(&req.body[..])
        })
        .and_then(|mut r| r.body_mut().read_to_string())
        .map_err(|e| explain(e, &self.endpoint(), self.timeout))?;
        text_of(&reply).map(|t| clean(&t))
    }
}

/// Sends through an agent with both timeouts, trying IPv4 first for
/// `localhost` (see `voices::ipv4_first`).
fn send<T>(
    url: &str,
    timeout: Duration,
    call: impl Fn(&ureq::Agent) -> Result<T, ureq::Error>,
) -> Result<T, ureq::Error> {
    use ureq::config::IpFamily;
    let families =
        if super::voices::ipv4_first(url) { vec![IpFamily::Ipv4Only, IpFamily::Any] } else { vec![IpFamily::Any] };
    let mut last = ureq::Error::ConnectionFailed;
    for family in families {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(timeout))
            .ip_family(family)
            .build()
            .into();
        match call(&agent) {
            Err(e @ (ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound)) => last = e,
            done => return done,
        }
    }
    Err(last)
}

/// A failed request in the learner's terms: down, slow, or refusing.
fn explain(e: ureq::Error, url: &str, timeout: Duration) -> anyhow::Error {
    match e {
        ureq::Error::StatusCode(code) => anyhow!("speech server at {url} answered HTTP {code}"),
        ureq::Error::Timeout(_) => anyhow!("speech server at {url} took longer than {} s", timeout.as_secs()),
        other => anyhow!("speech server at {url} did not answer — is it running? ({other})"),
    }
}

/// The text of a `{"text": …}` reply, the field both shapes answer with.
fn text_of(reply: &str) -> Result<String> {
    let json: serde_json::Value =
        serde_json::from_str(reply).map_err(|e| anyhow!("speech server reply is not JSON ({e}): {reply:.120}"))?;
    json.get("text")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("speech server reply has no \"text\": {reply:.120}"))
}

/// 16-bit PCM mono WAV of `samples` at 16 kHz.
fn wav(samples: &[f32]) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// A `multipart/form-data` body: the text fields, then the WAV as `file_field`.
fn multipart(fields: &[(&str, &str)], file_field: &str, audio: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in fields {
        let part = format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n");
        out.extend_from_slice(part.as_bytes());
    }
    let head = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{file_field}\"; filename=\"answer.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    );
    out.extend_from_slice(head.as_bytes());
    out.extend_from_slice(audio);
    out.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    out
}

/// Percent-encodes everything but the URL-safe characters.
fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    /// A one-request server: hands over what it received, waits `delay`, then answers `reply`.
    fn server(reply: impl Into<String>, delay: Duration) -> (String, mpsc::Receiver<String>) {
        let reply = reply.into();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else { return };
            let mut reader = BufReader::new(&stream);
            let mut head = String::new();
            let mut length = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = v.trim().parse().unwrap_or(0);
                }
                head.push_str(&line);
            }
            let mut body = vec![0; length];
            let _ = reader.read_exact(&mut body);
            let _ = tx.send(format!("{head}\r\n{}", String::from_utf8_lossy(&body)));
            std::thread::sleep(delay);
            let _ = (&stream).write_all(reply.as_bytes());
        });
        (base, rx)
    }

    const OK_JSON: &str = concat!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 23\r\nConnection: close\r\n\r\n",
        "{\"text\":\" Me too. [x]\"}"
    );

    fn empty(status: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    }

    fn remote(engine: SttEngine, url: &str) -> Remote {
        Remote::new(engine, url, "").unwrap()
    }

    #[test]
    fn the_local_engine_has_no_server() {
        assert_eq!(Remote::new(SttEngine::Local, "http://x:9000", "base"), None);
    }

    #[test]
    fn an_openai_server_gets_the_answer_as_a_wav_file_with_the_language() {
        let (base, rx) = server(OK_JSON, Duration::ZERO);
        let r = remote(SttEngine::Openai, &format!("{base}/v1/"));
        let heard = r.pass(&[0.25; 8_000], "pt-BR", None).unwrap();
        assert_eq!(heard, "Me too.", "the reply's text, cleaned like whisper.cpp's");
        let req = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(req.starts_with("POST /v1/audio/transcriptions "), "{req:.80}");
        assert!(req.contains("name=\"language\"\r\n\r\npt\r\n"), "the practiced language, as an ISO code");
        assert!(req.contains("name=\"file\"; filename=\"answer.wav\""));
        assert!(req.contains("RIFF") && req.contains("WAVE"));
        assert!(!req.contains("name=\"model\""), "no model set: the server picks");
        assert!(!req.contains("name=\"prompt\""), "the first pass has no hint");
    }

    #[test]
    fn the_hinted_pass_sends_the_hint_and_the_model_when_one_is_set() {
        let (base, rx) = server(OK_JSON, Duration::ZERO);
        let r = Remote { model: "turbo".into(), ..remote(SttEngine::Openai, &format!("{base}/v1")) };
        r.pass(&[0.0; 100], "en", Some("Me too.")).unwrap();
        let req = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(req.contains("name=\"prompt\"\r\n\r\nMe too.\r\n"));
        assert!(req.contains("name=\"model\"\r\n\r\nturbo\r\n"));
    }

    #[test]
    fn an_asr_server_gets_the_language_and_hint_in_the_query() {
        let (base, rx) = server(OK_JSON, Duration::ZERO);
        let r = remote(SttEngine::Asr, &base);
        assert_eq!(r.pass(&[0.0; 100], "es", Some("¿Qué tal?")).unwrap(), "Me too.");
        let req = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let first = req.lines().next().unwrap();
        assert_eq!(first, "POST /asr?language=es&output=json&initial_prompt=%C2%BFQu%C3%A9%20tal%3F HTTP/1.1");
        assert!(req.contains("name=\"audio_file\"; filename=\"answer.wav\""));
    }

    #[test]
    fn a_server_that_is_down_fails_fast_with_its_address() {
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let r = remote(SttEngine::Asr, &format!("http://127.0.0.1:{port}"));
        let started = std::time::Instant::now();
        let err = format!("{:#}", r.pass(&[0.0; 100], "en", None).unwrap_err());
        assert!(err.contains(&format!("127.0.0.1:{port}/asr")) && err.contains("did not answer"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(r.reachable().is_err());
    }

    #[test]
    fn a_server_slower_than_the_timeout_does_not_hold_the_lesson() {
        let (base, _rx) = server(OK_JSON, Duration::from_secs(5));
        let r = Remote { timeout: Duration::from_secs(1), ..remote(SttEngine::Openai, &base) };
        let started = std::time::Instant::now();
        let err = format!("{:#}", r.pass(&[0.0; 100], "en", None).unwrap_err());
        assert!(err.contains("took longer than 1 s"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_server_error_is_reported_with_its_status() {
        let (base, _rx) = server(empty("500 Internal Server Error"), Duration::ZERO);
        let err = format!("{:#}", remote(SttEngine::Openai, &base).pass(&[0.0; 100], "en", None).unwrap_err());
        assert!(err.contains("HTTP 500"), "{err}");
    }

    #[test]
    fn any_http_answer_means_the_server_is_reachable() {
        let (base, _rx) = server(empty("404 Not Found"), Duration::ZERO);
        remote(SttEngine::Asr, &base).reachable().unwrap();
    }

    #[test]
    fn a_reply_without_text_is_an_error_not_silence() {
        assert!(text_of("{\"detail\":\"no file\"}").unwrap_err().to_string().contains("no \"text\""));
        assert!(text_of("<html>").is_err());
        assert_eq!(text_of("{\"text\":\" Hi.\",\"segments\":[]}").unwrap(), " Hi.");
    }

    #[test]
    fn the_wav_is_16_bit_mono_at_16_khz() {
        let w = wav(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16_000);
        assert_eq!(u16::from_le_bytes([w[34], w[35]]), 16, "bits per sample");
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), i16::MAX);
        assert_eq!(i16::from_le_bytes([w[50], w[51]]), i16::MAX, "clipped, not wrapped");
    }
}
