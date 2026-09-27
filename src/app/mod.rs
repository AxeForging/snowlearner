//! The desktop app: window/overlay, the control panel window, frame loop,
//! hotkeys, IPC and speech — wired to the scene and the lesson controller.

pub mod lesson;
pub mod menu;
pub mod platform;

use crate::config::level::Commitment;
use crate::config::paths::Paths;
use crate::config::settings::Settings;
use crate::control::hotkeys::Hotkeys;
use crate::control::ipc::{self, Command};
use crate::learn::deck::Deck;
use crate::render::canvas::Canvas;
use crate::render::gpu::Gpu;
use crate::scene::Scene;
use crate::speech::worker::{Speech, SpeechEvent, VoiceSettings};
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
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

const FPS: f32 = 30.0;
/// Window mode keeps at least this many art pixels on screen.
const WINDOW_VIRTUAL: (u32, u32) = (320, 180);
const MENU_SCALE: u32 = 3;
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
    let speech = Speech::start(
        VoiceSettings {
            native_voice: settings.voice_native.clone(),
            learning_voice: settings.voice_learning.clone(),
            native_lang: settings.native.clone(),
            model: paths.model_file(&settings.model),
        },
        move |ev| {
            let _ = speech_proxy.send_event(UserEvent::Speech(ev));
        },
    );
    if speech.tts.is_none() {
        eprintln!("No text-to-speech engine found; captions will still show. Run `snowlearner doctor`.");
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
    ];
    let hotkeys = if session.global_hotkeys() {
        match Hotkeys::register(&bindings) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("Global hotkeys unavailable: {e:#}");
                None
            }
        }
    } else if crate::control::gnome::installed() {
        eprintln!("Hotkeys: GNOME shortcuts active ({}, {}, {}).", bindings[0].0, bindings[1].0, bindings[2].0);
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
        modifiers: Modifiers::default(),
        orb: None,
        orb_press: None,
        orb_cursor: (0.0, 0.0),
        overlay_origin: (0, 0),
        save_orb_at: None,
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

/// A window with its GPU surface and art canvas.
struct Surface {
    window: Arc<Window>,
    gpu: Gpu,
    canvas: Canvas,
    scale: u32,
}

struct App {
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
    cursor: (f64, f64),
    modifiers: Modifiers,
    /// The draggable magic orb (overlay mode; window mode draws it in-scene).
    orb: Option<Surface>,
    orb_press: Option<(f64, f64)>,
    orb_cursor: (f64, f64),
    /// Top-left of the overlay window in screen pixels (maps the orb to the scene).
    overlay_origin: (i32, i32),
    save_orb_at: Option<Instant>,
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
            Command::Pause => self.toggle_pause(),
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
        match Gpu::new(window.clone(), true) {
            Ok(gpu) => {
                self.orb = Some(Surface { window, gpu, canvas: Canvas::new(ORB_ART, ORB_ART), scale });
                self.sync_hole(pos.x, pos.y);
            }
            Err(e) => eprintln!("orb GPU init failed: {e:#}"),
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
            o.gpu.present(&o.canvas, o.scale);
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
            _ => {}
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
            Action::Quit => el.exit(),
            Action::Close => self.menu_win = None,
        }
    }

    fn open_menu(&mut self, el: &ActiveEventLoop) {
        if let Some(m) = &self.menu_win {
            m.window.focus_window();
            return;
        }
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
        match Gpu::new(window.clone(), false) {
            Ok(gpu) => {
                self.menu_win =
                    Some(Surface { window, gpu, canvas: Canvas::new(menu::WIDTH, menu::HEIGHT), scale: MENU_SCALE })
            }
            Err(e) => eprintln!("panel GPU init failed: {e:#}"),
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
            main.gpu.present(&main.canvas, main.scale);
        }
    }

    fn draw_menu(&mut self) {
        let time = self.scene.as_ref().map(|s| s.time).unwrap_or(0.0);
        self.menu.status = format!(
            "Hoje: {}/{}  ·  combo x{}",
            self.lesson.done_today(),
            self.settings.daily_goal,
            self.lesson.combo()
        );
        if let Some(m) = &mut self.menu_win {
            self.menu.draw(&mut m.canvas, &self.settings, time);
            m.gpu.present(&m.canvas, m.scale);
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

fn menu_key(key: &Key) -> Option<menu::Key> {
    Some(match key {
        Key::Named(NamedKey::ArrowUp) => menu::Key::Up,
        Key::Named(NamedKey::ArrowDown) => menu::Key::Down,
        Key::Named(NamedKey::ArrowLeft) => menu::Key::Left,
        Key::Named(NamedKey::ArrowRight) => menu::Key::Right,
        Key::Named(NamedKey::Enter | NamedKey::Space) => menu::Key::Enter,
        Key::Named(NamedKey::Escape) => menu::Key::Esc,
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
        let gpu = match Gpu::new(window.clone(), self.resolved.overlay) {
            Ok(g) => g,
            Err(e) => {
                eprintln!("GPU init failed: {e:#}");
                el.exit();
                return;
            }
        };
        let transparent = gpu.transparent;
        if self.resolved.overlay && !transparent {
            eprintln!("This compositor has no transparent windows; drawing the full winter scene instead.");
        }
        let size = window.inner_size();
        self.main = Some(Surface { window, gpu, canvas: Canvas::new(1, 1), scale: 1 });
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
        if self.resolved.overlay {
            self.open_orb(el);
        }
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if let WindowEvent::ModifiersChanged(m) = &event {
            self.modifiers = *m;
        }
        let is_orb = self.orb.as_ref().is_some_and(|o| o.window.id() == id);
        if is_orb {
            match event {
                WindowEvent::RedrawRequested => self.draw_orb(),
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
                    if let Some(k) = menu_key(&logical_key) {
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
                    main.gpu.resize(size.width, size.height);
                }
                self.fit(size.width, size.height);
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::CursorMoved { position, .. } => self.cursor = (position.x, position.y),
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
            UserEvent::Speech(ev) => self.input(Input::Speech(ev)),
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
