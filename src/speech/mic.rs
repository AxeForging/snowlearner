//! Microphone capture (cpal: WASAPI / CoreAudio / ALSA-PipeWire) until the
//! endpointer decides the learner is done talking.

use super::endpoint::{Endpointer, Status};
use super::resample::{WHISPER_RATE, resample, to_mono};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct Recording {
    /// 16 kHz mono, ready for whisper.
    pub samples: Vec<f32>,
    pub heard_speech: bool,
}

pub fn default_input_name() -> Option<String> {
    let dev = cpal::default_host().default_input_device()?;
    dev.description().ok().map(|d| d.to_string()).or_else(|| Some("default input".into()))
}

pub fn record(max_seconds: f32) -> Result<Recording> {
    let host = cpal::default_host();
    let device = host.default_input_device().context("no microphone found")?;
    let supported = device.default_input_config().context("microphone has no usable input config")?;
    let rate = supported.sample_rate();
    let channels = supported.channels();
    let config = supported.config();
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let err_fn = |e| eprintln!("microphone stream error: {e}");

    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _: &_| {
                let _ = tx.send(data.to_vec());
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _: &_| {
                let _ = tx.send(data.iter().map(|s| *s as f32 / i16::MAX as f32).collect());
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _: &_| {
                let _ = tx.send(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect());
            },
            err_fn,
            None,
        ),
        other => bail!("unsupported microphone sample format {other:?}"),
    }
    .map_err(|e| anyhow!("opening microphone: {e}"))?;
    stream.play().map_err(|e| anyhow!("starting microphone: {e}"))?;

    let mut endpoint = Endpointer::new(rate, max_seconds);
    let mut mono = Vec::new();
    let deadline = Instant::now() + Duration::from_secs_f32(max_seconds + 2.0);
    let heard = loop {
        let chunk = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => continue,
            Err(_) => break false,
        };
        let m = to_mono(&chunk, channels);
        let status = endpoint.feed(&m);
        mono.extend(m);
        if let Status::Done { speech } = status {
            break speech;
        }
        if Instant::now() > deadline {
            break false;
        }
    };
    drop(stream);
    Ok(Recording { samples: resample(&mono, rate, WHISPER_RATE), heard_speech: heard })
}
