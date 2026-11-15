//! App launcher entry so snowlearner shows up in the desktop's app menu
//! (Linux freedesktop `.desktop` file; macOS/Windows have no equivalent here).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "snowlearner.desktop";

/// A stock theme icon, for when the mage's PNG could not be written.
pub const FALLBACK_ICON: &str = "accessories-dictionary";

/// The `.desktop` entry that starts the app. `exe` is quoted so paths with
/// spaces work (Exec field quoting rules). `icon` is a theme icon name or an
/// absolute path to an image.
pub fn desktop_entry(exe: &str, icon: &str) -> String {
    let quoted = exe.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Snowlearner\n\
         Comment=Pratique um idioma falando: o mago de gelo congela a tela\n\
         Exec=\"{quoted}\" run\n\
         Icon={icon}\n\
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
pub fn install(exe: &str, icon: &str, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let file = dir.join(FILE_NAME);
    std::fs::write(&file, desktop_entry(exe, icon)).with_context(|| format!("writing {}", file.display()))?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_runs_the_installed_binary_even_with_spaces_in_the_path() {
        let e = desktop_entry("/home/ana maria/.local/bin/snowlearner", FALLBACK_ICON);
        assert!(e.starts_with("[Desktop Entry]\n"));
        assert!(e.contains("Exec=\"/home/ana maria/.local/bin/snowlearner\" run\n"), "{e}");
        assert!(e.contains("Terminal=false"));
    }

    #[test]
    fn quotes_in_the_path_are_escaped() {
        assert!(desktop_entry("/tmp/a\"b/snow", FALLBACK_ICON).contains("Exec=\"/tmp/a\\\"b/snow\" run"));
    }

    #[test]
    fn the_entry_shows_the_icon_it_is_given() {
        let e = desktop_entry("/usr/bin/snowlearner", "/home/ana/.local/share/snowlearner/snowlearner.png");
        assert!(e.contains("Icon=/home/ana/.local/share/snowlearner/snowlearner.png\n"), "{e}");
        assert!(desktop_entry("/usr/bin/snowlearner", FALLBACK_ICON).contains("Icon=accessories-dictionary\n"));
    }

    #[test]
    fn installing_twice_refreshes_the_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let first = install("/old/snowlearner", FALLBACK_ICON, dir.path()).unwrap();
        let second = install("/new/snowlearner", FALLBACK_ICON, dir.path()).unwrap();
        assert_eq!(first, second);
        assert!(std::fs::read_to_string(second).unwrap().contains("/new/snowlearner"));
    }
}
