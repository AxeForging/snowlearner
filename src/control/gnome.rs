//! GNOME custom shortcuts: on Wayland apps can't grab global keys, so we ask
//! GNOME to run `snowlearner say|summary|menu` for us (via `gsettings`).
//! `snowlearner shortcuts install` adds them; `shortcuts remove` takes them out.

use anyhow::{Context, Result, bail};
use std::process::Command as Proc;

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const ENTRY: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
const BASE: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
const PREFIX: &str = "snowlearner-";

pub struct Shortcut {
    /// Short id, e.g. "say".
    pub id: &'static str,
    pub name: &'static str,
    /// Our hotkey syntax, e.g. "Ctrl+Alt+M".
    pub keys: String,
}

/// "Ctrl+Alt+M" → "<Control><Alt>m".
pub fn to_gnome_binding(spec: &str) -> Result<String> {
    let mut out = String::new();
    let parts: Vec<&str> = spec.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let Some((key, mods)) = parts.split_last() else { bail!("empty hotkey") };
    for m in mods {
        out.push_str(match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "<Control>",
            "alt" | "option" => "<Alt>",
            "shift" => "<Shift>",
            "super" | "meta" | "cmd" | "win" => "<Super>",
            other => bail!("unknown modifier {other:?} in {spec:?}"),
        });
    }
    let key = key.to_ascii_lowercase();
    let key = key.strip_prefix("key").filter(|k| k.len() == 1).unwrap_or(&key);
    out.push_str(key);
    Ok(out)
}

/// Parses a gsettings string array: "['/a/', '/b/']" or "@as []".
pub fn parse_list(v: &str) -> Vec<String> {
    let v = v.trim().trim_start_matches("@as").trim();
    let inner = v.trim_start_matches('[').trim_end_matches(']');
    inner.split(',').map(|s| s.trim().trim_matches('\'').to_string()).filter(|s| !s.is_empty()).collect()
}

pub fn format_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|i| format!("'{i}'")).collect::<Vec<_>>().join(", "))
}

fn path(id: &str) -> String {
    format!("{BASE}{PREFIX}{id}/")
}

/// Existing entries minus ours, plus ours (idempotent install).
pub fn merged(existing: &[String], ids: &[&str]) -> Vec<String> {
    let mut out: Vec<String> =
        existing.iter().filter(|p| !p.starts_with(&format!("{BASE}{PREFIX}"))).cloned().collect();
    out.extend(ids.iter().map(|id| path(id)));
    out
}

pub fn without_ours(existing: &[String]) -> Vec<String> {
    existing.iter().filter(|p| !p.starts_with(&format!("{BASE}{PREFIX}"))).cloned().collect()
}

pub fn available() -> bool {
    crate::speech::tts::in_path("gsettings").is_some()
        && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.to_uppercase().contains("GNOME"))
}

fn gsettings(args: &[&str]) -> Result<String> {
    let out = Proc::new("gsettings").args(args).output().context("running gsettings")?;
    if !out.status.success() {
        bail!("gsettings {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True when our shortcuts are registered in GNOME.
pub fn installed() -> bool {
    available()
        && gsettings(&["get", SCHEMA, "custom-keybindings"])
            .map(|v| parse_list(&v).iter().any(|p| p.starts_with(&format!("{BASE}{PREFIX}"))))
            .unwrap_or(false)
}

pub fn install(exe: &str, shortcuts: &[Shortcut]) -> Result<()> {
    for s in shortcuts {
        let p = format!("{ENTRY}:{}", path(s.id));
        gsettings(&["set", &p, "name", s.name])?;
        gsettings(&["set", &p, "command", &format!("{exe} {}", s.id)])?;
        gsettings(&["set", &p, "binding", &to_gnome_binding(&s.keys)?])?;
    }
    let current = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    let ids: Vec<&str> = shortcuts.iter().map(|s| s.id).collect();
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&merged(&current, &ids))])?;
    Ok(())
}

pub fn remove() -> Result<usize> {
    let current = parse_list(&gsettings(&["get", SCHEMA, "custom-keybindings"])?);
    let kept = without_ours(&current);
    let removed = current.len() - kept.len();
    gsettings(&["set", SCHEMA, "custom-keybindings", &format_list(&kept)])?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys_become_gnome_accelerators() {
        assert_eq!(to_gnome_binding("Ctrl+Alt+M").unwrap(), "<Control><Alt>m");
        assert_eq!(to_gnome_binding("super + shift + KeyJ").unwrap(), "<Super><Shift>j");
        assert!(to_gnome_binding("Hyper+M").is_err());
        assert!(to_gnome_binding("").is_err());
    }

    #[test]
    fn gsettings_lists_parse_both_empty_forms() {
        assert!(parse_list("@as []").is_empty());
        assert_eq!(parse_list("['/a/', '/b/']"), vec!["/a/", "/b/"]);
        assert_eq!(format_list(&["/a/".into()]), "['/a/']");
    }

    #[test]
    fn install_keeps_other_shortcuts_and_is_idempotent() {
        let mine = format!("{BASE}custom0/");
        let once = merged(std::slice::from_ref(&mine), &["say", "menu"]);
        assert_eq!(once.len(), 3);
        assert_eq!(once[0], mine, "user's own shortcut untouched");
        let twice = merged(&once, &["say", "menu"]);
        assert_eq!(once, twice);
        assert_eq!(without_ours(&twice), vec![mine]);
    }
}
