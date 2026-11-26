//! Microphone capture until the endpointer decides the learner is done
//! (or they press "done"), reporting the live level for the meter.

use super::audio;
use super::endpoint::{Endpointer, ListenPlan, Status};
use super::highpass::HighPass;
use super::resample::{WHISPER_RATE, resample, to_mono};
use super::trace;
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
    let name = device.description().map(|d| d.name().to_string()).unwrap_or_else(|_| "?".into());
    trace::line(format_args!("mic {name:?}: {rate} Hz, {channels} ch, {:?}", supported.sample_format()));
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let format = supported.sample_format();
    if !matches!(format, cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16) {
        bail!("unsupported microphone sample format {format:?}");
    }
    let open = |config: cpal::StreamConfig| {
        let tx = tx.clone();
        let err_fn = |e| eprintln!("microphone stream error: {e}");
        match format {
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
            _ => device.build_input_stream(
                config,
                move |data: &[u16], _: &_| {
                    let _ = tx.send(data.iter().map(|s| (*s as f32 - 32768.0) / 32768.0).collect());
                },
                err_fn,
                None,
            ),
        }
    };
    let stream = audio::open_stream(config, true, open).map_err(|e| anyhow!("opening microphone: {e}"))?;
    drop(tx); // the stream holds its own sender
    stream.play().map_err(|e| anyhow!("starting microphone: {e}"))?;

    let mut endpoint = Endpointer::new(rate, plan);
    let mut highpass = HighPass::new(rate);
    let mut mono = Vec::new();
    let deadline = Instant::now() + Duration::from_secs_f32(plan.max_total() + 1.0);
    let mut last_level = Instant::now();
    let mut peak = 0f32;
    let (mut chunks, mut last_chunk, mut widest) = (0u32, Instant::now(), Duration::ZERO);
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
        chunks += 1;
        if chunks > 1 {
            widest = widest.max(last_chunk.elapsed());
        }
        last_chunk = Instant::now();
        let mut m = to_mono(&chunk, channels);
        highpass.process(&mut m); // before measuring: some mics sit far from zero
        let status = endpoint.feed(&m);
        peak = peak.max(endpoint.level());
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
    let heard_s = mono.len() as f32 / rate as f32;
    trace::line(format_args!(
        "mic closed after {heard_s:.1} s: peak level {peak:.4}, speech gate {:.4}, speech {}",
        endpoint.threshold(),
        if heard { "yes" } else { "no" }
    ));
    trace::line(format_args!(
        "mic delivered {chunks} blocks of {:.0} ms on average, {:.0} ms apart at most",
        heard_s * 1000.0 / chunks.max(1) as f32,
        widest.as_secs_f32() * 1000.0
    ));
    on_level(0.0, false);
    Ok(Recording { samples: resample(&mono, rate, WHISPER_RATE), heard_speech: heard })
}
