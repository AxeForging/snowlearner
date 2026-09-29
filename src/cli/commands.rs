//! Subcommand implementations.

use super::{AudioAction, Cli, Cmd, ConfigAction, ModelAction, RunArgs, ShortcutAction, VoicesAction};
use crate::app::platform::{self, Session};
use crate::config::paths::Paths;
use crate::config::settings::{MODELS, Settings};
use crate::control::gnome;
use crate::control::ipc::{self, Command};
use crate::learn::deck::Deck;
use crate::render::{canvas::Canvas, png_out};
use crate::scene::Scene;
use crate::scene::hud::{Caption, Status};
use crate::store::history::History;
use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use std::io::{Read, Write};

pub fn dispatch(cli: Cli) -> Result<()> {
    let paths = Paths::resolve()?;
    let settings = load_settings(&paths, &cli.run)?;
    match cli.command.unwrap_or(Cmd::Run) {
        Cmd::Run => {
            crate::control::console::release_own_console(&paths.log_file());
            crate::app::run(settings, paths, cli.run.level)
        }
        Cmd::Setup { lang, no_model } => setup(settings, &paths, lang.as_deref(), no_model),
        Cmd::Say => remote(&settings, Command::Challenge),
        Cmd::Summary => remote(&settings, Command::Summary),
        Cmd::Quit => remote(&settings, Command::Quit),
        Cmd::Menu => remote(&settings, Command::Menu),
        Cmd::Shortcuts { action } => shortcuts(&settings, action),
        Cmd::Pause => remote(&settings, Command::Pause),
        Cmd::Grab => remote(&settings, Command::Grab),
        Cmd::Progress { print: false } if remote(&settings, Command::Progress).is_ok() => Ok(()),
        Cmd::Progress { .. } => progress(&settings, &paths),
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
            println!("log      {}", paths.log_file().display());
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
        Cmd::Audio { action } => audio(&settings, &paths, action),
        Cmd::Voices { action } => voices(&settings, action),
        Cmd::Snapshot { out, seconds, width, height, scale, overlay, caption, seed, scenario, frames, fps } => {
            let shot = Shot { seconds, width, height, scale, overlay, caption, seed, scenario, frames, fps };
            snapshot(&settings, &paths, &out, shot, cli.run.level)
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
            let list = shortcut_list(settings);
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

fn shortcut_list(settings: &Settings) -> Vec<gnome::Shortcut> {
    vec![
        gnome::Shortcut { id: "say", name: "Snowlearner: praticar", keys: settings.hotkey_challenge.clone() },
        gnome::Shortcut { id: "summary", name: "Snowlearner: resumo", keys: settings.hotkey_summary.clone() },
        gnome::Shortcut { id: "menu", name: "Snowlearner: painel", keys: settings.hotkey_menu.clone() },
        gnome::Shortcut { id: "grab", name: "Snowlearner: mão mágica", keys: settings.hotkey_grab.clone() },
        gnome::Shortcut { id: "progress", name: "Snowlearner: progresso", keys: settings.hotkey_progress.clone() },
    ]
}

/// Everything a first start needs, each step idempotent and non-fatal: a
/// missing network or desktop feature is reported, not an abort.
fn setup(mut settings: Settings, paths: &Paths, lang: Option<&str>, no_model: bool) -> Result<()> {
    let file = paths.config_file();
    if let Some(lang) = lang {
        Deck::load(lang, &paths.decks_dir()).with_context(|| format!("--lang {lang}"))?;
        settings.learning = lang.to_string();
    }
    if lang.is_some() || !file.exists() {
        settings.save(&file)?;
        println!("✓ configuração    {} (aprendendo: {})", file.display(), settings.learning);
    } else {
        println!("✓ configuração    {} (mantida)", file.display());
    }

    if no_model || !cfg!(feature = "stt") {
        println!("- reconhecimento  pulado: você confirma as frases com {}", settings.hotkey_challenge);
    } else if let Err(e) = download_model(paths, &settings.model) {
        println!("! reconhecimento  não baixou ({e:#}); tente de novo: snowlearner model download");
    } else {
        println!("✓ reconhecimento  modelo {} pronto (offline)", settings.model);
    }

    let exe = std::env::current_exe()?.canonicalize()?.display().to_string();
    if gnome::available() {
        match gnome::install(&exe, &shortcut_list(&settings)) {
            Ok(()) => println!(
                "✓ atalhos         GNOME: {} praticar, {} progresso",
                settings.hotkey_challenge, settings.hotkey_progress
            ),
            Err(e) => println!("! atalhos         não registrados ({e:#})"),
        }
    } else if platform::Session::detect().global_hotkeys() {
        println!("✓ atalhos         {} praticar, {} progresso", settings.hotkey_challenge, settings.hotkey_progress);
    } else {
        println!("! atalhos         configure no seu desktop: `snowlearner doctor` mostra como");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    match crate::control::launcher::applications_dir().map(|d| crate::control::launcher::install(&exe, &d)) {
        Some(Ok(f)) => println!("✓ menu de apps    {}", f.display()),
        Some(Err(e)) => println!("! menu de apps    {e:#}"),
        None => {}
    }

    println!("\nPronto! Abra o Snowlearner pelo menu de apps ou rode: snowlearner");
    println!("Você começa pelas primeiras palavras; veja seu progresso com {}.", settings.hotkey_progress);
    Ok(())
}

fn sample_text(lang: &str) -> &'static str {
    match lang.split('-').next().unwrap_or(lang) {
        "pt" => "Olá! Esta é a voz em português.",
        "es" => "¡Hola! Esta es la voz en español.",
        _ => "Hello! This is the English voice.",
    }
}

fn voices(settings: &Settings, action: VoicesAction) -> Result<()> {
    let voice = settings.voice();
    match action {
        VoicesAction::List { lang } => {
            let lang = lang.unwrap_or_else(|| settings.learning.clone());
            let list = voice.voices(&lang)?;
            println!("{} · {lang}: {} voice(s)", voice.describe(), list.len());
            for v in list {
                println!("  {v}");
            }
        }
        VoicesAction::Test { lang, voice: name, text } => {
            let lang = lang.unwrap_or_else(|| settings.learning.clone());
            let configured = if lang == settings.native { &settings.voice_native } else { &settings.voice_learning };
            let name = name.unwrap_or_else(|| configured.clone());
            let text = text.unwrap_or_else(|| sample_text(&lang).to_string());
            println!("{} · {lang} · {}", voice.describe(), if name.is_empty() { "automatic voice" } else { &name });
            voice.speak(&text, &lang, &name, crate::speech::tts::Speed::Normal)?;
        }
    }
    Ok(())
}

#[cfg(feature = "audio")]
fn audio(settings: &Settings, paths: &Paths, action: AudioAction) -> Result<()> {
    use crate::speech::audio;
    let mark = |name: &str, chosen: &str| if name == chosen { "*" } else { " " };
    match action {
        AudioAction::Mics => {
            println!("microphones (* = configured; empty config = system default):");
            for m in audio::input_names() {
                println!(" {} {m}", mark(&m, &settings.mic));
            }
        }
        AudioAction::Speakers => {
            println!("speakers (* = configured; empty config = system default):");
            for m in audio::output_names() {
                println!(" {} {m}", mark(&m, &settings.speaker));
            }
        }
        AudioAction::TestMic => test_mic(settings, paths)?,
    }
    Ok(())
}

#[cfg(not(feature = "audio"))]
fn audio(_: &Settings, _: &Paths, _: AudioAction) -> Result<()> {
    bail!("this build has no audio support (build with the default features)")
}

#[cfg(feature = "stt")]
fn test_mic(settings: &Settings, paths: &Paths) -> Result<()> {
    use crate::speech::endpoint::ListenPlan;
    use std::sync::atomic::AtomicBool;
    let mic = if settings.mic.is_empty() { "system default".to_string() } else { settings.mic.clone() };
    println!("Mic: {mic}. Say something in {} (you have ~6 s to start)...", settings.learning);
    let stop = AtomicBool::new(false);
    let plan = ListenPlan { think: 6.0, expected: 3.0 };
    let rec = crate::speech::mic::record(plan, &settings.mic, &stop, |level, speaking| {
        let bars = ((level.sqrt() * 2.2).clamp(0.0, 1.0) * 30.0) as usize;
        print!("\r  [{:<30}] {}", "#".repeat(bars), if speaking { "speaking " } else { "listening" });
        std::io::stdout().flush().ok();
    })?;
    println!();
    if !rec.heard_speech {
        println!("No speech detected. Check the mic and its volume, or pick another: `snowlearner audio mics`.");
        return Ok(());
    }
    let recognizer = crate::speech::stt::Recognizer::load(&paths.model_file(&settings.model))?;
    let text = recognizer.transcribe(&rec.samples, &settings.learning)?;
    println!("Heard: \"{text}\"");
    Ok(())
}

#[cfg(all(feature = "audio", not(feature = "stt")))]
fn test_mic(_: &Settings, _: &Paths) -> Result<()> {
    bail!("this build has no speech recognition")
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
        let tts = settings.voice();
        tts.speak(
            &format!("Hoje você praticou {learned} frases."),
            &settings.native,
            &settings.voice_native,
            crate::speech::tts::Speed::Normal,
        )?;
        for r in rows.iter().filter(|r| r.successes > 0) {
            tts.speak(
                &r.say,
                &settings.learning,
                &settings.voice_learning,
                crate::app::lesson::target_speed(&settings.max_level),
            )?;
        }
    }
    Ok(())
}

fn progress(settings: &Settings, paths: &Paths) -> Result<()> {
    let deck = Deck::load(&settings.learning, &paths.decks_dir())?;
    let history = History::open(&paths.db_file())?;
    let stats = history.stats(&deck.language, Local::now().date_naive())?;
    let selection = deck.selection(settings.topic_filter().as_deref(), &settings.max_level);
    let report = crate::learn::progress::progress(&deck.phrases, &selection, &stats);
    print!("{}", report.render_text(&deck.language_name));
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
                "{} practice · {} recap · {} panel · {} progress",
                settings.hotkey_challenge, settings.hotkey_summary, settings.hotkey_menu, settings.hotkey_progress
            )
        } else {
            "not available here — see below".into()
        }
    );
    let voice = settings.voice();
    println!(
        "{}voice (TTS)   {}{}",
        ok(voice.available()),
        voice.describe(),
        if voice.available() { "" } else { " — install speech-dispatcher/espeak-ng or set tts_engine" }
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
        let mic =
            if settings.mic.is_empty() { crate::speech::mic::default_input_name() } else { Some(settings.mic.clone()) };
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
            (settings.hotkey_grab.as_str(), Command::Grab),
            (settings.hotkey_progress.as_str(), Command::Progress),
            ("(any key)", Command::Pause),
        ];
        println!("\n{}", platform::shortcut_help(&exe, &bindings));
    }
    Ok(())
}

/// A scripted event in a snapshot clip: (seconds into the clip, action).
type Cue = (f32, Box<dyn Fn(&mut Scene)>);

pub struct Shot {
    pub seconds: f32,
    pub width: i32,
    pub height: i32,
    pub scale: u32,
    pub overlay: bool,
    pub caption: bool,
    pub seed: u64,
    pub scenario: super::Scenario,
    pub frames: u32,
    pub fps: u32,
}

fn lesson_caption(p: &crate::learn::deck::Phrase, status: Status) -> Caption {
    Caption {
        segments: p.cue.clone(),
        say: p.say.clone(),
        active: p.cue.iter().position(|s| s.is_target()),
        meaning: p.meaning.clone(),
        status,
        feedback: None,
        heard: None,
        footer: String::new(),
        tag: match &p.level {
            Some(l) => format!("{} · {l} · repita", p.topic),
            None => format!("{} · repita", p.topic),
        },
        listen: None,
    }
}

fn snapshot(
    settings: &Settings,
    paths: &Paths,
    out: &std::path::Path,
    shot: Shot,
    level: Option<crate::config::level::Commitment>,
) -> Result<()> {
    use super::Scenario;
    use crate::scene::hud::{Meter, Stats};
    let Shot { seconds, width, height, scale, overlay, caption, seed, scenario, frames, fps } = shot;
    if !(80..=4000).contains(&width) || !(60..=4000).contains(&height) {
        bail!("--width must be 80..=4000 and --height 60..=4000");
    }
    if !(1..=600).contains(&frames) || !(1..=60).contains(&fps) {
        bail!("--frames must be 1..=600 and --fps 1..=60");
    }
    let pace = level.unwrap_or(settings.commitment).pace();
    let mut scene = Scene::new(width, height, seed, pace, overlay);
    let deck = Deck::load(&settings.learning, &paths.decks_dir())?;
    scene.tips = vec![format!("Aperte {} e fale comigo!", settings.hotkey_challenge)];
    scene.show_orb = !overlay;
    scene.hole = (10.0, 10.0);
    let step = |scene: &mut Scene, secs: f32| {
        for _ in 0..(secs.max(0.0) * 30.0) as i32 {
            scene.step(1.0 / 30.0);
        }
    };
    step(&mut scene, seconds);
    let phrase = deck.phrases.iter().find(|p| p.situation.is_some()).unwrap_or(&deck.phrases[0]).clone();
    // Scripted events at a given frame time (seconds into the clip).
    let mut script: Vec<Cue> = Vec::new();
    match scenario {
        Scenario::Idle => {
            if caption {
                scene.hud.caption = Some(lesson_caption(&phrase, Status::Speaking));
            }
        }
        Scenario::Lesson => {
            scene.hud.stats = Some(Stats { done: 4, goal: 10, combo: 2, label: "EN · trabalho".into() });
            scene.set_practicing(true);
            scene.mage_say("Repita, se for capaz!", 3.0);
            step(&mut scene, 1.0);
            scene.set_listening(true);
            let mut cap = lesson_caption(&phrase, Status::Listening);
            cap.active = None;
            cap.footer = format!("Terminou? {} · Esc: cancelar", settings.hotkey_challenge);
            cap.listen = Some(Meter { level: 0.0, speaking: false, think_left: 6.0, think_total: 6.0 });
            scene.hud.caption = Some(cap);
            let say = phrase.say.clone();
            script.push((0.0, Box::new(|s: &mut Scene| s.set_listening(true))));
            for i in 0..24 {
                let t = 0.4 + i as f32 * 0.08;
                script.push((
                    t,
                    Box::new(move |s: &mut Scene| {
                        if let Some(m) = s.hud.caption.as_mut().and_then(|c| c.listen.as_mut()) {
                            m.speaking = true;
                            m.level = 0.03 + 0.12 * ((i as f32 * 1.7).sin().abs());
                        }
                    }),
                ));
            }
            script.push((
                2.4,
                Box::new(move |s: &mut Scene| {
                    if let Some(c) = s.hud.caption.as_mut() {
                        c.listen = None;
                        c.status = Status::Passed;
                        c.footer = "Ctrl+Alt+M: próxima frase".into();
                        c.heard = Some(say.to_lowercase());
                        c.feedback = Some(
                            c.say
                                .split_whitespace()
                                .map(|w| crate::speech::matcher::WordHit { word: w.into(), hit: true })
                                .collect(),
                        );
                    }
                    s.celebrate(1.0);
                    s.mage_say("Argh! Não!", 3.0);
                }),
            ));
        }
        Scenario::Fight => {
            scene.spawn_mobs();
            step(&mut scene, 4.0);
            script.push((0.2, Box::new(|s: &mut Scene| s.cast_skill(crate::scene::Skill::IcicleRain))));
            script.push((2.4, Box::new(|s: &mut Scene| s.cast_skill(crate::scene::Skill::Friend))));
        }
        Scenario::Blackhole => {
            script.push((0.6, Box::new(|s: &mut Scene| s.set_paused(true))));
            script.push((3.6, Box::new(|s: &mut Scene| s.set_paused(false))));
        }
    }
    let mut canvas = Canvas::new(width, height);
    let frame_dt = 1.0 / fps as f32;
    let mut t = 0.0f32;
    let stem = out.with_extension("");
    for f in 0..frames {
        for (at, action) in &script {
            if *at >= t && *at < t + frame_dt {
                action(&mut scene);
            }
        }
        scene.draw(&mut canvas);
        let path = if frames == 1 {
            out.to_path_buf()
        } else {
            std::path::PathBuf::from(format!("{}-{f:03}.png", stem.display()))
        };
        png_out::write(&canvas, scale, &path)?;
        let sub = (frame_dt * 30.0).round().max(1.0) as i32;
        for _ in 0..sub {
            scene.step(frame_dt / sub as f32);
        }
        t += frame_dt;
    }
    if frames == 1 {
        println!("wrote {} ({}x{})", out.display(), width as u32 * scale, height as u32 * scale);
    } else {
        println!(
            "wrote {frames} frames {}-000.png … ({}x{})",
            stem.display(),
            width as u32 * scale,
            height as u32 * scale
        );
    }
    Ok(())
}
