//! The fire mage — your champion. He only shows up while you practice:
//! teleports in, charges a fireball only while your voice is heard (#112), and
//! casts it at the frost mage when you get the phrase right (or fizzles when you
//! don't). The charge is only shown: it never changes how much snow melts.
//! A real flame rises from his staff while you speak, stays lit and pulses
//! while your answer is being understood, goes out in smoke on a miss and
//! feeds the fireball on a hit (#140).

use super::mage::{BODY, FEET, HEIGHT, WIDTH};
use crate::render::canvas::{Canvas, Rgba, bayer, hex};

const PAL: &[(char, Rgba)] = &[
    ('H', hex(0x8a1c1c)),
    ('h', hex(0xd93a2a)),
    ('S', hex(0xffd64a)),
    ('F', hex(0xf4c49a)),
    ('E', hex(0x141028)),
    ('B', hex(0x6b3f1d)),
    ('b', hex(0x4a2a14)),
    ('R', hex(0xb8341f)),
    ('r', hex(0x7a1f16)),
    ('T', hex(0xffc13d)),
    ('K', hex(0x3a2718)),
];

const ARRIVE: f32 = 0.6;
const CAST: f32 = 0.35;
const FIZZLE: f32 = 0.8;
const LEAVE: f32 = 0.7;

/// Mic level (RMS) below which the learner is taken as silent: a pause holds the charge.
const QUIET: f32 = 0.015;
/// Mic level of a normal speaking voice: the charge grows at full speed.
const VOICE_FULL: f32 = 0.1;
/// Charge per second at a full voice: about 2 s of speech fills it.
const CHARGE_RATE: f32 = 0.55;
/// Flame growth per second while speaking: half a second of a normal voice
/// already makes it more than half its height.
const FLAME_RISE: f32 = 1.6;
/// Flame lost per second once nobody listens (a listen cancelled).
const FLAME_FALL: f32 = 1.5;
/// The flame's height above the crystal, empty to full (px).
const FLAME_H: (f32, f32) = (3.0, 19.0);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Act {
    Hidden,
    Arrive,
    Ready,
    Cast,
    Fizzle,
    Leave,
}

/// What the fire mage casts for a right answer: the fuller the answer, the
/// bigger the spell. Same fireball art, scaled; one fireball per answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Spell {
    /// Short answer: a small fireball.
    Spark,
    /// Complete answer: the usual fireball.
    #[default]
    Fireball,
    /// Polished answer: a big fireball.
    Blaze,
}

impl Spell {
    /// Share of `Pace::melt_fraction` this spell melts.
    pub fn melt(self) -> f32 {
        match self {
            Spell::Spark => 0.6,
            Spell::Fireball => 1.0,
            Spell::Blaze => 1.5,
        }
    }

    /// Half-size of the fireball in art pixels; also scales the impact embers.
    pub fn radius(self) -> i32 {
        match self {
            Spell::Spark => 1,
            Spell::Fireball => 2,
            Spell::Blaze => 3,
        }
    }
}

pub enum Event {
    /// Fireball leaves the staff here.
    Release { x: f32, y: f32 },
}

pub struct Pyro {
    pub x: f32,
    /// +1 faces right.
    pub dir: f32,
    pub act: Act,
    t: f32,
    /// 0..1 fireball charge, grows while your voice is heard.
    pub charge: f32,
    /// 0..1 how loud the learner is speaking right now (0 when silent).
    voice: f32,
    /// 0..1 the staff's flame (#140): rises with the voice, holds while the
    /// answer is understood, out on a miss, spent on the fireball.
    pub flame: f32,
    /// The answer is being transcribed: the flame pulses and holds.
    thinking: bool,
    focused: bool,
    pending_cast: bool,
}

impl Default for Pyro {
    fn default() -> Self {
        Pyro {
            x: 0.0,
            dir: 1.0,
            act: Act::Hidden,
            t: 0.0,
            charge: 0.0,
            voice: 0.0,
            flame: 0.0,
            thinking: false,
            focused: false,
            pending_cast: false,
        }
    }
}

impl Pyro {
    fn set(&mut self, act: Act) {
        self.act = act;
        self.t = 0.0;
    }

    pub fn visible(&self) -> bool {
        self.act != Act::Hidden
    }

    /// Teleports in at `x`, facing `toward_x`.
    pub fn arrive(&mut self, x: f32, toward_x: f32) {
        if matches!(self.act, Act::Hidden | Act::Leave) {
            self.x = x;
            self.dir = if toward_x >= x { 1.0 } else { -1.0 };
            self.charge = 0.0;
            self.set(Act::Arrive);
        }
    }

    pub fn leave(&mut self) {
        if !matches!(self.act, Act::Hidden | Act::Leave) {
            // Let a fireball in progress finish first.
            if self.act == Act::Cast {
                return;
            }
            self.focused = false;
            self.set(Act::Leave);
        }
    }

    /// The mic is open for an answer. The charge itself waits for the voice.
    pub fn focus(&mut self, on: bool) {
        self.focused = on;
        if !on {
            self.voice = 0.0;
            self.thinking = false;
        }
    }

    /// The answer is being transcribed: the flame stays lit and pulses.
    pub fn think(&mut self, on: bool) {
        self.thinking = on && self.focused;
    }

    /// The learner's voice right now: `level` is the mic RMS, `speaking` that
    /// speech has started. Only heard while the mic is open for an answer.
    pub fn voice(&mut self, level: f32, speaking: bool) {
        self.voice =
            if self.focused && speaking { ((level - QUIET) / (VOICE_FULL - QUIET)).clamp(0.0, 1.0) } else { 0.0 };
    }

    pub fn cast(&mut self) {
        match self.act {
            Act::Ready => self.set(Act::Cast),
            Act::Arrive => self.pending_cast = true,
            _ => {}
        }
    }

    pub fn fizzle(&mut self) {
        self.flame = 0.0;
        if self.act == Act::Ready {
            self.charge = 0.0;
            self.set(Act::Fizzle);
        }
    }

    fn staff_top(&self, feet_y: f32) -> (f32, f32) {
        let x = if self.dir >= 0.0 { self.x + (WIDTH - 2) as f32 } else { self.x + 1.0 };
        (x, feet_y - HEIGHT as f32 - 6.0)
    }

    pub fn step(&mut self, dt: f32, feet_y: f32) -> Option<Event> {
        self.t += dt;
        if self.focused {
            self.charge = (self.charge + dt * CHARGE_RATE * self.voice).min(1.0);
            if self.voice > 0.0 {
                self.flame = (self.flame + dt * FLAME_RISE * (0.5 + self.voice)).min(1.0);
            }
        } else if self.act != Act::Cast {
            self.flame = (self.flame - dt * FLAME_FALL).max(0.0);
        }
        match self.act {
            Act::Arrive if self.t > ARRIVE => {
                self.set(Act::Ready);
                if std::mem::take(&mut self.pending_cast) {
                    self.set(Act::Cast);
                }
                None
            }
            Act::Cast if self.t > CAST => {
                let (x, y) = self.staff_top(feet_y);
                self.charge = 0.0;
                self.flame = 0.0; // it became the fireball
                self.set(Act::Ready);
                Some(Event::Release { x, y })
            }
            Act::Fizzle if self.t > FIZZLE => {
                self.set(Act::Ready);
                None
            }
            Act::Leave if self.t > LEAVE => {
                self.set(Act::Hidden);
                None
            }
            _ => None,
        }
    }

    /// 0..1 how "materialized" he is (teleport effects).
    fn presence(&self) -> f32 {
        match self.act {
            Act::Hidden => 0.0,
            Act::Arrive => self.t / ARRIVE,
            Act::Leave => 1.0 - self.t / LEAVE,
            _ => 1.0,
        }
    }

    /// The flame above the crystal at (`sx`, `sy`): a flickering teardrop in
    /// dithered layers, white at the core, with sparks rising off a tall one.
    fn draw_flame(&self, c: &mut Canvas, sx: f32, sy: f32, time: f32) {
        let pulse = if self.thinking { (time * 8.0).sin() * 1.5 } else { 0.0 };
        let h = FLAME_H.0 + (FLAME_H.1 - FLAME_H.0) * self.flame + pulse;
        let w = 1.5 + self.flame * 3.5;
        let base = sy + 2.0;
        let layers = [hex(0xd93a2a), hex(0xff7a2a), hex(0xffc13d), hex(0xfff4b0), hex(0xffffff)];
        for j in 0..h.max(1.0) as i32 {
            let up = j as f32 / h.max(1.0);
            // Widest a third of the way up, a point at the top, swaying more up high.
            let half = w * (1.0 - up).powf(0.8) * (up * 3.0).clamp(0.45, 1.0);
            let sway = (time * 11.0 + j as f32 * 0.45).sin() * up * 1.6;
            let y = (base - j as f32).round() as i32;
            let r = half.ceil() as i32;
            for dx in -r..=r {
                let d = (dx as f32 - sway).abs() / half.max(0.5);
                if d > 1.0 || (d > 0.7 && bayer(sx as i32 + dx, y) > 0.5) {
                    continue;
                }
                let heat = ((1.0 - d) * 0.7 + (1.0 - up) * 0.5).min(0.999);
                c.set(sx as i32 + dx, y, layers[(heat * layers.len() as f32) as usize]);
            }
        }
        for k in 0..(self.flame * 5.0) as i32 {
            let rise = (time * 22.0 + k as f32 * 7.3) % 14.0;
            let x = sx + ((time * 3.0 + k as f32 * 2.1).sin() * 3.0);
            let spark = hex(if k % 2 == 0 { 0xfff4b0 } else { 0xffc13d });
            c.set(x.round() as i32, (base - h - rise).round() as i32, spark);
        }
    }

    pub fn draw(&self, c: &mut Canvas, feet_y: f32, time: f32) {
        if !self.visible() {
            return;
        }
        let presence = self.presence().clamp(0.0, 1.0);
        let flip = self.dir < 0.0;
        let (ox, oy) = (self.x.round() as i32, (feet_y - HEIGHT as f32).round() as i32);

        // Pillar of fire while teleporting.
        if matches!(self.act, Act::Arrive | Act::Leave) {
            let cx = ox + WIDTH / 2;
            for y in (oy - 12)..(feet_y as i32) {
                for dx in -4..=4 {
                    let k = 1.0 - (dx as f32).abs() / 5.0;
                    let flicker = ((time * 20.0 + y as f32 * 0.7).sin() + 1.0) * 0.25;
                    if (1.0 - presence) * k + flicker * 0.3 > bayer(cx + dx, y) + 0.2 {
                        let col = if dx.abs() < 2 { hex(0xfff4b0) } else { hex(0xff7a2a) };
                        c.set(cx + dx, y, col);
                    }
                }
            }
        }

        // Staff with a flame crystal; its glow and its flame grow with your
        // voice and flare as the fireball leaves.
        let (sx, sy) = self.staff_top(feet_y);
        let cast = if self.act == Act::Cast { 0.4 } else { 0.0 };
        let glow = 0.3 + self.flame * 0.5 + self.voice * 0.2 + cast;
        c.glow(sx, sy - self.flame * 6.0, 3.0 + self.flame * 9.0 + self.voice * 2.0, glow * presence, hex(0xff7a2a));
        if presence > 0.5 {
            for y in (sy as i32 + 3)..(feet_y as i32) {
                c.set(sx as i32, y, if y % 5 == 0 { hex(0x8b5a2b) } else { hex(0x5a3418) });
            }
            self.draw_flame(c, sx, sy, time);
        }

        // Body, dissolving in and out through the dither pattern.
        let mut body = Canvas::new(WIDTH, HEIGHT);
        body.sprite(BODY, PAL, 0, 0, flip);
        body.sprite(&[FEET[0]], PAL, 0, HEIGHT - 1, flip);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if let Some(p) = body.get(x, y).filter(|p| p[3] != 0)
                    && presence > bayer(ox + x, oy + y)
                {
                    c.set(ox + x, oy + y, p);
                }
            }
        }
        if self.act == Act::Fizzle {
            // The flame went out: a puff of smoke rising off the staff.
            for i in 0..10 {
                let y = sy - self.t * 14.0 - i as f32 * 2.0;
                let spread = 1.0 + i as f32 * 0.4 + self.t * 3.0;
                c.dot(sx + ((time * 6.0 + i as f32 * 1.7).sin() * spread), y, hex(0x8f96a8));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(p: &mut Pyro, seconds: f32) -> usize {
        let mut released = 0;
        for _ in 0..(seconds * 60.0) as i32 {
            if let Some(Event::Release { .. }) = p.step(1.0 / 60.0, 100.0) {
                released += 1;
            }
        }
        released
    }

    #[test]
    fn appears_only_for_a_lesson_and_leaves_afterwards() {
        let mut p = Pyro::default();
        assert!(!p.visible());
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        assert_eq!(p.act, Act::Ready);
        assert_eq!(p.dir, 1.0, "faces the frost mage");
        p.leave();
        run(&mut p, 1.0);
        assert!(!p.visible());
    }

    #[test]
    fn a_cast_right_after_arriving_still_fires_once() {
        let mut p = Pyro::default();
        p.arrive(20.0, 0.0);
        p.cast();
        assert_eq!(run(&mut p, 2.0), 1);
    }

    #[test]
    fn leaving_mid_cast_waits_for_the_fireball() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.cast();
        p.leave();
        assert_eq!(run(&mut p, 0.5), 1);
    }

    #[test]
    fn speaking_charges_the_fireball_and_a_miss_fizzles_it() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.focus(true);
        p.voice(0.12, true);
        run(&mut p, 2.0);
        assert!(p.charge > 0.5);
        p.fizzle();
        assert_eq!(p.charge, 0.0);
        assert_eq!(run(&mut p, 1.0), 0);
    }

    #[test]
    fn an_open_mic_in_silence_charges_nothing() {
        // #112: the power answers the voice, not the open mic.
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.focus(true);
        run(&mut p, 2.0);
        assert_eq!(p.charge, 0.0, "nothing said yet");
        p.voice(0.004, false);
        run(&mut p, 2.0);
        assert_eq!(p.charge, 0.0, "room noise before speaking");
    }

    #[test]
    fn the_charge_grows_with_the_voice_and_holds_in_a_pause() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.focus(true);
        p.voice(0.05, true);
        run(&mut p, 1.0);
        let soft = p.charge;
        p.voice(0.12, true);
        run(&mut p, 1.0);
        assert!(soft > 0.0 && p.charge - soft > soft, "a fuller voice charges faster: {soft} then {}", p.charge);
        let before = p.charge;
        p.voice(0.01, true); // a pause: speech started, nothing heard now
        run(&mut p, 2.0);
        assert_eq!(p.charge, before, "a pause holds the charge");
    }

    fn ready() -> Pyro {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.focus(true);
        p
    }

    #[test]
    fn the_flame_rises_fast_with_the_voice_and_holds_while_thinking() {
        let mut p = ready();
        run(&mut p, 2.0);
        assert_eq!(p.flame, 0.0, "silence lights nothing");
        p.voice(0.05, true);
        run(&mut p, 0.5);
        assert!(p.flame > 0.5, "half a second of a normal voice: {}", p.flame);
        p.voice(0.0, true);
        p.think(true);
        let lit = p.flame;
        run(&mut p, 3.0);
        assert_eq!(p.flame, lit, "holds while the answer is understood");
    }

    #[test]
    fn a_miss_puts_the_flame_out_and_a_hit_spends_it_on_the_fireball() {
        let mut p = ready();
        p.voice(0.12, true);
        run(&mut p, 1.0);
        p.focus(false);
        p.fizzle();
        assert_eq!(p.flame, 0.0, "out on a miss");
        run(&mut p, 1.0);
        let mut p = ready();
        p.voice(0.12, true);
        run(&mut p, 1.0);
        p.focus(false);
        p.cast();
        assert!(p.flame > 0.5, "still burning as he casts");
        assert_eq!(run(&mut p, 1.0), 1);
        assert_eq!(p.flame, 0.0, "it became the fireball");
    }

    #[test]
    fn a_cancelled_listen_lets_the_flame_die_down() {
        let mut p = ready();
        p.voice(0.12, true);
        run(&mut p, 1.0);
        p.focus(false);
        run(&mut p, 1.0);
        assert_eq!(p.flame, 0.0);
    }

    #[test]
    fn the_flame_is_drawn_tall_and_in_one_bit() {
        let shot = |flame: f32| {
            let mut p = ready();
            p.flame = flame;
            let mut c = Canvas::new(80, 80);
            p.draw(&mut c, 70.0, 0.3);
            let lit = (0..80 * 80).filter(|i| c.bytes()[i * 4 + 3] == 255).count();
            assert!(c.bytes().chunks(4).all(|px| px[3] == 0 || px[3] == 255), "1-bit alpha");
            lit
        };
        assert!(shot(1.0) > shot(0.0) + 40, "a full flame is much bigger");
    }

    #[test]
    fn the_voice_is_ignored_once_the_mic_closes() {
        let mut p = Pyro::default();
        p.arrive(20.0, 200.0);
        run(&mut p, 1.0);
        p.voice(0.12, true);
        run(&mut p, 1.0);
        assert_eq!(p.charge, 0.0, "not listening yet");
        p.focus(true);
        p.voice(0.12, true);
        p.focus(false);
        run(&mut p, 1.0);
        assert_eq!(p.charge, 0.0, "closing the mic forgets the voice");
    }
}
