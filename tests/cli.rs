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
fn config_path_says_where_the_log_is() {
    let home = Home::new();
    let out = stdout(&home.run(&["config", "path"]));
    let log = home.dir.path().join("data").join("snowlearner.log");
    assert!(out.contains(&format!("log      {}", log.display())), "{out}");
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
fn snapshot_stages_the_snowstorm_as_an_animation() {
    let home = Home::new();
    let out = home.dir.path().join("storm.png");
    let o = home.run(&[
        "snapshot",
        out.to_str().unwrap(),
        "--scenario",
        "snowstorm",
        "--frames",
        "3",
        "--width",
        "200",
        "--height",
        "100",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    for f in 0..3 {
        assert!(home.dir.path().join(format!("storm-{f:03}.png")).exists(), "frame {f}");
    }
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

#[test]
fn progress_without_the_app_prints_a_path_that_starts_with_words() {
    let home = Home::new();
    home.config(&format!("ipc_port = {}\n", free_port()));
    let o = home.run(&["progress"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("Progresso em inglês: 0 de "), "{out}");
    assert!(out.contains("Etapa atual: palavras"), "{out}");
    assert!(out.contains("Aprendendo agora:"), "{out}");
}

/// Topic names under "Por tema:" in `progress --print`.
fn progress_topics(home: &Home) -> Vec<String> {
    let o = home.run(&["progress", "--print"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    let (_, by_topic) = out.split_once("Por tema:\n").expect("a per-topic section");
    by_topic.lines().filter_map(|l| l.split("  ").map(str::trim).find(|w| !w.is_empty())).map(String::from).collect()
}

#[test]
fn progress_covers_only_the_ticked_topics_and_an_old_single_topic_config_still_works() {
    let home = Home::new();
    let port = free_port();
    home.config(&format!("ipc_port = {port}\ntopic = 'restaurante'\nmax_level = 'B1'\n"));
    assert_eq!(progress_topics(&home), ["restaurante"], "the old `topic` key still loads");
    home.config(&format!("ipc_port = {port}\ntopics = ['viagem', 'Restaurante']\nmax_level = 'B1'\n"));
    let mut both = progress_topics(&home);
    both.sort();
    assert_eq!(both, ["restaurante", "viagem"], "several topics at once");
    home.config(&format!("ipc_port = {port}\ntopics = []\nmax_level = 'B1'\n"));
    assert!(progress_topics(&home).len() > 5, "nothing ticked = every topic");
}

#[test]
fn progress_counts_a_word_as_known_after_two_successes() {
    let home = Home::new();
    home.config(&format!("ipc_port = {}\n", free_port()));
    let h = home.history();
    for ok in [true, true, false] {
        let say = if ok { "Water." } else { "Hello." };
        h.record(&Attempt {
            at: Local::now(),
            language: "en".into(),
            say: say.into(),
            meaning: "m".into(),
            heard: say.into(),
            score: if ok { 1.0 } else { 0.1 },
            success: ok,
        })
        .unwrap();
    }
    let out = stdout(&home.run(&["progress", "--print"]));
    assert!(out.contains("Progresso em inglês: 1 de "), "{out}");
    assert!(out.contains(", 1 aprendendo"), "the missed word is still being learned: {out}");
    assert!(!out.contains("Aprendendo agora:\n  Water."), "known words leave the learning list: {out}");
}

#[test]
fn progress_opens_the_panel_of_a_running_instance() {
    let home = Home::new();
    let (tx, rx) = mpsc::channel();
    let addr = ipc::serve(0, move |c| tx.send(c).unwrap()).unwrap();
    home.config(&format!("ipc_port = {}\n", addr.port()));
    let o = home.run(&["progress"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Command::Progress);
    assert!(stdout(&o).is_empty(), "the panel shows it, not the terminal");
}

impl Home {
    /// `setup` without touching the real desktop: no GNOME, launcher in the temp home.
    fn setup(&self, args: &[&str]) -> Output {
        Proc::new(env!("CARGO_BIN_EXE_snowlearner"))
            .arg("setup")
            .args(args)
            .env("SNOWLEARNER_HOME", self.dir.path())
            .env("XDG_DATA_HOME", self.dir.path().join("xdg"))
            .env_remove("XDG_CURRENT_DESKTOP")
            .output()
            .expect("running snowlearner setup")
    }
}

#[test]
fn setup_writes_the_config_with_the_chosen_language_and_a_launcher() {
    let home = Home::new();
    let o = home.setup(&["--lang", "es", "--no-model"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let config = std::fs::read_to_string(home.dir.path().join("config/config.toml")).unwrap();
    assert!(config.contains("learning = \"es\""), "{config}");
    let out = stdout(&o);
    assert!(out.contains("Pronto!"), "{out}");
    assert!(out.contains("reconhecimento  pulado"), "{out}");
    if cfg!(all(unix, not(target_os = "macos"))) {
        let entry = std::fs::read_to_string(home.dir.path().join("xdg/applications/snowlearner.desktop")).unwrap();
        assert!(entry.contains(env!("CARGO_BIN_EXE_snowlearner").rsplit('/').next().unwrap()), "{entry}");
    }
    let progress = stdout(&home.run(&["progress", "--print"]));
    assert!(progress.contains("espanhol"), "setup's language is used afterwards: {progress}");
}

#[test]
fn setup_again_keeps_the_existing_config() {
    let home = Home::new();
    home.config("learning = \"es\"\ndaily_goal = 7\n");
    let o = home.setup(&["--no-model"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("(mantida)"));
    let config = std::fs::read_to_string(home.dir.path().join("config/config.toml")).unwrap();
    assert_eq!(config, "learning = \"es\"\ndaily_goal = 7\n", "user edits untouched");
}

#[test]
fn setup_rejects_an_unknown_language_before_changing_anything() {
    let home = Home::new();
    let o = home.setup(&["--lang", "klingon", "--no-model"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("klingon"), "{}", stderr(&o));
    assert!(!home.dir.path().join("config/config.toml").exists());
}

#[test]
fn an_english_speaker_gets_spanish_meanings_in_english() {
    let o = Home::new().run(&["decks", "--native", "en", "--learning", "es"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("[en → es]"), "{out}");
    assert!(out.lines().any(|l| l.contains("Hola.") && l.contains("Hello.")), "{out}");
    assert!(!out.contains("Olá."), "no Portuguese meanings: {out}");
}

#[test]
fn an_english_speaker_can_learn_brazilian_portuguese() {
    let home = Home::new();
    let o = home.setup(&["--native", "en", "--lang", "pt-BR", "--no-model"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let config = std::fs::read_to_string(home.dir.path().join("config/config.toml")).unwrap();
    assert!(config.contains("native = \"en\"") && config.contains("learning = \"pt-BR\""), "{config}");
    let progress = stdout(&home.run(&["progress", "--print"]));
    assert!(progress.contains("Progress in Portuguese"), "{progress}");
    assert!(progress.contains("Current stage: words"), "{progress}");
    let decks = stdout(&home.run(&["decks"]));
    assert!(decks.lines().any(|l| l.contains("Oi.") && l.contains("Hi.")), "{decks}");
}

#[test]
fn your_own_language_is_checked_before_anything_runs() {
    let home = Home::new();
    let o = home.run(&["decks", "--native", "fr"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("--native"), "{}", stderr(&o));
    let o = home.run(&["decks", "--native", "en"]);
    assert!(!o.status.success(), "English is the default deck: an English speaker can't learn it");
    assert!(stderr(&o).contains("own language"), "{}", stderr(&o));
    let o = home.setup(&["--lang", "pt-BR", "--no-model"]);
    assert!(!o.status.success(), "pt-BR speakers don't learn pt-BR");
    assert!(!home.dir.path().join("config/config.toml").exists());
}
