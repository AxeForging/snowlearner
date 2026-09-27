//! App launcher entry so snowlearner shows up in the desktop's app menu
//! (Linux freedesktop `.desktop` file; macOS/Windows have no equivalent here).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "snowlearner.desktop";

/// The `.desktop` entry that starts the app. `exe` is quoted so paths with
/// spaces work (Exec field quoting rules).
pub fn desktop_entry(exe: &str) -> String {
    let quoted = exe.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Snowlearner\n\
         Comment=Pratique um idioma falando: o mago de gelo congela a tela\n\
         Exec=\"{quoted}\" run\n\
         Icon=accessories-dictionary\n\
         Terminal=false\n\
         Categories=Education;Languages;\n\
         Keywords=language;english;spanish;inglês;espanhol;\n"
    )
}

/// `$XDG_DATA_HOME/applications` (usually `~/.local/share/applications`).
pub fn applications_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|d| d.data_dir().join("applications"))
}

/// Writes (or refreshes) the launcher; returns where.
pub fn install(exe: &str, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = dir.join(FILE_NAME);
    std::fs::write(&file, desktop_entry(exe)).with_context(|| format!("writing {}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_runs_the_installed_binary_even_with_spaces_in_the_path() {
        let e = desktop_entry("/home/ana maria/.local/bin/snowlearner");
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("Exec=\"/home/ana maria/.local/bin/snowlearner\" run\n"), "{e}");
        assert!(e.contains("Terminal=false"));
    }

    #[test]
    fn quotes_in_the_path_are_escaped() {
        assert!(desktop_entry("/tmp/a\"b/snow").contains("Exec=\"/tmp/a\\\"b/snow\" run"));
    }

    #[test]
    fn installing_twice_refreshes_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = install("/old/snowlearner", dir.path()).unwrap();
        let second = install("/new/snowlearner", dir.path()).unwrap();
        assert_eq!(first, second);
        assert!(std::fs::read_to_string(second).unwrap().contains("/new/snowlearner"));
    }
}
