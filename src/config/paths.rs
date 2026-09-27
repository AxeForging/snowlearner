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

    pub fn models_dir(&self) -> PathBuf {
        self.data_dir.join("models")
    }

    pub fn model_file(&self, name: &str) -> PathBuf {
        self.models_dir().join(format!("ggml-{name}.bin"))
    }
}
