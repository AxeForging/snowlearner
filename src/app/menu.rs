//! The control panel ("Painel"): a small pixel window to change settings live
//! and trigger actions, in three tabs — JOGO (game), ÁUDIO (mic, voices) and
//! PROGRESSO (what you know, what you're learning, what comes next).
//! Pure state + drawing; the app owns the window.
//! Keys: ↑/↓ choose, ←/→ change, Enter act, Tab switch tab, Esc close. Mouse: click.

use crate::config::level::Commitment;
use crate::config::settings::{Settings, WindowMode};
use crate::lang::text::{language_name, topic};
use crate::lang::{Native, T};
use crate::learn::deck::{Answer, LEVELS, PRE_A1};
use crate::learn::picker::Practice;
use crate::learn::progress::{Progress, Tally};
use crate::render::canvas::{Canvas, Rgba, hex};
use crate::render::font;
use crate::speech::voices::{TtsEngine, kokoro_default, kokoro_voice};

pub const WIDTH: i32 = 232;
pub const HEIGHT: i32 = 212;
const ROW_H: i32 = 12;
const TABS_Y: i32 = 13;
const TOP: i32 = 28;
const GOALS: &[u32] = &[3, 5, 10, 15, 20, 30, 50];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// The learner's own language: every text, meaning and the native voice.
    Native,
    Language,
    Commitment,
    Topic,
    Level,
    Practice,
    /// Which answer tier to practice (short, complete, polished, or all).
    Answer,
    Goal,
    Mode,
    PracticeNow,
    Summary,
    Pause,
    Quit,
    Mic,
    TestMic,
    Engine,
    /// Address of the HTTP voice server (typed in the panel).
    Endpoint,
    VoiceNative,
    VoiceLearning,
    TestVoices,
    Speaker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Game,
    Audio,
    Progress,
}

const TABS: [(Tab, T); 3] = [(Tab::Game, T::TabGame), (Tab::Audio, T::TabAudio), (Tab::Progress, T::TabProgress)];

pub const GAME: &[Item] = &[
    Item::Native,
    Item::Language,
    Item::Commitment,
    Item::Topic,
    Item::Level,
    Item::Practice,
    Item::Answer,
    Item::Goal,
    Item::Mode,
    Item::PracticeNow,
    Item::Summary,
    Item::Pause,
    Item::Quit,
];

pub const AUDIO: &[Item] = &[
    Item::Mic,
    Item::TestMic,
    Item::Engine,
    Item::Endpoint,
    Item::VoiceNative,
    Item::VoiceLearning,
    Item::TestVoices,
    Item::Speaker,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    Esc,
    /// Typed text, for the address field.
    Char(char),
    Backspace,
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
    /// What the PROGRESSO tab shows (the app refreshes it).
    pub progress: Option<Progress>,
    /// The address being typed; keys go to it until Enter or Esc.
    edit: Option<String>,
}

/// Completes a typed voice server address: `192.168.0.10:8880` becomes
/// `http://192.168.0.10:8880/v1` (Kokoro-FastAPI's base). An explicit path
/// is kept. None when it can't be an http(s) address.
pub fn normalize_url(raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) {
        return None;
    }
    let (scheme, rest) = match t.split_once("://") {
        Some((sc, r)) if sc.eq_ignore_ascii_case("http") || sc.eq_ignore_ascii_case("https") => (sc, r),
        Some(_) => return None,
        None => ("http", t),
    };
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    let path = if rest.contains('/') { "" } else { "/v1" };
    Some(format!("{}://{rest}{path}", scheme.to_ascii_lowercase()))
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

/// Shortens `text` with "…" until it fits `max` pixels.
fn fit(text: &str, max: i32) -> String {
    let mut out = text.to_string();
    while font::text_width(&out) > max && out.chars().count() > 1 {
        out = format!("{}…", out.chars().take(out.chars().count() - 2).collect::<String>());
    }
    out
}

/// Known (green), learning (yellow), not started (dark).
fn meter(c: &mut Canvas, x: i32, y: i32, w: i32, t: Tally) {
    c.rect(x, y, w, 5, hex(0x1d2a5a));
    if t.total == 0 {
        return;
    }
    let known = (t.known * w as usize / t.total) as i32;
    let learning = ((t.known + t.learning) * w as usize / t.total) as i32 - known;
    c.rect(x, y, known, 5, hex(0x7dff9b));
    c.rect(x + known, y, learning, 5, hex(0xffd64a));
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
            progress: None,
            edit: None,
        }
    }

    /// The address being typed, if the field is open.
    pub fn editing(&self) -> Option<&str> {
        self.edit.as_deref()
    }

    /// Keys while the address field is open: type, fix, save or cancel.
    fn edit_key(&mut self, key: Key, s: &mut Settings) -> Action {
        let Some(buf) = &mut self.edit else { return Action::None };
        match key {
            Key::Char(c) if !c.is_control() && buf.chars().count() < 200 => buf.push(c),
            Key::Backspace => drop(buf.pop()),
            Key::Enter => match normalize_url(buf) {
                Some(url) => {
                    s.tts_url = url;
                    s.tts_engine = TtsEngine::Http;
                    self.edit = None;
                    self.sel = self.items().iter().position(|i| *i == Item::TestVoices).unwrap_or(self.sel);
                    self.test_result = T::AddressSaved.get(s.native).into();
                    return Action::Changed(Item::Endpoint);
                }
                None => self.test_result = T::AddressInvalid.get(s.native).into(),
            },
            Key::Esc => self.edit = None,
            _ => {}
        }
        Action::None
    }

    pub fn items(&self) -> &'static [Item] {
        match self.tab {
            Tab::Game => GAME,
            Tab::Audio => AUDIO,
            Tab::Progress => &[],
        }
    }

    pub fn item(&self) -> Item {
        self.items()[self.sel.min(self.items().len() - 1)]
    }

    pub fn switch_tab(&mut self) {
        let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
        self.show(TABS[(i + 1) % TABS.len()].0);
    }

    pub fn show(&mut self, tab: Tab) {
        self.tab = tab;
        self.sel = 0;
    }

    fn change(&self, item: Item, s: &mut Settings, forward: bool) -> Action {
        match item {
            Item::Native => {
                s.native = cycle(&Native::ALL, &s.native, forward);
                // Nobody learns their own language: move on to one they can.
                if s.native.is(&s.learning)
                    && let Some(other) = self.learnable(s.native).first()
                {
                    s.learning = other.clone();
                }
            }
            Item::Language => s.learning = cycle(&self.learnable(s.native), &s.learning, forward),
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
            Item::Answer => s.answer = cycle(&Answer::CHOICES, &s.answer, forward),
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

    fn activate(&mut self, item: Item, s: &mut Settings) -> Action {
        match item {
            Item::PracticeNow => Action::PracticeNow,
            Item::Summary => Action::Summary,
            Item::Pause => Action::TogglePause,
            Item::Quit => Action::Quit,
            Item::TestMic => Action::TestMic,
            Item::TestVoices => Action::TestVoices,
            Item::Endpoint => {
                self.edit = Some(s.tts_url.clone());
                self.test_result = T::AddressTyping.get(s.native).into();
                Action::None
            }
            other => self.change(other, s, true),
        }
    }

    pub fn key(&mut self, key: Key, s: &mut Settings) -> Action {
        if self.edit.is_some() {
            return self.edit_key(key, s);
        }
        let n = self.items().len();
        match key {
            // PROGRESSO has nothing to select.
            Key::Up | Key::Down | Key::Left | Key::Right | Key::Enter if n == 0 => Action::None,
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
            Key::Char(_) | Key::Backspace => Action::None,
        }
    }

    /// Click at art coordinates: tabs switch pages; rows select and activate.
    pub fn click(&mut self, x: i32, y: i32, s: &mut Settings) -> Action {
        if !(0..WIDTH).contains(&x) {
            return Action::None;
        }
        if (TABS_Y..TABS_Y + ROW_H).contains(&y) {
            let want = TABS[(x * TABS.len() as i32 / WIDTH) as usize].0;
            if want != self.tab {
                self.show(want);
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

    /// Under a Kokoro voice row: who the voice is (gender, language and
    /// accent, read from its id), or which one "automática" plays.
    pub fn voice_note(&self, s: &Settings) -> Option<String> {
        if self.tab != Tab::Audio || s.tts_engine != TtsEngine::Http {
            return None;
        }
        let (voice, lang) = match self.item() {
            Item::VoiceNative => (&s.voice_native, s.native.code()),
            Item::VoiceLearning => (&s.voice_learning, s.learning.as_str()),
            _ => return None,
        };
        if voice.is_empty() {
            return kokoro_voice(kokoro_default(lang))
                .map(|k| T::VoiceAutoUses.fill(s.native, &[&k.describe(s.native)]));
        }
        kokoro_voice(voice).map(|k| k.describe(s.native))
    }

    /// Languages this native can learn: never their own.
    fn learnable(&self, native: Native) -> Vec<String> {
        self.languages.iter().filter(|l| !native.is(l)).cloned().collect()
    }

    fn value(&self, item: Item, s: &Settings) -> String {
        let n = s.native;
        let auto = |v: &str, empty: T| if v.is_empty() { empty.get(n).to_string() } else { shorten(v, 20) };
        match item {
            Item::Native => language_name(n, n.code()),
            Item::Language => language_name(n, &s.learning),
            Item::Commitment => s.commitment.label(n).into(),
            Item::Topic if s.topic.is_empty() => T::AllTopicsShort.get(n).into(),
            Item::Topic => shorten(&topic(n, &s.topic), 20),
            Item::Level if s.max_level == PRE_A1 => T::LevelBeginner.get(n).into(),
            Item::Level => T::LevelUpTo.fill(n, &[&s.max_level]),
            Item::Practice => match s.practice {
                Practice::Auto => T::PracticeAuto,
                Practice::Repeat => T::PracticeRepeat,
                Practice::Recall => T::PracticeRecall,
            }
            .get(n)
            .into(),
            Item::Answer => s.answer.label(n).into(),
            Item::Goal => T::GoalPhrases.fill(n, &[&s.daily_goal]),
            Item::Mode => match s.mode {
                WindowMode::Auto => T::ModeAuto,
                WindowMode::Window => T::ModeWindow,
                WindowMode::Overlay => T::ModeOverlay,
            }
            .get(n)
            .into(),
            Item::Pause => if self.paused { T::Paused } else { T::Active }.get(n).into(),
            Item::Mic => auto(&s.mic, T::SystemDefault),
            Item::Speaker => auto(&s.speaker, T::SystemDefault),
            Item::Endpoint => {
                let url = s.tts_url.split_once("://").map_or(s.tts_url.as_str(), |(_, r)| r);
                shorten(url, 22)
            }
            Item::Engine => match s.tts_engine {
                TtsEngine::System => T::EngineSystem.get(n).into(),
                TtsEngine::Http => "HTTP (Kokoro…)".into(),
                TtsEngine::Command => T::EngineCommand.get(n).into(),
            },
            Item::VoiceNative | Item::VoiceLearning => {
                let v = if item == Item::VoiceNative { &s.voice_native } else { &s.voice_learning };
                match kokoro_voice(v).filter(|_| s.tts_engine == TtsEngine::Http) {
                    Some(k) => k.short(n),
                    None => auto(v, T::VoiceAuto),
                }
            }
            _ => String::new(),
        }
    }

    fn label(item: Item, native: Native) -> &'static str {
        match item {
            Item::Native => T::ItemNative,
            Item::Language => T::ItemLanguage,
            Item::Commitment => T::ItemCommitment,
            Item::Topic => T::ItemTopic,
            Item::Level => T::ItemLevel,
            Item::Practice => T::ItemPractice,
            Item::Answer => T::ItemAnswer,
            Item::Goal => T::ItemGoal,
            Item::Mode => T::ItemMode,
            Item::PracticeNow => T::ItemPracticeNow,
            Item::Summary => T::ItemSummary,
            Item::Pause => T::ItemPause,
            Item::Quit => T::ItemQuit,
            Item::Mic => T::ItemMic,
            Item::TestMic => T::ItemTestMic,
            Item::Engine => T::ItemEngine,
            Item::Endpoint => T::ItemEndpoint,
            Item::VoiceNative => T::ItemVoiceNative,
            Item::VoiceLearning => T::ItemVoiceLearning,
            Item::TestVoices => T::ItemTestVoices,
            Item::Speaker => T::ItemSpeaker,
        }
        .get(native)
    }

    fn draw_progress(&self, c: &mut Canvas, n: Native, ink: Rgba, dim: Rgba, accent: Rgba) {
        let gold = hex(0xffd64a);
        let Some(p) = &self.progress else {
            font::draw(c, 6, TOP - 2, T::Loading.get(n), dim);
            return;
        };
        let o = p.overall;
        let head = T::ProgressHead.fill(n, &[&o.known, &o.total, &o.percent(), &o.learning]);
        font::draw(c, 6, TOP - 2, &fit(&head, WIDTH - 12), ink);
        for (k, (stage, t)) in p.stages.iter().enumerate() {
            let y = TOP + (k as i32 + 1) * ROW_H;
            let here = p.current == Some(*stage);
            if here {
                font::draw(c, 3, y - 2, ">", gold);
            }
            font::draw(c, 10, y - 2, stage.label(n), if here { gold } else { ink });
            meter(c, 76, y + 3, 100, *t);
            let n = format!("{}/{}", t.known, t.total);
            font::draw(c, WIDTH - font::text_width(&n) - 6, y - 2, &n, accent);
        }
        let mut y = TOP + 4 * ROW_H + 2;
        if p.learning.is_empty() && p.next_up.is_empty() {
            font::draw(c, 6, y - 2, T::KnowsAll.get(n), gold);
            return;
        }
        font::draw(c, 6, y - 2, T::LearningNow.get(n), dim);
        for it in p.learning.iter().take(4) {
            y += ROW_H - 1;
            font::draw(c, 10, y - 2, &fit(&format!("{} = {}", it.say, it.meaning), WIDTH - 16), ink);
        }
        if !p.next_up.is_empty() {
            y += ROW_H + 1;
            let next: Vec<&str> = p.next_up.iter().map(|i| i.say.as_str()).collect();
            font::draw(c, 6, y - 2, &fit(&T::NextUp.fill(n, &[&next.join(" · ")]), WIDTH - 12), dim);
        }
    }

    pub fn draw(&self, c: &mut Canvas, s: &Settings, time: f32) {
        let n = s.native;
        let (bg, ink, dim, accent, sel_bg): (Rgba, Rgba, Rgba, Rgba, Rgba) =
            (hex(0x0e0c2c), hex(0xe6ecff), hex(0x8f96d8), hex(0x9be8ff), hex(0x2a5a9a));
        c.clear(bg);
        for x in 0..c.w {
            c.set(x, 0, hex(0x4ea2d8));
            c.set(x, c.h - 1, hex(0x4ea2d8));
        }
        font::draw(c, 6, 1, T::PanelTitle.get(n), hex(0xffd64a));
        let tab_w = WIDTH / TABS.len() as i32;
        for (i, (tab, name)) in TABS.iter().enumerate() {
            let x0 = i as i32 * tab_w;
            let on = *tab == self.tab;
            c.rect(x0 + 2, TABS_Y, tab_w - 4, ROW_H, if on { hex(0x2a1650) } else { hex(0x151233) });
            if on {
                c.rect(x0 + 2, TABS_Y + ROW_H - 1, tab_w - 4, 1, hex(0xb46cff));
            }
            let name = name.get(n);
            let tw = font::text_width(name);
            font::draw(c, x0 + (tab_w - tw) / 2, TABS_Y - 2, name, if on { hex(0xffffff) } else { dim });
        }
        if self.tab == Tab::Progress {
            self.draw_progress(c, n, ink, dim, accent);
            font::draw(c, 6, c.h - 13, T::ProgressFooter.get(n), dim);
            return;
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
            font::draw(c, 10, y - 2, Self::label(*item, n), if selected { hex(0xffffff) } else { ink });
            if *item == Item::Endpoint
                && let Some(buf) = &self.edit
            {
                // The end of what is typed, so the cursor stays in view.
                let room = c.w - 70;
                let mut shown: String = buf.clone();
                while font::text_width(&shown) > room - 8 && !shown.is_empty() {
                    shown.remove(0);
                }
                let cursor = if (time * 3.0) as i32 % 2 == 0 { "_" } else { " " };
                let text = format!("{shown}{cursor}");
                let w = font::text_width(&text);
                c.rect(c.w - room - 6, y, room + 2, ROW_H, hex(0x151233));
                font::draw(c, c.w - w - 6, y - 2, &text, hex(0xffd64a));
                continue;
            }
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
                font::draw(c, 116, y - 1, T::SaySomething.get(n), accent);
            }
            let note = self.meter.is_none().then(|| self.voice_note(s)).flatten();
            let text = note.as_deref().unwrap_or(&self.test_result);
            for (k, line) in font::wrap(text, WIDTH - 16).iter().take(3).enumerate() {
                font::draw(c, 6, y + 10 + k as i32 * (ROW_H - 1), line, hex(0xe6ecff));
            }
        } else {
            font::draw(c, 6, foot_y - 2, &self.status, accent);
        }
        font::draw(c, 6, c.h - 13, T::PanelFooter.get(n), dim);
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
        select(&mut m, Item::Language);
        assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Language));
        assert_eq!(s.learning, "es");
        assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Language));
        assert_eq!(s.learning, "en", "wraps around");
        m.key(Key::Down, &mut s);
        m.key(Key::Right, &mut s);
        assert_eq!(s.commitment, Commitment::Committed);
    }

    fn full_menu() -> Menu {
        Menu::new(vec!["en".into(), "es".into(), "pt-BR".into()], vec!["trabalho".into()])
    }

    #[test]
    fn my_language_cycles_and_never_leaves_you_learning_it() {
        let mut m = full_menu();
        let mut s = Settings::default();
        assert_eq!(m.item(), Item::Native, "your own language comes first");
        assert_eq!(m.value(Item::Native, &s), "português");
        assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Native));
        assert_eq!(s.native, Native::En);
        assert_eq!(s.learning, "es", "English speakers don't learn English: moved on to the next language");
        s.validate().unwrap();
        assert_eq!(m.value(Item::Native, &s), "English");
        assert_eq!(m.value(Item::Language, &s), "Spanish");
        assert_eq!(m.key(Key::Left, &mut s), Action::Changed(Item::Native));
        assert_eq!((s.native, s.learning.as_str()), (Native::PtBr, "es"), "still learnable: kept");
    }

    #[test]
    fn the_language_row_skips_your_own_language() {
        let mut m = full_menu();
        for native in Native::ALL {
            let learning = if native == Native::En { "es" } else { "en" };
            let mut s = Settings { native, learning: learning.into(), ..Default::default() };
            select(&mut m, Item::Language);
            let mut seen = Vec::new();
            for _ in 0..4 {
                m.key(Key::Right, &mut s);
                seen.push(s.learning.clone());
                s.validate().unwrap();
            }
            assert!(seen.iter().all(|l| !native.is(l)), "{native:?} offered {seen:?}");
            assert_eq!(seen.iter().collect::<std::collections::HashSet<_>>().len(), 2, "{native:?}: {seen:?}");
        }
    }

    #[test]
    fn the_whole_panel_reads_in_english_for_an_english_speaker() {
        let mut m = full_menu();
        let s =
            Settings { native: Native::En, learning: "pt-BR".into(), topic: "trabalho".into(), ..Default::default() };
        assert_eq!(Menu::label(Item::Native, s.native), "My language");
        assert_eq!(m.value(Item::Language, &s), "Portuguese");
        assert_eq!(m.value(Item::Topic, &s), "work", "topic keys read in English");
        assert_eq!(m.value(Item::Answer, &s), "all");
        assert_eq!(m.value(Item::Goal, &s), "10 phrases");
        select(&mut m, Item::Endpoint);
        let mut s2 = s.clone();
        m.key(Key::Enter, &mut s2);
        assert_eq!(m.test_result, T::AddressTyping.get(Native::En));
        for tab in [Tab::Game, Tab::Audio, Tab::Progress] {
            m.tab = tab;
            let mut c = Canvas::new(WIDTH, HEIGHT);
            m.draw(&mut c, &s, 0.0);
            assert_eq!(c.opaque_in(0, 0, WIDTH, HEIGHT), (WIDTH * HEIGHT) as usize);
        }
    }

    #[test]
    fn every_game_row_and_the_status_fit_above_the_footer() {
        let status_y = TOP + GAME.len() as i32 * ROW_H + 2;
        assert!(status_y + font::LINE_H <= HEIGHT - 13, "status row {status_y} runs into the footer");
    }

    #[test]
    fn tab_cycles_game_audio_progress_and_back() {
        let mut m = menu();
        let mut s = Settings::default();
        m.key(Key::Tab, &mut s);
        assert_eq!(m.item(), Item::Mic);
        m.key(Key::Tab, &mut s);
        assert_eq!(m.tab, Tab::Progress);
        m.key(Key::Tab, &mut s);
        assert_eq!(m.item(), Item::Native);
        assert_eq!(m.click(WIDTH / 2, TABS_Y + 3, &mut s), Action::None);
        assert_eq!(m.tab, Tab::Audio, "clicking the tab header switches");
        m.click(WIDTH - 10, TABS_Y + 3, &mut s);
        assert_eq!(m.tab, Tab::Progress);
    }

    #[test]
    fn the_progress_tab_has_nothing_to_select_and_never_panics() {
        let mut m = menu();
        let mut s = Settings::default();
        m.show(Tab::Progress);
        for k in [Key::Up, Key::Down, Key::Left, Key::Right, Key::Enter] {
            assert_eq!(m.key(k, &mut s), Action::None);
        }
        assert_eq!(m.click(50, TOP + 20, &mut s), Action::None);
        assert_eq!(m.key(Key::Esc, &mut s), Action::Close);
    }

    #[test]
    fn the_progress_tab_draws_stages_items_and_long_text_fits() {
        use crate::learn::deck::Phrase;
        use crate::learn::progress::progress;
        let long = "Could you please walk me through the whole deployment process again?";
        let phrases: Vec<Phrase> = ["Água.", "Hello.", "Good morning.", long]
            .iter()
            .map(|s| Phrase { level: Some("A1".into()), ..Phrase::new(s, "significado bem comprido demais") })
            .collect();
        let s = Settings::default();
        let mut m = menu();
        m.show(Tab::Progress);
        for p in [
            None,
            Some(progress(&phrases, &[0, 1, 2, 3], &Default::default())),
            Some(progress(&phrases, &[], &Default::default())),
        ] {
            m.progress = p;
            let mut c = Canvas::new(WIDTH, HEIGHT);
            m.draw(&mut c, &s, 0.0);
            assert_eq!(c.opaque_in(0, 0, WIDTH, HEIGHT), (WIDTH * HEIGHT) as usize);
        }
        assert!(font::text_width(&fit(long, 100)) <= 100);
        assert!(font::supports(&fit(long, 100)));
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

    fn type_text(m: &mut Menu, s: &mut Settings, text: &str) {
        for ch in text.chars() {
            assert_eq!(m.key(Key::Char(ch), s), Action::None);
        }
    }

    #[test]
    fn the_tts_address_is_typed_saved_and_leads_to_the_voice_test() {
        let mut m = menu();
        let mut s = Settings::default();
        select(&mut m, Item::Endpoint);
        assert_eq!(m.key(Key::Enter, &mut s), Action::None, "Enter starts editing");
        assert!(m.editing().is_some());
        for _ in 0..s.tts_url.chars().count() {
            m.key(Key::Backspace, &mut s);
        }
        type_text(&mut m, &mut s, "192.168.0.10:88800");
        m.key(Key::Backspace, &mut s);
        m.key(Key::Down, &mut s); // arrows don't leave the field
        assert_eq!(m.editing(), Some("192.168.0.10:8880"));
        assert_eq!(m.key(Key::Enter, &mut s), Action::Changed(Item::Endpoint));
        assert_eq!(s.tts_url, "http://192.168.0.10:8880/v1");
        assert_eq!(s.tts_engine, TtsEngine::Http, "an address means the HTTP engine");
        assert!(m.editing().is_none());
        assert_eq!(m.item(), Item::TestVoices, "Enter again plays the voices through it");
        assert_eq!(m.key(Key::Enter, &mut s), Action::TestVoices);
        s.validate().unwrap();
    }

    #[test]
    fn esc_cancels_the_address_edit_without_closing_the_panel() {
        let mut m = menu();
        let mut s = Settings::default();
        let before = s.tts_url.clone();
        select(&mut m, Item::Endpoint);
        m.key(Key::Enter, &mut s);
        type_text(&mut m, &mut s, "zzz");
        assert_eq!(m.key(Key::Esc, &mut s), Action::None);
        assert!(m.editing().is_none());
        assert_eq!(s.tts_url, before);
        assert_eq!(m.key(Key::Esc, &mut s), Action::Close);
    }

    #[test]
    fn an_unusable_address_is_refused_and_stays_in_the_field() {
        let mut m = menu();
        let mut s = Settings::default();
        let before = s.tts_url.clone();
        select(&mut m, Item::Endpoint);
        m.key(Key::Enter, &mut s);
        for _ in 0..before.chars().count() {
            m.key(Key::Backspace, &mut s);
        }
        type_text(&mut m, &mut s, "ftp://x");
        assert_eq!(m.key(Key::Enter, &mut s), Action::None);
        assert_eq!(s.tts_url, before);
        assert_eq!(m.editing(), Some("ftp://x"), "kept for fixing");
        assert!(m.test_result.contains("http://"), "{}", m.test_result);
    }

    #[test]
    fn a_long_address_being_typed_draws_inside_the_panel() {
        let mut m = menu();
        let mut s = Settings::default();
        select(&mut m, Item::Endpoint);
        m.key(Key::Enter, &mut s);
        type_text(&mut m, &mut s, "/very/long/path/to/some/kokoro/server/behind/a/proxy/v1");
        for t in [0.0, 0.4] {
            let mut c = Canvas::new(WIDTH, HEIGHT);
            m.draw(&mut c, &s, t);
            assert_eq!(c.opaque_in(0, 0, WIDTH, HEIGHT), (WIDTH * HEIGHT) as usize);
        }
        assert!(font::supports(&m.test_result), "{}", m.test_result);
    }

    #[test]
    fn kokoro_voices_show_who_they_are_in_the_row_and_below_it() {
        let mut m = menu();
        m.voices_learning = vec!["af_heart".into(), "bf_emma".into()];
        let mut s = Settings { tts_engine: TtsEngine::Http, ..Default::default() };
        select(&mut m, Item::VoiceLearning);
        assert_eq!(
            m.voice_note(&s).as_deref(),
            Some("Automática, usa Heart: voz feminina, inglês americano (af_heart)")
        );
        m.key(Key::Right, &mut s);
        m.key(Key::Right, &mut s);
        assert_eq!(s.voice_learning, "bf_emma", "the setting keeps Kokoro's id");
        assert_eq!(m.value(Item::VoiceLearning, &s), "Emma · RU");
        assert_eq!(m.voice_note(&s).as_deref(), Some("Emma: voz feminina, inglês britânico (bf_emma)"));
        select(&mut m, Item::VoiceNative);
        assert_eq!(
            m.voice_note(&s).as_deref(),
            Some("Automática, usa Dora: voz feminina, português do Brasil (pf_dora)")
        );
        select(&mut m, Item::Speaker);
        assert_eq!(m.voice_note(&s), None, "only on the voice rows");
    }

    #[test]
    fn system_voices_keep_their_own_names() {
        let mut m = menu();
        let s = Settings { voice_learning: "Microsoft Zira Desktop".into(), ..Default::default() };
        select(&mut m, Item::VoiceLearning);
        assert_eq!(m.value(Item::VoiceLearning, &s), "Microsoft Zira Desk…");
        assert_eq!(m.voice_note(&s), None, "nothing to decode outside Kokoro");
    }

    #[test]
    fn the_level_starts_at_pre_a1_and_says_it_is_for_beginners() {
        let mut m = menu();
        let mut s = Settings { max_level: "A1".into(), ..Default::default() };
        select(&mut m, Item::Level);
        m.key(Key::Left, &mut s);
        assert_eq!(s.max_level, "PRE-A1");
        assert_eq!(m.value(Item::Level, &s), "pré-A1 · iniciante");
        m.key(Key::Right, &mut s);
        assert_eq!(m.value(Item::Level, &s), "até A1");
        s.validate().unwrap();
    }

    #[test]
    fn the_answer_row_cycles_all_short_complete_polished() {
        let mut m = menu();
        let mut s = Settings::default();
        select(&mut m, Item::Answer);
        let mut seen = vec![m.value(Item::Answer, &s)];
        for _ in 0..4 {
            assert_eq!(m.key(Key::Right, &mut s), Action::Changed(Item::Answer));
            seen.push(m.value(Item::Answer, &s));
        }
        assert_eq!(seen, ["todas", "curta", "completa", "polida", "todas"]);
        m.key(Key::Left, &mut s);
        assert_eq!(s.answer, Answer::Polished, "left goes back");
    }

    #[test]
    fn tts_addresses_are_completed_and_checked() {
        assert_eq!(normalize_url("192.168.0.10:8880").as_deref(), Some("http://192.168.0.10:8880/v1"));
        assert_eq!(normalize_url(" localhost:8880 ").as_deref(), Some("http://localhost:8880/v1"));
        assert_eq!(normalize_url("http://h:1/v1/").as_deref(), Some("http://h:1/v1"));
        assert_eq!(normalize_url("https://tts.example.com/api").as_deref(), Some("https://tts.example.com/api"));
        assert_eq!(normalize_url("HTTP://Kokoro:8880").as_deref(), Some("http://Kokoro:8880/v1"));
        for bad in ["", "   ", "http://", "ftp://x", "http://a b"] {
            assert_eq!(normalize_url(bad), None, "{bad:?}");
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
            for native in Native::ALL {
                let s = Settings { native, learning: "es".into(), ..s.clone() };
                assert!(font::supports(Menu::label(*item, native)), "{item:?}");
                assert!(font::supports(&m.value(*item, &s)), "{item:?}");
            }
        }
        for tab in [Tab::Game, Tab::Audio, Tab::Progress] {
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
