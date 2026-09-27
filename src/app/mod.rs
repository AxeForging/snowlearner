//! The desktop app: window/overlay, frame loop, hotkeys, IPC and speech wired
//! to the scene and the lesson controller.

pub mod lesson;
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
use platform::{Resolved, Session};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId, WindowLevel};

const FPS: f32 = 30.0;
/// Window mode keeps at least this many art pixels on screen.
const WINDOW_VIRTUAL: (u32, u32) = (320, 180);

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

    let hotkeys = if session.global_hotkeys() {
        match Hotkeys::register(&settings.hotkey_challenge, &settings.hotkey_summary) {
            Ok(h) => Some(h),
            Err(e) => {
                eprintln!("Global hotkeys unavailable: {e:#}");
                None
            }
        }
    } else {
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "snowlearner".into());
        eprintln!("{}", platform::shortcut_help(&exe, &settings.hotkey_challenge, &settings.hotkey_summary));
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
    };
    let lesson = Lesson::new(deck, opts, history);

    let mut app = App {
        settings,
        resolved,
        commitment,
        lesson,
        speech,
        hotkeys,
        window: None,
        gpu: None,
        scene: None,
        canvas: Canvas::new(1, 1),
        scale: 1,
        last: Instant::now(),
        next_frame: Instant::now(),
    };
    event_loop.run_app(&mut app).context("event loop failed")?;
    Ok(())
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

struct App {
    settings: Settings,
    resolved: Resolved,
    commitment: Commitment,
    lesson: Lesson,
    speech: Speech,
    hotkeys: Option<Hotkeys>,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    scene: Option<Scene>,
    canvas: Canvas,
    scale: u32,
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
        self.scale = if self.resolved.overlay {
            self.settings.pixel_scale
        } else {
            (width / WINDOW_VIRTUAL.0).min(height / WINDOW_VIRTUAL.1).max(1)
        };
        let (vw, vh) = (width.div_ceil(self.scale) as i32, height.div_ceil(self.scale) as i32);
        self.canvas = Canvas::new(vw, vh);
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

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        if let Some(scene) = &mut self.scene {
            scene.step(dt);
        }
        self.input(Input::Tick { dt, now: chrono::Local::now() });
        if let (Some(scene), Some(gpu)) = (&self.scene, &mut self.gpu) {
            scene.draw(&mut self.canvas);
            gpu.present(&self.canvas, self.scale);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
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
        self.gpu = Some(gpu);
        self.window = Some(window);
        self.fit(size.width, size.height);
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(1);
        let mut scene = Scene::new(self.canvas.w, self.canvas.h, seed, self.commitment.pace(), transparent);
        scene.tips = self.lesson.tips();
        scene
            .hud
            .toast(format!("Snowlearner · {} · {}", self.commitment.label_pt(), self.settings.hotkey_challenge), 5.0);
        self.scene = Some(scene);
        self.last = Instant::now();
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
                self.fit(size.width, size.height);
            }
            WindowEvent::RedrawRequested => self.frame(),
            WindowEvent::KeyboardInput {
                event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. },
                ..
            } => match logical_key {
                Key::Named(NamedKey::Space | NamedKey::Enter) => self.input(Input::Primary),
                Key::Named(NamedKey::Escape) => self.input(Input::Dismiss),
                Key::Character(c) if c.eq_ignore_ascii_case("s") => self.input(Input::Summary),
                _ => {}
            },
            _ => {}
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Ipc(Command::Quit) => el.exit(),
            UserEvent::Ipc(Command::Challenge) => self.input(Input::Primary),
            UserEvent::Ipc(Command::Summary) => self.input(Input::Summary),
            UserEvent::Ipc(Command::Dismiss) => self.input(Input::Dismiss),
            UserEvent::Speech(ev) => self.input(Input::Speech(ev)),
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let pressed = self.hotkeys.as_ref().map(Hotkeys::poll).unwrap_or_default();
        for cmd in pressed {
            match cmd {
                Command::Challenge => self.input(Input::Primary),
                Command::Summary => self.input(Input::Summary),
                _ => {}
            }
        }
        let now = Instant::now();
        if now >= self.next_frame {
            self.next_frame = now + Duration::from_secs_f32(1.0 / FPS);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        }
        el.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}
