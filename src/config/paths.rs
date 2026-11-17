//! Where snowlearner keeps its files. Platform dirs by default
//! (`~/.config/snowlearner`, `%APPDATA%\axeforge\snowlearner`, …);
//! `SNOWLEARNER_HOME` puts everything under one folder (tests, portable use).

use anyhow::{Context, Result};
use std::path::PathBuf;

pub const HOME_ENV: &str = "SNOWLEARNER_HOME";

#[derive(Debug, Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Paths> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|v| !v.is_empty()) {
            let home = PathBuf::from(home);
            return Ok(Paths { config_dir: home.join("config"), data_dir: home.join("data") });
        }
        let dirs = directories::ProjectDirs::from("dev", "axeforge", "snowlearner")
            .context("could not determine a home directory for config files")?;
        Ok(Paths { config_dir: dirs.config_dir().to_path_buf(), data_dir: dirs.data_dir().to_path_buf() })
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn decks_dir(&self) -> PathBuf {
        self.config_dir.join("decks")
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("history.sqlite3")
    }

    /// stdout/stderr of an app started without a terminal (see `control::console`).
    pub fn log_file(&self) -> PathBuf {
        self.data_dir.join("snowlearner.log")
    }

    /// Answers kept for diagnosis when `keep_recordings` is on (see `speech::recordings`).
    pub fn recordings_dir(&self) -> PathBuf {
        self.data_dir.join("recordings")
    }

    /// The mage as a PNG for the Linux app menu (written by `setup`).
    pub fn menu_icon_file(&self) -> PathBuf {
        self.data_dir.join("snowlearner.png")
    }

    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    pub fn model_file(&self, name: &str) -> PathBuf {
        self.models_dir().join(format!("ggml-{name}.bin"))
    }
}

/// `path` without the `\\?\` prefix Windows puts on canonical paths
/// (`canonicalize` returns `\\?\C:\…`): the form people read and other programs
/// expect. Network paths (`\\?\UNC\…`) and every other path are left as they are.
pub fn plain(path: PathBuf) -> PathBuf {
    let drive = path
        .to_string_lossy()
        .strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .map(PathBuf::from);
    drive.unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_canonical_path_reads_without_its_verbatim_prefix() {
        let exe = r"C:\Users\livepc\AppData\Local\Programs\snowlearner\snowlearner.exe";
        assert_eq!(plain(PathBuf::from(format!(r"\\?\{exe}"))), PathBuf::from(exe));
        assert_eq!(plain(PathBuf::from(exe)), PathBuf::from(exe), "already plain");
        let unc = r"\\?\UNC\server\share\snowlearner.exe";
        assert_eq!(plain(PathBuf::from(unc)), PathBuf::from(unc), "a network path keeps its prefix");
        assert_eq!(plain(PathBuf::from("/usr/bin/snowlearner")), PathBuf::from("/usr/bin/snowlearner"));
    }
}
