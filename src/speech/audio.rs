//! Audio devices (cpal: WASAPI / CoreAudio / ALSA-PipeWire): listing, picking
//! by name, and playing WAV audio from HTTP/command voices on a chosen speaker.

use super::resample::{resample, to_mono};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc;
use std::time::Duration;

fn name_of(d: &cpal::Device) -> String {
    d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| "unknown device".into())
}

/// Drops ALSA pseudo-devices nobody wants to pick and duplicate names
/// (devices are chosen by name, so duplicates are indistinguishable anyway).
pub fn useful(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let junk = n.starts_with("Discard all samples") || n.trim_end_matches([',', ' ']).is_empty();
        if !junk && !out.contains(&n) {
            out.push(n);
        }
    }
    out
}

pub fn input_names() -> Vec<String> {
    useful(
        cpal::default_host().input_devices().map(|it| it.map(|d| name_of(&d)).collect::<Vec<_>>()).unwrap_or_default(),
    )
}

pub fn output_names() -> Vec<String> {
    useful(
        cpal::default_host().output_devices().map(|it| it.map(|d| name_of(&d)).collect::<Vec<_>>()).unwrap_or_default(),
    )
}

/// `name` empty = system default. A configured device that disappeared falls
/// back to the default instead of failing.
pub fn input(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if !name.is_empty()
        && let Some(d) = host.input_devices()?.find(|d| name_of(d) == name)
    {
        return Ok(d);
    }
    host.default_input_device().context("no microphone found")
}

pub fn output(name: &str) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if !name.is_empty()
        && let Some(d) = host.output_devices()?.find(|d| name_of(d) == name)
    {
        return Ok(d);
    }
    host.default_output_device().context("no speaker found")
}

/// WAV bytes → mono f32 samples + sample rate.
pub fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    let mut r = hound::WavReader::new(std::io::Cursor::new(bytes)).context("not a WAV file")?;
    let spec = r.spec();
    let raw: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
            r.samples::<i32>().map(|s| s.map(|v| v as f32 / max)).collect::<Result<_, _>>()?
        }
    };
    Ok((to_mono(&raw, spec.channels), spec.sample_rate))
}

/// Mono samples → interleaved frames for a device with `channels` channels.
pub fn spread(mono: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    mono.iter().flat_map(|&s| std::iter::repeat_n(s, ch)).collect()
}

/// Plays WAV audio on `speaker` and blocks until it finishes.
pub fn play_wav(bytes: &[u8], speaker: &str) -> Result<()> {
    let (mono, rate) = decode_wav(bytes)?;
    let device = output(speaker)?;
    let supported = device.default_output_config().context("speaker has no usable output config")?;
    let (out_rate, channels) = (supported.sample_rate(), supported.channels());
    let data = spread(&resample(&mono, rate, out_rate), channels);
    let total = data.len();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let mut pos = 0usize;
    let config = supported.config();
    let err_fn = |e| eprintln!("speaker stream error: {e}");
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            config,
            move |out: &mut [f32], _: &_| {
                for o in out.iter_mut() {
                    *o = data.get(pos).copied().unwrap_or(0.0);
                    pos += 1;
                }
                if pos >= total {
                    let _ = done_tx.send(());
                }
            },
            err_fn,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            config,
            move |out: &mut [i16], _: &_| {
                for o in out.iter_mut() {
                    *o = (data.get(pos).copied().unwrap_or(0.0).clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    pos += 1;
                }
                if pos >= total {
                    let _ = done_tx.send(());
                }
            },
            err_fn,
            None,
        ),
        other => bail!("unsupported speaker sample format {other:?}"),
    }
    .map_err(|e| anyhow!("opening speaker: {e}"))?;
    stream.play().map_err(|e| anyhow!("starting speaker: {e}"))?;
    let secs = total as f32 / (out_rate as f32 * channels.max(1) as f32);
    let _ = done_rx.recv_timeout(Duration::from_secs_f32(secs + 2.0));
    std::thread::sleep(Duration::from_millis(120)); // let the device drain its buffer
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let spec = hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut w = hound::WavWriter::new(&mut buf, spec).unwrap();
        for s in samples {
            w.write_sample(*s).unwrap();
        }
        w.finalize().unwrap();
        buf.into_inner()
    }

    #[test]
    fn decodes_stereo_16bit_wav_to_mono() {
        let bytes = wav(&[16384, 0, -16384, 0], 24_000, 2);
        let (mono, rate) = decode_wav(&bytes).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(mono.len(), 2);
        assert!((mono[0] - 0.25).abs() < 1e-3 && (mono[1] + 0.25).abs() < 1e-3);
    }

    #[test]
    fn garbage_is_rejected_not_played() {
        assert!(decode_wav(b"<html>error</html>").is_err());
    }

    #[test]
    fn pseudo_devices_and_duplicates_are_hidden() {
        let names = [
            "Discard all samples (playback) or generate zero samples (capture)",
            "PipeWire Sound Server",
            "HD-Audio Generic, ALC245 Analog",
            "HD-Audio Generic, ALC245 Analog",
            "acp-pdm-mach, ",
        ];
        assert_eq!(
            useful(names.map(String::from)),
            vec!["PipeWire Sound Server", "HD-Audio Generic, ALC245 Analog", "acp-pdm-mach, "]
        );
    }

    #[test]
    fn mono_is_spread_to_every_channel() {
        assert_eq!(spread(&[0.1, 0.2], 2), vec![0.1, 0.1, 0.2, 0.2]);
    }
}
