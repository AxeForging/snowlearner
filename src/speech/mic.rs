//! Microphone capture until the endpointer decides the learner is done
//! (or they press "done"), reporting the live level for the meter.

use super::audio;
use super::endpoint::{Endpointer, ListenPlan, Status};
use super::resample::{WHISPER_RATE, resample, to_mono};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct Recording {
    /// 16 kHz mono, ready for whisper.
    pub samples: Vec<f32>,
    pub heard_speech: bool,
}

pub fn default_input_name() -> Option<String> {
    audio::input("").ok().map(|d| d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| "default".into()))
}

/// Records from `device` ("" = default). `on_level(rms, speaking)` is called
/// ~15×/s; setting `stop` ends the turn early.
pub fn record(
    plan: ListenPlan,
    device: &str,
    stop: &AtomicBool,
    mut on_level: impl FnMut(f32, bool),
) -> Result<Recording> {
    let device = audio::input(device)?;
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

    let mut endpoint = Endpointer::new(rate, plan);
    let mut mono = Vec::new();
    let deadline = Instant::now() + Duration::from_secs_f32(plan.max_total() + 1.0);
    let mut last_level = Instant::now();
    let heard = loop {
        if stop.swap(false, Ordering::SeqCst)
            && let Status::Done { speech } = endpoint.finish()
        {
            break speech;
        }
        let chunk = match rx.recv_timeout(Duration::from_millis(60)) {
            Ok(c) => c,
            Err(mpsc::RecvTimeoutError::Timeout) if Instant::now() < deadline => continue,
            Err(_) => break endpoint.started(),
        };
        let m = to_mono(&chunk, channels);
        let status = endpoint.feed(&m);
        mono.extend(m);
        if last_level.elapsed() >= Duration::from_millis(66) {
            last_level = Instant::now();
            on_level(endpoint.level(), endpoint.started());
        }
        if let Status::Done { speech } = status {
            break speech;
        }
        if Instant::now() > deadline {
            break endpoint.started();
        }
    };
    drop(stream);
    on_level(0.0, false);
    Ok(Recording { samples: resample(&mono, rate, WHISPER_RATE), heard_speech: heard })
}
