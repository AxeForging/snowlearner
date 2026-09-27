//! On-screen text: lesson captions (legendas) with highlights and per-word
//! feedback, the end-of-day summary panel, and short toasts.

use crate::learn::cue::Segment;
use crate::render::canvas::{Canvas, Rgba, hex};
use crate::render::font;
use crate::speech::matcher::WordHit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// TTS is reading the cue.
    Speaking,
    /// Mic is open.
    Listening,
    /// Recognizer is working.
    Thinking,
    Passed,
    Failed,
    /// No recognizer available: say it, then confirm with the hotkey.
    Confirm,
}

impl Status {
    fn label(self) -> &'static str {
        match self {
            Status::Speaking => "OUÇA",
            Status::Listening => "FALE AGORA",
            Status::Thinking => "PENSANDO",
            Status::Passed => "ACERTOU!",
            Status::Failed => "QUASE! TENTE DE NOVO",
            Status::Confirm => "FALE E CONFIRME",
        }
    }

    fn color(self) -> Rgba {
        match self {
            Status::Passed => hex(0x7dff9b),
            Status::Failed => hex(0xff8a6b),
            Status::Listening => hex(0xffd64a),
            _ => hex(0x9be8ff),
        }
    }
}

pub struct Caption {
    pub segments: Vec<Segment>,
    /// The phrase being practiced (target segments equal to it get feedback colors).
    pub say: String,
    /// Segment currently being read aloud.
    pub active: Option<usize>,
    pub meaning: String,
    pub status: Status,
    pub feedback: Option<Vec<WordHit>>,
    pub heard: Option<String>,
    pub footer: String,
    /// Small right-aligned header label: "trabalho · A2 · de memória".
    pub tag: String,
    /// Live microphone state while listening.
    pub listen: Option<Meter>,
}

/// Mic level meter + thinking-time countdown shown while listening.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meter {
    pub level: f32,
    pub speaking: bool,
    pub think_left: f32,
    pub think_total: f32,
}

/// Progress corner: today's count against the goal, combo, language/topic.
pub struct Stats {
    pub done: u32,
    pub goal: u32,
    pub combo: u32,
    pub label: String,
}

pub struct SummaryLine {
    pub say: String,
    pub meaning: String,
    pub ok: bool,
}

pub struct SummaryPanel {
    pub title: String,
    pub lines: Vec<SummaryLine>,
    pub active: Option<usize>,
    pub footer: String,
}

pub struct Toast {
    pub text: String,
    pub left: f32,
}

#[derive(Default)]
pub struct Hud {
    pub caption: Option<Caption>,
    pub summary: Option<SummaryPanel>,
    pub toast: Option<Toast>,
    pub stats: Option<Stats>,
    /// Key/mouse cheat sheet (window mode, `H`).
    pub help: Option<Vec<String>>,
    /// Compact shortcut sheet next to the orb (hover / Ctrl+Alt held).
    pub cheats: Option<Cheats>,
}

/// Shortcut sheet: a prefix ("Ctrl+Alt +") and key → action rows, drawn next to `anchor`.
#[derive(Debug, Clone, PartialEq)]
pub struct Cheats {
    pub anchor: (f32, f32),
    pub prefix: String,
    pub rows: Vec<(String, String)>,
    pub footer: String,
}

impl Cheats {
    /// Builds the sheet from hotkeys like "Ctrl+Alt+M", sharing their common prefix.
    pub fn from_hotkeys(anchor: (f32, f32), keys: &[(&str, &str)], footer: &str) -> Cheats {
        let split = |k: &str| {
            k.rsplit_once('+').map(|(p, l)| (format!("{p}+"), l.to_string())).unwrap_or((String::new(), k.to_string()))
        };
        let prefix = keys.first().map(|(k, _)| split(k).0).unwrap_or_default();
        let shared = keys.iter().all(|(k, _)| split(k).0 == prefix);
        let rows =
            keys.iter().map(|(k, what)| (if shared { split(k).1 } else { k.to_string() }, what.to_string())).collect();
        let prefix = if shared { prefix.trim_end_matches('+').to_string() + " +" } else { String::new() };
        Cheats { anchor, prefix, rows, footer: footer.to_string() }
    }
}

const INK: Rgba = hex(0xe6ecff);
const DIM: Rgba = hex(0x8f96d8);
const TARGET: Rgba = hex(0x9be8ff);
const HIT: Rgba = hex(0x7dff9b);
const MISS: Rgba = hex(0xff6b6b);
const PANEL: Rgba = hex(0x0e0c2c);
const BORDER: Rgba = hex(0x4ea2d8);

const MIC: &[&str] = &[".##.", ".##.", ".##.", "#..#", ".##.", "..#.", ".###"];
const SPEAKER: &[&str] = &["..#..", ".##.#", "###..", "###.#", ".##..", "..#.#"];
const CHECK: &[&str] = &["....#", "...#.", "#.#..", ".#..."];
const CROSS: &[&str] = &["#...#", ".#.#.", "..#..", ".#.#.", "#...#"];

struct Token {
    text: String,
    color: Rgba,
    marked: bool,
}

impl Hud {
    pub fn step(&mut self, dt: f32) {
        if let Some(t) = &mut self.toast {
            t.left -= dt;
            if t.left <= 0.0 {
                self.toast = None;
            }
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, seconds: f32) {
        self.toast = Some(Toast { text: text.into(), left: seconds });
    }

    pub fn draw(&self, c: &mut Canvas, ground_y: i32, time: f32) {
        if let Some(cap) = &self.caption {
            draw_caption(c, cap, ground_y, time);
        }
        if let Some(s) = &self.summary {
            draw_summary(c, s, time);
        }
        if let Some(st) = &self.stats {
            draw_stats(c, st);
        }
        if let Some(lines) = &self.help {
            draw_help(c, lines);
        }
        if let Some(ch) = &self.cheats {
            draw_cheats(c, ch);
        }
        if let Some(t) = &self.toast {
            let w = font::text_width(&t.text);
            font::draw_outlined(c, (c.w - w) / 2, 6, &t.text, hex(0xffffff), hex(0x1b1942));
        }
    }
}

fn draw_stats(c: &mut Canvas, st: &Stats) {
    let outline = hex(0x1b1942);
    let goal = format!("{}/{}", st.done, st.goal);
    let mut text = format!("{} · {goal}", st.label);
    if st.combo >= 2 {
        text.push_str(&format!(" · x{}", st.combo));
    }
    let w = font::text_width(&text);
    let (x, y) = (c.w - w - 6, 4 + font::LINE_H);
    font::draw_outlined(c, x, y, &text, hex(0xe6ecff), outline);
    // Goal bar under the text.
    let bar_w = w.min(60);
    let fill = (bar_w as u32 * st.done.min(st.goal)).checked_div(st.goal).map_or(bar_w, |f| f as i32);
    let by = y + font::LINE_H;
    c.rect(x + w - bar_w - 1, by - 1, bar_w + 2, 4, outline);
    c.rect(x + w - bar_w, by, fill, 2, if st.done >= st.goal { hex(0x7dff9b) } else { hex(0xffb03a) });
}

fn draw_cheats(c: &mut Canvas, ch: &Cheats) {
    let lh = font::LINE_H - 2;
    let key_w = ch.rows.iter().map(|(k, _)| font::text_width(k)).max().unwrap_or(0);
    let mut lines = ch.rows.len() as i32;
    let mut w = ch.rows.iter().map(|(_, a)| key_w + 10 + font::text_width(a)).max().unwrap_or(0);
    if !ch.prefix.is_empty() {
        lines += 1;
        w = w.max(font::text_width(&ch.prefix));
    }
    let footer = font::wrap(&ch.footer, 140);
    lines += footer.len() as i32;
    w = w.max(footer.iter().map(|l| font::text_width(l)).max().unwrap_or(0)) + 12;
    let h = lines * lh + 8;
    // Open toward the middle of the screen from the orb.
    let (ax, ay) = (ch.anchor.0 as i32, ch.anchor.1 as i32);
    let x = if ax > c.w / 2 { ax - w - 10 } else { ax + 10 };
    let y = if ay > c.h / 2 { ay - h } else { ay - 4 };
    let (x, y) = (x.clamp(2, (c.w - w - 2).max(2)), y.clamp(2, (c.h - h - 2).max(2)));
    panel(c, x, y, w, h);
    let mut yy = y + 3;
    if !ch.prefix.is_empty() {
        font::draw(c, x + 6, yy - 2, &ch.prefix, hex(0xffd64a));
        yy += lh;
    }
    for (k, action) in &ch.rows {
        // Pixel key cap.
        let kw = font::text_width(k) + 4;
        c.rect(x + 6, yy, kw, lh - 1, hex(0x2a1650));
        c.rect(x + 6, yy + lh - 2, kw, 1, hex(0xb46cff));
        font::draw(c, x + 8, yy - 3, k, hex(0xffffff));
        font::draw(c, x + 12 + key_w, yy - 3, action, INK);
        yy += lh;
    }
    for l in footer {
        font::draw(c, x + 6, yy - 3, &l, DIM);
        yy += lh;
    }
}

fn draw_help(c: &mut Canvas, lines: &[String]) {
    let lh = font::LINE_H - 1;
    let w = lines.iter().map(|l| font::text_width(l)).max().unwrap_or(0) + 12;
    let h = lines.len() as i32 * lh + 8;
    let (x, y) = (6, (c.h - h) / 2);
    panel(c, x, y, w.min(c.w - 12), h);
    for (i, l) in lines.iter().enumerate() {
        let col = if i == 0 { hex(0xffd64a) } else { INK };
        font::draw(c, x + 6, y + 3 + i as i32 * lh, l, col);
    }
}

fn panel(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    c.rect(x + 1, y, w - 2, h, PANEL);
    c.rect(x, y + 1, w, h - 2, PANEL);
    c.rect(x + 1, y, w - 2, 1, BORDER);
    c.rect(x + 1, y + h - 1, w - 2, 1, BORDER);
    c.rect(x, y + 1, 1, h - 2, BORDER);
    c.rect(x + w - 1, y + 1, 1, h - 2, BORDER);
    c.rect(x + 2, y + 1, w - 4, 1, hex(0x1d2a5a));
}

fn tokens(cap: &Caption) -> Vec<Token> {
    let mut out = Vec::new();
    for (i, seg) in cap.segments.iter().enumerate() {
        let active = cap.active == Some(i);
        let feedback = match (&cap.feedback, seg) {
            (Some(f), Segment::Target(t)) if t == &cap.say => Some(f),
            _ => None,
        };
        for (j, word) in seg.text().split_whitespace().enumerate() {
            let color = match (seg, feedback) {
                (_, Some(f)) => {
                    if f.get(j).is_some_and(|h| h.hit) {
                        HIT
                    } else {
                        MISS
                    }
                }
                (Segment::Target(_), None) => TARGET,
                (Segment::Native(_), None) if cap.active.is_some() && !active => DIM,
                (Segment::Native(_), None) => INK,
            };
            out.push(Token { text: word.to_string(), color, marked: active && seg.is_target() });
        }
    }
    out
}

/// Greedy layout of colored words into lines of at most `max_w` px.
fn layout(tokens: &[Token], max_w: i32) -> Vec<Vec<usize>> {
    let space = font::ADVANCE;
    let mut lines: Vec<Vec<usize>> = vec![vec![]];
    let mut w = 0;
    for (i, t) in tokens.iter().enumerate() {
        let tw = font::text_width(&t.text);
        let line = lines.last_mut().unwrap();
        if !line.is_empty() && w + space + tw > max_w {
            lines.push(vec![i]);
            w = tw;
        } else {
            w += if line.is_empty() { tw } else { space + tw };
            line.push(i);
        }
    }
    lines.retain(|l| !l.is_empty());
    lines
}

fn draw_caption(c: &mut Canvas, cap: &Caption, ground_y: i32, time: f32) {
    let pw = (c.w - 16).min(300);
    let inner = pw - 12;
    let toks = tokens(cap);
    let lines = layout(&toks, inner);
    let lh = font::LINE_H - 1;
    let mut rows = 1 + lines.len() as i32 + i32::from(cap.listen.is_some());
    let meaning_lines = if cap.meaning.is_empty() { vec![] } else { font::wrap(&format!("= {}", cap.meaning), inner) };
    rows += meaning_lines.len() as i32;
    let heard_lines = cap.heard.as_ref().map(|h| font::wrap(&format!("Ouvi: \"{h}\""), inner)).unwrap_or_default();
    rows += heard_lines.len() as i32;
    if !cap.footer.is_empty() {
        rows += 1;
    }
    let ph = rows * lh + 8;
    let px = (c.w - pw) / 2;
    // Upper part of the screen: clear of the characters and their speech bubbles.
    // Below the progress corner, above the characters and their bubbles.
    let py = (c.h / 8).max(2 * font::LINE_H + 8).min(ground_y - ph - 50).max(4);
    panel(c, px, py, pw, ph);

    // Header: icon + status.
    let mut y = py + 3;
    let icon_c = cap.status.color();
    match cap.status {
        Status::Speaking => c.sprite(SPEAKER, &[('#', icon_c)], px + 6, y + 4, false),
        Status::Listening | Status::Confirm => c.sprite(MIC, &[('#', icon_c)], px + 6, y + 3, false),
        Status::Passed => c.sprite(CHECK, &[('#', icon_c)], px + 6, y + 5, false),
        Status::Failed => c.sprite(CROSS, &[('#', icon_c)], px + 6, y + 4, false),
        Status::Thinking => {}
    }
    let mut label = cap.status.label().to_string();
    if matches!(cap.status, Status::Listening | Status::Thinking) {
        label.push_str(&".".repeat(1 + (time * 3.0) as usize % 3));
    }
    font::draw(c, px + 14, y, &label, icon_c);
    if !cap.tag.is_empty() {
        let tw = font::text_width(&cap.tag);
        if 14 + font::text_width(&label) + 12 + tw < pw {
            font::draw(c, px + pw - tw - 6, y, &cap.tag, hex(0x6e74b8));
        }
    }
    y += lh;
    if let Some(m) = &cap.listen {
        draw_meter(c, px + 6, y + 3, pw - 12, m, time);
        y += lh;
    }

    for line in &lines {
        let mut x = px + 6;
        for &i in line {
            let t = &toks[i];
            let tw = font::text_width(&t.text);
            if t.marked {
                c.rect(x - 1, y + 2, tw + 2, font::LINE_H - 3, hex(0x2a5a9a));
            }
            font::draw(c, x, y, &t.text, if t.marked { hex(0xffffff) } else { t.color });
            if t.color == TARGET || t.color == HIT || t.color == MISS {
                c.rect(x, y + font::ASCENT + 9, tw, 1, t.color);
            }
            x += tw + font::ADVANCE;
        }
        y += lh;
    }
    for l in &meaning_lines {
        font::draw(c, px + 6, y, l, DIM);
        y += lh;
    }
    for l in &heard_lines {
        font::draw(c, px + 6, y, l, hex(0xc9d3e8));
        y += lh;
    }
    if !cap.footer.is_empty() {
        font::draw(c, px + 6, y, &cap.footer, hex(0x6e74b8));
    }
}

/// 12-segment mic meter; then either "falando" or the thinking countdown bar.
fn draw_meter(c: &mut Canvas, x: i32, y: i32, w: i32, m: &Meter, time: f32) {
    const SEGS: i32 = 12;
    // Speech RMS lives around 0.02–0.3: a square-root curve makes quiet voices visible.
    let lit = ((m.level.sqrt() * 2.2).clamp(0.0, 1.0) * SEGS as f32).round() as i32;
    for i in 0..SEGS {
        let col = if i >= lit {
            hex(0x1d2a5a)
        } else if i < 7 {
            hex(0x7dff9b)
        } else if i < 10 {
            hex(0xffd64a)
        } else {
            hex(0xff6b6b)
        };
        c.rect(x + i * 4, y + 1, 3, 5, col);
    }
    let tx = x + SEGS * 4 + 6;
    if m.speaking {
        let dots = ".".repeat(1 + (time * 3.0) as usize % 3);
        font::draw(c, tx, y - 3, &format!("falando{dots}"), hex(0x7dff9b));
    } else {
        let bar_w = (w - (tx - x) - 30).max(10);
        let fill = if m.think_total > 0.0 { (bar_w as f32 * m.think_left / m.think_total).round() as i32 } else { 0 };
        c.rect(tx, y + 2, bar_w, 3, hex(0x1d2a5a));
        c.rect(tx, y + 2, fill, 3, hex(0x9be8ff));
        font::draw(c, tx + bar_w + 4, y - 3, &format!("{}s", m.think_left.ceil() as i32), hex(0x9be8ff));
    }
}

fn draw_summary(c: &mut Canvas, s: &SummaryPanel, time: f32) {
    let pw = (c.w - 16).min(320);
    let inner = pw - 22;
    let lh = font::LINE_H - 1;
    let mut rows: Vec<(Vec<String>, Rgba, Option<bool>, bool)> = Vec::new();
    for (i, l) in s.lines.iter().enumerate() {
        let active = s.active == Some(i);
        let text = format!("{} = {}", l.say, l.meaning);
        let color = if active {
            hex(0xffffff)
        } else if l.ok {
            INK
        } else {
            DIM
        };
        rows.push((font::wrap(&text, inner), color, Some(l.ok), active));
    }
    let max_rows = ((c.h - 40) / lh).max(3) as usize;
    let mut body_lines: usize = rows.iter().map(|r| r.0.len()).sum();
    while body_lines > max_rows && !rows.is_empty() {
        body_lines -= rows.remove(0).0.len(); // keep the most recent phrases
    }
    let ph = (body_lines as i32 + 3) * lh + 8;
    let (px, py) = ((c.w - pw) / 2, ((c.h - ph) / 2).max(4));
    panel(c, px, py, pw, ph);
    let mut y = py + 3;
    let title_w = font::text_width(&s.title);
    font::draw(c, px + (pw - title_w) / 2, y, &s.title, hex(0xffd64a));
    y += lh + 2;
    for (lines, color, ok, active) in rows {
        if active {
            let pulse = if (time * 4.0).sin() > 0.0 { hex(0x2a5a9a) } else { hex(0x1d2a5a) };
            c.rect(px + 3, y + 1, pw - 6, lines.len() as i32 * lh, pulse);
        }
        match ok {
            Some(true) => c.sprite(CHECK, &[('#', HIT)], px + 6, y + 5, false),
            Some(false) => c.sprite(CROSS, &[('#', MISS)], px + 6, y + 4, false),
            None => {}
        }
        for l in lines {
            font::draw(c, px + 16, y, &l, color);
            y += lh;
        }
    }
    font::draw(c, px + 6, py + ph - lh - 3, &s.footer, hex(0x9be8ff));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caption(feedback: Option<Vec<WordHit>>) -> Caption {
        Caption {
            segments: vec![
                Segment::Native("Para dizer que estou com fome, devo dizer:".into()),
                Segment::Target("I'm hungry".into()),
            ],
            say: "I'm hungry".into(),
            active: None,
            meaning: "Estou com fome".into(),
            status: Status::Listening,
            feedback,
            heard: None,
            footer: String::new(),
            tag: "social · A1 · repita".into(),
            listen: None,
        }
    }

    #[test]
    fn target_words_are_highlighted_and_native_words_are_not() {
        let t = tokens(&caption(None));
        let hungry = t.iter().find(|t| t.text == "hungry").unwrap();
        let fome = t.iter().find(|t| t.text == "fome,").unwrap();
        assert_eq!(hungry.color, TARGET);
        assert_eq!(fome.color, INK);
    }

    #[test]
    fn feedback_colors_each_target_word_by_hit_or_miss() {
        let fb = vec![WordHit { word: "I'm".into(), hit: true }, WordHit { word: "hungry".into(), hit: false }];
        let t = tokens(&caption(Some(fb)));
        assert_eq!(t.iter().find(|t| t.text == "I'm").unwrap().color, HIT);
        assert_eq!(t.iter().find(|t| t.text == "hungry").unwrap().color, MISS);
    }

    #[test]
    fn segment_being_read_is_marked_and_others_dimmed() {
        let mut cap = caption(None);
        cap.active = Some(1);
        let t = tokens(&cap);
        assert!(t.iter().filter(|t| t.marked).all(|t| t.text == "I'm" || t.text == "hungry"));
        assert_eq!(t.iter().find(|t| t.text == "Para").unwrap().color, DIM);
    }

    #[test]
    fn layout_wraps_but_never_drops_words() {
        let t = tokens(&caption(None));
        let lines = layout(&t, 80);
        assert!(lines.len() > 2);
        assert_eq!(lines.iter().map(Vec::len).sum::<usize>(), t.len());
    }

    #[test]
    fn cheat_sheet_shares_the_common_prefix() {
        let ch = Cheats::from_hotkeys((10.0, 10.0), &[("Ctrl+Alt+M", "praticar"), ("Ctrl+Alt+K", "painel")], "");
        assert_eq!(ch.prefix, "Ctrl+Alt +");
        assert_eq!(ch.rows, vec![("M".to_string(), "praticar".to_string()), ("K".to_string(), "painel".to_string())]);
        let mixed = Cheats::from_hotkeys((0.0, 0.0), &[("Ctrl+M", "a"), ("Alt+K", "b")], "");
        assert!(mixed.prefix.is_empty());
        assert_eq!(mixed.rows[1].0, "Alt+K");
    }

    #[test]
    fn cheat_sheet_opens_toward_the_screen_middle_and_stays_on_screen() {
        for anchor in [(310.0, 10.0), (10.0, 170.0)] {
            let hud = Hud {
                cheats: Some(Cheats::from_hotkeys(anchor, &[("Ctrl+Alt+M", "praticar")], "clique direito: pausar")),
                ..Default::default()
            };
            let mut c = Canvas::new(320, 180);
            hud.draw(&mut c, 180, 0.0);
            let total = c.opaque_in(0, 0, 320, 180);
            assert!(total > 0);
            let far = if anchor.0 > 160.0 { c.opaque_in(0, 0, 80, 180) } else { c.opaque_in(240, 0, 80, 180) };
            assert_eq!(far, 0, "opens from the orb's side, never across the whole screen");
        }
    }

    #[test]
    fn stats_corner_shows_goal_progress_and_combo() {
        let mut hud =
            Hud { stats: Some(Stats { done: 3, goal: 10, combo: 0, label: "EN".into() }), ..Default::default() };
        let mut plain = Canvas::new(200, 60);
        hud.draw(&mut plain, 60, 0.0);
        hud.stats.as_mut().unwrap().combo = 4;
        let mut combo = Canvas::new(200, 60);
        hud.draw(&mut combo, 60, 0.0);
        assert!(plain.opaque_in(100, 0, 100, 40) > 0);
        assert!(combo.opaque_in(0, 0, 200, 60) > plain.opaque_in(0, 0, 200, 60), "combo adds 'x4'");
    }

    #[test]
    fn caption_and_summary_draw_within_a_small_canvas() {
        let mut hud = Hud { caption: Some(caption(None)), ..Default::default() };
        hud.summary = Some(SummaryPanel {
            title: "RESUMO DE HOJE".into(),
            lines: (0..40)
                .map(|i| SummaryLine { say: format!("phrase {i}"), meaning: "x".into(), ok: i % 2 == 0 })
                .collect(),
            active: Some(39),
            footer: "40 frases".into(),
        });
        let mut c = Canvas::new(200, 120);
        hud.draw(&mut c, 120, 0.0);
        assert!(c.opaque_in(0, 0, 200, 120) > 500);
    }
}
