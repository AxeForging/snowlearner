//! Drives the real `snowlearner` binary with an isolated SNOWLEARNER_HOME.

use chrono::Local;
use snowlearner::config::settings::Settings;
use snowlearner::control::ipc::{self, Command};
use snowlearner::lang::T;
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
fn a_config_that_keeps_recordings_is_accepted_and_nothing_is_recorded_without_a_lesson() {
    let home = Home::new();
    home.config("keep_recordings = true\n");
    let o = home.run(&["decks"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(!home.dir.path().join("data").join("recordings").exists(), "only answers are kept");
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

/// A config from before 0.1.43 holds the old default `pixel_scale = 4`; the
/// first start after the update makes it automatic and writes that down (#204).
#[test]
fn the_first_start_after_an_update_writes_the_automatic_scale_into_an_old_config() {
    let home = Home::new();
    home.config("learning = \"es\"\ncommitment = \"committed\"\npixel_scale = 4\nmatch_threshold = 0.6\n");
    let o = home.run(&["doctor"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let path = home.dir.path().join("config/config.toml");
    let written = std::fs::read_to_string(&path).unwrap();
    for line in [
        "config_version = 1",
        "pixel_scale = 0",
        "learning = \"es\"",
        "commitment = \"committed\"",
        "match_threshold = 0.6",
    ] {
        assert!(written.contains(line), "missing {line:?}:\n{written}");
    }
    assert!(home.run(&["doctor"]).status.success());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), written, "written once");
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

/// On Linux the lists come from the sound server when one answers, else ALSA
/// (#205); a report has to say which.
#[cfg(feature = "audio")]
#[test]
fn the_device_lists_name_the_audio_system_they_come_from() {
    let hosts = ["ALSA", "PulseAudio", "WASAPI", "CoreAudio"];
    for (action, what) in [("mics", "microphones from "), ("speakers", "speakers from ")] {
        let o = Home::new().run(&["audio", action]);
        assert!(o.status.success(), "{}", stderr(&o));
        let out = stdout(&o);
        let head = out.lines().next().unwrap_or_default();
        assert!(head.starts_with(what) && hosts.iter().any(|h| head.contains(h)), "{head:?}");
    }
}

#[test]
fn doctor_names_the_speech_server_and_whether_it_answers() {
    let home = Home::new();
    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in server.incoming().flatten() {
            let mut buf = [0u8; 1024];
            let _ = std::io::Read::read(&mut s, &mut buf);
            let _ = std::io::Write::write_all(&mut s, b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
        }
    });
    home.config(&format!("stt_engine = 'asr'\nstt_url = 'http://127.0.0.1:{port}'\n"));
    let o = home.run(&["doctor"]);
    let out = stdout(&o);
    assert!(out.contains(&format!("asr server at http://127.0.0.1:{port}/asr (answers")), "{out}");
    assert!(!out.contains("model missing"), "a server needs no local model: {out}");

    let closed = free_port();
    home.config(&format!("stt_engine = 'openai'\nstt_url = 'http://127.0.0.1:{closed}/v1'\n"));
    let out = stdout(&home.run(&["doctor"]));
    assert!(out.contains("did not answer"), "{out}");
}

#[test]
fn doctor_checks_the_speech_key_and_never_prints_it_whole() {
    let home = Home::new();
    home.config("stt_engine = 'groq'\n");
    let out = stdout(&home.run(&["doctor"]));
    assert!(out.contains("groq server at https://api.groq.com/openai/v1/audio/transcriptions"), "{out}");
    assert!(out.contains("needs an API key"), "no key, no request: {out}");

    let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = server.local_addr().unwrap().port();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for mut s in server.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let n = std::io::Read::read(&mut s, &mut buf).unwrap_or(0);
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).to_string());
            let _ = std::io::Write::write_all(&mut s, b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        }
    });
    let key = "gsk_0123456789abcdefWXYZ";
    home.config(&format!("stt_engine = 'openai'\nstt_url = 'http://127.0.0.1:{port}/v1'\nstt_key = '{key}'\n"));
    let o = home.run(&["doctor"]);
    let (out, err) = (stdout(&o), stderr(&o));
    assert!(out.contains("takes the key gsk_…WXYZ"), "{out}");
    assert!(!out.contains(key) && !err.contains(key), "the key is never printed whole: {out}{err}");
    let req = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(req.starts_with("GET /v1/models "), "{req}");
}

#[test]
fn model_download_rejects_unknown_sizes_without_touching_the_network() {
    let o = Home::new().run(&["model", "download", "--name", "gigantic"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("unknown model"));
}

#[cfg(feature = "stt")]
#[test]
fn model_bench_without_a_model_says_how_to_get_one() {
    let o = Home::new().run(&["model", "bench"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("snowlearner model download"), "{}", stderr(&o));
}

#[cfg(feature = "stt")]
#[test]
fn model_bench_reports_a_wav_it_cannot_read() {
    let o = Home::new().run(&["model", "bench", "--wav", "/nope/answer.wav"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("reading /nope/answer.wav"), "{}", stderr(&o));
}

#[cfg(not(feature = "stt"))]
#[test]
fn model_bench_in_a_build_without_recognition_says_so() {
    let o = Home::new().run(&["model", "bench"]);
    assert!(!o.status.success());
    assert!(stderr(&o).contains("no speech recognition"), "{}", stderr(&o));
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

/// `exercise = "questions"` is a valid config (#220), and a history written
/// before the question columns existed still opens and still counts.
#[test]
fn the_question_exercise_is_accepted_and_a_history_from_before_still_counts() {
    let home = Home::new();
    home.config(&format!("exercise = \"questions\"\nipc_port = {}\n", free_port()));
    std::fs::create_dir_all(home.dir.path().join("data")).unwrap();
    {
        let c = rusqlite::Connection::open(home.dir.path().join("data/history.sqlite3")).unwrap();
        c.execute_batch(
            "CREATE TABLE attempts (id INTEGER PRIMARY KEY, at TEXT NOT NULL, day TEXT NOT NULL,
                 language TEXT NOT NULL, say TEXT NOT NULL, meaning TEXT NOT NULL, heard TEXT NOT NULL,
                 score REAL NOT NULL, success INTEGER NOT NULL);",
        )
        .unwrap();
        let now = Local::now();
        for _ in 0..2 {
            c.execute(
                "INSERT INTO attempts (at, day, language, say, meaning, heard, score, success)
                 VALUES (?1, ?2, 'en', 'Water.', 'Água.', 'water', 1.0, 1)",
                rusqlite::params![now.to_rfc3339(), now.date_naive().to_string()],
            )
            .unwrap();
        }
    }
    let o = home.run(&["doctor"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&home.run(&["progress", "--print"]));
    assert!(out.contains("Progresso em inglês: 1 de "), "the old answers still count: {out}");
    let h = home.history();
    assert!(h.question_log("en").unwrap().is_empty(), "and stay phrase answers");
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

#[test]
fn starting_again_opens_the_running_apps_panel_instead_of_failing() {
    let home = Home::new();
    let (tx, rx) = mpsc::channel();
    let addr = ipc::serve(0, move |c| tx.send(c).unwrap()).unwrap();
    home.config(&format!("ipc_port = {}\n", addr.port()));
    let o = home.run(&[]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(rx.recv_timeout(Duration::from_secs(3)).unwrap(), Command::Menu);
    let native = Settings::default().native;
    assert!(stdout(&o).contains(T::AlreadyRunning.get(native)), "{}", stdout(&o));
}

#[test]
fn starting_on_a_port_another_program_holds_names_the_port_and_where_to_change_it() {
    let home = Home::new();
    // Holds the port and never answers, like a program that is not snowlearner.
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = held.local_addr().unwrap().port();
    home.config(&format!("ipc_port = {port}\n"));
    let o = home.run(&[]);
    assert!(!o.status.success());
    let err = stderr(&o);
    assert!(err.contains(&port.to_string()) && err.contains("ipc_port"), "{err}");
    assert!(err.contains("config.toml"), "says which file to edit: {err}");
    assert!(!err.contains("already running"), "nothing else was running: {err}");
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
fn setup_shows_the_mage_in_the_app_menu_and_rewrites_it_every_time() {
    if !cfg!(all(unix, not(target_os = "macos"))) {
        return;
    }
    use snowlearner::render::{icon, png_out};
    let home = Home::new();
    for _ in 0..2 {
        let o = home.setup(&["--no-model"]);
        assert!(o.status.success(), "{}", stderr(&o));
        assert!(!stdout(&o).contains("! ícone"), "{}", stdout(&o));
    }
    let entry = std::fs::read_to_string(home.dir.path().join("xdg/applications/snowlearner.desktop")).unwrap();
    let path = entry.lines().find_map(|l| l.strip_prefix("Icon=")).expect("an Icon= line");
    let png = std::fs::read(path).unwrap_or_else(|e| panic!("Icon={path} must exist: {e}"));
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut px = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut px).unwrap();
    px.truncate(info.buffer_size());
    let art = snowlearner::scene::mage::portrait();
    assert_eq!((info.width, info.height), (art.w as u32 * icon::PNG_SCALE, art.h as u32 * icon::PNG_SCALE));
    assert_eq!(px, png_out::upscale(&art, icon::PNG_SCALE), "the menu icon is the mage");
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
