//! Drives the real `snowlearner` binary with an isolated SNOWLEARNER_HOME.

use chrono::Local;
use snowlearner::control::ipc::{self, Command};
use snowlearner::store::history::{Attempt, History};
use std::process::{Command as Proc, Output};
use std::sync::mpsc;
use std::time::Duration;

struct Home {
    dir: tempfile::TempDir,
}

impl Home {
    fn new() -> Home {
        Home { dir: tempfile::tempdir().unwrap() }
    }

    fn run(&self, args: &[&str]) -> Output {
        Proc::new(env!("CARGO_BIN_EXE_snowlearner"))
            .args(args)
            .env("SNOWLEARNER_HOME", self.dir.path())
            .output()
            .expect("running snowlearner")
    }

    fn config(&self, body: &str) {
        let dir = self.dir.path().join("config");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.toml"), body).unwrap();
    }

    fn history(&self) -> History {
        History::open(&self.dir.path().join("data/history.sqlite3")).unwrap()
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[test]
fn help_lists_the_commands() {
    let o = Home::new().run(&["--help"]);
    assert!(o.status.success());
    for cmd in ["say", "summary", "report", "doctor", "snapshot", "model"] {
        assert!(stdout(&o).contains(cmd), "missing {cmd} in help");
    }
}

#[test]
fn decks_lists_english_by_default_and_spanish_on_request() {
    let home = Home::new();
    let en = home.run(&["decks"]);
    assert!(en.status.success(), "{}", stderr(&en));
    assert!(stdout(&en).contains("Sorry, you're on mute"));
    assert!(stdout(&en).contains("Desculpa, você está no mudo"));
    let es = home.run(&["decks", "--learning", "es"]);
    assert!(stdout(&es).contains("¿Puedes compartir tu pantalla?"));
}

#[test]
fn unknown_deck_fails_with_a_hint() {
    let o = Home::new().run(&["decks", "--learning", "fr"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("fr.toml"), "{}", stderr(&o));
}

#[test]
fn config_init_writes_once_and_refuses_to_clobber() {
    let home = Home::new();
    let first = home.run(&["config", "init"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let again = home.run(&["config", "init"]);
    assert!(!again.status.success());
    assert!(stderr(&again).contains("--force"));
    assert!(home.run(&["config", "init", "--force"]).status.success());
}

#[test]
fn invalid_config_is_reported_with_the_bad_key() {
    let home = Home::new();
    home.config("commitment = 'extreme'\n");
    let o = home.run(&["decks"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("config"), "{}", stderr(&o));
}

#[test]
fn report_on_an_empty_day_says_so() {
    let o = Home::new().run(&["report", "--date", "2026-01-01"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("Nenhuma frase"));
}

#[test]
fn report_shows_practiced_phrases_with_marks() {
    let home = Home::new();
    let h = home.history();
    for (say, ok) in [("I'm hungry", true), ("Good morning", false)] {
        h.record(&Attempt {
            at: Local::now(),
            language: "en".into(),
            say: say.into(),
            meaning: "m".into(),
            heard: say.into(),
            score: if ok { 1.0 } else { 0.2 },
            success: ok,
        })
        .unwrap();
    }
    let o = home.run(&["report"]);
    let out = stdout(&o);
    assert!(out.contains("✓ I'm hungry"), "{out}");
    assert!(out.contains("✗ Good morning"), "{out}");
    assert!(out.contains("1 de 2"), "{out}");
}

#[test]
fn report_rejects_malformed_dates() {
    let o = Home::new().run(&["report", "--date", "27/09/2026"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("YYYY-MM-DD"));
}

#[test]
fn say_without_a_running_app_explains_how_to_start_it() {
    let home = Home::new();
    home.config(&format!("ipc_port = {}\n", free_port()));
    let o = home.run(&["say"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("not running"), "{}", stderr(&o));
}

#[test]
fn say_and_summary_reach_a_running_instance() {
    let home = Home::new();
    let (tx, rx) = mpsc::channel();
    let addr = ipc::serve(0, move |c| tx.send(c).unwrap()).unwrap();
    home.config(&format!("ipc_port = {}\n", addr.port()));
    assert!(home.run(&["say"]).status.success());
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Command::Challenge);
    assert!(home.run(&["summary"]).status.success());
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Command::Summary);
}

#[test]
fn snapshot_writes_a_png_of_the_requested_size() {
    let home = Home::new();
    let out = home.dir.path().join("shot.png");
    let o = home.run(&[
        "snapshot",
        out.to_str().unwrap(),
        "--seconds",
        "5",
        "--width",
        "200",
        "--height",
        "100",
        "--scale",
        "2",
        "--caption",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let (w, h) =
        (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()));
    assert_eq!((w, h), (400, 200));
}

#[test]
fn snapshot_rejects_absurd_sizes() {
    let home = Home::new();
    let out = home.dir.path().join("x.png");
    let o = home.run(&["snapshot", out.to_str().unwrap(), "--width", "10"]);
    assert!(!o.status.success());
    assert!(!out.exists());
}

#[test]
fn doctor_reports_every_subsystem() {
    let o = Home::new().run(&["doctor"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    for line in ["platform", "display", "hotkeys", "voice (TTS)", "recognition", "deck", "config", "commitment"] {
        assert!(out.contains(line), "doctor missing {line:?}:\n{out}");
    }
}

#[test]
fn model_download_rejects_unknown_sizes_without_touching_the_network() {
    let o = Home::new().run(&["model", "download", "--name", "gigantic"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("unknown model"));
}
