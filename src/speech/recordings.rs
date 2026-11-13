//! Copies of what the speech model was given (`keep_recordings = true`), for
//! diagnosing what it heard. They are the learner's own voice: written only on
//! their machine, next to the log, and only the newest few are kept.

use super::resample::WHISPER_RATE;
use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};

/// How many recordings are kept; the oldest go as new ones arrive.
pub const KEEP: usize = 20;

const PREFIX: &str = "answer-";

/// Writes `samples` (16 kHz mono, exactly what the model got before its
/// silence padding) as a 32-bit float WAV, so `snowlearner model bench --wav`
/// replays it sample for sample, then keeps only the newest `KEEP`.
pub fn save(dir: &Path, at: DateTime<Local>, samples: &[f32]) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = dir.join(name(at));
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut w = hound::WavWriter::create(&path, spec).with_context(|| format!("creating {}", path.display()))?;
    for &s in samples {
        w.write_sample(s)?;
    }
    w.finalize().with_context(|| format!("writing {}", path.display()))?;
    prune(dir, KEEP)?;
    Ok(path)
}

/// `answer-20261113-210501-123.wav`: names sort in the order they were made.
fn name(at: DateTime<Local>) -> String {
    format!("{PREFIX}{}.wav", at.format("%Y%m%d-%H%M%S-%3f"))
}

/// Deletes the oldest recordings until `keep` remain. Files the app did not
/// name are never touched.
fn prune(dir: &Path, keep: usize) -> Result<()> {
    let entries = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    let mut kept: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_ours(p)).collect();
    kept.sort();
    let extra = kept.len().saturating_sub(keep);
    for old in &kept[..extra] {
        std::fs::remove_file(old).with_context(|| format!("deleting {}", old.display()))?;
    }
    Ok(())
}

fn is_ours(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.starts_with(PREFIX) && name.ends_with(".wav")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(second: u32, milli: u32) -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 11, 13, 21, 5, second).unwrap() + chrono::Duration::milliseconds(milli.into())
    }

    fn names(dir: &Path) -> Vec<String> {
        let entries = std::fs::read_dir(dir).unwrap();
        let mut out: Vec<String> = entries.map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        out.sort();
        out
    }

    #[test]
    fn a_kept_recording_replays_exactly_what_the_model_got() {
        let dir = tempfile::tempdir().unwrap();
        let samples: Vec<f32> = (0..1600).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        let path = save(&dir.path().join("recordings"), at(1, 0), &samples).unwrap();
        let (back, rate) = crate::speech::audio::decode_wav(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rate, WHISPER_RATE);
        assert_eq!(back, samples, "float WAV, no rounding");
    }

    #[test]
    fn recording_names_sort_in_the_order_they_were_made() {
        assert_eq!(name(at(1, 7)), "answer-20261113-210501-007.wav");
        assert!(name(at(1, 7)) < name(at(1, 8)));
        assert!(name(at(1, 999)) < name(at(2, 0)));
    }

    #[test]
    fn only_the_newest_recordings_are_kept_and_other_files_are_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
        std::fs::write(dir.path().join("mine.wav"), "mine").unwrap();
        for i in 0..(KEEP as u32 + 3) {
            save(dir.path(), at(i, 0), &[0.1; 160]).unwrap();
        }
        let left = names(dir.path());
        let recordings: Vec<&String> = left.iter().filter(|n| n.starts_with(PREFIX)).collect();
        assert_eq!(recordings.len(), KEEP);
        assert_eq!(recordings[0], &name(at(3, 0)), "the three oldest went");
        assert!(left.contains(&"notes.txt".to_string()) && left.contains(&"mine.wav".to_string()));
    }
}
