//! `install.sh` end to end: the real binary packaged like a release, served by
//! a local HTTP server, installed into a temp home by the real script.
#![cfg(all(unix, not(target_os = "macos")))]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TARGET: &str = "x86_64-unknown-linux-gnu";

/// Serves files from `dir` (GET only, one request per connection).
fn serve(dir: PathBuf) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap_or_default();
            let path = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
            let reply = match std::fs::read(dir.join(&path)) {
                Ok(body) => [
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                        .into_bytes(),
                    body,
                ]
                .concat(),
                Err(_) => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = stream.write_all(&reply);
        }
    });
    base
}

/// Packages the built binary exactly like `.github/workflows/release.yml`.
fn release_dir(root: &Path) -> PathBuf {
    let dist = root.join("dist");
    let name = format!("snowlearner-{TARGET}");
    std::fs::create_dir_all(dist.join(&name)).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_snowlearner"), dist.join(&name).join("snowlearner")).unwrap();
    let tar = Command::new("tar").current_dir(&dist).args(["-czf", &format!("{name}.tar.gz"), &name]).status().unwrap();
    assert!(tar.success());
    let sum = Command::new("sha256sum").current_dir(&dist).arg(format!("{name}.tar.gz")).output().unwrap();
    std::fs::write(dist.join(format!("{name}.tar.gz.sha256")), sum.stdout).unwrap();
    dist
}

fn install(root: &Path, base: &str, args: &[&str]) -> Output {
    // Exactly like `curl … | sh -s -- <args>`: the script arrives on stdin.
    let script = std::fs::File::open(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh")).unwrap();
    Command::new("sh")
        .args(["-s", "--"])
        .stdin(script)
        .args(args)
        .env("SNOWLEARNER_DOWNLOAD_BASE", base)
        .env("SNOWLEARNER_BIN_DIR", root.join("bin"))
        .env("SNOWLEARNER_HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("xdg"))
        .env("HOME", root)
        .env_remove("XDG_CURRENT_DESKTOP")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn one_command_installs_the_binary_and_runs_setup() {
    if std::env::consts::ARCH != "x86_64" {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let base = serve(release_dir(root.path()));
    let o = install(root.path(), &base, &["--lang", "es", "--no-model"]);
    assert!(o.status.success(), "{}", text(&o));
    let bin = root.path().join("bin/snowlearner");
    let version = Command::new(&bin).arg("--version").output().unwrap();
    assert!(String::from_utf8_lossy(&version.stdout).starts_with("snowlearner "));
    assert!(text(&o).contains("Pronto!"), "setup ran: {}", text(&o));
    let config = std::fs::read_to_string(root.path().join("home/config/config.toml")).unwrap();
    assert!(config.contains("learning = \"es\""), "setup got the arguments: {config}");
    assert!(root.path().join("xdg/applications/snowlearner.desktop").exists());
    assert!(text(&o).contains("Adicione"), "tells how to add the bin dir to PATH");
}

#[test]
fn a_tampered_download_is_refused_and_nothing_is_installed() {
    if std::env::consts::ARCH != "x86_64" {
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let dist = release_dir(root.path());
    std::fs::write(dist.join(format!("snowlearner-{TARGET}.tar.gz.sha256")), format!("{} x\n", "0".repeat(64)))
        .unwrap();
    let o = install(root.path(), &serve(dist), &["--no-model"]);
    assert!(!o.status.success());
    assert!(text(&o).contains("checksum não confere"), "{}", text(&o));
    assert!(!root.path().join("bin/snowlearner").exists());
}

#[test]
fn a_missing_release_explains_what_failed() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let o = install(root.path(), &serve(empty), &[]);
    assert!(!o.status.success());
    assert!(text(&o).contains("não consegui baixar"), "{}", text(&o));
}
