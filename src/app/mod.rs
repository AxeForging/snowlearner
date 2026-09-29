//! The desktop app: window/overlay, the control panel window, frame loop,
//! hotkeys, IPC and speech — wired to the scene and the lesson controller.

pub mod lesson;
pub mod menu;
pub mod platform;

use crate::app::lesson::target_speed;
use crate::config::level::Commitment;
use crate::config::paths::Paths;
use crate::config::settings::Settings;
use crate::control::hotkeys::Hotkeys;
use crate::control::ipc::{self, Command};
use crate::learn::deck::Deck;
use crate::render::canvas::Canvas;
use crate::render::screen::Screen;
use crate::scene::Scene;
use crate::scene::hud::Cheats;
use crate::speech::tts::Speed;
use crate::speech::worker::{Job, Speech, SpeechEvent, Utterance, VoiceSettings};
use crate::store::history::History;
use anyhow::{Context, Result};
use lesson::{Input, Lesson, Options};
use menu::{Action, Item, Menu};
use platform::{Resolved, Session};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, Modifiers, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

const FPS: f32 = 30.0;
/// Window mode keeps at least this many art pixels on screen.
const WINDOW_VIRTUAL: (u32, u32) = (320, 180);
const MENU_SCALE: u32 = 3;
/// Job ids at or above this belong to the panel's mic/voice tests, not lessons.
const TEST_IDS: u64 = 1 << 40;
/// Orb window size in art pixels.
const ORB_ART: i32 = 22;

const HELP: &[&str] = &[
    "TECLAS",
    "Espaço: praticar    S: resumo do dia",
    "M: painel           P: pausar o mago",
    "1-4: compromisso    L: trocar idioma",
    "H: esta ajuda       Esc: cancelar",
    "Orbe: clique = praticar · Ctrl+clique = painel",
    "      botão direito = pausar (buraco negro!)",
    "Clique no mago, no guerreiro, na fogueira...",
];

#[derive(Debug)]
enum UserEvent {
    Ipc(Command),
    Speech(SpeechEvent),
    /// Device and voice lists for the panel, loaded off the UI thread.
    AudioLists(u64, Box<AudioLists>),
}

/// What the ÁUDIO tab cycles through. Loading it can be slow (a TTS server
/// to ask, PowerShell to start, many audio devices), so it never runs on the
/// UI thread.
#[derive(Debug)]
struct AudioLists {
    mics: Vec<String>,
    speakers: Vec<String>,
    voices_native: Vec<String>,
    voices_learning: Vec<String>,
}

impl AudioLists {
    fn load(settings: &Settings) -> AudioLists {
        let voice = settings.voice();
        // The panel cycles with ←/→: keep the lists short (the CLI shows everything).
        let short = |mut v: Vec<String>| {
            v.truncate(40);
            v
        };
        #[cfg(feature = "audio")]
        let (mics, speakers) = (crate::speech::audio::input_names(), crate::speech::audio::output_names());
        #[cfg(not(feature = "audio"))]
        let (mics, speakers) = (Vec::new(), Vec::new());
        AudioLists {
            mics,
            speakers,
            voices_native: short(voice.voices(&settings.native).unwrap_or_default()),
            voices_learning: short(voice.voices(&settings.learning).unwrap_or_default()),
        }
    }
}

pub fn run(settings: Settings, paths: Paths, level: Option<Commitment>) -> Result<()> {
    let session = Session::detect();
    let xwayland = std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty());
    let resolved = platform::resolve(settings.mode, session, xwayland);
    if let Some(note) = &resolved.note {
        eprintln!("{note}");
    }

    let deck = Deck::load(&settings.learning, &paths.decks_dir())?;
    let history = History::open(&paths.db_file())?;
    let event_loop = build_event_loop(&resolved)?;
    let proxy = event_loop.create_proxy();

    let ipc_proxy = proxy.clone();
    ipc::serve(settings.ipc_port, move |cmd| {
        let _ = ipc_proxy.send_event(UserEvent::Ipc(cmd));
    })?;

    let speech_proxy = proxy.clone();
    let speech = Speech::start(VoiceSettings::from(&settings, &paths), move |ev| {
        let _ = speech_proxy.send_event(UserEvent::Speech(ev));
    });
    if !speech.tts_ok {
        eprintln!("Voice engine unavailable ({}); captions will still show. Run `snowlearner doctor`.", speech.tts);
    }
    if !speech.can_listen {
        eprintln!(
            "Speech recognition off (no model). Phrases are confirmed with the hotkey. `snowlearner model download` enables it."
        );
    }

    let bindings = [
        (settings.hotkey_challenge.as_str(), Command::Challenge),
        (settings.hotkey_summary.as_str(), Command::Summary),
        (settings.hotkey_menu.as_str(), Command::Menu),
        (settings.hotkey_grab.as_str(), Command::Grab),
        (settings.hotkey_progress.as_str(), Command::Progress),
    ];
    let hotkeys = if session.global_hotkeys() {
        match Hotkeys::register(&bindings) {
            Ok(h) => {
                for why in &h.taken {
                    eprintln!("Hotkey unavailable: {why}");
                }
                Some(h)
            }
            Err(e) => {
                eprintln!("Global hotkeys unavailable: {e:#}");
                None
            }
        }
    } else if crate::control::gnome::installed() {
        let keys: Vec<&str> = bindings.iter().map(|(k, _)| *k).collect();
        eprintln!("Hotkeys: GNOME shortcuts active ({}).", keys.join(", "));
        let missing = crate::control::gnome::missing(&["say", "summary", "menu", "grab", "progress"]);
        if !missing.is_empty() {
            eprintln!("  not registered yet: {} — run `snowlearner setup` to add them.", missing.join(", "));
        }
        None
    } else {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "snowlearner".into());
        eprintln!("{}", platform::shortcut_help(&exe, &bindings));
        None
    };

    let commitment = level.unwrap_or(settings.commitment);
    let opts = Options {
        native: settings.native.clone(),
        threshold: settings.match_threshold,
        can_listen: speech.can_listen,
        listen_seconds: settings.listen_seconds,
        hotkey: settings.hotkey_challenge.clone(),
        summary_hotkey: settings.hotkey_summary.clone(),
        summary_at: settings.summary_at()?,
        ask_every: commitment.pace().ask_every,
        practice: settings.practice,
        topic: settings.topic_filter(),
        max_level: settings.max_level.clone(),
        daily_goal: settings.daily_goal,
    };
    let languages = available_languages(&paths, &settings.learning);
    let menu = Menu::new(languages, deck.topics());
    let lesson = Lesson::new(deck, opts, history);

    let mut app = App {
        proxy: proxy.clone(),
        lists_gen: 0,
        settings,
        paths,
        resolved,
        commitment,
        lesson,
        speech,
        hotkeys,
        main: None,
        scene: None,
        menu,
        menu_win: None,
        want_menu: false,
        progress_at: Instant::now(),
        modifiers: Modifiers::default(),
        orb: None,
        orb_press: None,
        orb_cursor: (0.0, 0.0),
        overlay_origin: (0, 0),
        save_orb_at: None,
        orb_hover: false,
        mods_held: false,
        hand_until: None,
        hand_on: false,
        test_id: TEST_IDS,
        cursor: (0.0, 0.0),
        last: Instant::now(),
        next_frame: Instant::now(),
    };
    event_loop.run_app(&mut app).context("event loop failed")?;
    Ok(())
}

/// Built-in decks plus any custom `<decks_dir>/<lang>.toml`.
fn available_languages(paths: &Paths, current: &str) -> Vec<String> {
    let mut langs: Vec<String> = Deck::builtin_languages().iter().map(|l| l.to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(paths.decks_dir()) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "toml")
                && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                && !langs.iter().any(|l| l == stem)
            {
                langs.push(stem.to_string());
            }
        }
    }
    if !langs.iter().any(|l| l == current) {
        langs.push(current.to_string());
    }
    langs
}

fn build_event_loop(resolved: &Resolved) -> Result<EventLoop<UserEvent>> {
    let mut builder = EventLoop::<UserEvent>::with_user_event();
    #[cfg(all(unix, not(target_os = "macos")))]
    if resolved.force_x11 {
        use winit::platform::x11::EventLoopBuilderExtX11;
        builder.with_x11();
    }
    let _ = resolved;
    builder.build().context("could not open a window (no display?)")
}

/// A window with its screen (CPU or GPU presenter) and art canvas.
struct Surface {
    window: Arc<Window>,
    screen: Screen,
    canvas: Canvas,
    scale: u32,
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    /// Latest panel list request; older answers are dropped.
    lists_gen: u64,
    settings: Settings,
    paths: Paths,
    resolved: Resolved,
    commitment: Commitment,
    lesson: Lesson,
    speech: Speech,
    hotkeys: Option<Hotkeys>,
    main: Option<Surface>,
    scene: Option<Scene>,
    menu: Menu,
    menu_win: Option<Surface>,
    /// Open the panel on the next loop turn (needs the ActiveEventLoop).
    want_menu: bool,
    /// When the PROGRESSO numbers were last read from the history.
    progress_at: Instant,
    cursor: (f64, f64),
    modifiers: Modifiers,
    /// The draggable magic orb (overlay mode; window mode draws it in-scene).
    orb: Option<Surface>,
    orb_press: Option<(f64, f64)>,
    orb_cursor: (f64, f64),
    /// Top-left of the overlay window in screen pixels (maps the orb to the scene).
    overlay_origin: (i32, i32),
    save_orb_at: Option<Instant>,
    /// Why the cheat sheet is up: hovering the orb and/or Ctrl+Alt held.
    orb_hover: bool,
    mods_held: bool,
    /// Magic hand requested by `snowlearner grab` until this moment.
    hand_until: Option<Instant>,
    hand_on: bool,
    test_id: u64,
    last: Instant,
    next_frame: Instant,
}

impl App {
    fn overlay_attributes(&self, el: &ActiveEventLoop) -> WindowAttributes {
        let monitor = el.primary_monitor().or_else(|| el.available_monitors().next());
        let (pos, size) = monitor
            .map(|m| (m.position(), m.size()))
            .unwrap_or((PhysicalPosition::new(0, 0), PhysicalSize::new(1280, 720)));
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("Snowlearner")
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(pos)
            .with_inner_size(size);
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::x11::WindowAttributesExtX11;
            // Unmanaged window: stays above, no taskbar entry, no focus stealing.
            attrs = attrs.with_override_redirect(true);
        }
        attrs
    }

    fn window_attributes(&self) -> WindowAttributes {
        Window::default_attributes()
            .with_title("Snowlearner")
            .with_inner_size(PhysicalSize::new(1280, 720))
            .with_min_inner_size(PhysicalSize::new(WINDOW_VIRTUAL.0, WINDOW_VIRTUAL.1))
    }

    fn fit(&mut self, width: u32, height: u32) {
        let Some(main) = &mut self.main else { return };
        main.scale = if self.resolved.overlay {
            self.settings.pixel_scale
        } else {
            (width / WINDOW_VIRTUAL.0).min(height / WINDOW_VIRTUAL.1).max(1)
        };
        let (vw, vh) = (width.div_ceil(main.scale) as i32, height.div_ceil(main.scale) as i32);
        main.canvas = Canvas::new(vw, vh);
        if let Some(scene) = &mut self.scene {
            scene.resize(vw, vh);
        }
    }

    fn input(&mut self, input: Input) {
        let Some(scene) = &mut self.scene else { return };
        for job in self.lesson.handle(input, scene) {
            self.speech.send(job);
        }
    }

    fn command(&mut self, cmd: Command, el: &ActiveEventLoop) {
        match cmd {
            Command::Challenge => self.input(Input::Primary),
            Command::Summary => self.input(Input::Summary),
            Command::Dismiss => self.input(Input::Dismiss),
            Command::Menu => self.want_menu = true,
            Command::Progress => {
                self.menu.show(menu::Tab::Progress);
                self.menu.progress = None; // fresh numbers
                self.want_menu = true;
            }
            Command::Pause => self.toggle_pause(),
            Command::Grab => {
                self.hand_until = match self.hand_until {
                    Some(_) => None,
                    None => Some(Instant::now() + Duration::from_secs(15)),
                };
            }
            Command::Quit => el.exit(),
        }
    }

    fn ctrl(&self) -> bool {
        self.modifiers.state().control_key() || self.modifiers.state().super_key()
    }

    fn open_orb(&mut self, el: &ActiveEventLoop) {
        let scale = self.settings.pixel_scale.max(2);
        let side = ORB_ART as u32 * scale;
        let monitor = el.primary_monitor().or_else(|| el.available_monitors().next());
        let (mpos, msize) = monitor
            .map(|m| (m.position(), m.size()))
            .unwrap_or((PhysicalPosition::new(0, 0), PhysicalSize::new(1280, 720)));
        let pos = if self.settings.orb_x >= 0 && self.settings.orb_y >= 0 {
            PhysicalPosition::new(self.settings.orb_x, self.settings.orb_y)
        } else {
            PhysicalPosition::new(mpos.x + msize.width as i32 - side as i32 - 24, mpos.y + 48)
        };
        #[allow(unused_mut)]
        let mut attrs = Window::default_attributes()
            .with_title("Snowlearner orb")
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(pos)
            .with_inner_size(PhysicalSize::new(side, side));
        #[cfg(windows)]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_skip_taskbar(true);
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            attrs = attrs.with_x11_window_type(vec![WindowType::Utility]);
        }
        let Ok(window) = el.create_window(attrs).map(Arc::new) else {
            eprintln!("could not create the orb; use the panel hotkey instead");
            return;
        };
        match Screen::new(window.clone(), true) {
            Ok(screen) => {
                self.orb = Some(Surface { window, screen, canvas: Canvas::new(ORB_ART, ORB_ART), scale });
                self.sync_hole(pos.x, pos.y);
            }
            Err(e) => eprintln!("could not draw the orb: {e:#}"),
        }
    }

    /// Points the black hole at the orb's center (screen → scene coordinates).
    fn sync_hole(&mut self, orb_x: i32, orb_y: i32) {
        let (Some(orb), Some(main), Some(scene)) = (&self.orb, &self.main, &mut self.scene) else { return };
        let half = (ORB_ART as u32 * orb.scale / 2) as i32;
        let (sx, sy) = (orb_x + half - self.overlay_origin.0, orb_y + half - self.overlay_origin.1);
        scene.hole = (sx as f32 / main.scale as f32, sy as f32 / main.scale as f32);
    }

    fn draw_orb(&mut self) {
        let (time, paused) = self.scene.as_ref().map(|s| (s.time, s.paused())).unwrap_or((0.0, false));
        if let Some(o) = &mut self.orb {
            o.canvas.clear(crate::render::canvas::CLEAR);
            let c = ORB_ART as f32 / 2.0;
            crate::scene::vortex::draw_orb(&mut o.canvas, (c, c), 6.0, time, paused);
            o.screen.present(&o.canvas, o.scale);
        }
    }

    /// Orb clicks (overlay orb window or the in-scene orb): click = practice,
    /// Ctrl+click = panel.
    fn orb_click(&mut self) {
        if self.ctrl() {
            self.want_menu = true;
        } else {
            self.input(Input::Primary);
        }
    }

    fn toggle_pause(&mut self) {
        self.menu.paused = !self.menu.paused;
        if let Some(scene) = &mut self.scene {
            self.lesson.set_paused(self.menu.paused, scene);
            scene.set_paused(self.menu.paused);
            let msg = if self.menu.paused { "Mago pausado. Bom foco!" } else { "O mago voltou!" };
            scene.hud.toast(msg, 3.0);
        }
    }

    /// A setting changed in the panel (or by a key): persist and apply live.
    fn apply(&mut self, item: Item) {
        if let Err(e) = self.settings.save(&self.paths.config_file()) {
            eprintln!("could not save settings: {e:#}");
        }
        if item == Item::Language {
            self.refresh_audio_lists(); // the learning voices follow the language
        }
        let Some(scene) = &mut self.scene else { return };
        match item {
            Item::Language => match Deck::load(&self.settings.learning, &self.paths.decks_dir()) {
                Ok(deck) => {
                    self.menu.topics = deck.topics();
                    self.settings.topic.clear();
                    self.lesson.set_options(|o| o.topic = None, scene);
                    self.lesson.set_deck(deck, scene);
                }
                Err(e) => scene.hud.toast(format!("Deck indisponível: {e:#}"), 5.0),
            },
            Item::Commitment => {
                self.commitment = self.settings.commitment;
                let pace = self.commitment.pace();
                scene.set_pace(pace);
                self.lesson.set_options(|o| o.ask_every = pace.ask_every, scene);
                scene.hud.toast(format!("Compromisso: {}", self.commitment.label_pt()), 2.5);
            }
            Item::Topic => {
                let topic = self.settings.topic_filter();
                self.lesson.set_options(|o| o.topic = topic, scene);
            }
            Item::Level => {
                let lvl = self.settings.max_level.clone();
                self.lesson.set_options(|o| o.max_level = lvl, scene);
            }
            Item::Practice => {
                let p = self.settings.practice;
                self.lesson.set_options(|o| o.practice = p, scene);
            }
            Item::Goal => {
                let g = self.settings.daily_goal;
                self.lesson.set_options(|o| o.daily_goal = g, scene);
            }
            Item::Mode => scene.hud.toast("Modo de tela muda ao reiniciar o Snowlearner", 3.0),
            Item::Mic | Item::Speaker | Item::Engine | Item::Endpoint | Item::VoiceNative | Item::VoiceLearning => {
                self.speech.send(Job::Configure(Box::new(VoiceSettings::from(&self.settings, &self.paths))));
                if matches!(item, Item::Engine | Item::Endpoint) {
                    let desc = self.settings.voice().describe();
                    scene.hud.toast(format!("Voz: {desc}"), 3.0);
                    self.refresh_audio_lists();
                }
            }
            _ => {}
        }
        if matches!(item, Item::Language | Item::Level) {
            self.fit_topic();
        }
    }

    /// Keeps topic and level compatible: the panel lists only topics with
    /// something at this level, and a topic left with nothing is dropped
    /// (saved, and said) instead of every lesson failing to find a phrase.
    fn fit_topic(&mut self) {
        self.menu.topics = self.lesson.topics();
        let Some(scene) = &mut self.scene else { return };
        if let Some(topic) = self.lesson.drop_empty_topic(scene) {
            self.settings.topic.clear();
            if let Err(e) = self.settings.save(&self.paths.config_file()) {
                eprintln!("could not save settings: {e:#}");
            }
            scene.hud.toast(crate::app::lesson::topic_dropped(&topic), 6.0);
        }
    }

    fn menu_action(&mut self, action: Action, el: &ActiveEventLoop) {
        match action {
            Action::None => {}
            Action::Changed(item) => self.apply(item),
            Action::PracticeNow => {
                self.menu_win = None;
                self.input(Input::Primary);
            }
            Action::Summary => {
                self.menu_win = None;
                self.input(Input::Summary);
            }
            Action::TogglePause => self.toggle_pause(),
            Action::TestMic => {
                self.test_id += 1;
                self.menu.meter = Some(0.0);
                self.menu.test_result = "Ouvindo... fale uma frase em voz alta.".into();
                self.speech.send(Job::MicTest { id: self.test_id, lang: self.settings.learning.clone() });
            }
            Action::TestVoices => {
                self.test_id += 1;
                self.menu.test_result = "Tocando as duas vozes...".into();
                let sample = match self.settings.learning.as_str() {
                    "es" => "¡Hola! Esta es la voz en español.",
                    _ => "Hello! This is the English voice.",
                };
                let parts = vec![
                    Utterance {
                        text: "Olá! Esta é a voz em português.".into(),
                        lang: self.settings.native.clone(),
                        speed: Speed::Normal,
                    },
                    Utterance {
                        text: sample.into(),
                        lang: self.settings.learning.clone(),
                        speed: target_speed(&self.settings.max_level),
                    },
                ];
                self.speech.send(Job::Speak { id: self.test_id, parts });
            }
            Action::Quit => el.exit(),
            Action::Close => self.menu_win = None,
        }
    }

    /// Reloads the ÁUDIO tab lists (voices depend on the engine and language)
    /// in the background; the panel shows what it had until they arrive.
    fn refresh_audio_lists(&mut self) {
        self.lists_gen += 1;
        let (id, settings, proxy) = (self.lists_gen, self.settings.clone(), self.proxy.clone());
        let spawned = std::thread::Builder::new().name("audio-lists".into()).spawn(move || {
            let started = Instant::now();
            let lists = AudioLists::load(&settings);
            crate::speech::trace::line(format_args!("panel lists in {:.1} s", started.elapsed().as_secs_f32()));
            let _ = proxy.send_event(UserEvent::AudioLists(id, Box::new(lists)));
        });
        if let Err(e) = spawned {
            eprintln!("could not load the panel lists: {e}");
        }
    }

    /// Results of the panel's mic/voice tests (not lesson events).
    fn test_event(&mut self, ev: SpeechEvent) {
        if ev.id() != self.test_id {
            return;
        }
        match ev {
            SpeechEvent::Level { level, .. } => self.menu.meter = Some(level),
            SpeechEvent::Thinking { .. } => self.menu.test_result = "Analisando...".into(),
            SpeechEvent::Heard { text, .. } => {
                self.menu.meter = None;
                self.menu.test_result = format!("✓ Ouvi: \"{text}\"");
            }
            SpeechEvent::NoSpeech { .. } => {
                self.menu.meter = None;
                self.menu.test_result = "Não ouvi nada. Confira o microfone escolhido e o volume.".into();
            }
            SpeechEvent::Failed { error, .. } => {
                self.menu.meter = None;
                self.menu.test_result = format!("Erro: {error}");
            }
            // The worker reports Spoken even after a part failed: keep the error.
            SpeechEvent::Spoken { .. } if !self.menu.test_result.starts_with("Erro:") => {
                self.menu.test_result = "✓ Vozes tocadas. Troque em Voz pt-BR / Voz do idioma.".into()
            }
            SpeechEvent::Spoken { .. } => {}
            SpeechEvent::Part { .. } => {}
        }
    }

    /// Magic hand: on while Ctrl+Alt is held, for 15 s after `grab`, or while
    /// someone is being carried. The overlay only takes clicks while it's on.
    fn update_hand(&mut self) {
        if self.hand_until.is_some_and(|t| Instant::now() >= t) {
            self.hand_until = None;
        }
        let holding = self.scene.as_ref().is_some_and(|s| s.holding().is_some());
        let want = self.mods_held || self.hand_until.is_some() || holding;
        if want == self.hand_on {
            return;
        }
        self.hand_on = want;
        if let Some(main) = &self.main {
            if self.resolved.overlay {
                let _ = main.window.set_cursor_hittest(want);
            }
            main.window.set_cursor_visible(!want);
        }
        if let Some(scene) = &mut self.scene {
            scene.set_hand(want);
            if want {
                let scale = self.main.as_ref().map(|m| m.scale).unwrap_or(1) as f64;
                scene.hand_move((self.cursor.0 / scale) as f32, (self.cursor.1 / scale) as f32);
            }
        }
    }

    /// Shows or hides the shortcut sheet next to the orb.
    fn update_cheats(&mut self) {
        let want = self.orb_hover || self.mods_held;
        let Some(scene) = &mut self.scene else { return };
        if !want {
            scene.hud.cheats = None;
            return;
        }
        if scene.hud.cheats.is_some() {
            return;
        }
        let keys = [
            (self.settings.hotkey_challenge.as_str(), "praticar / terminei"),
            (self.settings.hotkey_menu.as_str(), "painel"),
            (self.settings.hotkey_summary.as_str(), "resumo do dia"),
            (self.settings.hotkey_grab.as_str(), "mão mágica"),
            (self.settings.hotkey_progress.as_str(), "progresso"),
        ];
        scene.hud.cheats =
            Some(Cheats::from_hotkeys(scene.hole, &keys, "Orbe: clique pratica · Ctrl+clique painel · direito pausa"));
    }

    fn open_menu(&mut self, el: &ActiveEventLoop) {
        if let Some(m) = &self.menu_win {
            m.window.focus_window();
            return;
        }
        self.refresh_audio_lists();
        let size = PhysicalSize::new(menu::WIDTH as u32 * MENU_SCALE, menu::HEIGHT as u32 * MENU_SCALE);
        let attrs = Window::default_attributes()
            .with_title("Snowlearner · Painel")
            .with_inner_size(size)
            .with_resizable(false)
            .with_window_level(WindowLevel::AlwaysOnTop);
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("could not open the panel: {e}");
                return;
            }
        };
        match Screen::new(window.clone(), false) {
            Ok(screen) => {
                self.menu_win =
                    Some(Surface { window, screen, canvas: Canvas::new(menu::WIDTH, menu::HEIGHT), scale: MENU_SCALE })
            }
            Err(e) => eprintln!("could not draw the panel: {e:#}"),
        }
    }

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if let Some(scene) = &mut self.scene {
            scene.step(dt);
        }
        self.input(Input::Tick { dt, now: chrono::Local::now() });
        if let (Some(scene), Some(main)) = (&self.scene, &mut self.main) {
            scene.draw(&mut main.canvas);
            main.screen.present(&main.canvas, main.scale);
        }
    }

    fn draw_menu(&mut self) {
        let time = self.scene.as_ref().map(|s| s.time).unwrap_or(0.0);
        // Progress reads the history: refresh on open and every few seconds, not every frame.
        if self.menu_win.is_some()
            && self.menu.tab == menu::Tab::Progress
            && (self.menu.progress.is_none() || self.progress_at.elapsed() > Duration::from_secs(3))
        {
            self.menu.progress = Some(self.lesson.progress());
            self.progress_at = Instant::now();
        }
        self.menu.status = format!(
            "Hoje: {}/{}  ·  combo x{}",
            self.lesson.done_today(),
            self.settings.daily_goal,
            self.lesson.combo()
        );
        if let Some(m) = &mut self.menu_win {
            self.menu.draw(&mut m.canvas, &self.settings, time);
            m.screen.present(&m.canvas, m.scale);
        }
    }

    fn main_key(&mut self, key: Key) {
        match key {
            Key::Named(NamedKey::Space | NamedKey::Enter) => self.input(Input::Primary),
            Key::Named(NamedKey::Escape) => self.input(Input::Dismiss),
            Key::Named(NamedKey::Tab) => self.want_menu = true,
            Key::Character(c) => match c.to_lowercase().as_str() {
                "s" => self.input(Input::Summary),
                "m" => self.want_menu = true,
                "p" => self.toggle_pause(),
                "h" => {
                    if let Some(scene) = &mut self.scene {
                        scene.hud.help = match scene.hud.help {
                            Some(_) => None,
                            None => Some(HELP.iter().map(|s| s.to_string()).collect()),
                        };
                    }
                }
                "l" => {
                    let mut s = self.settings.clone();
                    self.menu.sel = 0;
                    if let Action::Changed(item) = self.menu.key(menu::Key::Right, &mut s) {
                        self.settings = s;
                        self.apply(item);
                    }
                }
                d @ ("1" | "2" | "3" | "4") => {
                    self.settings.commitment = match d {
                        "1" => Commitment::Chill,
                        "2" => Commitment::Steady,
                        "3" => Commitment::Committed,
                        _ => Commitment::Relentless,
                    };
                    self.apply(Item::Commitment);
                }
                _ => {}
            },
            _ => {}
        }
    }
}

/// `editing`: the panel's address field is open, so typed text goes to it.
fn menu_key(key: &Key, editing: bool, ctrl: bool) -> Option<menu::Key> {
    if editing {
        match key {
            Key::Named(NamedKey::Backspace) => return Some(menu::Key::Backspace),
            Key::Named(NamedKey::Space) => return Some(menu::Key::Char(' ')),
            Key::Character(text) if !ctrl => return text.chars().next().map(menu::Key::Char),
            _ => {}
        }
    }
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => menu::Key::Up,
        Key::Named(NamedKey::ArrowDown) => menu::Key::Down,
        Key::Named(NamedKey::ArrowLeft) => menu::Key::Left,
        Key::Named(NamedKey::ArrowRight) => menu::Key::Right,
        Key::Named(NamedKey::Enter | NamedKey::Space) => menu::Key::Enter,
        Key::Named(NamedKey::Escape) => menu::Key::Esc,
        Key::Named(NamedKey::Tab) => menu::Key::Tab,
        _ => return None,
    })
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.main.is_some() {
            return;
        }
        let attrs = if self.resolved.overlay { self.overlay_attributes(el) } else { self.window_attributes() };
        let window = match el.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("could not create window: {e}");
                el.exit();
                return;
            }
        };
        if self.resolved.overlay && window.set_cursor_hittest(false).is_err() {
            eprintln!("This desktop cannot make the overlay click-through.");
        }
        let screen = match Screen::new(window.clone(), self.resolved.overlay) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("could not draw the window: {e:#}");
                el.exit();
                return;
            }
        };
        let transparent = screen.transparent();
        if self.resolved.overlay && !transparent {
            eprintln!("This compositor has no transparent windows; drawing the full winter scene instead.");
        }
        let size = window.inner_size();
        self.main = Some(Surface { window, screen, canvas: Canvas::new(1, 1), scale: 1 });
        self.fit(size.width, size.height);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
        let (w, h) = self.main.as_ref().map(|m| (m.canvas.w, m.canvas.h)).unwrap_or((320, 180));
        let mut scene = Scene::new(w, h, seed, self.commitment.pace(), transparent);
        self.lesson.attach(&mut scene);
        if self.resolved.overlay {
            self.overlay_origin =
                self.main.as_ref().and_then(|m| m.window.outer_position().ok()).map(|p| (p.x, p.y)).unwrap_or((0, 0));
        } else {
            scene.show_orb = true;
            scene.hole = (10.0, 10.0);
        }
        let hint = if self.resolved.overlay { self.settings.hotkey_menu.clone() } else { "H".into() };
        scene.hud.toast(format!("Snowlearner · {} · ajuda/painel: {hint}", self.commitment.label_pt()), 5.0);
        self.scene = Some(scene);
        self.fit_topic();
        if self.resolved.overlay {
            self.open_orb(el);
        }
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = *m;
        }
        // Shown again or refocused: what the window system kept may be stale.
        if matches!(
            event,
            WindowEvent::Focused(_) | WindowEvent::Occluded(false) | WindowEvent::ScaleFactorChanged { .. }
        ) {
            for s in [&mut self.main, &mut self.menu_win, &mut self.orb].into_iter().flatten() {
                if s.window.id() == id {
                    s.screen.invalidate();
                }
            }
        }
        let is_orb = self.orb.as_ref().is_some_and(|o| o.window.id() == id);
        if is_orb {
            match event {
                WindowEvent::RedrawRequested => self.draw_orb(),
                WindowEvent::CursorEntered { .. } => self.orb_hover = true,
                WindowEvent::CursorLeft { .. } => self.orb_hover = false,
                WindowEvent::CursorMoved { position, .. } => {
                    self.orb_cursor = (position.x, position.y);
                    if let Some((px, py)) = self.orb_press {
                        // Moving while pressed = drag the orb around.
                        if (position.x - px).abs() + (position.y - py).abs() > 4.0 {
                            self.orb_press = None;
                            if let Some(o) = &self.orb {
                                let _ = o.window.drag_window();
                            }
                        }
                    }
                }
                WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => match state {
                    ElementState::Pressed if self.ctrl() => self.want_menu = true,
                    ElementState::Pressed => self.orb_press = Some(self.orb_cursor),
                    ElementState::Released => {
                        if self.orb_press.take().is_some() {
                            self.orb_click();
                        }
                    }
                },
                WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. } => {
                    self.toggle_pause()
                }
                WindowEvent::Moved(pos) => {
                    self.sync_hole(pos.x, pos.y);
                    self.settings.orb_x = pos.x;
                    self.settings.orb_y = pos.y;
                    self.save_orb_at = Some(Instant::now() + Duration::from_secs(1));
                }
                _ => {}
            }
            return;
        }
        let is_menu = self.menu_win.as_ref().is_some_and(|m| m.window.id() == id);
        if is_menu {
            match event {
                WindowEvent::CloseRequested => self.menu_win = None,
                WindowEvent::RedrawRequested => self.draw_menu(),
                WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
                WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } => {
                    let (x, y) =
                        ((self.cursor.0 / MENU_SCALE as f64) as i32, (self.cursor.1 / MENU_SCALE as f64) as i32);
                    let action = self.menu.click(x, y, &mut self.settings);
                    self.menu_action(action, el);
                }
                WindowEvent::KeyboardInput {
                    event: KeyEvent { logical_key, state: ElementState::Pressed, .. },
                    ..
                } => {
                    let (editing, ctrl) = (self.menu.editing().is_some(), self.modifiers.state().control_key());
                    if let Some(k) = menu_key(&logical_key, editing, ctrl) {
                        let action = self.menu.key(k, &mut self.settings);
                        self.menu_action(action, el);
                    }
                }
                _ => {}
            }
            return;
        }
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(main) = &mut self.main {
                    main.screen.resize(size.width, size.height);
                }
                self.fit(size.width, size.height);
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                let scale = self.main.as_ref().map(|m| m.scale).unwrap_or(1) as f64;
                let (x, y) = ((position.x / scale) as f32, (position.y / scale) as f32);
                self.orb_hover = self.scene.as_ref().is_some_and(|sc| sc.orb_at(x, y));
                if self.hand_on
                    && let Some(scene) = &mut self.scene
                {
                    scene.hand_move(x, y);
                }
            }
            WindowEvent::ModifiersChanged(m) => {
                let st = m.state();
                self.mods_held = st.control_key() && st.alt_key();
            }
            WindowEvent::MouseInput { state: ElementState::Released, button: MouseButton::Left, .. } => {
                if let Some(scene) = &mut self.scene {
                    scene.release();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } if self.hand_on => {
                let scale = self.main.as_ref().map(|m| m.scale).unwrap_or(1) as f64;
                let (x, y) = ((self.cursor.0 / scale) as f32, (self.cursor.1 / scale) as f32);
                if let Some(scene) = &mut self.scene {
                    scene.grab_at(x, y);
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button, .. } => {
                let scale = self.main.as_ref().map(|m| m.scale).unwrap_or(1) as f64;
                let (x, y) = ((self.cursor.0 / scale) as f32, (self.cursor.1 / scale) as f32);
                let on_orb = self.scene.as_ref().is_some_and(|s| s.orb_at(x, y));
                match (button, on_orb) {
                    (MouseButton::Left, true) => self.orb_click(),
                    (MouseButton::Right, true) => self.toggle_pause(),
                    (MouseButton::Left, false) => {
                        if let Some(scene) = &mut self.scene {
                            scene.poke(x, y);
                        }
                    }
                    _ => {}
                }
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. },
                ..
            } => self.main_key(logical_key),
            _ => {}
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Ipc(cmd) => self.command(cmd, el),
            UserEvent::Speech(ev) if ev.id() >= TEST_IDS => self.test_event(ev),
            UserEvent::Speech(ev) => self.input(Input::Speech(ev)),
            UserEvent::AudioLists(id, lists) if id == self.lists_gen => {
                let AudioLists { mics, speakers, voices_native, voices_learning } = *lists;
                (self.menu.mics, self.menu.speakers) = (mics, speakers);
                (self.menu.voices_native, self.menu.voices_learning) = (voices_native, voices_learning);
                if let Some(m) = &mut self.menu_win {
                    m.screen.invalidate();
                    m.window.request_redraw();
                }
            }
            UserEvent::AudioLists(..) => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let pressed = self.hotkeys.as_ref().map(Hotkeys::poll).unwrap_or_default();
        for cmd in pressed {
            self.command(cmd, el);
        }
        if std::mem::take(&mut self.want_menu) {
            self.open_menu(el);
        }
        if self.resolved.overlay
            && let Some(held) = crate::control::modkeys::ctrl_alt_held()
        {
            self.mods_held = held;
        }
        self.update_cheats();
        self.update_hand();
        if self.save_orb_at.is_some_and(|t| Instant::now() >= t) {
            self.save_orb_at = None;
            if let Err(e) = self.settings.save(&self.paths.config_file()) {
                eprintln!("could not save the orb position: {e:#}");
            }
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.next_frame = now + Duration::from_secs_f32(1.0 / FPS);
            if let Some(m) = &self.main {
                m.window.request_redraw();
            }
            if let Some(m) = &self.menu_win {
                m.window.request_redraw();
            }
            if let Some(o) = &self.orb {
                o.window.request_redraw();
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
