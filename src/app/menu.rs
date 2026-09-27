//! The control panel ("Painel"): a small pixel window to change settings live
//! and trigger actions, in two tabs — JOGO (game) and ÁUDIO (mic, voices).
//! Pure state + drawing; the app owns the window.
//! Keys: ↑/↓ choose, ←/→ change, Enter act, Tab switch tab, Esc close. Mouse: click.

use crate::config::level::Commitment;
use crate::config::settings::{Settings, WindowMode};
use crate::learn::deck::LEVELS;
use crate::learn::picker::Practice;
use crate::render::canvas::{Canvas, Rgba, hex};
use crate::render::font;
use crate::speech::voices::TtsEngine;

pub const WIDTH: i32 = 232;
pub const HEIGHT: i32 = 188;
const ROW_H: i32 = 12;
const TABS_Y: i32 = 13;
const TOP: i32 = 28;
const GOALS: &[u32] = &[3, 5, 10, 15, 20, 30, 50];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Language,
    Commitment,
    Topic,
    Level,
    Practice,
    Goal,
    Mode,
    PracticeNow,
    Summary,
    Pause,
    Quit,
    Mic,
    TestMic,
    Engine,
    VoiceNative,
    VoiceLearning,
    TestVoices,
    Speaker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Game,
    Audio,
}

pub const GAME: &[Item] = &[
    Item::Language,
    Item::Commitment,
    Item::Topic,
    Item::Level,
    Item::Practice,
    Item::Goal,
    Item::Mode,
    Item::PracticeNow,
    Item::Summary,
    Item::Pause,
    Item::Quit,
];

pub const AUDIO: &[Item] =
    &[Item::Mic, Item::TestMic, Item::Engine, Item::VoiceNative, Item::VoiceLearning, Item::TestVoices, Item::Speaker];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    Esc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// A setting changed: save and apply.
    Changed(Item),
    PracticeNow,
    Summary,
    TogglePause,
    TestMic,
    TestVoices,
    Quit,
    Close,
}

pub struct Menu {
    pub tab: Tab,
    pub sel: usize,
    pub languages: Vec<String>,
    /// Topics of the current deck ("" = all is added automatically).
    pub topics: Vec<String>,
    pub mics: Vec<String>,
    pub speakers: Vec<String>,
    pub voices_native: Vec<String>,
    pub voices_learning: Vec<String>,
    pub paused: bool,
    /// Live mic level while testing (None = not testing).
    pub meter: Option<f32>,
    /// Result line of the last mic/voice test.
    pub test_result: String,
    /// Line shown at the bottom ("Hoje: 4/10 · combo x2").
    pub status: String,
}

fn cycle<T: PartialEq + Clone>(options: &[T], current: &T, forward: bool) -> T {
    let n = options.len();
    if n == 0 {
        return current.clone();
    }
    let i = options.iter().position(|o| o == current).unwrap_or(0);
    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
    options[j].clone()
}

/// "" (automatic/default) first, then the given names.
fn with_default(names: &[String]) -> Vec<String> {
    std::iter::once(String::new()).chain(names.iter().cloned()).collect()
}

fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max - 1).collect::<String>()) }
}

impl Menu {
    pub fn new(languages: Vec<String>, topics: Vec<String>) -> Menu {
        Menu {
            tab: Tab::Game,
            sel: 0,
            languages,
            topics,
            mics: Vec::new(),
            speakers: Vec::new(),
            voices_native: Vec::new(),
            voices_learning: Vec::new(),
            paused: false,
            meter: None,
            test_result: String::new(),
            status: String::new(),
        }
    }

    pub fn items(&self) -> &'static [Item] {
        match self.tab {
            Tab::Game => GAME,
            Tab::Audio => AUDIO,
        }
    }

    pub fn item(&self) -> Item {
        self.items()[self.sel.min(self.items().len() - 1)]
    }

    pub fn switch_tab(&mut self) {
        self.tab = if self.tab == Tab::Game { Tab::Audio } else { Tab::Game };
        self.sel = 0;
    }

    fn change(&self, item: Item, s: &mut Settings, forward: bool) -> Action {
        match item {
            Item::Language => s.learning = cycle(&self.languages, &s.learning, forward),
            Item::Commitment => {
                let all = [Commitment::Chill, Commitment::Steady, Commitment::Committed, Commitment::Relentless];
                s.commitment = cycle(&all, &s.commitment, forward);
            }
            Item::Topic => s.topic = cycle(&with_default(&self.topics), &s.topic, forward),
            Item::Level => {
                let all: Vec<String> = LEVELS.iter().map(|l| l.to_string()).collect();
                s.max_level = cycle(&all, &s.max_level, forward);
            }
            Item::Practice => {
                s.practice = cycle(&[Practice::Auto, Practice::Repeat, Practice::Recall], &s.practice, forward)
            }
            Item::Goal => {
                s.daily_goal = if forward {
                    GOALS.iter().copied().find(|g| *g > s.daily_goal).unwrap_or(GOALS[0])
                } else {
                    GOALS.iter().rev().copied().find(|g| *g < s.daily_goal).unwrap_or(*GOALS.last().unwrap())
                };
            }
            Item::Mode => {
                s.mode = cycle(&[WindowMode::Auto, WindowMode::Window, WindowMode::Overlay], &s.mode, forward)
            }
            Item::Mic => s.mic = cycle(&with_default(&self.mics), &s.mic, forward),
            Item::Speaker => s.speaker = cycle(&with_default(&self.speakers), &s.speaker, forward),
            Item::Engine => {
                // Only offer engines that can work with the current config.
                let mut all = vec![TtsEngine::System, TtsEngine::Http];
                if !s.tts_command.trim().is_empty() {
                    all.push(TtsEngine::Command);
                }
                s.tts_engine = cycle(&all, &s.tts_engine, forward);
                s.voice_native.clear();
                s.voice_learning.clear();
            }
            Item::VoiceNative => s.voice_native = cycle(&with_default(&self.voices_native), &s.voice_native, forward),
            Item::VoiceLearning => {
                s.voice_learning = cycle(&with_default(&self.voices_learning), &s.voice_learning, forward)
            }
            _ => return Action::None,
        }
        Action::Changed(item)
    }

    fn activate(&self, item: Item, s: &mut Settings) -> Action {
        match item {
            Item::PracticeNow => Action::PracticeNow,
            Item::Summary => Action::Summary,
            Item::Pause => Action::TogglePause,
            Item::Quit => Action::Quit,
            Item::TestMic => Action::TestMic,
            Item::TestVoices => Action::TestVoices,
            other => self.change(other, s, true),
        }
    }

    pub fn key(&mut self, key: Key, s: &mut Settings) -> Action {
        let n = self.items().len();
        match key {
            Key::Up => {
                self.sel = (self.sel + n - 1) % n;
                Action::None
            }
            Key::Down => {
                self.sel = (self.sel + 1) % n;
                Action::None
            }
            Key::Left => self.change(self.item(), s, false),
            Key::Right => self.change(self.item(), s, true),
            Key::Enter => self.activate(self.item(), s),
            Key::Tab => {
                self.switch_tab();
                Action::None
            }
            Key::Esc => Action::Close,
        }
    }

    /// Click at art coordinates: tabs switch pages; rows select and activate.
    pub fn click(&mut self, x: i32, y: i32, s: &mut Settings) -> Action {
        if !(0..WIDTH).contains(&x) {
            return Action::None;
        }
        if (TABS_Y..TABS_Y + ROW_H).contains(&y) {
            let want = if x < WIDTH / 2 { Tab::Game } else { Tab::Audio };
            if want != self.tab {
                self.switch_tab();
            }
            return Action::None;
        }
        if y < TOP {
            return Action::None;
        }
        let row = ((y - TOP) / ROW_H) as usize;
        if row >= self.items().len() {
            return Action::None;
        }
        self.sel = row;
        self.activate(self.items()[row], s)
    }

    fn value(&self, item: Item, s: &Settings) -> String {
        let auto = |v: &str, empty: &str| if v.is_empty() { empty.to_string() } else { shorten(v, 20) };
        match item {
            Item::Language => match s.learning.as_str() {
                "en" => "inglês".into(),
                "es" => "espanhol".into(),
                other => other.into(),
            },
            Item::Commitment => s.commitment.label_pt().into(),
            Item::Topic => auto(&s.topic, "todos"),
            Item::Level => format!("até {}", s.max_level),
            Item::Practice => match s.practice {
                Practice::Auto => "automático".into(),
                Practice::Repeat => "repetir".into(),
                Practice::Recall => "de memória".into(),
            },
            Item::Goal => format!("{} frases", s.daily_goal),
            Item::Mode => match s.mode {
                WindowMode::Auto => "auto*".into(),
                WindowMode::Window => "janela*".into(),
                WindowMode::Overlay => "sobre a tela*".into(),
            },
            Item::Pause => if self.paused { "pausado" } else { "ativo" }.into(),
            Item::Mic => auto(&s.mic, "padrão do sistema"),
            Item::Speaker => auto(&s.speaker, "padrão do sistema"),
            Item::Engine => match s.tts_engine {
                TtsEngine::System => "sistema".into(),
                TtsEngine::Http => "HTTP (Kokoro…)".into(),
                TtsEngine::Command => "comando".into(),
            },
            Item::VoiceNative => auto(&s.voice_native, "automática"),
            Item::VoiceLearning => auto(&s.voice_learning, "automática"),
            _ => String::new(),
        }
    }

    fn label(item: Item) -> &'static str {
        match item {
            Item::Language => "Idioma",
            Item::Commitment => "Compromisso",
            Item::Topic => "Tema",
            Item::Level => "Nível",
            Item::Practice => "Prática",
            Item::Goal => "Meta diária",
            Item::Mode => "Tela",
            Item::PracticeNow => "> Praticar agora",
            Item::Summary => "> Resumo do dia",
            Item::Pause => "Mago",
            Item::Quit => "> Sair",
            Item::Mic => "Microfone",
            Item::TestMic => "> Testar microfone",
            Item::Engine => "Motor de voz",
            Item::VoiceNative => "Voz pt-BR",
            Item::VoiceLearning => "Voz do idioma",
            Item::TestVoices => "> Testar vozes",
            Item::Speaker => "Alto-falante",
        }
    }

    pub fn draw(&self, c: &mut Canvas, s: &Settings, time: f32) {
        let (bg, ink, dim, accent, sel_bg): (Rgba, Rgba, Rgba, Rgba, Rgba) =
            (hex(0x0e0c2c), hex(0xe6ecff), hex(0x8f96d8), hex(0x9be8ff), hex(0x2a5a9a));
        c.clear(bg);
        for x in 0..c.w {
            c.set(x, 0, hex(0x4ea2d8));
            c.set(x, c.h - 1, hex(0x4ea2d8));
        }
        font::draw(c, 6, 1, "SNOWLEARNER · PAINEL", hex(0xffd64a));
        for (i, (tab, name)) in [(Tab::Game, "JOGO"), (Tab::Audio, "ÁUDIO")].iter().enumerate() {
            let x0 = i as i32 * WIDTH / 2;
            let on = *tab == self.tab;
            c.rect(x0 + 2, TABS_Y, WIDTH / 2 - 4, ROW_H, if on { hex(0x2a1650) } else { hex(0x151233) });
            if on {
                c.rect(x0 + 2, TABS_Y + ROW_H - 1, WIDTH / 2 - 4, 1, hex(0xb46cff));
            }
            let tw = font::text_width(name);
            font::draw(c, x0 + (WIDTH / 2 - tw) / 2, TABS_Y - 2, name, if on { hex(0xffffff) } else { dim });
        }
        for (i, item) in self.items().iter().enumerate() {
            let y = TOP + i as i32 * ROW_H;
            let selected = i == self.sel;
            if selected {
                c.rect(2, y, c.w - 4, ROW_H, sel_bg);
                if (time * 3.0) as i32 % 2 == 0 {
                    font::draw(c, 3, y - 2, ">", hex(0xffffff));
                }
            }
            font::draw(c, 10, y - 2, Self::label(*item), if selected { hex(0xffffff) } else { ink });
            let value = self.value(*item, s);
            if !value.is_empty() {
                let text = if selected { format!("< {value} >") } else { value };
                let w = font::text_width(&text);
                font::draw(c, c.w - w - 6, y - 2, &text, if selected { hex(0xffffff) } else { accent });
            }
        }
        let foot_y = TOP + GAME.len() as i32 * ROW_H + 2;
        if self.tab == Tab::Audio {
            let y = TOP + AUDIO.len() as i32 * ROW_H + 6;
            if let Some(level) = self.meter {
                let lit = ((level.sqrt() * 2.2).clamp(0.0, 1.0) * 20.0).round() as i32;
                for i in 0..20 {
                    let col = if i >= lit {
                        hex(0x1d2a5a)
                    } else if i < 12 {
                        hex(0x7dff9b)
                    } else if i < 17 {
                        hex(0xffd64a)
                    } else {
                        hex(0xff6b6b)
                    };
                    c.rect(10 + i * 5, y + 2, 4, 6, col);
                }
                font::draw(c, 116, y - 1, "fale algo…", accent);
            }
            for (k, line) in font::wrap(&self.test_result, WIDTH - 16).iter().take(3).enumerate() {
                font::draw(c, 6, y + 10 + k as i32 * (ROW_H - 1), line, hex(0xe6ecff));
            }
        } else {
            font::draw(c, 6, foot_y - 2, &self.status, accent);
        }
        font::draw(c, 6, c.h - 13, "*ao reiniciar  ↑↓ ←→ Enter  Tab: aba  Esc", dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        let mut m = Menu::new(vec!["en".into(), "es".into()], vec!["trabalho".into(), "viagem".into()]);
        m.mics = vec!["USB Mic".into(), "Laptop Mic".into()];
        m.voices_learning = vec!["af_heart".into(), "am_adam".into()];
        m
    }

    fn select(m: &mut Menu, item: Item) {
        if !m.items().contains(&item) {
            m.switch_tab();
        }
        m.sel = m.items().iter().position(|i| *i == item).unwrap();
    }

    #[test]
    fn arrows_change_the_selected_setting_and_report_it() {
        let mut m = menu();
        let mut s = Settings::default();
        assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Language));
        assert_eq!(s.learning, "es");
        assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Language));
        assert_eq!(s.learning, "en", "wraps around");
        m.key(Key::Down, &mut s);
        m.key(Key::Right, &mut s);
        assert_eq!(s.commitment, Commitment::Committed);
    }

    #[test]
    fn tab_switches_to_audio_and_back() {
        let mut m = menu();
        let mut s = Settings::default();
        m.key(Key::Tab, &mut s);
        assert_eq!(m.item(), Item::Mic);
        m.key(Key::Tab, &mut s);
        assert_eq!(m.item(), Item::Language);
        assert_eq!(m.click(WIDTH - 10, TABS_Y + 3, &mut s), Action::None);
        assert_eq!(m.tab, Tab::Audio, "clicking the tab header switches");
    }

    #[test]
    fn mic_and_voices_cycle_through_default_then_each_option() {
        let mut m = menu();
        let mut s = Settings::default();
        select(&mut m, Item::Mic);
        m.key(Key::Right, &mut s);
        assert_eq!(s.mic, "USB Mic");
        m.key(Key::Right, &mut s);
        m.key(Key::Right, &mut s);
        assert_eq!(s.mic, "", "back to the system default");
        select(&mut m, Item::VoiceLearning);
        m.key(Key::Right, &mut s);
        assert_eq!(s.voice_learning, "af_heart");
    }

    #[test]
    fn switching_engine_resets_voices_and_hides_an_unconfigured_command() {
        let mut m = menu();
        let mut s = Settings { voice_native: "Luciana".into(), ..Default::default() };
        select(&mut m, Item::Engine);
        m.key(Key::Right, &mut s);
        assert_eq!(s.tts_engine, TtsEngine::Http);
        assert!(s.voice_native.is_empty(), "voices belong to the old engine");
        m.key(Key::Right, &mut s);
        assert_eq!(s.tts_engine, TtsEngine::System, "command skipped: no tts_command set");
        s.validate().unwrap();
    }

    #[test]
    fn test_buttons_trigger_actions() {
        let mut m = menu();
        let mut s = Settings::default();
        select(&mut m, Item::TestMic);
        assert_eq!(m.key(Key::Enter, &mut s), Action::TestMic);
        select(&mut m, Item::TestVoices);
        assert_eq!(m.key(Key::Enter, &mut s), Action::TestVoices);
    }

    #[test]
    fn every_change_keeps_settings_valid() {
        let mut m = menu();
        let mut s = Settings::default();
        for tab in [Tab::Game, Tab::Audio] {
            m.tab = tab;
            for i in 0..m.items().len() {
                m.sel = i;
                for _ in 0..9 {
                    m.key(Key::Right, &mut s);
                    m.key(Key::Left, &mut s);
                    m.key(Key::Left, &mut s);
                    s.validate().unwrap();
                }
            }
        }
    }

    #[test]
    fn actions_and_navigation_wrap() {
        let mut m = menu();
        let mut s = Settings::default();
        assert_eq!(m.key(Key::Up, &mut s), Action::None);
        assert_eq!(m.item(), Item::Quit);
        assert_eq!(m.key(Key::Enter, &mut s), Action::Quit);
        assert_eq!(m.key(Key::Esc, &mut s), Action::Close);
    }

    #[test]
    fn clicking_a_row_selects_and_activates_it() {
        let mut m = menu();
        let mut s = Settings::default();
        let pause_row = GAME.iter().position(|i| *i == Item::Pause).unwrap() as i32;
        assert_eq!(m.click(50, TOP + pause_row * ROW_H + 3, &mut s), Action::TogglePause);
        assert_eq!(m.item(), Item::Pause);
        assert_eq!(m.click(50, 5, &mut s), Action::None, "title bar does nothing");
        assert_eq!(m.click(50, HEIGHT + 50, &mut s), Action::None);
    }

    #[test]
    fn panel_renders_both_tabs_with_supported_glyphs() {
        let mut m = menu();
        m.meter = Some(0.1);
        m.test_result = "Ouvi: \"I'm hungry\" ✓".into();
        let s = Settings::default();
        for item in GAME.iter().chain(AUDIO) {
            assert!(font::supports(Menu::label(*item)), "{item:?}");
            assert!(font::supports(&m.value(*item, &s)), "{item:?}");
        }
        for tab in [Tab::Game, Tab::Audio] {
            m.tab = tab;
            let mut c = Canvas::new(WIDTH, HEIGHT);
            m.draw(&mut c, &s, 0.0);
            assert_eq!(c.opaque_in(0, 0, WIDTH, HEIGHT), (WIDTH * HEIGHT) as usize);
        }
    }

    #[test]
    fn long_device_names_are_shortened() {
        assert_eq!(shorten("Built-in Audio Analog Stereo Microphone", 10), "Built-in …");
        assert_eq!(shorten("USB", 10), "USB");
    }
}
