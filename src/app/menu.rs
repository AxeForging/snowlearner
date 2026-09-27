//! The control panel ("Painel"): a small pixel window to change settings live
//! and trigger actions. Pure state + drawing; the app owns the window.
//! Keys: ↑/↓ choose, ←/→ change, Enter act, Esc close. Mouse: click a row.

use crate::config::level::Commitment;
use crate::config::settings::{Settings, WindowMode};
use crate::learn::deck::LEVELS;
use crate::learn::picker::Practice;
use crate::render::canvas::{Canvas, Rgba, hex};
use crate::render::font;

pub const WIDTH: i32 = 220;
pub const HEIGHT: i32 = 176;
const ROW_H: i32 = 12;
const TOP: i32 = 22;
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
}

pub const ITEMS: &[Item] = &[
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
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
    Quit,
    Close,
}

pub struct Menu {
    pub sel: usize,
    pub languages: Vec<String>,
    /// Topics of the current deck ("" = all is added automatically).
    pub topics: Vec<String>,
    pub paused: bool,
    /// Line shown at the bottom ("Hoje: 4/10 · combo x2").
    pub status: String,
}

fn cycle<T: PartialEq + Clone>(options: &[T], current: &T, forward: bool) -> T {
    let n = options.len();
    let i = options.iter().position(|o| o == current).unwrap_or(0);
    let j = if forward { (i + 1) % n } else { (i + n - 1) % n };
    options[j].clone()
}

impl Menu {
    pub fn new(languages: Vec<String>, topics: Vec<String>) -> Menu {
        Menu { sel: 0, languages, topics, paused: false, status: String::new() }
    }

    pub fn item(&self) -> Item {
        ITEMS[self.sel]
    }

    fn change(&self, item: Item, s: &mut Settings, forward: bool) -> Action {
        match item {
            Item::Language => s.learning = cycle(&self.languages, &s.learning, forward),
            Item::Commitment => {
                let all = [Commitment::Chill, Commitment::Steady, Commitment::Committed, Commitment::Relentless];
                s.commitment = cycle(&all, &s.commitment, forward);
            }
            Item::Topic => {
                let mut all = vec![String::new()];
                all.extend(self.topics.iter().cloned());
                s.topic = cycle(&all, &s.topic, forward);
            }
            Item::Level => {
                let all: Vec<String> = LEVELS.iter().map(|l| l.to_string()).collect();
                s.max_level = cycle(&all, &s.max_level, forward);
            }
            Item::Practice => {
                s.practice = cycle(&[Practice::Auto, Practice::Repeat, Practice::Recall], &s.practice, forward)
            }
            Item::Goal => {
                let next = if forward {
                    GOALS.iter().copied().find(|g| *g > s.daily_goal).unwrap_or(GOALS[0])
                } else {
                    GOALS.iter().rev().copied().find(|g| *g < s.daily_goal).unwrap_or(*GOALS.last().unwrap())
                };
                s.daily_goal = next;
            }
            Item::Mode => {
                s.mode = cycle(&[WindowMode::Auto, WindowMode::Window, WindowMode::Overlay], &s.mode, forward)
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
            other => self.change(other, s, true),
        }
    }

    pub fn key(&mut self, key: Key, s: &mut Settings) -> Action {
        match key {
            Key::Up => {
                self.sel = (self.sel + ITEMS.len() - 1) % ITEMS.len();
                Action::None
            }
            Key::Down => {
                self.sel = (self.sel + 1) % ITEMS.len();
                Action::None
            }
            Key::Left => self.change(self.item(), s, false),
            Key::Right => self.change(self.item(), s, true),
            Key::Enter => self.activate(self.item(), s),
            Key::Esc => Action::Close,
        }
    }

    /// Click at art coordinates: selects the row and activates it.
    pub fn click(&mut self, x: i32, y: i32, s: &mut Settings) -> Action {
        if !(0..WIDTH).contains(&x) || y < TOP {
            return Action::None;
        }
        let row = ((y - TOP) / ROW_H) as usize;
        if row >= ITEMS.len() {
            return Action::None;
        }
        self.sel = row;
        self.activate(ITEMS[row], s)
    }

    fn value(&self, item: Item, s: &Settings) -> String {
        match item {
            Item::Language => match s.learning.as_str() {
                "en" => "inglês".into(),
                "es" => "espanhol".into(),
                other => other.into(),
            },
            Item::Commitment => s.commitment.label_pt().into(),
            Item::Topic => {
                if s.topic.is_empty() {
                    "todos".into()
                } else {
                    s.topic.clone()
                }
            }
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
            Item::Pause => {
                if self.paused {
                    "pausado".into()
                } else {
                    "ativo".into()
                }
            }
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
        font::draw(c, 6, 3, "SNOWLEARNER · PAINEL", hex(0xffd64a));
        for (i, item) in ITEMS.iter().enumerate() {
            let y = TOP + i as i32 * ROW_H;
            let selected = i == self.sel;
            if selected {
                c.rect(2, y, c.w - 4, ROW_H, sel_bg);
                let blink = (time * 3.0) as i32 % 2 == 0;
                if blink {
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
        let foot_y = TOP + ITEMS.len() as i32 * ROW_H + 2;
        font::draw(c, 6, foot_y - 2, &self.status, accent);
        font::draw(c, 6, foot_y + 9, "*ao reiniciar  ↑↓ ←→ Enter Esc", dim);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Menu {
        Menu::new(vec!["en".into(), "es".into()], vec!["trabalho".into(), "viagem".into()])
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
        m.key(Key::Left, &mut s);
        m.key(Key::Left, &mut s);
        assert_eq!(s.commitment, Commitment::Chill);
    }

    #[test]
    fn topic_cycles_through_all_then_each_deck_topic() {
        let mut m = menu();
        let mut s = Settings::default();
        m.sel = ITEMS.iter().position(|i| *i == Item::Topic).unwrap();
        m.key(Key::Right, &mut s);
        assert_eq!(s.topic, "trabalho");
        m.key(Key::Right, &mut s);
        m.key(Key::Right, &mut s);
        assert_eq!(s.topic, "", "back to all topics");
    }

    #[test]
    fn every_change_keeps_settings_valid() {
        let mut m = menu();
        let mut s = Settings::default();
        for i in 0..ITEMS.len() {
            m.sel = i;
            for _ in 0..9 {
                m.key(Key::Right, &mut s);
                m.key(Key::Left, &mut s);
                m.key(Key::Left, &mut s);
                s.validate().unwrap();
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
        m.sel = ITEMS.iter().position(|i| *i == Item::PracticeNow).unwrap();
        assert_eq!(m.key(Key::Enter, &mut s), Action::PracticeNow);
    }

    #[test]
    fn clicking_a_row_selects_and_activates_it() {
        let mut m = menu();
        let mut s = Settings::default();
        let pause_row = ITEMS.iter().position(|i| *i == Item::Pause).unwrap() as i32;
        assert_eq!(m.click(50, TOP + pause_row * ROW_H + 3, &mut s), Action::TogglePause);
        assert_eq!(m.item(), Item::Pause);
        assert_eq!(m.click(50, 5, &mut s), Action::None, "title bar does nothing");
        assert_eq!(m.click(50, HEIGHT + 50, &mut s), Action::None);
    }

    #[test]
    fn panel_renders_every_row_with_supported_glyphs() {
        let m = menu();
        let s = Settings::default();
        for item in ITEMS {
            assert!(font::supports(Menu::label(*item)), "{item:?}");
            assert!(font::supports(&m.value(*item, &s)), "{item:?}");
        }
        let mut c = Canvas::new(WIDTH, HEIGHT);
        m.draw(&mut c, &s, 0.0);
        assert!(c.opaque_in(0, 0, WIDTH, HEIGHT) == (WIDTH * HEIGHT) as usize);
    }
}
