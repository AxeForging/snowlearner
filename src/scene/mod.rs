//! The whole winter world as a pure, seeded simulation: `step(dt)` advances it,
//! `draw(canvas)` renders it. No window, audio or clock inside, so it runs the
//! same in the app, in `snowlearner snapshot` and in tests.

pub mod backdrop;
pub mod fire;
pub mod friends;
pub mod frost;
pub mod hand;
pub mod hud;
pub mod ice;
pub mod mage;
pub mod mobs;
pub mod pyro;
pub mod rng;
pub mod snow;
pub mod vortex;
pub mod warrior;

use crate::config::level::Pace;
use crate::render::canvas::{CLEAR, Canvas, hex};
use backdrop::{Backdrop, GROUND_BAND};
use fire::Fire;
use friends::Friend;
use frost::{Edge, Frost};
use hud::Hud;
use ice::{Cube, Icicle, Kind, Particle};
use mage::Mage;
use mobs::Mob;
use pyro::Pyro;
use rng::Rng;
use snow::Snow;
use warrior::Warrior;

/// Radius of the in-scene orb (window mode), art pixels.
pub const ORB_R: f32 = 6.0;

const ICE_COLORS: [u32; 4] = [0xffffff, 0xc8f4ff, 0x8ad8f5, 0x4ea2d8];
const GRAVITY: f32 = 130.0;

/// Seconds the sun shines after a combo.
pub const SUN_SECONDS: f32 = 8.0;
/// Extra share of snow the sun melts on arrival (never everything).
pub const SUN_MELT: f32 = 0.35;

/// The fire mage's fireball, carrying how much it will melt on impact.
struct Fireball {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    flight: f32,
    power: f32,
}

/// What the frost mage's summon ritual is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    Friend,
    IcicleRain,
}

/// What a click in window mode touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Poke {
    /// The magic orb (window mode): click = practice, Ctrl+click = panel.
    Orb,
    Mage,
    Warrior,
    Fire,
    Friend,
    Snow,
    Nothing,
}

struct Flake {
    x: f32,
    y: f32,
    speed: f32,
    phase: f32,
    /// In the air; the rest of the pool waits until the screen freezes more.
    falling: bool,
}

/// Share of the flake pool falling on a clear screen: a few flakes, just
/// enough to show it has started to snow.
const SNOWFALL_MIN: f32 = 0.02;
/// Never fewer flakes than this on a clear screen (small windows).
const SNOWFALL_FLOOR: usize = 6;
/// Spread of the heap a shattered cube leaves (`Snow::add`).
const CUBE_SPREAD: f32 = 9.0;

/// Flakes falling out of a pool of `pool` at freeze `level` (0 clear, 1
/// buried): a few on a clear screen, growing slowly at first (quadratic)
/// and reaching the whole pool once buried.
fn snowfall(pool: usize, level: f32) -> usize {
    let l = level.clamp(0.0, 1.0);
    let share = SNOWFALL_MIN + (1.0 - SNOWFALL_MIN) * l * l;
    ((pool as f32 * share).round() as usize).max(SNOWFALL_FLOOR).min(pool)
}

pub struct Scene {
    pub w: i32,
    pub h: i32,
    /// Overlay mode: no backdrop, the desktop shows through.
    pub transparent: bool,
    pub time: f32,
    pace: Pace,
    rng: Rng,
    pub mage: Mage,
    pub warrior: Warrior,
    pub snow: Snow,
    pub frost: Frost,
    pub fires: Vec<Fire>,
    pub hud: Hud,
    /// Warrior tips (pt-BR), provided by the app (hotkey hints, phrase tips).
    pub tips: Vec<String>,
    cubes: Vec<Cube>,
    particles: Vec<Particle>,
    friends: Vec<Friend>,
    flakes: Vec<Flake>,
    backdrop: Option<Backdrop>,
    /// The fire mage, present only during lessons.
    pub pyro: Pyro,
    fireballs: Vec<Fireball>,
    pending_power: Vec<f32>,
    icicles: Vec<Icicle>,
    mobs: Vec<Mob>,
    next_mobs: f32,
    skill: Skill,
    next_icicles: f32,
    /// Seconds of sunshine left.
    sun_t: f32,
    /// The frost mage can't throw while stunned (after the sun).
    stun_t: f32,
    pokes: u32,
    /// Magic hand position while hand mode is on (Ctrl+Alt / `snowlearner grab`).
    pub hand: Option<(f32, f32)>,
    grabbed: Option<(hand::Who, f32, f32)>,
    lift_mage: Option<hand::Lift>,
    lift_warrior: Option<hand::Lift>,
    paused: bool,
    /// Pause black-hole animation state.
    vortex: vortex::Phase,
    /// Where the black hole (the orb) is, in art coordinates.
    pub hole: (f32, f32),
    /// Draw the orb inside the scene (window mode; overlay has its own orb window).
    pub show_orb: bool,
    practicing: bool,
    throws: u32,
    next_throw: f32,
    next_summon: f32,
    next_tip: f32,
}

impl Scene {
    pub fn new(w: i32, h: i32, seed: u64, pace: Pace, transparent: bool) -> Self {
        let mut rng = Rng::new(seed);
        let (w, h) = (w.max(80), h.max(60));
        let pool = (w * h / 500).max(12) as usize;
        let light = snowfall(pool, 0.0);
        let flakes: Vec<Flake> = (0..pool)
            .map(|i| Flake {
                x: rng.range(0.0, w as f32),
                y: rng.range(0.0, h as f32 - 1.0),
                speed: rng.range(5.0, 16.0),
                phase: rng.range(0.0, 7.0),
                falling: i < light,
            })
            .collect();
        let mut s = Scene {
            w,
            h,
            transparent,
            time: 0.0,
            pace,
            mage: Mage::new(w as f32 * 0.15),
            warrior: Warrior::new(w as f32 * 0.7),
            snow: Snow::new(w, snow_cap(h)),
            frost: Frost::new(w, h),
            fires: Vec::new(),
            hud: Hud::default(),
            tips: Vec::new(),
            cubes: Vec::new(),
            particles: Vec::new(),
            friends: Vec::new(),
            flakes,
            backdrop: None,
            pyro: Pyro::default(),
            fireballs: Vec::new(),
            pending_power: Vec::new(),
            icicles: Vec::new(),
            mobs: Vec::new(),
            next_mobs: 40.0_f32.min(pace.mobs_every),
            skill: Skill::Friend,
            next_icicles: pace.icicles_every * 0.4,
            sun_t: 0.0,
            stun_t: 0.0,
            pokes: 0,
            hand: None,
            grabbed: None,
            lift_mage: None,
            lift_warrior: None,
            paused: false,
            vortex: vortex::Phase::Open,
            hole: (w as f32 - 14.0, 14.0),
            show_orb: false,
            practicing: false,
            throws: 0,
            next_throw: 1.5,
            next_summon: 25.0_f32.min(pace.summon_every),
            next_tip: 12.0,
            rng,
        };
        if !transparent {
            s.backdrop = Some(Backdrop::new(w, h));
        }
        s
    }

    pub fn resize(&mut self, w: i32, h: i32) {
        let (w, h) = (w.max(80), h.max(60));
        if (w, h) == (self.w, self.h) {
            return;
        }
        self.w = w;
        self.h = h;
        self.snow.resize(w, snow_cap(h));
        self.frost.resize(w, h);
        if !self.transparent {
            self.backdrop = Some(Backdrop::new(w, h));
        }
        self.mage.x = self.mage.x.min(w as f32 - mage::WIDTH as f32);
        self.warrior.x = self.warrior.x.min(w as f32 - warrior::WIDTH as f32);
    }

    pub fn set_pace(&mut self, pace: Pace) {
        self.pace = pace;
    }

    pub fn ground_y(&self) -> f32 {
        if self.transparent { self.h as f32 } else { (self.h - GROUND_BAND) as f32 }
    }

    /// Surface height (y) under column `x`, snow included.
    pub fn feet_y(&self, x: f32) -> f32 {
        self.ground_y() - self.snow.height_at(x)
    }

    /// 0 = clear, 1 = buried: drives the warrior's chill and corner frost.
    pub fn freeze_level(&self) -> f32 {
        (self.snow.fill() * 0.6 + self.frost.coverage() * 0.4 / 0.5).min(1.0)
    }

    pub fn cubes_in_flight(&self) -> usize {
        self.cubes.len()
    }

    // ---- events from the lesson controller ----

    /// A lesson is on: the frost mage stops to watch, the fire mage teleports
    /// in on the other side, the warrior cheers you on.
    pub fn set_practicing(&mut self, on: bool) {
        if on && !self.practicing {
            self.warrior.say("Vai lá, você consegue!", 3.0);
            let mx = self.mage.x + mage::WIDTH as f32 / 2.0;
            let w = self.w as f32;
            let x = if mx < w / 2.0 { w * 0.72 } else { w * 0.18 };
            self.pyro.arrive(x.min(w - mage::WIDTH as f32 - 2.0), mx);
        }
        if !on {
            self.pyro.leave();
            self.pyro.focus(false);
        }
        self.practicing = on;
        self.mage.watch(on || self.paused);
    }

    /// Pause (meetings, focus time): the frost mage stops casting.
    /// Pause (meetings, focus time): everything trembles and is sucked into the
    /// orb's black hole; resuming spits it all back out, exactly as it was.
    pub fn set_paused(&mut self, on: bool) {
        if on == self.paused {
            return;
        }
        self.paused = on;
        self.mage.watch(on || self.practicing);
        self.vortex = if on { vortex::Phase::Closing(0.0) } else { vortex::Phase::Opening(0.0) };
        if on {
            self.mage.say("Nããão! O buraco negro!", 1.5);
        }
    }

    /// Hand mode on/off (showing the magic hand). Turning it off drops whoever is held.
    pub fn set_hand(&mut self, on: bool) {
        if !on {
            self.release();
            self.hand = None;
        }
    }

    /// Moves the hand; a held character follows it.
    pub fn hand_move(&mut self, x: f32, y: f32) {
        self.hand = Some((x, y));
        let Some((who, ox, oy)) = self.grabbed else { return };
        let (w, ground) = (self.w as f32, self.ground_y());
        match who {
            hand::Who::Mage => {
                self.mage.x = (x - ox).clamp(0.0, w - mage::WIDTH as f32);
                if let Some(l) = &mut self.lift_mage {
                    l.y = (y + oy).min(ground);
                }
            }
            hand::Who::Warrior => {
                self.warrior.x = (x - ox).clamp(0.0, w - warrior::WIDTH as f32);
                if let Some(l) = &mut self.lift_warrior {
                    l.y = (y + oy).min(ground);
                }
            }
        }
    }

    /// Tries to pick someone up at (x, y). Frozen warriors are too heavy.
    pub fn grab_at(&mut self, x: f32, y: f32) -> Option<hand::Who> {
        self.hand = Some((x, y));
        let inside = |ox: f32, feet: f32, w: i32, h: i32| {
            x >= ox - 2.0 && x <= ox + w as f32 + 2.0 && y >= feet - h as f32 - 3.0 && y <= feet + 2.0
        };
        let mfeet = self.lift_mage.map(|l| l.y).unwrap_or_else(|| self.feet_y(self.mage.x + 8.0));
        let wfeet = self.lift_warrior.map(|l| l.y).unwrap_or_else(|| self.feet_y(self.warrior.x + 6.0));
        if inside(self.mage.x, mfeet, mage::WIDTH, mage::HEIGHT) {
            self.grabbed = Some((hand::Who::Mage, x - self.mage.x, mfeet - y));
            self.lift_mage = Some(hand::Lift::new(mfeet));
            let line = *self.rng.pick(hand::MAGE_HELD);
            self.mage.say(line, 2.2);
            return Some(hand::Who::Mage);
        }
        if inside(self.warrior.x, wfeet, warrior::WIDTH, warrior::HEIGHT) && self.warrior.act != warrior::Act::Frozen {
            self.grabbed = Some((hand::Who::Warrior, x - self.warrior.x, wfeet - y));
            self.lift_warrior = Some(hand::Lift::new(wfeet));
            let line = *self.rng.pick(hand::WARRIOR_HELD);
            self.warrior.say(line, 2.2);
            return Some(hand::Who::Warrior);
        }
        None
    }

    /// Lets go: whoever was held falls into the snow.
    pub fn release(&mut self) {
        if let Some((who, _, _)) = self.grabbed.take() {
            let lift = match who {
                hand::Who::Mage => &mut self.lift_mage,
                hand::Who::Warrior => &mut self.lift_warrior,
            };
            if let Some(l) = lift {
                l.held = false;
            }
        }
    }

    pub fn holding(&self) -> Option<hand::Who> {
        self.grabbed.map(|(w, _, _)| w)
    }

    /// Held characters complain now and then; released ones fall and land with a puff.
    fn step_lifts(&mut self, dt: f32) {
        let mx = self.mage.x + mage::WIDTH as f32 / 2.0;
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let (mground, wground) = (self.feet_y(mx), self.feet_y(wx));
        let mut says = Vec::new();
        for (who, lift, ground, x) in [
            (hand::Who::Mage, &mut self.lift_mage, mground, mx),
            (hand::Who::Warrior, &mut self.lift_warrior, wground, wx),
        ] {
            let Some(l) = lift else { continue };
            if l.held {
                l.talk_in -= dt;
                if l.talk_in <= 0.0 {
                    l.talk_in = 3.0;
                    says.push((who, true, x));
                }
            } else if l.fall(dt, ground) {
                *lift = None;
                says.push((who, false, x));
            }
        }
        for (who, held, x) in says {
            let line = match (who, held) {
                (hand::Who::Mage, true) => *self.rng.pick(hand::MAGE_HELD),
                (hand::Who::Mage, false) => *self.rng.pick(hand::MAGE_LANDED),
                (hand::Who::Warrior, true) => *self.rng.pick(hand::WARRIOR_HELD),
                (hand::Who::Warrior, false) => *self.rng.pick(hand::WARRIOR_LANDED),
            };
            match who {
                hand::Who::Mage => self.mage.say(line, 2.2),
                hand::Who::Warrior => self.warrior.say(line, 2.2),
            }
            if !held {
                self.burst_snow(x, 12);
            }
        }
    }

    /// Fully swallowed (nothing of the world on screen).
    pub fn swallowed(&self) -> bool {
        self.vortex == vortex::Phase::Closed
    }

    /// In-scene orb position when `show_orb` (window mode).
    pub fn orb_at(&self, x: f32, y: f32) -> bool {
        self.show_orb && ((x - self.hole.0).powi(2) + (y - self.hole.1).powi(2)).sqrt() <= ORB_R + 2.0
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    /// The learner is speaking: the fire mage charges his fireball.
    pub fn set_listening(&mut self, on: bool) {
        self.pyro.focus(on);
    }

    /// A correct phrase: the fire mage casts. The melt (`pace.melt_fraction`
    /// × `power`) happens when the fireball hits the frost mage.
    pub fn celebrate(&mut self, power: f32) {
        self.pyro.focus(false);
        let melt = self.pace.melt_fraction * power;
        if self.pyro.visible() {
            self.pending_power.push(melt);
            self.pyro.cast();
        } else {
            let (x, y) = (self.mage.x + mage::WIDTH as f32 / 2.0, self.feet_y(self.mage.x) - 12.0);
            self.impact(x, y, melt);
        }
    }

    /// A wrong answer: the fireball fizzles and the frost mage fires back.
    pub fn miss(&mut self) {
        self.pyro.focus(false);
        self.pyro.fizzle();
        self.mage.start_throw(false);
    }

    fn impact(&mut self, x: f32, y: f32, melt: f32) {
        let gy = self.ground_y();
        for (px, hgt) in self.snow.melt(melt) {
            let py = gy - hgt;
            for _ in 0..2 {
                let (vx, vy, life) =
                    (self.rng.range(-8.0, 8.0), self.rng.range(-35.0, -15.0), self.rng.range(0.6, 1.4));
                self.particles.push(Particle::new(Kind::Steam, px, py, vx, vy, life, hex(0xc9d3e8)));
            }
        }
        self.frost.melt(melt);
        self.snow.thaw(x, 24.0, 40.0, 0.25);
        for i in 0..40 {
            let a = self.rng.range(0.0, std::f32::consts::TAU);
            let sp = self.rng.range(20.0, 90.0);
            let col = hex([0xfff4b0, 0xffc13d, 0xff7a2a, 0xd93a2a][i % 4]);
            let life = self.rng.range(0.4, 1.0);
            self.particles.push(Particle::new(Kind::Ember, x, y, a.cos() * sp, a.sin() * sp, life, col));
        }
        let mx = self.mage.x + mage::WIDTH as f32 / 2.0;
        if (mx - x).abs() < 30.0 {
            self.mage.stagger();
        }
        self.warrior.warm_burst();
        self.warrior.say("Que calor bom! Valeu!", 3.0);
    }

    /// Combo reward: the sun shines for a while, melting a big chunk (never
    /// all of it) and stunning the frost mage.
    pub fn sun(&mut self) {
        self.sun_t = SUN_SECONDS;
        self.stun_t = SUN_SECONDS * 2.5;
        self.snow.melt(SUN_MELT);
        self.frost.melt(SUN_MELT * 1.5);
        self.mage.say("Aaah! O sol!!", 3.0);
        self.warrior.say("Que solzão!", 3.0);
    }

    pub fn sun_active(&self) -> bool {
        self.sun_t > 0.0
    }

    pub fn mage_say(&mut self, text: impl Into<String>, seconds: f32) {
        self.mage.say(text, seconds);
    }

    pub fn mage_bubble(&self) -> Option<&str> {
        self.mage.bubble.as_ref().map(|b| b.text.as_str())
    }

    /// Window-mode click at art coordinates. Pure fun: nothing here melts snow
    /// for free — that stays the job of speaking.
    pub fn poke(&mut self, x: f32, y: f32) -> Poke {
        let hit = |ox: f32, oy_feet: f32, w: i32, h: i32| {
            x >= ox - 2.0 && x <= ox + w as f32 + 2.0 && y >= oy_feet - h as f32 - 2.0 && y <= oy_feet + 2.0
        };
        if self.orb_at(x, y) {
            return Poke::Orb;
        }
        let mx = self.mage.x;
        if hit(mx, self.feet_y(mx + 8.0), mage::WIDTH, mage::HEIGHT + 6) {
            self.pokes += 1;
            let lines = ["Ei! Não me cutuque!", "Mais neve pra você!", "Fale uma frase, se tiver coragem!"];
            let line = lines[self.pokes as usize % lines.len()];
            self.mage.say(line, 2.0);
            self.mage.start_throw(self.pokes % 3 == 0);
            return Poke::Mage;
        }
        let wx = self.warrior.x;
        if hit(wx, self.feet_y(wx + 6.0), warrior::WIDTH, warrior::HEIGHT + 6) {
            if self.warrior.act != warrior::Act::Frozen {
                let tip = if self.tips.is_empty() {
                    "Oi! Aperte o atalho e fale comigo!".to_string()
                } else {
                    self.rng.pick(&self.tips).clone()
                };
                self.warrior.say(tip, 6.0);
            }
            return Poke::Warrior;
        }
        let ground = self.ground_y();
        if let Some(fire) = self.fires.iter_mut().find(|f| (f.x - x).abs() < 8.0 && (ground - y) < 40.0) {
            fire.life = (fire.life + 5.0).min(fire::LIFETIME);
            let fx = fire.x;
            for _ in 0..12 {
                let (vx, vy) = (self.rng.range(-20.0, 20.0), self.rng.range(-60.0, -20.0));
                self.particles.push(Particle::new(Kind::Ember, fx, y, vx, vy, 0.8, hex(0xffc13d)));
            }
            return Poke::Fire;
        }
        let friend_hit = self.friends.iter().position(|f| {
            let fy = self.ground_y() - self.snow.height_at(f.x + f.kind.width() as f32 / 2.0);
            x >= f.x && x <= f.x + f.kind.width() as f32 && y >= fy - f.kind.height() as f32 && y <= fy
        });
        if let Some(i) = friend_hit {
            let f = self.friends.remove(i);
            self.burst_snow(f.x + f.kind.width() as f32 / 2.0, 20);
            return Poke::Friend;
        }
        if y >= self.feet_y(x) - 1.0 {
            for _ in 0..8 {
                let (vx, vy) = (self.rng.range(-30.0, 30.0), self.rng.range(-50.0, -15.0));
                self.particles.push(Particle::new(Kind::Shard, x, y, vx, vy, 0.8, hex(0xffffff)));
            }
            return Poke::Snow;
        }
        Poke::Nothing
    }

    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        self.hud.step(dt);
        self.vortex = self.vortex.step(dt);
        if self.vortex != vortex::Phase::Open {
            return; // the world is frozen inside the black hole
        }
        let (w, h) = (self.w as f32, self.h as f32);

        self.sun_t = (self.sun_t - dt).max(0.0);
        self.stun_t = (self.stun_t - dt).max(0.0);
        if self.sun_t > 0.0 {
            self.snow.melt(0.04 * dt);
        }
        if !self.practicing && !self.paused && self.stun_t <= 0.0 {
            self.next_throw -= dt;
            self.next_summon -= dt;
            self.next_icicles -= dt;
            self.next_mobs -= dt;
            if self.next_mobs <= 0.0 {
                self.spawn_mobs();
                self.next_mobs = self.pace.mobs_every * self.rng.range(0.8, 1.2);
            }
            if self.next_icicles <= 0.0 && !self.mage.busy() {
                self.cast_skill(Skill::IcicleRain);
                self.next_icicles = self.pace.icicles_every * self.rng.range(0.8, 1.2);
            } else if self.next_summon <= 0.0 && !self.mage.busy() {
                self.cast_skill(Skill::Friend);
                self.next_summon = self.pace.summon_every * self.rng.range(0.8, 1.2);
            } else if self.next_throw <= 0.0 && !self.mage.busy() {
                self.throws += 1;
                self.mage.start_throw(self.throws % 4 == 0);
                self.next_throw = self.pace.throw_every * self.rng.range(0.8, 1.2);
            }
        }

        self.step_lifts(dt);

        // Mage.
        let roll = self.rng.f32();
        let mage_event = if self.lift_mage.is_some() {
            self.mage.tick_bubble(dt);
            None
        } else {
            self.mage.step(dt, self.pace.walk_speed, 2.0, w - mage::WIDTH as f32 - 2.0, roll)
        };
        match mage_event {
            Some(mage::Event::Release { x, y }) => {
                let feet = self.feet_y(self.mage.x + mage::WIDTH as f32 / 2.0);
                let (hx, hy) = (x, feet + y);
                let n = if self.mage.volley { 3 } else { 1 };
                for i in 0..n {
                    let tx = self.pick_landing(hx);
                    let ty = self.feet_y(tx);
                    let flight = 0.6 + (tx - hx).abs() / w * 0.9 + i as f32 * 0.12;
                    self.cubes.push(Cube::aimed(hx, hy - 4.0, tx, ty, flight, GRAVITY));
                }
            }
            Some(mage::Event::Summoned) if self.skill == Skill::IcicleRain => {
                let n = (w / 18.0).clamp(8.0, 40.0) as usize;
                for _ in 0..n {
                    let x = self.rng.range(2.0, w - 6.0);
                    let delay = self.rng.range(0.4, 1.8);
                    self.icicles.push(Icicle::new(x, delay));
                }
            }
            Some(mage::Event::Summoned) => {
                let kind = if self.rng.chance(0.5) { friends::Kind::Snowman } else { friends::Kind::Penguin };
                let x = self.rng.range(w * 0.1, w * 0.9 - kind.width() as f32);
                self.friends.push(Friend::new(kind, x));
                self.burst_snow(x + kind.width() as f32 / 2.0, 14);
            }
            None => {}
        }
        let charge = self.mage.charge_level();
        if charge > 0.0 && self.rng.chance(0.8) {
            let feet = self.feet_y(self.mage.x + mage::WIDTH as f32 / 2.0);
            let (hx, hy) = self.mage.hand(feet);
            let (a, r) = (self.rng.range(0.0, std::f32::consts::TAU), self.rng.range(8.0, 12.0) + charge * 3.0);
            let col = if self.rng.chance(0.5) { hex(0xffffff) } else { hex(0x9be8ff) };
            let (px, py) = (hx + a.cos() * r, hy - 4.0 + a.sin() * r);
            self.particles.push(Particle::new(
                Kind::Spark,
                px,
                py,
                -a.cos() * r / 0.25,
                -a.sin() * r / 0.25,
                0.25,
                col,
            ));
        }

        // Fire mage and his fireballs.
        let pyro_feet = self.feet_y(self.pyro.x + mage::WIDTH as f32 / 2.0);
        if let Some(pyro::Event::Release { x, y }) = self.pyro.step(dt, pyro_feet) {
            let power =
                if self.pending_power.is_empty() { self.pace.melt_fraction } else { self.pending_power.remove(0) };
            let tx = self.mage.x + mage::WIDTH as f32 / 2.0;
            let ty = self.feet_y(tx) - 12.0;
            let flight = 0.5 + (tx - x).abs() / w * 0.6;
            let g = 60.0;
            self.fireballs.push(Fireball {
                x,
                y,
                vx: (tx - x) / flight,
                vy: (ty - y - 0.5 * g * flight * flight) / flight,
                age: 0.0,
                flight,
                power,
            });
        }
        let mut hits = Vec::new();
        for (i, fb) in self.fireballs.iter_mut().enumerate() {
            fb.age += dt;
            fb.x += fb.vx * dt;
            fb.vy += 60.0 * dt;
            fb.y += fb.vy * dt;
            if self.rng.chance(0.9) {
                let col = hex(*self.rng.pick(&[0xfff4b0, 0xffc13d, 0xff7a2a]));
                let (vx, vy) = (self.rng.range(-10.0, 10.0), self.rng.range(-20.0, 5.0));
                self.particles.push(Particle::new(Kind::Ember, fb.x, fb.y, vx, vy, 0.4, col));
            }
            if fb.age >= fb.flight {
                hits.push(i);
            }
        }
        for i in hits.into_iter().rev() {
            let fb = self.fireballs.remove(i);
            self.impact(fb.x, fb.y, fb.power);
        }

        // Icicle rain.
        let mut fallen = Vec::new();
        for (i, ic) in self.icicles.iter_mut().enumerate() {
            ic.step(dt);
            let (tx, ty) = ic.tip();
            if ty >= ground_at(&self.snow, self.transparent, self.h, tx) - 1.0 || ty > h + 10.0 {
                fallen.push(i);
            }
        }
        for i in fallen.into_iter().rev() {
            let ic = self.icicles.remove(i);
            let (x, y) = ic.tip();
            self.shatter_small(x, y);
        }

        // Cubes in flight.
        let mut landed = Vec::new();
        let ground = self.ground_y();
        for (i, cube) in self.cubes.iter_mut().enumerate() {
            cube.step(dt);
            if self.rng.chance(0.35) {
                let (vx, vy) = (self.rng.range(-6.0, 6.0), self.rng.range(4.0, 14.0));
                self.particles.push(Particle::new(Kind::Spark, cube.x, cube.y, vx, vy, 0.4, hex(0xbff3ff)));
            }
            let surface = ground - self.snow.height_at(cube.x);
            if (cube.age > 0.1 && cube.vy > 0.0 && cube.y >= surface - 1.0)
                || cube.x < -10.0
                || cube.x > w + 10.0
                || cube.y > h + 10.0
            {
                landed.push(i);
            }
        }
        for i in landed.into_iter().rev() {
            let cube = self.cubes.remove(i);
            self.shatter(cube.x, cube.y);
        }

        // Friends.
        let gy = self.ground_y();
        let mut bursts = Vec::new();
        for f in &mut self.friends {
            let feet = gy - self.snow.height_at(f.x + f.kind.width() as f32 / 2.0);
            if let Some(friends::Event::Burst { x, y }) = f.step(dt, feet) {
                bursts.push((x, y));
            }
        }
        self.friends.retain(Friend::alive);
        for (x, y) in bursts {
            self.frost_burst(x, y);
        }

        // Fires.
        for fire in &mut self.fires {
            fire.life -= dt;
            self.snow.thaw(fire.x, 14.0, 3.0 * fire.strength(), dt);
        }
        self.fires.retain(Fire::alive);
        for i in 0..self.fires.len() {
            if self.rng.chance(0.3) {
                let fx = self.fires[i].x + self.rng.range(-3.0, 3.0);
                let fy = self.feet_y(fx) - 4.0;
                let (vx, vy) = (self.rng.range(-6.0, 6.0), self.rng.range(-30.0, -15.0));
                let col = hex(*self.rng.pick(&[0xfff4b0, 0xffc13d, 0xff7a2a]));
                self.particles.push(Particle::new(Kind::Ember, fx, fy, vx, vy, 0.7, col));
            }
        }

        // Warrior.
        let freeze = self.freeze_level();
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let near_fire = self.fires.iter().any(|f| (f.x - wx).abs() < fire::WARM_RADIUS);
        if self.warrior.warmth < 0.45 && !near_fire && self.fires.len() < 2 && self.warrior.act == warrior::Act::Wander
        {
            self.warrior.start_building();
            self.warrior.say("Brrr... vou acender uma fogueira!", 3.0);
        }
        let roll = self.rng.f32();
        let wc = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        self.warrior.foe = if self.warrior.act == warrior::Act::Frozen {
            None
        } else {
            self.mobs
                .iter()
                .filter(|m| m.fighting() && (m.center() - wc).abs() < 110.0)
                .min_by(|a, b| (a.center() - wc).abs().total_cmp(&(b.center() - wc).abs()))
                .map(Mob::center)
        };
        let warrior_event = if self.lift_warrior.is_some() {
            self.warrior.tick_bubble(dt);
            None
        } else {
            self.warrior.step(dt, freeze, near_fire, 4.0, w - warrior::WIDTH as f32 - 4.0, roll)
        };
        match warrior_event {
            Some(warrior::Event::FireLit { x }) => self.fires.push(Fire::new(x.clamp(6.0, w - 6.0))),
            Some(warrior::Event::Strike { x }) => self.strike(x),
            None => {}
        }
        let wc = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let frozen = self.warrior.act == warrior::Act::Frozen;
        let mut bites = 0;
        for m in &mut self.mobs {
            if let Some(mobs::Event::Bite) = m.step(dt, wc) {
                bites += 1;
            }
        }
        mobs::separate(&mut self.mobs, dt);
        self.mobs.retain(Mob::present);
        if bites > 0 && !frozen {
            self.warrior.warmth = (self.warrior.warmth - 0.06 * bites as f32).max(0.01);
            if self.rng.chance(0.4) {
                self.warrior.say("Ai! Que frio!", 1.2);
            }
        }
        self.next_tip -= dt;
        if self.next_tip <= 0.0
            && !self.practicing
            && self.warrior.bubble.is_none()
            && self.warrior.act != warrior::Act::Frozen
        {
            if let Some(tip) = (!self.tips.is_empty()).then(|| self.rng.pick(&self.tips).clone()) {
                self.warrior.say(tip, 6.0);
            }
            self.next_tip = self.rng.range(35.0, 80.0);
        }

        // Ambient: corner frost creeps faster the more buried the screen is.
        self.frost.creep(dt, 0.02 + self.snow.fill() * 0.25);
        self.snow.settle();
        self.snow.settle();
        self.frost.settle();

        let ground = self.ground_y();
        let snow = &self.snow;
        for p in &mut self.particles {
            p.step(dt, |x| ground - snow.height_at(x));
        }
        self.particles.retain(Particle::alive);
        self.particles.truncate(4000);

        // The sky holds its snow while you practice, like the mage his cubes.
        let target = if self.practicing || self.paused { 0 } else { snowfall(self.flakes.len(), self.freeze_level()) };
        self.fall_flakes(dt, target);
    }

    /// Moves the snowfall with `target` flakes of the pool in the air. New
    /// ones enter at the top; surplus ones stop only once they land, so the
    /// snowfall thickens and thins without flakes popping in or out mid-air.
    /// Every flake that lands — on the ground pile, or on the snow of the
    /// side wall the wind blew it into — stays there as one pixel on the
    /// column or row it hit (one height per column/row, so thousands of
    /// grains cost nothing). Returns how many settled.
    fn fall_flakes(&mut self, dt: f32, target: usize) -> usize {
        let w = self.w as f32;
        let (transparent, time, h) = (self.transparent, self.time, self.h);
        let gust = wind(time);
        let (snow, frost) = (&mut self.snow, &mut self.frost);
        // A new flake starts in the open sky, between whatever the walls hold.
        let spawn_x = |f: &Flake, frost: &Frost| {
            let (l, r) = (frost.depth_at(Edge::Left, 0.0), frost.depth_at(Edge::Right, 0.0));
            (f.phase * 997.0 + time * 31.0).rem_euclid(w).clamp(l + 1.0, (w - r - 2.0).max(l + 1.0))
        };
        let mut settled = 0;
        for (i, f) in self.flakes.iter_mut().enumerate() {
            if !f.falling {
                if i >= target {
                    continue;
                }
                f.falling = true;
                f.y = -1.0;
                f.x = spawn_x(f, frost);
            }
            f.y += f.speed * dt;
            f.x += ((time * 1.3 + f.phase).sin() * 3.0 + gust) * dt;
            let stays = lands_on_pile(f, transparent);
            let wall = if f.x <= frost.depth_at(Edge::Left, f.y) {
                Some(Edge::Left)
            } else if f.x >= w - frost.depth_at(Edge::Right, f.y) {
                Some(Edge::Right)
            } else {
                None
            };
            let landed = match wall {
                Some(edge) => {
                    settled += usize::from(stays && frost.add_grain(edge, f.y));
                    true
                }
                None if f.y >= ground_at(snow, transparent, h, f.x) => {
                    settled += usize::from(stays && snow.add_grain(f.x));
                    true
                }
                None => false,
            };
            if landed {
                f.y = -1.0;
                f.x = spawn_x(f, frost);
                f.falling = i < target;
            }
        }
        settled
    }

    /// Somewhere across the screen, not right on top of the mage.
    fn pick_landing(&mut self, from_x: f32) -> f32 {
        let w = self.w as f32;
        for _ in 0..8 {
            let x = self.rng.range(4.0, w - 4.0);
            if (x - from_x).abs() > w * 0.15 {
                return x;
            }
        }
        (from_x + w * 0.5) % w
    }

    fn shatter(&mut self, x: f32, y: f32) {
        let x = x.clamp(0.0, self.w as f32 - 1.0);
        self.snow.add(x, self.pace.snow_per_cube, CUBE_SPREAD);
        for i in 0..11 {
            let (vx, vy, life) = (self.rng.range(-70.0, 70.0), self.rng.range(-90.0, -20.0), self.rng.range(0.6, 1.4));
            let p = Particle::new(Kind::Shard, x, y - 2.0, vx, vy, life, hex(ICE_COLORS[i % 4]));
            self.particles.push(if i < 3 { p.big() } else { p });
        }
        if let Some(fire) = self.fires.iter_mut().find(|f| (f.x - x).abs() < 8.0) {
            fire.douse();
            for _ in 0..8 {
                let (vx, vy) = (self.rng.range(-8.0, 8.0), self.rng.range(-32.0, -14.0));
                self.particles.push(Particle::new(Kind::Steam, x, y - 3.0, vx, vy, 1.0, hex(0xc9d3e8)));
            }
        }
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        if (wx - x).abs() < 7.0 && self.warrior.act != warrior::Act::Frozen {
            self.warrior.warmth = (self.warrior.warmth - 0.1).max(0.01);
            self.warrior.say("Ai! Gelado!", 1.5);
        }
    }

    pub fn mobs_out(&self) -> usize {
        self.mobs.iter().filter(|m| m.fighting()).count()
    }

    /// A wave of 1–3 frost mobs, usually from the edge farther from the
    /// warrior, sometimes split between both. Each one starts its own distance
    /// off-screen, so they arrive one by one instead of in formation.
    pub fn spawn_mobs(&mut self) {
        let w = self.w as f32;
        let wc = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let n = 1 + (self.rng.f32() * 3.0) as usize;
        let split = n > 1 && self.rng.chance(MOB_WAVE_SPLIT);
        for _ in 0..n {
            if self.mobs.len() >= 5 {
                break;
            }
            let kind = if self.rng.chance(0.6) { mobs::Kind::Slime } else { mobs::Kind::Bat };
            let mut mob = Mob::new(kind, 0.0, &mut self.rng);
            let from_left = if split { self.rng.chance(0.5) } else { wc > w / 2.0 };
            let late = self.rng.range(0.0, MOB_ARRIVAL_SPREAD_S) * kind.speed() * mob.pace;
            mob.x = if from_left { -10.0 - late } else { w + 2.0 + late };
            self.mobs.push(mob);
        }
        self.warrior.say("Monstros de gelo! Deixa comigo!", 2.5);
    }

    fn strike(&mut self, x: f32) {
        let Some(m) = self.mobs.iter_mut().filter(|m| m.fighting()).find(|m| (m.center() - x).abs() < 10.0) else {
            return;
        };
        let wc = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let mc = m.center();
        if m.hit(wc) {
            self.warrior.warmth = (self.warrior.warmth + 0.08).min(1.0);
            let y = self.feet_y(mc) - 6.0;
            for i in 0..14 {
                let (vx, vy) = (self.rng.range(-25.0, 25.0), self.rng.range(-50.0, -15.0));
                let col = hex([0xfff4b0, 0xffc13d, 0xff7a2a][i % 3]);
                self.particles.push(Particle::new(Kind::Ember, mc, y, vx, vy, 0.8, col));
            }
            if self.rng.chance(0.5) {
                self.warrior.say("Toma!", 1.0);
            }
        } else {
            let y = self.feet_y(mc) - 5.0;
            for _ in 0..4 {
                let (vx, vy) = (self.rng.range(-30.0, 30.0), self.rng.range(-40.0, -10.0));
                self.particles.push(Particle::new(Kind::Spark, mc, y, vx, vy, 0.3, hex(0xffffff)));
            }
        }
    }

    /// Starts the frost mage's summon ritual for a skill.
    pub fn cast_skill(&mut self, skill: Skill) {
        if self.mage.busy() {
            return;
        }
        self.skill = skill;
        self.mage.start_summon();
        let line = match skill {
            Skill::Friend => "Venham, amigos do gelo!",
            Skill::IcicleRain => "Chuva de gelo!",
        };
        self.mage.say(line, 2.0);
    }

    pub fn icicles_falling(&self) -> usize {
        self.icicles.len()
    }

    pub fn friends_out(&self) -> usize {
        self.friends.len()
    }

    /// An icicle hit: less snow than a cube, but many of them.
    fn shatter_small(&mut self, x: f32, y: f32) {
        let x = x.clamp(0.0, self.w as f32 - 1.0);
        self.snow.add(x, self.pace.snow_per_cube * 0.5, 4.0);
        for i in 0..5 {
            let (vx, vy, life) = (self.rng.range(-40.0, 40.0), self.rng.range(-60.0, -15.0), self.rng.range(0.4, 0.9));
            self.particles.push(Particle::new(Kind::Shard, x, y - 1.0, vx, vy, life, hex(ICE_COLORS[i % 4])));
        }
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        if (wx - x).abs() < 6.0 && self.warrior.act != warrior::Act::Frozen {
            self.warrior.warmth = (self.warrior.warmth - 0.05).max(0.01);
            self.warrior.say("Ai! Pingente!", 1.2);
        }
    }

    fn burst_snow(&mut self, x: f32, n: usize) {
        let y = self.feet_y(x);
        for i in 0..n {
            let (vx, vy) = (self.rng.range(-40.0, 40.0), self.rng.range(-70.0, -20.0));
            self.particles.push(Particle::new(Kind::Shard, x, y - 1.0, vx, vy, 0.8, hex(ICE_COLORS[i % 2])));
        }
    }

    /// A friend sprays frost onto the nearest edge and one other edge.
    fn frost_burst(&mut self, x: f32, y: f32) {
        let (w, h) = (self.w as f32, self.h as f32);
        let strength = self.frost_strength();
        let near = self.frost.nearest_edge(x, y);
        let pos = |e: Edge| if e == Edge::Top { x } else { y };
        self.frost.burst(near, pos(near), strength, h * 0.15);
        let other = *self.rng.pick(&[Edge::Top, Edge::Left, Edge::Right]);
        let at = if other == Edge::Top { self.rng.range(0.0, w) } else { self.rng.range(0.0, h * 0.8) };
        self.frost.burst(other, at, strength * 0.7, h * 0.15);
        self.snow.add(x, self.pace.snow_per_cube * 1.5, 8.0);
        for i in 0..30 {
            let a = self.rng.range(-3.0, -0.14); // upward fan
            let sp = self.rng.range(60.0, 140.0);
            let p = Particle::new(
                Kind::Spark,
                x,
                y,
                a.cos() * sp,
                a.sin() * sp,
                self.rng.range(0.5, 1.0),
                hex(ICE_COLORS[i % 3]),
            );
            self.particles.push(p);
        }
    }

    fn frost_strength(&self) -> f32 {
        (self.h.min(self.w) as f32 * 0.08).max(4.0) * (self.pace.snow_per_cube / 4.0)
    }

    pub fn draw(&self, c: &mut Canvas) {
        if let Some(b) = &self.backdrop {
            b.draw_sky(c, self.time);
            for f in self.flakes.iter().filter(|f| f.falling && f.speed < 9.0) {
                c.dot(f.x, f.y, hex(0x6e74b8));
            }
            b.draw_land(c);
        } else {
            c.clear(CLEAR);
        }
        let gy = self.ground_y();
        if self.vortex == vortex::Phase::Open {
            self.draw_world(c);
        } else {
            let (k, swirl, tremble) = self.vortex.params();
            let mut world = Canvas::new(c.w, c.h);
            self.draw_world(&mut world);
            if tremble > 0 && k >= 1.0 {
                let t = (self.time * 40.0) as i32;
                c.blit(&world, (t % 3 - 1) * tremble, ((t / 3) % 3 - 1) * tremble);
            } else {
                let mut warped = Canvas::new(c.w, c.h);
                vortex::warp(&world, &mut warped, self.hole, k, swirl);
                c.blit(&warped, 0, 0);
            }
            let size = 4.0 + (1.0 - k) * 12.0;
            if !(self.transparent && self.vortex == vortex::Phase::Closed) {
                vortex::draw_hole(c, self.hole, size, self.time);
            }
        }
        if self.show_orb && self.vortex == vortex::Phase::Open {
            vortex::draw_orb(c, self.hole, ORB_R, self.time, false);
        } else if self.show_orb && self.vortex == vortex::Phase::Closed {
            vortex::draw_orb(c, self.hole, ORB_R, self.time, true);
        }
        self.hud.draw(c, gy as i32, self.time);
        if let Some((hx, hy)) = self.hand {
            hand::draw_hand(c, hx, hy, self.grabbed.is_some(), self.time);
        }
    }

    /// Everything that lives in the world (not the landscape, not the HUD).
    fn draw_world(&self, c: &mut Canvas) {
        let gy = self.ground_y();
        self.snow.draw(c, gy as i32, self.time, self.transparent);
        // Edge snow is scenery: everyone and everything they say stays in front.
        self.frost.draw(c, self.transparent);
        for fire in &self.fires {
            fire.draw(c, self.feet_y(fire.x), self.time);
        }
        for f in &self.friends {
            f.draw(c, self.feet_y(f.x + f.kind.width() as f32 / 2.0), self.time);
        }
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        let dangle = |lift: &Option<hand::Lift>, t: f32| match lift {
            Some(l) if l.held => (t * 9.0).sin() * 1.2,
            _ => 0.0,
        };
        let wfeet = self.lift_warrior.map(|l| l.y).unwrap_or_else(|| self.feet_y(wx));
        let wdx = dangle(&self.lift_warrior, self.time);
        self.warrior.draw_at(c, self.warrior.x + wdx, wfeet, self.time);
        if self.lift_warrior.is_some_and(|l| l.held) && self.warrior.act != warrior::Act::Frozen {
            hand::draw_shy(c, (self.warrior.x + wdx).round() as i32, wfeet.round() as i32 - warrior::HEIGHT, self.time);
        }
        for m in &self.mobs {
            m.draw(c, self.feet_y(m.center()), self.time);
        }
        let mx = self.mage.x + mage::WIDTH as f32 / 2.0;
        let mfeet = self.lift_mage.map(|l| l.y).unwrap_or_else(|| self.feet_y(mx));
        let mdx = dangle(&self.lift_mage, self.time);
        self.mage.draw_at(c, self.mage.x + mdx, mfeet, self.time);
        let px = self.pyro.x + mage::WIDTH as f32 / 2.0;
        self.pyro.draw(c, self.feet_y(px), self.time);
        for fb in &self.fireballs {
            c.glow(fb.x, fb.y, 6.0, 0.8, hex(0xff7a2a));
            c.rect(fb.x.round() as i32 - 2, fb.y.round() as i32 - 2, 4, 4, hex(0xffc13d));
            c.rect(fb.x.round() as i32 - 1, fb.y.round() as i32 - 1, 2, 2, hex(0xfff4b0));
        }
        for cube in &self.cubes {
            cube.draw(c);
        }
        for ic in &self.icicles {
            ic.draw(c, self.time);
        }
        for p in &self.particles {
            p.draw(c);
        }
        if self.sun_t > 0.0 {
            draw_sun(c, self.time, (self.sun_t / 1.0).min(1.0));
        }
        let near_flakes = self.flakes.iter().filter(|f| f.falling && (f.speed >= 9.0 || self.backdrop.is_none()));
        for f in near_flakes {
            c.dot(f.x, f.y, if f.speed > 13.0 { hex(0xffffff) } else { hex(0xc9d0f2) });
        }
        if let Some(b) = &self.mage.bubble {
            let top = mfeet as i32 - mage::HEIGHT - 8;
            warrior::draw_bubble(c, mx as i32, top, &b.text);
        }
    }
}

/// Wind across the screen: turns from one side to the other every half
/// period, so each side wall gets its turn of snow.
const WIND_PERIOD_S: f32 = 60.0;
/// Strongest gust, px/s (positive blows right).
const WIND_PX_S: f32 = 3.0;

fn wind(time: f32) -> f32 {
    WIND_PX_S * (std::f32::consts::TAU * time / WIND_PERIOD_S).sin()
}

/// Flakes drawn in front of the land (all of them over the desktop) land on
/// the pile; the far ones in window mode fall behind the hills.
fn lands_on_pile(f: &Flake, transparent: bool) -> bool {
    transparent || f.speed >= 9.0
}

/// Mobs of a wave reach the screen up to this many seconds apart.
const MOB_ARRIVAL_SPREAD_S: f32 = 4.0;
/// Share of multi-mob waves whose mobs each pick a side of the screen.
const MOB_WAVE_SPLIT: f32 = 0.35;

fn ground_at(snow: &Snow, transparent: bool, h: i32, x: f32) -> f32 {
    let g = if transparent { h as f32 } else { (h - GROUND_BAND) as f32 };
    g - snow.height_at(x)
}

/// Big pixel sun at the top center; `fade` 0..1 near the end.
fn draw_sun(c: &mut Canvas, time: f32, fade: f32) {
    let (cx, cy, r) = (c.w as f32 / 2.0, (c.h as f32 * 0.16).max(14.0), (c.h as f32 * 0.07).max(7.0));
    c.glow(cx, cy, r * 3.0, 0.45 * fade, hex(0xffe08a));
    for k in 0..12 {
        let a = k as f32 / 12.0 * std::f32::consts::TAU + time * 0.4;
        for d in 0..(r * 0.8) as i32 {
            let dd = r * 1.3 + d as f32;
            if fade > crate::render::canvas::bayer((cx + a.cos() * dd) as i32, (cy + a.sin() * dd) as i32) {
                c.dot(cx + a.cos() * dd, cy + a.sin() * dd, hex(0xffc13d));
            }
        }
    }
    for y in (cy - r) as i32..=(cy + r) as i32 {
        for x in (cx - r) as i32..=(cx + r) as i32 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d <= r && fade > crate::render::canvas::bayer(x, y) * 0.5 {
                c.set(x, y, if d < r * 0.6 { hex(0xfff4b0) } else { hex(0xffd64a) });
            }
        }
    }
}

fn snow_cap(h: i32) -> f32 {
    (h as f32 * 0.22).max(8.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::level::Commitment;

    fn run(s: &mut Scene, seconds: f32) {
        let dt = 1.0 / 30.0;
        for _ in 0..(seconds / dt) as i32 {
            s.step(dt);
        }
    }

    #[test]
    fn ignoring_the_mage_buries_the_screen_over_time() {
        let mut s = Scene::new(240, 135, 1, Commitment::Steady.pace(), true);
        run(&mut s, 20.0);
        let early = s.snow.fill();
        run(&mut s, 120.0);
        assert!(early > 0.0);
        assert!(s.snow.fill() > early);
        assert!(s.freeze_level() > 0.0);
    }

    #[test]
    fn higher_commitment_freezes_faster() {
        let mut chill = Scene::new(240, 135, 3, Commitment::Chill.pace(), true);
        let mut relentless = Scene::new(240, 135, 3, Commitment::Relentless.pace(), true);
        run(&mut chill, 90.0);
        run(&mut relentless, 90.0);
        assert!(
            relentless.snow.fill() > chill.snow.fill() * 2.0,
            "{} vs {}",
            relentless.snow.fill(),
            chill.snow.fill()
        );
    }

    #[test]
    fn a_correct_phrase_melts_snow_and_frost() {
        let mut s = Scene::new(240, 135, 2, Commitment::Relentless.pace(), true);
        run(&mut s, 150.0);
        let (snow, frost) = (s.snow.fill(), s.frost.coverage());
        assert!(frost > 0.0, "friends should have frosted the edges");
        s.celebrate(1.0);
        let melt = Commitment::Relentless.pace().melt_fraction;
        assert!(s.snow.fill() <= snow * (1.0 - melt) + 1e-4);
        assert!(s.frost.coverage() <= frost * (1.0 - melt) + 1e-4);
        assert!(s.snow.fill() > 0.0, "one answer never clears everything");
    }

    #[test]
    fn during_a_lesson_the_fire_mage_melts_on_impact_not_instantly() {
        let mut s = Scene::new(240, 135, 2, Commitment::Relentless.pace(), true);
        run(&mut s, 150.0);
        s.set_practicing(true);
        run(&mut s, 1.0);
        assert!(s.pyro.visible());
        let before = s.snow.fill();
        s.celebrate(1.0);
        assert!((s.snow.fill() - before).abs() < 1e-3, "nothing melts until the fireball lands");
        let mut staggered = false;
        for _ in 0..90 {
            s.step(1.0 / 30.0);
            staggered |= s.mage.act == mage::Act::Stagger;
        }
        assert!(s.snow.fill() < before, "fireball landed");
        assert!(s.snow.fill() > before * 0.5, "one answer melts only a portion");
        assert!(staggered, "the frost mage takes the hit");
        s.set_practicing(false);
        run(&mut s, 1.0);
        assert!(!s.pyro.visible(), "the fire mage leaves after the lesson");
    }

    #[test]
    fn a_miss_fizzles_and_the_frost_mage_fires_back() {
        let mut s = Scene::new(240, 135, 3, Commitment::Steady.pace(), true);
        s.set_practicing(true);
        run(&mut s, 1.0);
        s.miss();
        run(&mut s, 1.2);
        assert!(s.cubes_in_flight() > 0 || s.snow.fill() > 0.0);
    }

    #[test]
    fn the_sun_melts_a_big_chunk_but_not_everything_and_stuns_the_mage() {
        let mut s = Scene::new(240, 135, 4, Commitment::Relentless.pace(), true);
        run(&mut s, 150.0);
        let before = s.snow.fill();
        s.sun();
        assert!(s.sun_active());
        assert!(s.snow.fill() < before * 0.7 && s.snow.fill() > 0.0);
        let after_sun = s.snow.fill();
        run(&mut s, 10.0);
        assert!(s.snow.fill() <= after_sun + 1e-3, "stunned mage adds no snow");
        assert!(!s.sun_active());
    }

    #[test]
    fn clicking_characters_makes_them_react_without_free_melting() {
        let mut s = Scene::new(240, 135, 5, Commitment::Steady.pace(), false);
        s.tips = vec!["Dica de teste".into()];
        run(&mut s, 3.0);
        let fill = s.snow.fill();
        let mx = s.mage.x + 8.0;
        assert_eq!(s.poke(mx, s.feet_y(mx) - 10.0), Poke::Mage);
        assert!(s.mage_bubble().is_some());
        let wx = s.warrior.x + 6.0;
        assert_eq!(s.poke(wx, s.feet_y(wx) - 8.0), Poke::Warrior);
        assert_eq!(s.warrior.bubble.as_ref().unwrap().text, "Dica de teste");
        assert_eq!(s.poke(120.0, 5.0), Poke::Nothing);
        assert!(s.snow.fill() >= fill - 1e-3);
    }

    #[test]
    fn friends_show_up_early_and_come_and_go() {
        let mut s = Scene::new(240, 135, 8, Commitment::Steady.pace(), true);
        let mut seen = false;
        for _ in 0..(40 * 30) {
            s.step(1.0 / 30.0);
            seen |= s.friends_out() > 0;
        }
        assert!(seen, "a friend should be summoned within the first ~30s");
        run(&mut s, 10.0);
        assert_eq!(s.friends_out(), 0, "summons are temporary");
    }

    #[test]
    fn icicle_rain_falls_from_the_top_and_adds_snow() {
        let mut s = Scene::new(240, 135, 9, Commitment::Steady.pace(), true);
        s.cast_skill(Skill::IcicleRain);
        assert_eq!(s.mage_bubble(), Some("Chuva de gelo!"));
        let before = s.snow.fill();
        let mut peak = 0;
        for _ in 0..(2 * 30) {
            s.step(1.0 / 30.0);
            peak = peak.max(s.icicles_falling());
        }
        assert!(peak >= 8);
        run(&mut s, 4.0);
        assert_eq!(s.icicles_falling(), 0);
        assert!(s.snow.fill() > before);
    }

    #[test]
    fn the_warrior_fights_off_a_mob_wave() {
        let mut s = Scene::new(240, 135, 10, Commitment::Chill.pace(), true);
        s.spawn_mobs();
        assert!(s.mobs_out() >= 1);
        let mut fought = false;
        for _ in 0..(40 * 30) {
            s.step(1.0 / 30.0);
            fought |= s.warrior.act == warrior::Act::Fight;
        }
        assert!(fought);
        assert_eq!(s.mobs_out(), 0, "all mobs defeated");
        assert_ne!(s.warrior.act, warrior::Act::Frozen);
    }

    #[test]
    fn pausing_swallows_the_world_and_resuming_returns_it_unchanged() {
        let mut s = Scene::new(240, 135, 11, Commitment::Relentless.pace(), true);
        run(&mut s, 60.0);
        let snow = s.snow.fill();
        let mut before = Canvas::new(240, 135);
        s.draw(&mut before);
        s.set_paused(true);
        run(&mut s, 3.0);
        assert!(s.swallowed());
        let mut during = Canvas::new(240, 135);
        s.draw(&mut during);
        assert!(during.opaque_in(0, 0, 240, 135) < before.opaque_in(0, 0, 240, 135) / 10, "screen is clean");
        run(&mut s, 120.0);
        assert!((s.snow.fill() - snow).abs() < 1e-6, "nothing happens while paused");
        s.set_paused(false);
        run(&mut s, 0.6);
        assert!(!s.swallowed());
        run(&mut s, 1.0);
        let mut after = Canvas::new(240, 135);
        s.draw(&mut after);
        assert!(after.opaque_in(0, 0, 240, 135) > before.opaque_in(0, 0, 240, 135) / 2, "everything came back");
        run(&mut s, 30.0);
        assert!(s.snow.fill() >= snow, "the mage is back at work");
    }

    #[test]
    fn window_mode_orb_is_clickable_and_shows_the_hole_when_paused() {
        let mut s = Scene::new(240, 135, 12, Commitment::Steady.pace(), false);
        s.show_orb = true;
        s.hole = (12.0, 12.0);
        assert_eq!(s.poke(12.0, 12.0), Poke::Orb);
        s.set_paused(true);
        run(&mut s, 3.0);
        let mut c = Canvas::new(240, 135);
        s.draw(&mut c);
        assert_eq!(c.get(12, 12), Some(hex(0x05030d)), "black hole in the orb");
    }

    #[test]
    fn the_magic_hand_lifts_the_mage_who_complains_and_falls_when_released() {
        let mut s = Scene::new(240, 135, 13, Commitment::Steady.pace(), true);
        run(&mut s, 1.0);
        let (mx, feet) = (s.mage.x + 8.0, s.feet_y(s.mage.x + 8.0));
        assert_eq!(s.grab_at(mx, feet - 10.0), Some(hand::Who::Mage));
        assert!(hand::MAGE_HELD.contains(&s.mage_bubble().unwrap()), "pissy");
        s.hand_move(150.0, 40.0);
        let before = s.snow.fill();
        run(&mut s, 3.0);
        assert!((s.mage.x - 142.0).abs() < 2.0, "carried along");
        assert!(s.snow.fill() <= before + 1e-3 || s.cubes_in_flight() == 0, "no throwing while dangling");
        s.release();
        run(&mut s, 2.0);
        assert!(s.holding().is_none());
        let mut c = Canvas::new(240, 135);
        s.draw(&mut c);
    }

    #[test]
    fn the_warrior_is_shy_but_a_frozen_one_cannot_be_lifted() {
        let mut s = Scene::new(240, 135, 14, Commitment::Steady.pace(), true);
        let (wx, feet) = (s.warrior.x + 6.0, s.feet_y(s.warrior.x + 6.0));
        assert_eq!(s.grab_at(wx, feet - 8.0), Some(hand::Who::Warrior));
        assert!(hand::WARRIOR_HELD.contains(&s.warrior.bubble.as_ref().unwrap().text.as_str()));
        s.set_hand(false);
        assert!(s.holding().is_none(), "turning the hand off drops him");
        run(&mut s, 2.0);
        s.warrior.warmth = 0.001;
        s.snow.dust(100.0);
        run(&mut s, 3.0);
        assert_eq!(s.warrior.act, warrior::Act::Frozen);
        let feet = s.feet_y(s.warrior.x + 6.0);
        assert_eq!(s.grab_at(s.warrior.x + 6.0, feet - 8.0), None);
    }

    #[test]
    fn grabbing_empty_space_does_nothing_but_shows_the_hand() {
        let mut s = Scene::new(240, 135, 15, Commitment::Steady.pace(), true);
        assert_eq!(s.grab_at(120.0, 5.0), None);
        assert!(s.hand.is_some());
    }

    #[test]
    fn mage_holds_fire_while_you_practice() {
        let mut s = Scene::new(240, 135, 4, Commitment::Relentless.pace(), true);
        s.set_practicing(true);
        run(&mut s, 30.0);
        assert_eq!(s.cubes_in_flight(), 0);
        assert!(s.snow.fill() < 1e-3, "no cubes should have landed, fill {}", s.snow.fill());
    }

    #[test]
    fn the_warrior_eventually_builds_a_fire_when_cold() {
        let mut s = Scene::new(240, 135, 5, Commitment::Relentless.pace(), true);
        let mut saw_fire = false;
        for _ in 0..(200 * 30) {
            s.step(1.0 / 30.0);
            saw_fire |= !s.fires.is_empty();
        }
        assert!(saw_fire);
    }

    #[test]
    fn overlay_mode_leaves_the_desktop_visible_and_window_mode_paints_everything() {
        let mut overlay = Scene::new(200, 120, 6, Commitment::Steady.pace(), true);
        let mut window = Scene::new(200, 120, 6, Commitment::Steady.pace(), false);
        run(&mut overlay, 5.0);
        run(&mut window, 5.0);
        let (mut a, mut b) = (Canvas::new(200, 120), Canvas::new(200, 120));
        overlay.draw(&mut a);
        window.draw(&mut b);
        assert!(a.opaque_in(0, 0, 200, 120) < 200 * 120 / 4);
        assert_eq!(b.opaque_in(0, 0, 200, 120), 200 * 120);
    }

    #[test]
    fn same_seed_renders_identical_frames() {
        let mut a = Scene::new(160, 90, 9, Commitment::Steady.pace(), false);
        let mut b = Scene::new(160, 90, 9, Commitment::Steady.pace(), false);
        run(&mut a, 8.0);
        run(&mut b, 8.0);
        let (mut ca, mut cb) = (Canvas::new(160, 90), Canvas::new(160, 90));
        a.draw(&mut ca);
        b.draw(&mut cb);
        assert_eq!(ca.bytes(), cb.bytes());
    }

    #[test]
    fn resize_keeps_the_world_running() {
        let mut s = Scene::new(200, 120, 7, Commitment::Steady.pace(), false);
        run(&mut s, 30.0);
        s.resize(320, 180);
        run(&mut s, 5.0);
        let mut c = Canvas::new(320, 180);
        s.draw(&mut c);
        assert_eq!(c.opaque_in(0, 0, 320, 180), 320 * 180);
    }

    fn falling(s: &Scene) -> usize {
        s.flakes.iter().filter(|f| f.falling).count()
    }

    /// Falling count matches the curve, give or take the few surplus flakes
    /// still finishing their fall after the level dipped.
    fn on_curve(s: &Scene) -> bool {
        let target = snowfall(s.flakes.len(), s.freeze_level());
        falling(s).abs_diff(target) <= s.flakes.len() / 50
    }

    #[test]
    fn snowfall_starts_with_a_few_flakes_and_grows_slowly_then_to_the_whole_pool() {
        assert_eq!(snowfall(1000, 0.0), 20, "2% on a clear screen");
        assert_eq!(snowfall(100, 0.0), 6, "but never fewer than a handful");
        assert_eq!(snowfall(1000, 0.25), 81, "still light a quarter of the way");
        assert_eq!(snowfall(1000, 0.5), 265);
        assert_eq!(snowfall(1000, 1.0), 1000);
        assert_eq!(snowfall(1000, 7.0), 1000, "clamped");
        assert_eq!(snowfall(1000, -1.0), 20, "clamped");
        assert_eq!(snowfall(4, 0.0), 4, "a tiny pool falls whole");
        let mut last = 0;
        for i in 0..=100 {
            let n = snowfall(777, i as f32 / 100.0);
            assert!(n >= last, "monotonic at {i}");
            last = n;
        }
    }

    #[test]
    fn a_clear_screen_starts_with_a_light_snowfall_already_in_the_air() {
        let s = Scene::new(480, 270, 1, Commitment::Chill.pace(), true);
        assert_eq!(falling(&s), snowfall(s.flakes.len(), 0.0));
        let high = s.flakes.iter().filter(|f| f.falling && f.y > s.h as f32 * 0.5).count();
        assert!(high > 0, "spread over the screen, not all waiting at the top");
    }

    #[test]
    fn as_the_screen_freezes_new_flakes_enter_from_the_sky() {
        let mut s = Scene::new(480, 270, 2, Commitment::Chill.pace(), true);
        let before: Vec<bool> = s.flakes.iter().map(|f| f.falling).collect();
        s.snow.dust(1000.0); // a full pile
        s.step(1.0 / 30.0);
        for (f, was) in s.flakes.iter().zip(before) {
            if f.falling && !was {
                assert!(f.y <= 1.0, "a new flake starts at the top, got y={}", f.y);
            }
        }
        run(&mut s, 120.0);
        let level = s.freeze_level();
        assert!(level > 0.55, "a full pile alone is ~0.6 of the freeze level, got {level}");
        assert!(on_curve(&s), "{} falling at level {level}", falling(&s));
        assert!(falling(&s) > snowfall(s.flakes.len(), 0.0) * 5, "far thicker than on a clear screen");
    }

    #[test]
    fn melting_thins_the_snowfall_as_flakes_land_never_mid_air() {
        let mut s = Scene::new(480, 270, 3, Commitment::Chill.pace(), true);
        s.snow.dust(1000.0);
        run(&mut s, 120.0);
        let full = falling(&s);
        s.snow.melt(1.0);
        s.step(1.0 / 30.0);
        assert!(falling(&s) as f32 > full as f32 * 0.95, "no flake vanishes in the air");
        run(&mut s, 120.0);
        assert!(on_curve(&s), "{} falling at level {}", falling(&s), s.freeze_level());
    }

    fn pile(s: &Scene) -> f32 {
        s.snow.fill() * s.snow.width() as f32 * s.snow.cap()
    }

    /// Snow on the ground plus snow stuck to the side walls, in pixels.
    fn walls(s: &Scene) -> f32 {
        (0..s.h).map(|y| s.frost.depth_at(Edge::Left, y as f32) + s.frost.depth_at(Edge::Right, y as f32)).sum()
    }

    #[test]
    fn every_flake_that_lands_stays_as_one_pixel_of_snow() {
        let mut s = Scene::new(480, 270, 5, Commitment::Chill.pace(), true);
        let all = s.flakes.len();
        let before = pile(&s) + walls(&s);
        let mut landed = 0;
        for _ in 0..(60 * 30) {
            landed += s.fall_flakes(1.0 / 30.0, all);
            s.time += 1.0 / 30.0;
        }
        assert!(landed > 300, "enough landings to measure, got {landed}");
        let added = pile(&s) + walls(&s) - before;
        assert!((added - landed as f32).abs() < 0.5, "{landed} grains landed, ground and walls grew {added}");
    }

    #[test]
    fn flakes_stay_on_screen_until_they_land() {
        let mut s = Scene::new(480, 270, 8, Commitment::Chill.pace(), true);
        let all = s.flakes.len();
        for _ in 0..(90 * 30) {
            s.fall_flakes(1.0 / 30.0, all);
            s.time += 1.0 / 30.0;
            for f in s.flakes.iter().filter(|f| f.falling) {
                assert!((0.0..s.w as f32).contains(&f.x), "flake left the screen sideways at x={}", f.x);
                assert!(f.y <= s.h as f32, "flake fell through the floor at y={}", f.y);
            }
        }
    }

    #[test]
    fn practicing_stops_new_flakes_but_those_in_the_air_still_land() {
        let mut s = Scene::new(480, 270, 6, Commitment::Relentless.pace(), true);
        let in_air = falling(&s);
        s.set_practicing(true);
        let before = pile(&s);
        run(&mut s, 60.0);
        assert_eq!(falling(&s), 0, "no new snowfall while you practice");
        let added = pile(&s) - before;
        assert!((added - in_air as f32).abs() < 0.5, "the {in_air} flakes in the air settled, pile grew {added}");
    }

    /// Waves over many seeds: where each mob starts, relative to its side.
    fn waves(n: u64) -> Vec<Vec<f32>> {
        (0..n)
            .map(|seed| {
                let mut s = Scene::new(240, 135, seed, Commitment::Chill.pace(), true);
                s.spawn_mobs();
                s.mobs.iter().map(|m| m.x).collect()
            })
            .collect()
    }

    #[test]
    fn a_wave_arrives_spread_out_not_in_a_fixed_formation() {
        let mut gaps = Vec::new();
        for wave in waves(200) {
            let mut left: Vec<f32> = wave.iter().copied().filter(|x| *x < 0.0).collect();
            left.sort_by(f32::total_cmp);
            gaps.extend(left.windows(2).map(|w| w[1] - w[0]));
        }
        assert!(gaps.len() > 20, "enough multi-mob waves: {}", gaps.len());
        let fixed = gaps.iter().filter(|g| (**g - 14.0).abs() < 0.5).count();
        assert!(fixed * 10 < gaps.len(), "still mostly 14 px apart: {fixed}/{}", gaps.len());
        let (lo, hi) = gaps.iter().fold((f32::MAX, f32::MIN), |(l, h), g| (l.min(*g), h.max(*g)));
        assert!(hi - lo > 30.0, "arrival gaps barely vary: {lo}..{hi}");
    }

    #[test]
    fn some_waves_come_from_both_sides() {
        let multi: Vec<Vec<f32>> = waves(300).into_iter().filter(|w| w.len() > 1).collect();
        let both = multi.iter().filter(|w| w.iter().any(|x| *x < 0.0) && w.iter().any(|x| *x > 240.0)).count();
        let share = both as f32 / multi.len() as f32;
        assert!((0.1..0.4).contains(&share), "{both} of {} multi-mob waves split", multi.len());
    }

    #[test]
    fn the_wind_turns_from_one_side_to_the_other() {
        let samples: Vec<f32> = (0..600).map(|i| wind(i as f32 * WIND_PERIOD_S / 600.0)).collect();
        assert!(samples.iter().any(|&v| v > WIND_PX_S * 0.9), "blows right");
        assert!(samples.iter().any(|&v| v < -WIND_PX_S * 0.9), "blows left");
        assert!(samples.iter().all(|v| v.abs() <= WIND_PX_S + 1e-3));
        let mean = samples.iter().sum::<f32>() / samples.len() as f32;
        assert!(mean.abs() < 0.1, "no side is favored over a cycle: {mean}");
    }

    #[test]
    fn flakes_that_reach_a_wall_stick_where_they_hit_on_both_sides() {
        let mut s = Scene::new(480, 270, 9, Commitment::Chill.pace(), true);
        let all = s.flakes.len();
        for _ in 0..(WIND_PERIOD_S as i32 * 2 * 30) {
            s.fall_flakes(1.0 / 30.0, all);
            s.time += 1.0 / 30.0;
        }
        let side = |e: Edge| (0..s.h).map(|y| s.frost.depth_at(e, y as f32)).sum::<f32>();
        assert!(
            side(Edge::Left) > 10.0 && side(Edge::Right) > 10.0,
            "left {} right {}",
            side(Edge::Left),
            side(Edge::Right)
        );
        let rows_hit = (0..s.h).filter(|&y| s.frost.depth_at(Edge::Left, y as f32) > 0.0).count();
        assert!(rows_hit > 20, "spread over the wall's height, not one spot: {rows_hit} rows");
    }

    #[test]
    fn characters_and_their_words_stay_in_front_of_the_wall_snow() {
        let mut s = Scene::new(480, 270, 12, Commitment::Chill.pace(), true);
        s.warrior.x = 2.0;
        s.warrior.say("Aperte Ctrl+Alt+M e fale comigo!", 9.0);
        let mut clear = Canvas::new(480, 270);
        s.draw(&mut clear);
        for y in 0..270 {
            while s.frost.add_grain(Edge::Left, y as f32) {}
        }
        let mut walled = Canvas::new(480, 270);
        s.draw(&mut walled);
        let feet = s.feet_y(s.warrior.x + warrior::WIDTH as f32 / 2.0) as i32;
        let (mut seen, mut hidden) = (0, 0);
        for y in (feet - 40).max(0)..feet - 1 {
            for x in 0..60 {
                if clear.opaque_in(x, y, 1, 1) == 1 {
                    seen += 1;
                    hidden += usize::from(clear.get(x, y) != walled.get(x, y));
                }
            }
        }
        assert!(seen > 50, "the warrior and his bubble are on screen: {seen}");
        assert_eq!(hidden, 0, "{hidden} of {seen} warrior/bubble pixels covered by wall snow");
    }
}
