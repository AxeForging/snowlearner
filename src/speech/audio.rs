//! Audio devices (cpal: WASAPI / CoreAudio / ALSA-PipeWire): listing, picking
//! by name, and playing WAV audio from HTTP/command voices on a chosen speaker.

use super::resample::{resample, to_mono};
use anyhow::{Context, Result, anyhow, bail};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
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

/// A sound server's loopback of an output ("Monitor of …"): it records what the
/// speakers play, not a voice. PulseAudio and pipewire-pulse name it
/// `<sink>.monitor`; the name is the same in every language, the description is not.
pub fn is_monitor(id: &str) -> bool {
    id.ends_with(".monitor")
}

fn monitor(d: &cpal::Device) -> bool {
    d.id().is_ok_and(|id| is_monitor(id.id()))
}

/// Which audio system the lists come from: "ALSA" or "PulseAudio" on Linux
/// (#205), "WASAPI" on Windows, "CoreAudio" on macOS.
pub fn host_name() -> &'static str {
    cpal::default_host().id().name()
}

pub fn input_names() -> Vec<String> {
    useful(
        cpal::default_host()
            .input_devices()
            .map(|it| it.filter(|d| !monitor(d)).map(|d| name_of(&d)).collect::<Vec<_>>())
            .unwrap_or_default(),
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

/// Streaming TTS servers (Kokoro-FastAPI) send the header before they know
/// the length and write 0xFFFFFFFF as the RIFF and data sizes. A data size
/// that overruns what arrived becomes what arrived (whole sample frames).
/// Kokoro then appends the header it could not seek back to fix — as long as
/// the real one, zeros but the RIFF size and the data size — which played as
/// a click after every line; when those sizes match the file, they are used.
fn with_real_sizes(bytes: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let u32_at = |i: usize| bytes.get(i..i + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return bytes.into();
    }
    let (mut i, mut align) = (12usize, 1usize);
    while let Some(size) = u32_at(i + 4) {
        let body = i + 8;
        match &bytes[i..i + 4] {
            b"fmt " => {
                align = bytes.get(body + 12..body + 14).map_or(1, |b| u16::from_le_bytes([b[0], b[1]])).max(1) as usize
            }
            b"data" if size > bytes.len() - body => {
                let len = bytes.len();
                let patched = u32_at(len - 4).filter(|&n| {
                    len.checked_sub(body).and_then(|rest| rest.checked_sub(body)) == Some(n)
                        && u32_at(len - body + 4) == Some(body + n - 8)
                });
                let real = patched.unwrap_or(len - body) / align * align;
                let mut fixed = bytes[..body + real].to_vec();
                fixed[i + 4..i + 8].copy_from_slice(&(real as u32).to_le_bytes());
                let riff = fixed.len() as u32 - 8;
                fixed[4..8].copy_from_slice(&riff.to_le_bytes());
                return fixed.into();
            }
            _ => {}
        }
        i = body + size + (size & 1);
    }
    bytes.into()
}

/// WAV bytes → mono f32 samples + sample rate.
pub fn decode_wav(bytes: &[u8]) -> Result<(Vec<f32>, u32)> {
    let bytes = with_real_sizes(bytes);
    let mut r = hound::WavReader::new(std::io::Cursor::new(&bytes[..])).context("not a WAV file")?;
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

/// Plays mono samples on their own thread and stream (#163), so a sound effect
/// overlaps the voice instead of waiting behind it. A speaker that cannot open
/// a second stream skips the sound; that is logged, never an error.
pub fn play_detached(mono: Vec<f32>, rate: u32, speaker: String, what: &'static str) {
    let spawned = std::thread::Builder::new().name("effect".into()).spawn(move || {
        super::trace::line(format_args!("playing the {what} ({:.1} s)", mono.len() as f32 / rate as f32));
        if let Err(e) = play_samples(&mono, rate, &speaker) {
            super::trace::line(format_args!("the {what} did not play: {e:#}"));
        }
    });
    if let Err(e) = spawned {
        super::trace::line(format_args!("the {what} did not play: {e}"));
    }
}

/// What a callback carries on PulseAudio (#209), in ms: left to itself the
/// server buffers about 2 s, which cut the end of every line and handed the
/// endpointer the microphone in 2 s blocks. Other hosts keep their defaults.
const MIC_CALLBACK_MS: u32 = 20;
const SPEAKER_CALLBACK_MS: u32 = 40;
/// Kept after the last sample has played, for the device's own buffer.
const DRAIN_MARGIN: Duration = Duration::from_millis(120);

/// The buffer a stream asks `host` for at `rate` Hz: fixed on PulseAudio,
/// the host's default everywhere else (ALSA, WASAPI, CoreAudio).
pub fn buffer_size(host: &str, rate: u32, input: bool) -> cpal::BufferSize {
    if host != "PulseAudio" {
        return cpal::BufferSize::Default;
    }
    let ms = if input { MIC_CALLBACK_MS } else { SPEAKER_CALLBACK_MS };
    cpal::BufferSize::Fixed((rate as u64 * ms as u64 / 1000).max(1) as u32)
}

/// How long to keep a speaker open once the last sample is written: what the
/// device still holds (`latency`, as the stream reports it) plus a margin.
pub fn drain_wait(latency: Duration) -> Duration {
    latency + DRAIN_MARGIN
}

/// Opens a stream with the buffer `buffer_size` picks; a host that refuses it
/// gets its default instead, and the trace says so.
pub fn open_stream<E: std::fmt::Display>(
    mut config: cpal::StreamConfig,
    input: bool,
    open: impl Fn(cpal::StreamConfig) -> Result<cpal::Stream, E>,
) -> Result<cpal::Stream, E> {
    config.buffer_size = buffer_size(host_name(), config.sample_rate, input);
    if config.buffer_size == cpal::BufferSize::Default {
        return open(config);
    }
    open(config).or_else(|e| {
        super::trace::line(format_args!("{:?} refused ({e}); opening with the default buffer", config.buffer_size));
        open(cpal::StreamConfig { buffer_size: cpal::BufferSize::Default, ..config })
    })
}

/// Copies the next samples into `out`, converted for the device; true once
/// everything has been written. `latency`: the stream's latency in µs, kept
/// at its largest so the speaker stays open until the last sample plays.
fn fill<T: Copy>(
    out: &mut [T],
    data: &[f32],
    pos: &AtomicUsize,
    latency: &AtomicU64,
    info: &cpal::OutputCallbackInfo,
    convert: fn(f32) -> T,
) -> bool {
    let ts = info.timestamp();
    latency.fetch_max(ts.playback.duration_since(ts.callback).as_micros() as u64, Ordering::Relaxed);
    let mut p = pos.load(Ordering::Relaxed);
    for o in out.iter_mut() {
        *o = convert(data.get(p).copied().unwrap_or(0.0));
        p += 1;
    }
    pos.store(p, Ordering::Relaxed);
    p >= data.len()
}

/// Plays mono samples at `rate` Hz on `speaker` and blocks until they finish.
pub fn play_samples(mono: &[f32], rate: u32, speaker: &str) -> Result<()> {
    let device = output(speaker)?;
    let supported = device.default_output_config().context("speaker has no usable output config")?;
    let (out_rate, channels) = (supported.sample_rate(), supported.channels());
    let data = Arc::new(spread(&resample(mono, rate, out_rate), channels));
    let total = data.len();
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let (pos, latency) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicU64::new(0)));
    let format = supported.sample_format();
    if !matches!(format, cpal::SampleFormat::F32 | cpal::SampleFormat::I16) {
        bail!("unsupported speaker sample format {format:?}");
    }
    let open = |config: cpal::StreamConfig| {
        let (data, pos, latency, done) = (data.clone(), pos.clone(), latency.clone(), done_tx.clone());
        let err_fn = |e| eprintln!("speaker stream error: {e}");
        if format == cpal::SampleFormat::F32 {
            device.build_output_stream(
                config,
                move |out: &mut [f32], info: &_| {
                    if fill(out, &data, &pos, &latency, info, |s| s) {
                        let _ = done.send(());
                    }
                },
                err_fn,
                None,
            )
        } else {
            device.build_output_stream(
                config,
                move |out: &mut [i16], info: &_| {
                    if fill(out, &data, &pos, &latency, info, |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16) {
                        let _ = done.send(());
                    }
                },
                err_fn,
                None,
            )
        }
    };
    let stream = open_stream(supported.config(), false, open).map_err(|e| anyhow!("opening speaker: {e}"))?;
    stream.play().map_err(|e| anyhow!("starting speaker: {e}"))?;
    let secs = total as f32 / (out_rate as f32 * channels.max(1) as f32);
    let _ = done_rx.recv_timeout(Duration::from_secs_f32(secs + 2.0));
    std::thread::sleep(drain_wait(Duration::from_micros(latency.load(Ordering::Relaxed))));
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

    /// What Kokoro-FastAPI streams: sizes unknown when the header went out,
    /// so RIFF and data lengths are 0xFFFFFFFF, with a LIST chunk in between.
    fn streamed(samples: &[i16]) -> Vec<u8> {
        let plain = wav(samples, 24_000, 1);
        let data_at = plain.windows(4).position(|w| w == b"data").unwrap();
        let mut out = plain[..data_at].to_vec();
        out[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(b"LIST\x04\x00\x00\x00INFO");
        out.extend_from_slice(b"data");
        out.extend_from_slice(&u32::MAX.to_le_bytes());
        out.extend_from_slice(&plain[data_at + 8..]);
        out
    }

    #[test]
    fn a_streamed_wav_with_unknown_lengths_plays_what_arrived() {
        let (mono, rate) = decode_wav(&streamed(&[16384, -16384, 8192])).unwrap();
        assert_eq!(rate, 24_000);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < 1e-3 && (mono[2] - 0.25).abs() < 1e-3);
    }

    /// Kokoro-FastAPI's streamed WAV also ends with the header it could not
    /// seek back to fix: a copy as long as the real header, zeros except the
    /// RIFF size (at 4) and the data size (at the end).
    fn with_header_patch(samples: &[i16]) -> Vec<u8> {
        let mut bytes = streamed(samples);
        let header = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let audio = (bytes.len() - header) as u32;
        let mut patch = vec![0u8; header];
        patch[4..8].copy_from_slice(&(header as u32 + audio - 8).to_le_bytes());
        patch[header - 4..].copy_from_slice(&audio.to_le_bytes());
        bytes.extend_from_slice(&patch);
        bytes
    }

    #[test]
    fn kokoros_trailing_header_patch_is_not_played_as_a_click() {
        let (mono, _) = decode_wav(&with_header_patch(&[16384, -16384, 8192, 0])).unwrap();
        assert_eq!(mono.len(), 4, "the 78-byte patch is not audio");
        assert_eq!(mono[3], 0.0, "ends where the speech ends, no spike");
    }

    #[test]
    fn a_stream_cut_mid_sample_keeps_the_whole_samples() {
        let mut bytes = streamed(&[16384, -16384, 8192]);
        bytes.pop();
        let (mono, _) = decode_wav(&bytes).unwrap();
        assert_eq!(mono.len(), 2);
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
    fn a_sound_servers_monitor_is_not_a_microphone() {
        assert!(is_monitor("alsa_output.pci-0000_00_1f.3.analog-stereo.monitor"));
        assert!(is_monitor("RDPSink.monitor"));
        assert!(!is_monitor("alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo"));
        assert!(!is_monitor("bluez_input.00:1B:66:AA:BB:CC"));
        assert!(!is_monitor("monitor"), "ALSA has no such suffix");
    }

    #[test]
    fn a_microphone_that_is_gone_falls_back_to_the_default() {
        let default = cpal::default_host().default_input_device().map(|d| name_of(&d));
        let picked = input("a microphone unplugged long ago").ok().map(|d| name_of(&d));
        assert_eq!(picked, default, "same as the system default, never an error of its own");
    }

    #[test]
    fn the_sound_server_gets_small_fixed_buffers_and_other_hosts_their_own() {
        use cpal::BufferSize::{Default, Fixed};
        assert_eq!(buffer_size("PulseAudio", 48_000, true), Fixed(960), "20 ms of microphone");
        assert_eq!(buffer_size("PulseAudio", 44_100, false), Fixed(1764), "40 ms of speaker");
        for host in ["ALSA", "WASAPI", "CoreAudio"] {
            assert_eq!(buffer_size(host, 48_000, true), Default, "{host}");
            assert_eq!(buffer_size(host, 48_000, false), Default, "{host}");
        }
    }

    #[test]
    fn the_speaker_stays_open_until_what_it_holds_has_played() {
        assert!(drain_wait(Duration::from_secs(2)) >= Duration::from_secs(2), "2 s queued, 2 s waited");
        assert_eq!(drain_wait(Duration::ZERO), Duration::from_millis(120), "the old margin when none is reported");
    }

    #[test]
    fn mono_is_spread_to_every_channel() {
        assert_eq!(spread(&[0.1, 0.2], 2), vec![0.1, 0.1, 0.2, 0.2]);
    }
}
