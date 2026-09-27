//! Keeps a heavy, file-backed resource (the whisper model, ~150 MB) in memory
//! only while it is being used: loaded on demand, reloaded when its file
//! changes, and dropped after sitting idle. Pure — the caller passes the time.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the speech model stays loaded after the last listen. Covers the
/// retries of one lesson; lessons minutes apart pay a sub-second reload.
pub const SPEECH_MODEL_IDLE: Duration = Duration::from_secs(120);

pub struct Resident<T> {
    held: Option<(PathBuf, T)>,
    last_used: Instant,
    idle: Duration,
}

impl<T> Resident<T> {
    pub fn new(idle: Duration, now: Instant) -> Resident<T> {
        Resident { held: None, last_used: now, idle }
    }

    /// The value loaded from `key`, loading it first when nothing (or another
    /// file) is held. A failed load leaves nothing held, so the next call retries.
    pub fn get_or_load<E>(
        &mut self,
        key: &Path,
        now: Instant,
        load: impl FnOnce(&Path) -> Result<T, E>,
    ) -> Result<&T, E> {
        self.last_used = now;
        if self.held.as_ref().is_none_or(|(k, _)| k != key) {
            self.held = None; // free the old one before loading the new one
            self.held = Some((key.to_path_buf(), load(key)?));
        }
        Ok(&self.held.as_ref().unwrap().1)
    }

    /// Marks the value as just used (call when a long job that used it ends).
    pub fn touch(&mut self, now: Instant) {
        self.last_used = now;
    }

    /// Drops the value once it has been idle for the whole idle period.
    /// Returns true when something was released.
    pub fn release_if_idle(&mut self, now: Instant) -> bool {
        if self.held.is_some() && now.saturating_duration_since(self.last_used) >= self.idle {
            self.held = None;
            return true;
        }
        false
    }

    /// When `release_if_idle` should next be called; `None` when nothing is held.
    pub fn deadline(&self) -> Option<Instant> {
        self.held.as_ref().map(|_| self.last_used + self.idle)
    }

    pub fn is_loaded(&self) -> bool {
        self.held.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const IDLE: Duration = Duration::from_secs(120);

    fn loader(count: &Cell<u32>) -> impl Fn(&Path) -> Result<String, String> + '_ {
        move |p| {
            count.set(count.get() + 1);
            Ok(p.display().to_string())
        }
    }

    #[test]
    fn loads_on_first_use_and_reuses_it_afterwards() {
        let t0 = Instant::now();
        let loads = Cell::new(0);
        let mut r = Resident::new(IDLE, t0);
        assert!(!r.is_loaded());
        assert_eq!(r.deadline(), None, "nothing to wake up for");
        assert_eq!(r.get_or_load(Path::new("base"), t0, loader(&loads)).unwrap(), "base");
        assert_eq!(r.get_or_load(Path::new("base"), t0 + Duration::from_secs(5), loader(&loads)).unwrap(), "base");
        assert_eq!(loads.get(), 1);
    }

    #[test]
    fn a_different_model_file_replaces_the_loaded_one() {
        let t0 = Instant::now();
        let loads = Cell::new(0);
        let mut r = Resident::new(IDLE, t0);
        r.get_or_load(Path::new("base"), t0, loader(&loads)).unwrap();
        assert_eq!(r.get_or_load(Path::new("small"), t0, loader(&loads)).unwrap(), "small");
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn released_exactly_when_the_idle_period_ends_not_before() {
        let t0 = Instant::now();
        let loads = Cell::new(0);
        let mut r = Resident::new(IDLE, t0);
        r.get_or_load(Path::new("base"), t0, loader(&loads)).unwrap();
        assert_eq!(r.deadline(), Some(t0 + IDLE));
        assert!(!r.release_if_idle(t0 + IDLE - Duration::from_millis(1)));
        assert!(r.is_loaded());
        assert!(r.release_if_idle(t0 + IDLE));
        assert!(!r.is_loaded());
        assert_eq!(r.deadline(), None);
        assert!(!r.release_if_idle(t0 + IDLE * 2), "nothing left to release");
        r.get_or_load(Path::new("base"), t0 + IDLE * 3, loader(&loads)).unwrap();
        assert_eq!(loads.get(), 2, "reloaded on the next lesson");
    }

    #[test]
    fn use_and_touch_push_the_release_back() {
        let t0 = Instant::now();
        let loads = Cell::new(0);
        let mut r = Resident::new(IDLE, t0);
        r.get_or_load(Path::new("base"), t0, loader(&loads)).unwrap();
        let later = t0 + Duration::from_secs(100);
        r.get_or_load(Path::new("base"), later, loader(&loads)).unwrap();
        assert!(!r.release_if_idle(t0 + IDLE), "used again at +100 s");
        // A long listen ends at +200 s: the idle clock starts there.
        r.touch(t0 + Duration::from_secs(200));
        assert!(!r.release_if_idle(later + IDLE));
        assert!(r.release_if_idle(t0 + Duration::from_secs(200) + IDLE));
    }

    #[test]
    fn a_failed_load_holds_nothing_and_is_retried() {
        let t0 = Instant::now();
        let mut r: Resident<String> = Resident::new(IDLE, t0);
        let err = r.get_or_load(Path::new("missing"), t0, |_| Err("no model")).unwrap_err();
        assert_eq!(err, "no model");
        assert!(!r.is_loaded());
        assert_eq!(r.get_or_load(Path::new("missing"), t0, |_| Ok::<_, &str>("ok".into())).unwrap(), "ok");
    }

    #[test]
    fn a_failed_switch_does_not_keep_the_old_model_around() {
        let t0 = Instant::now();
        let loads = Cell::new(0);
        let mut r = Resident::new(IDLE, t0);
        r.get_or_load(Path::new("base"), t0, loader(&loads)).unwrap();
        assert!(r.get_or_load(Path::new("broken"), t0, |_| Err("bad file")).is_err());
        assert!(!r.is_loaded(), "old model freed even though the new one failed");
    }
}
