//! Subcommand implementations.

use super::{Cli, Cmd, ConfigAction, ModelAction, RunArgs, ShortcutAction};
use crate::app::platform::{self, Session};
use crate::config::paths::Paths;
use crate::config::settings::{MODELS, Settings};
use crate::control::gnome;
use crate::control::ipc::{self, Command};
use crate::learn::deck::Deck;
use crate::render::{canvas::Canvas, png_out};
use crate::scene::Scene;
use crate::scene::hud::{Caption, Status};
use crate::speech::tts::Tts;
use crate::store::history::History;
use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use std::io::{Read, Write};

pub fn dispatch(cli: Cli) -> Result<()> {
    let paths = Paths::resolve()?;
    let settings = load_settings(&paths, &cli.run)?;
    match cli.command.unwrap_or(Cmd::Run) {
        Cmd::Run => crate::app::run(settings, paths, cli.run.level),
        Cmd::Say => remote(&settings, Command::Challenge),
        Cmd::Summary => remote(&settings, Command::Summary),
        Cmd::Quit => remote(&settings, Command::Quit),
        Cmd::Menu => remote(&settings, Command::Menu),
        Cmd::Shortcuts { action } => shortcuts(&settings, action),
        Cmd::Pause => remote(&settings, Command::Pause),
        Cmd::Report { date, speak } => report(&settings, &paths, date.as_deref(), speak),
        Cmd::Decks => decks(&settings, &paths),
        Cmd::Config { action: ConfigAction::Init { force } } => {
            let file = paths.config_file();
            Settings::write_default(&file, force)?;
            println!("wrote {}", file.display());
            Ok(())
        }
        Cmd::Config { action: ConfigAction::Path } => {
            println!("config   {}", paths.config_file().display());
            println!("decks    {}", paths.decks_dir().display());
            println!("history  {}", paths.db_file().display());
            println!("models   {}", paths.models_dir().display());
            Ok(())
        }
        Cmd::Model { action: ModelAction::Download { name } } => {
            download_model(&paths, name.as_deref().unwrap_or(&settings.model))
        }
        Cmd::Model { action: ModelAction::Path } => {
            let p = paths.model_file(&settings.model);
            println!("{} ({})", p.display(), if p.exists() { "present" } else { "missing" });
            Ok(())
        }
        Cmd::Doctor => doctor(&settings, &paths),
        Cmd::Snapshot { out, seconds, width, height, scale, overlay, caption, seed } => {
            snapshot(&settings, &paths, &out, seconds, width, height, scale, overlay, caption, seed, cli.run.level)
        }
    }
}

fn load_settings(paths: &Paths, run: &RunArgs) -> Result<Settings> {
    let mut s = Settings::load(&paths.config_file())?;
    if let Some(m) = run.mode {
        s.mode = m;
    }
    if let Some(l) = run.level {
        s.commitment = l;
    }
    if let Some(lang) = &run.learning {
        s.learning = lang.clone();
    }
    s.validate()?;
    Ok(s)
}

fn remote(settings: &Settings, cmd: Command) -> Result<()> {
    ipc::send(settings.ipc_port, cmd)
}

fn shortcuts(settings: &Settings, action: ShortcutAction) -> Result<()> {
    if !gnome::available() {
        bail!(
            "automatic shortcuts are only supported on GNOME; bind these commands in your desktop's keyboard settings: {} say / summary / menu",
            std::env::current_exe()?.display()
        );
    }
    match action {
        ShortcutAction::Install => {
            let exe = std::env::current_exe()?.canonicalize()?.display().to_string();
            let list = [
                gnome::Shortcut { id: "say", name: "Snowlearner: praticar", keys: settings.hotkey_challenge.clone() },
                gnome::Shortcut { id: "summary", name: "Snowlearner: resumo", keys: settings.hotkey_summary.clone() },
                gnome::Shortcut { id: "menu", name: "Snowlearner: painel", keys: settings.hotkey_menu.clone() },
            ];
            gnome::install(&exe, &list)?;
            for s in &list {
                println!("  {:<12} → {exe} {}", s.keys, s.id);
            }
            println!("GNOME shortcuts installed (remove with `snowlearner shortcuts remove`).");
        }
        ShortcutAction::Remove => println!("removed {} snowlearner shortcut(s)", gnome::remove()?),
    }
    Ok(())
}

fn parse_day(date: Option<&str>) -> Result<NaiveDate> {
    match date {
        None => Ok(Local::now().date_naive()),
        Some(d) => {
            NaiveDate::parse_from_str(d, "%Y-%m-%d").with_context(|| format!("--date must be YYYY-MM-DD, got {d:?}"))
        }
    }
}

fn report(settings: &Settings, paths: &Paths, date: Option<&str>, speak: bool) -> Result<()> {
    let day = parse_day(date)?;
    let history = History::open(&paths.db_file())?;
    let rows = history.day_summary(&settings.learning, day)?;
    println!("Resumo de {} ({})", day.format("%d/%m/%Y"), settings.learning);
    if rows.is_empty() {
        println!("  Nenhuma frase praticada.");
        return Ok(());
    }
    for r in &rows {
        let mark = if r.successes > 0 { "✓" } else { "✗" };
        println!("  {mark} {} = {}  ({}/{} acertos)", r.say, r.meaning, r.successes, r.attempts);
    }
    let learned = rows.iter().filter(|r| r.successes > 0).count();
    println!("  {learned} de {} frases acertadas", rows.len());
    if speak {
        let tts = Tts::detect();
        tts.speak(&format!("Hoje você praticou {learned} frases."), &settings.native, &settings.voice_native, false)?;
        for r in rows.iter().filter(|r| r.successes > 0) {
            tts.speak(&r.say, &settings.learning, &settings.voice_learning, true)?;
        }
    }
    Ok(())
}

fn decks(settings: &Settings, paths: &Paths) -> Result<()> {
    let deck = Deck::load(&settings.learning, &paths.decks_dir())?;
    println!("{} [{} → {}] {} phrases", deck.title, deck.native, deck.language, deck.phrases.len());
    for p in &deck.phrases {
        println!("  {:<32} {}", p.say, p.meaning);
    }
    Ok(())
}

fn download_model(paths: &Paths, name: &str) -> Result<()> {
    if !MODELS.contains(&name) {
        bail!("unknown model {name:?}; choose one of {MODELS:?}");
    }
    let dest = paths.model_file(name);
    if dest.exists() {
        println!("already present: {}", dest.display());
        return Ok(());
    }
    std::fs::create_dir_all(paths.models_dir())?;
    let url = format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{name}.bin");
    println!("downloading {url}");
    let resp = ureq::get(&url).call().with_context(|| format!("downloading {url}"))?;
    let total = resp.body().content_length();
    let mut reader = resp.into_body().into_with_config().limit(2_000_000_000).reader();
    let part = dest.with_extension("bin.part");
    let mut file = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut last_pct = u64::MAX;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        done += n as u64;
        if let Some(t) = total {
            let pct = done * 100 / t.max(1);
            if pct != last_pct {
                last_pct = pct;
                print!("\r  {pct:>3}%  {:.1}/{:.1} MB", done as f64 / 1e6, t as f64 / 1e6);
                std::io::stdout().flush().ok();
            }
        }
    }
    println!();
    if total.is_some_and(|t| t != done) || done < 1_000_000 {
        let _ = std::fs::remove_file(&part);
        bail!("download incomplete ({done} bytes); try again");
    }
    std::fs::rename(&part, &dest)?;
    println!("saved {}", dest.display());
    Ok(())
}

fn doctor(settings: &Settings, paths: &Paths) -> Result<()> {
    let session = Session::detect();
    let xwayland = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    let resolved = platform::resolve(settings.mode, session, xwayland);
    let ok = |b: bool| if b { "ok " } else { "!! " };
    println!("snowlearner {}", env!("CARGO_PKG_VERSION"));
    println!("{}platform      {}", ok(session != Session::Unknown), session.name());
    println!(
        "{}display       {:?} → {}{}",
        ok(true),
        settings.mode,
        if resolved.overlay { "overlay" } else { "window" },
        resolved.note.as_deref().map(|n| format!("  ({n})")).unwrap_or_default()
    );
    let hk = session.global_hotkeys();
    println!(
        "{}hotkeys       {}",
        ok(hk),
        if hk {
            format!(
                "{} practice · {} recap · {} panel",
                settings.hotkey_challenge, settings.hotkey_summary, settings.hotkey_menu
            )
        } else {
            "not available here — see below".into()
        }
    );
    let tts = Tts::detect();
    println!(
        "{}voice (TTS)   {}",
        ok(tts.engine().is_some()),
        tts.engine()
            .map(|e| e.name().to_string())
            .unwrap_or_else(|| "none — install speech-dispatcher or espeak-ng".into())
    );
    let stt_built = cfg!(feature = "stt");
    let model = paths.model_file(&settings.model);
    println!(
        "{}recognition   {}",
        ok(stt_built && model.exists()),
        match (stt_built, model.exists()) {
            (false, _) => "not in this build (phrases are confirmed with the hotkey)".to_string(),
            (true, true) => format!("whisper {} at {}", settings.model, model.display()),
            (true, false) => format!("model missing — run `snowlearner model download` ({})", model.display()),
        }
    );
    #[cfg(feature = "stt")]
    {
        let mic = crate::speech::mic::default_input_name();
        println!("{}microphone    {}", ok(mic.is_some()), mic.unwrap_or_else(|| "none found".into()));
    }
    match Deck::load(&settings.learning, &paths.decks_dir()) {
        Ok(d) => println!(
            "{}deck          {} ({} phrases, {} → {})",
            ok(true),
            d.title,
            d.phrases.len(),
            d.native,
            d.language
        ),
        Err(e) => println!("{}deck          {e:#}", ok(false)),
    }
    let cfg = paths.config_file();
    println!(
        "{}config        {}{}",
        ok(true),
        cfg.display(),
        if cfg.exists() { "" } else { " (defaults; `snowlearner config init`)" }
    );
    println!("{}commitment    {:?} ({})", ok(true), settings.commitment, settings.commitment.label_pt());
    if !hk {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "snowlearner".into());
        let bindings = [
            (settings.hotkey_challenge.as_str(), Command::Challenge),
            (settings.hotkey_summary.as_str(), Command::Summary),
            (settings.hotkey_menu.as_str(), Command::Menu),
            ("(any key)", Command::Pause),
        ];
        println!("\n{}", platform::shortcut_help(&exe, &bindings));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn snapshot(
    settings: &Settings,
    paths: &Paths,
    out: &std::path::Path,
    seconds: f32,
    width: i32,
    height: i32,
    scale: u32,
    overlay: bool,
    caption: bool,
    seed: u64,
    level: Option<crate::config::level::Commitment>,
) -> Result<()> {
    if !(80..=4000).contains(&width) || !(60..=4000).contains(&height) {
        bail!("--width must be 80..=4000 and --height 60..=4000");
    }
    let pace = level.unwrap_or(settings.commitment).pace();
    let mut scene = Scene::new(width, height, seed, pace, overlay);
    let deck = Deck::load(&settings.learning, &paths.decks_dir())?;
    scene.tips = vec![format!("Aperte {} e fale comigo!", settings.hotkey_challenge)];
    let dt = 1.0 / 30.0;
    for _ in 0..(seconds.max(0.0) / dt) as i32 {
        scene.step(dt);
    }
    if caption {
        let p = &deck.phrases[0];
        let target = p.cue.iter().position(|s| s.is_target());
        scene.hud.caption = Some(Caption {
            segments: p.cue.clone(),
            say: p.say.clone(),
            active: target,
            meaning: p.meaning.clone(),
            status: Status::Speaking,
            feedback: None,
            heard: None,
            footer: String::new(),
            tag: format!("{} · repita", p.topic),
        });
    }
    let mut canvas = Canvas::new(width, height);
    scene.draw(&mut canvas);
    png_out::write(&canvas, scale, out)?;
    println!("wrote {} ({}x{})", out.display(), width as u32 * scale, height as u32 * scale);
    Ok(())
}
