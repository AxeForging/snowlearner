//! The whole winter world as a pure, seeded simulation: `step(dt)` advances it,
//! `draw(canvas)` renders it. No window, audio or clock inside, so it runs the
//! same in the app, in `snowlearner snapshot` and in tests.

pub mod backdrop;
pub mod blanket;
pub mod fire;
pub mod friends;
pub mod frost;
pub mod glass;
pub mod hand;
pub mod hud;
pub mod ice;
pub mod mage;
pub mod mobs;
pub mod owl;
pub mod pyro;
pub mod rng;
pub mod slide;
pub mod snow;
pub mod vortex;
pub mod warrior;

use std::cell::RefCell;

use crate::config::level::Pace;
use crate::lang::{Lines, Native, T};
use crate::render::canvas::{CLEAR, Canvas, hex};
use backdrop::{Backdrop, GROUND_BAND};
use blanket::Blanket;
use fire::Fire;
use friends::Friend;
use frost::{Edge, Frost};
use glass::Glass;
use hud::Hud;
use ice::{Cube, Icicle, Kind, Particle};
use mage::Mage;
use mobs::Mob;
use pyro::{Pyro, Spell};
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
    spell: Spell,
}

/// What the frost mage's summon ritual is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skill {
    Friend,
    IcicleRain,
    /// One ice owl, only once the pile is deep (`owl::PILE_LEVEL`).
    Owl,
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
    /// The pile reached the ice line and the edges are freezing.
    frozen_over: bool,
    /// Frost crystals on the glass, laid out for this screen size.
    glass: Glass,
    /// The snow blanket as last drawn; redrawn only when the snow moves.
    blanket: RefCell<Blanket>,
    /// Reused frames for the black-hole warp (world, warped): no allocation per frame.
    warp_frames: RefCell<(Canvas, Canvas)>,
    pub fires: Vec<Fire>,
    pub hud: Hud,
    /// The learner's own language: what the actors say.
    native: Native,
    /// Warrior tips (in the learner's language), provided by the app (hotkey hints, phrase tips).
    pub tips: Vec<String>,
    cubes: Vec<Cube>,
    particles: Vec<Particle>,
    friends: Vec<Friend>,
    flakes: Vec<Flake>,
    backdrop: Option<Backdrop>,
    /// The fire mage, present only during lessons.
    pub pyro: Pyro,
    fireballs: Vec<Fireball>,
    pending_power: Vec<(f32, Spell)>,
    /// The spell cast for the last right answer.
    last_spell: Option<Spell>,
    icicles: Vec<Icicle>,
    mobs: Vec<Mob>,
    next_mobs: f32,
    /// The frost mage's ice owl, one at most, and its snowballs (capped).
    owl: Option<owl::Owl>,
    snowballs: Vec<owl::Snowball>,
    next_owl: f32,
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
    /// The orb's radius in scene pixels (the overlay's orb window may be drawn
    /// at a bigger scale than the scene).
    pub orb_r: f32,
    /// Over the desktop: a rect (x, y, w, h, scene pixels) left clear, where the
    /// panel window sits. The overlay stacks above every managed window on X11,
    /// so drawing there would cover the panel.
    pub keep_clear: Option<(i32, i32, i32, i32)>,
    practicing: bool,
    throws: u32,
    next_throw: f32,
    next_summon: f32,
    next_tip: f32,
    /// Weather-only screen (the other monitors in overlay mode): the freeze
    /// level it follows, set from outside. No actors, no lesson, no HUD.
    weather: Option<f32>,
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
            frozen_over: false,
            glass: Glass::new(w, h),
            blanket: RefCell::default(),
            warp_frames: RefCell::new((Canvas::new(1, 1), Canvas::new(1, 1))),
            fires: Vec::new(),
            hud: Hud::default(),
            native: Native::default(),
            tips: Vec::new(),
            cubes: Vec::new(),
            particles: Vec::new(),
            friends: Vec::new(),
            flakes,
            backdrop: None,
            pyro: Pyro::default(),
            fireballs: Vec::new(),
            pending_power: Vec::new(),
            last_spell: None,
            icicles: Vec::new(),
            mobs: Vec::new(),
            next_mobs: 40.0_f32.min(pace.mobs_every),
            owl: None,
            snowballs: Vec::with_capacity(owl::MAX_SNOWBALLS),
            next_owl: owl::EVERY * 0.5,
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
            orb_r: ORB_R,
            keep_clear: None,
            practicing: false,
            throws: 0,
            next_throw: 1.5,
            next_summon: 25.0_f32.min(pace.summon_every),
            next_tip: 12.0,
            weather: None,
            rng,
        };
        if !transparent {
            s.backdrop = Some(Backdrop::new(w, h));
        }
        s
    }

    /// A screen with only the weather — snowfall, the pile and the wall snow —
    /// following the freeze level given by `set_freeze_target` (the primary
    /// screen's). Nobody lives here: no mages, warrior, mobs, lesson or HUD.
    pub fn weather(w: i32, h: i32, seed: u64, pace: Pace, transparent: bool) -> Self {
        let mut s = Scene::new(w, h, seed, pace, transparent);
        s.weather = Some(0.0);
        s
    }

    /// The level (0 clear, 1 buried) a weather-only screen drifts toward.
    /// A full scene makes its own weather and ignores it.
    pub fn set_freeze_target(&mut self, level: f32) {
        if let Some(t) = &mut self.weather {
            *t = level.clamp(0.0, 1.0);
        }
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
        self.glass = Glass::new(w, h);
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

    /// How far the ground pile is toward the ice line: 0 bare, 1 there.
    fn pile_level(&self) -> f32 {
        (self.snow.mean() / (self.h as f32 * ICE_LINE)).min(1.0)
    }

    /// Whether the ground pile reached the ice line, where the edges freeze;
    /// once frozen over, only a real melt (practicing) below 90% of the line
    /// thaws them, not a campfire nibbling at it.
    fn edges_freeze(&self) -> bool {
        self.pile_level() >= 1.0 || (self.frozen_over && self.pile_level() >= 0.9)
    }

    /// Pace amounts (pixels) on this screen, see `PACE_HEIGHT`.
    fn scaled(&self, px: f32) -> f32 {
        px * self.h as f32 / PACE_HEIGHT
    }

    /// 0 = clear, 1 = buried: drives the warrior's chill and corner frost.
    pub fn freeze_level(&self) -> f32 {
        (self.pile_level() * 0.6 + self.frost.coverage() * 0.4 / 0.5).min(1.0)
    }

    pub fn cubes_in_flight(&self) -> usize {
        self.cubes.len()
    }

    // ---- events from the lesson controller ----

    /// A lesson is on: the frost mage stops to watch, the fire mage teleports
    /// in on the other side, the warrior cheers you on.
    pub fn set_practicing(&mut self, on: bool) {
        if on && !self.practicing {
            self.warrior.say(T::WarriorCheer.get(self.native), 3.0);
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
            self.mage.say(T::MageBlackHole.get(self.native), 1.5);
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
            let line = *self.rng.pick(Lines::MageHeld.get(self.native));
            self.mage.say(line, 2.2);
            return Some(hand::Who::Mage);
        }
        if inside(self.warrior.x, wfeet, warrior::WIDTH, warrior::HEIGHT) && self.warrior.act != warrior::Act::Frozen {
            self.grabbed = Some((hand::Who::Warrior, x - self.warrior.x, wfeet - y));
            self.lift_warrior = Some(hand::Lift::new(wfeet));
            let line = *self.rng.pick(Lines::WarriorHeld.get(self.native));
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
                (hand::Who::Mage, true) => *self.rng.pick(Lines::MageHeld.get(self.native)),
                (hand::Who::Mage, false) => *self.rng.pick(Lines::MageLanded.get(self.native)),
                (hand::Who::Warrior, true) => *self.rng.pick(Lines::WarriorHeld.get(self.native)),
                (hand::Who::Warrior, false) => *self.rng.pick(Lines::WarriorLanded.get(self.native)),
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

    /// On the orb: drawn in-scene (window mode) or its own window over the
    /// overlay. The snow never takes this spot (`draw`), so it stays clickable.
    pub fn orb_at(&self, x: f32, y: f32) -> bool {
        ((x - self.hole.0).powi(2) + (y - self.hole.1).powi(2)).sqrt() <= self.orb_r + 2.0
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
    pub fn celebrate(&mut self, power: f32, spell: Spell) {
        self.pyro.focus(false);
        self.last_spell = Some(spell);
        let melt = self.pace.melt_fraction * power * spell.melt();
        if self.pyro.visible() {
            self.pending_power.push((melt, spell));
            self.pyro.cast();
        } else {
            let (x, y) = (self.mage.x + mage::WIDTH as f32 / 2.0, self.feet_y(self.mage.x) - 12.0);
            self.impact(x, y, melt, spell);
        }
    }

    /// The spell the fire mage cast for the last right answer.
    pub fn last_spell(&self) -> Option<Spell> {
        self.last_spell
    }

    /// A wrong answer: the fireball fizzles and the frost mage fires back.
    pub fn miss(&mut self) {
        self.pyro.focus(false);
        self.pyro.fizzle();
        self.mage.start_throw(false);
    }

    fn impact(&mut self, x: f32, y: f32, melt: f32, spell: Spell) {
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
        for i in 0..20 * spell.radius() as usize {
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
        self.warrior.say(T::WarriorWarm.get(self.native), 3.0);
    }

    /// Combo reward: the sun shines for a while, melting a big chunk (never
    /// all of it) and stunning the frost mage.
    pub fn sun(&mut self) {
        self.sun_t = SUN_SECONDS;
        self.stun_t = SUN_SECONDS * 2.5;
        self.snow.melt(SUN_MELT);
        self.frost.melt(SUN_MELT * 1.5);
        self.mage.say(T::MageSun.get(self.native), 3.0);
        self.warrior.say(T::WarriorSun.get(self.native), 3.0);
    }

    pub fn sun_active(&self) -> bool {
        self.sun_t > 0.0
    }

    /// The learner's own language: the actors' lines and the HUD follow it.
    pub fn set_native(&mut self, native: Native) {
        self.native = native;
        self.hud.native = native;
    }

    pub fn native(&self) -> Native {
        self.native
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
            let lines = Lines::MagePoked.get(self.native);
            let line = lines[self.pokes as usize % lines.len()];
            self.mage.say(line, 2.0);
            self.mage.start_throw(self.pokes % 3 == 0);
            return Poke::Mage;
        }
        let wx = self.warrior.x;
        if hit(wx, self.feet_y(wx + 6.0), warrior::WIDTH, warrior::HEIGHT + 6) {
            if self.warrior.act != warrior::Act::Frozen {
                let tip = if self.tips.is_empty() {
                    T::WarriorHello.get(self.native).to_string()
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
        if let Some(target) = self.weather {
            self.step_weather(dt, target);
            return;
        }
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
            self.next_owl -= dt;
            if self.next_mobs <= 0.0 {
                self.spawn_mobs();
                self.next_mobs = self.pace.mobs_every * self.rng.range(0.8, 1.2);
            }
            if self.next_owl <= 0.0 && !self.mage.busy() && self.owl_ready() {
                self.cast_skill(Skill::Owl);
                self.next_owl = owl::EVERY * self.rng.range(0.8, 1.2);
            } else if self.next_icicles <= 0.0 && !self.mage.busy() {
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
            Some(mage::Event::Summoned) if self.skill == Skill::Owl && self.owl_ready() => {
                let from_left = self.rng.chance(0.5);
                let y = self.rng.range(h * 0.1, h * 0.25);
                self.owl = Some(owl::Owl::new(from_left, w, y));
            }
            // The pile froze over (or melted) mid-summon: the owl stays away.
            Some(mage::Event::Summoned) if self.skill == Skill::Owl => {}
            Some(mage::Event::Summoned) => {
                let kind = if self.rng.chance(0.5) { friends::Kind::Snowman } else { friends::Kind::Penguin };
                let x = self.rng.range(w * 0.1, w * 0.9 - kind.width() as f32);
                self.friends.push(Friend::new(kind, x));
                self.burst_snow(x + kind.width() as f32 / 2.0, 14);
            }
            None => {}
        }
        let can = self.lift_mage.is_none() && self.mage.act == mage::Act::Walk;
        let slid = self.slide(can, self.mage.x, mage::WIDTH, self.mage.dir, 2.0, dt);
        if let Some(x) = slid {
            self.mage.x = x;
            // Only when nobody talks: two bubbles at once overlap.
            if !self.mage.sliding && self.mage.bubble.is_none() && self.warrior.bubble.is_none() {
                self.mage.say(T::MageSlide.get(self.native), 2.0);
            }
        }
        self.mage.sliding = slid.is_some();
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
            let (power, spell) = if self.pending_power.is_empty() {
                (self.pace.melt_fraction, Spell::Fireball)
            } else {
                self.pending_power.remove(0)
            };
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
                spell,
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
            self.impact(fb.x, fb.y, fb.power, fb.spell);
        }

        // The ice owl and its snowballs (no new ones while you practice).
        if let Some(o) = &mut self.owl {
            let drop = o.step(dt, w);
            if let Some(at) = drop.filter(|_| !self.practicing && self.snowballs.len() < owl::MAX_SNOWBALLS) {
                self.snowballs.push(owl::Snowball::new(at, o.dir));
            }
            if o.gone(w) {
                self.owl = None;
            }
        }
        let mut i = 0;
        while i < self.snowballs.len() {
            let b = &mut self.snowballs[i];
            b.step(dt);
            let (x, y) = (b.x, b.y);
            if y >= self.feet_y(x) - 1.0 || !(-4.0..w + 4.0).contains(&x) {
                self.snowballs.swap_remove(i);
                self.shatter_small(x, y);
            } else {
                i += 1;
            }
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
            self.snow.hollow(fire.x, 14.0, 3.0 * fire.strength(), dt);
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
            self.warrior.say(T::WarriorFire.get(self.native), 3.0);
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
        let can = self.lift_warrior.is_none() && self.warrior.act == warrior::Act::Wander;
        let slid = self.slide(can, self.warrior.x, warrior::WIDTH, self.warrior.dir, 4.0, dt);
        if let Some(x) = slid {
            self.warrior.x = x;
            if !self.warrior.sliding && self.warrior.bubble.is_none() && self.mage.bubble.is_none() {
                self.warrior.say(T::WarriorSlide.get(self.native), 2.0);
            }
        }
        self.warrior.sliding = slid.is_some();
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
                self.warrior.say(T::WarriorCold.get(self.native), 1.2);
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

        // Ambient: past the ice line, corner frost creeps faster the more
        // buried the screen is.
        self.update_ice_line();
        if self.edges_freeze() {
            self.frost.creep(dt, 0.02 + self.pile_level() * 0.25);
        }
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

    /// At the ice line the pile stops rising and the screen freezes instead.
    fn update_ice_line(&mut self) {
        self.frozen_over = self.edges_freeze();
        self.snow.set_frozen(self.frozen_over);
        self.frost.set_pile(self.feet_y(0.0), self.feet_y(self.w as f32 - 1.0));
    }

    /// Weather-only step, in the same two stages as a full scene: the pile
    /// rises toward its share of `target` (freeze level = 0.6 × pile + 0.8 ×
    /// edges), freezes at the ice line, and only then do the edges take their
    /// share. Snowfall is as thick as `target` says.
    fn step_weather(&mut self, dt: f32, target: f32) {
        self.time += dt;
        // Paused on the main monitor: this screen's snow is in the black hole too.
        self.vortex = self.vortex.step(dt);
        if self.vortex != vortex::Phase::Open {
            return;
        }
        let k = (WEATHER_FOLLOW * dt).min(1.0);
        let (w, h) = (self.w as f32, self.h as f32);
        // A little past the line, so a pile meant to be at it gets there.
        let (pile_want, edges_want) = ((target / 0.6).min(1.02), ((target - 0.6) / 0.8).max(0.0));
        let pile = self.pile_level();
        if pile < pile_want {
            let x = self.rng.range(0.0, w);
            let total = (pile_want - pile) * k * w * h * ICE_LINE;
            self.snow.add(x, total / (CUBE_SPREAD * 1.77), CUBE_SPREAD);
        } else if pile > 0.0 {
            self.snow.melt((1.0 - pile_want / pile) * k);
        }
        self.update_ice_line();
        let cover = self.frost.coverage();
        if self.frozen_over && cover < edges_want {
            let edge = *self.rng.pick(&[Edge::Top, Edge::Left, Edge::Right]);
            let at = if edge == Edge::Top { self.rng.range(0.0, w) } else { self.rng.range(0.0, h) };
            let strength = self.frost_strength() * (edges_want - cover) / edges_want * WEATHER_FROST * dt;
            self.frost.burst(edge, at, strength, h * 0.15);
        } else if cover > edges_want {
            self.frost.melt((1.0 - edges_want / cover) * k);
        }
        self.snow.settle();
        self.frost.settle();
        self.fall_flakes(dt, snowfall(self.flakes.len(), target));
    }

    /// Moves the snowfall with `target` flakes of the pool in the air. New
    /// ones enter at the top; surplus ones stop only once they land, so the
    /// snowfall thickens and thins without flakes popping in or out mid-air.
    /// Every flake that lands — on the ground pile, or (past the ice line) on
    /// the side wall the wind blew it into, then sliding down to what holds
    /// it — stays as one pixel of snow (one height per column/row, so
    /// thousands of grains cost nothing). Returns how many settled.
    fn fall_flakes(&mut self, dt: f32, target: usize) -> usize {
        let w = self.w as f32;
        let (transparent, time, h) = (self.transparent, self.time, self.h);
        let gust = wind(time);
        let walls_hold = self.edges_freeze();
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
            // A wall holds a flake only past the ice line, on what is under
            // it; otherwise the flake slides down the glass, or the wall's
            // snow, until something does or it reaches the pile.
            let wall = match wall {
                Some(edge) if walls_hold && frost.holds(edge, f.y) => Some(edge),
                Some(edge) => {
                    let depth = frost.depth_at(edge, f.y);
                    f.x = if edge == Edge::Left { depth } else { w - 1.0 - depth }.clamp(0.0, w - 1.0);
                    None
                }
                None => None,
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

    /// Where someone at `x` heading `dir` ends up this frame if the pile ahead
    /// is steep enough to slide (`slide::steep`), kicking up a little snow;
    /// None when they just walk. `margin` keeps them on screen.
    fn slide(&mut self, can: bool, x: f32, width: i32, dir: f32, margin: f32, dt: f32) -> Option<f32> {
        let feet = x + width as f32 / 2.0;
        if !can || !slide::steep(&self.snow, feet, dir, self.h as f32 * ICE_LINE) {
            return None;
        }
        let to = (x + dir * slide::SPEED * dt).clamp(margin, self.w as f32 - width as f32 - margin);
        if self.rng.chance(0.5) {
            let (vx, vy) = (-dir * self.rng.range(10.0, 30.0), self.rng.range(-30.0, -10.0));
            let y = self.feet_y(feet);
            self.particles.push(Particle::new(Kind::Shard, feet - dir * 4.0, y - 1.0, vx, vy, 0.5, hex(0xffffff)));
        }
        Some(to)
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
        self.snow.add(x, self.scaled(self.pace.snow_per_cube), self.scaled(CUBE_SPREAD));
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
            self.warrior.say(T::WarriorIce.get(self.native), 1.5);
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
        self.warrior.say(T::WarriorMobs.get(self.native), 2.5);
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
                self.warrior.say(T::WarriorHit.get(self.native), 1.0);
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
        if self.mage.busy() || (skill == Skill::Owl && !self.owl_ready()) {
            return;
        }
        self.skill = skill;
        self.mage.start_summon();
        let line = match skill {
            Skill::Friend => T::MageFriends,
            Skill::IcicleRain => T::MageIcicles,
            Skill::Owl => T::MageOwl,
        }
        .get(self.native);
        self.mage.say(line, 2.0);
    }

    /// The owl may come: deep snow, the pile not frozen over yet (its
    /// snowballs could add nothing) and none flying yet.
    fn owl_ready(&self) -> bool {
        self.owl.is_none() && !self.frozen_over && self.pile_level() >= owl::PILE_LEVEL
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
        self.snow.add(x, self.scaled(self.pace.snow_per_cube * 0.5), self.scaled(4.0));
        for i in 0..5 {
            let (vx, vy, life) = (self.rng.range(-40.0, 40.0), self.rng.range(-60.0, -15.0), self.rng.range(0.4, 0.9));
            self.particles.push(Particle::new(Kind::Shard, x, y - 1.0, vx, vy, life, hex(ICE_COLORS[i % 4])));
        }
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        if (wx - x).abs() < 6.0 && self.warrior.act != warrior::Act::Frozen {
            self.warrior.warmth = (self.warrior.warmth - 0.05).max(0.01);
            self.warrior.say(T::WarriorIcicle.get(self.native), 1.2);
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
        let other = *self.rng.pick(&[Edge::Top, Edge::Left, Edge::Right]);
        let at = if other == Edge::Top { self.rng.range(0.0, w) } else { self.rng.range(0.0, h * 0.8) };
        if self.edges_freeze() {
            self.frost.burst(near, pos(near), strength, h * 0.15);
            self.frost.burst(other, at, strength * 0.7, h * 0.15);
        }
        self.snow.add(x, self.scaled(self.pace.snow_per_cube * 1.5), self.scaled(8.0));
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
        if self.weather.is_some() {
            if self.vortex == vortex::Phase::Open {
                self.draw_weather(c);
            } else {
                // Sucked toward the black hole on the main monitor (`hole` may
                // be off this screen); the hole itself is drawn over there.
                let (k, swirl, _) = self.vortex.params();
                let mut frames = self.warp_frames.borrow_mut();
                let (world, warped) = &mut *frames;
                if (world.w, world.h) != (c.w, c.h) {
                    (*world, *warped) = (Canvas::new(c.w, c.h), Canvas::new(c.w, c.h));
                }
                self.draw_weather(world);
                vortex::warp(world, warped, self.hole, k, swirl);
                c.clear(CLEAR);
                c.blit(warped, 0, 0);
            }
            self.clear_kept(c);
            return;
        }
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
            let mut frames = self.warp_frames.borrow_mut();
            let (world, warped) = &mut *frames;
            if (world.w, world.h) != (c.w, c.h) {
                (*world, *warped) = (Canvas::new(c.w, c.h), Canvas::new(c.w, c.h));
            }
            world.clear(CLEAR);
            self.draw_world(world);
            if tremble > 0 && k >= 1.0 {
                let t = (self.time * 40.0) as i32;
                c.blit(world, (t % 3 - 1) * tremble, ((t / 3) % 3 - 1) * tremble);
            } else {
                vortex::warp(world, warped, self.hole, k, swirl);
                c.blit(warped, 0, 0);
            }
            let size = 4.0 + (1.0 - k) * 12.0;
            if !(self.transparent && self.vortex == vortex::Phase::Closed) {
                vortex::draw_hole(c, self.hole, size, self.time);
            }
        }
        self.hud.draw(c, gy as i32, self.time);
        // The orb is on top of everything: snow, characters, bubbles, HUD.
        if self.show_orb && self.vortex == vortex::Phase::Open {
            vortex::draw_orb(c, self.hole, ORB_R, self.time, false);
        } else if self.show_orb && self.vortex == vortex::Phase::Closed {
            vortex::draw_orb(c, self.hole, ORB_R, self.time, true);
        } else if self.transparent && !self.show_orb {
            // Over the desktop the orb is its own window; leave its spot (glow
            // included) clear, so nothing here paints over it however deep the snow.
            clear_disc(c, self.hole, self.orb_r * 2.0);
        }
        self.clear_kept(c);
        if let Some((hx, hy)) = self.hand {
            hand::draw_hand(c, hx, hy, self.grabbed.is_some(), self.time);
        }
    }

    /// Weather only: the landscape (window mode), the glass frost, the blanket
    /// and the flakes.
    fn draw_weather(&self, c: &mut Canvas) {
        match &self.backdrop {
            Some(b) => {
                b.draw_sky(c, self.time);
                b.draw_land(c);
            }
            None => c.clear(CLEAR),
        }
        self.glass.draw(c, (self.frost.coverage() / GLASS_FULL_AT).min(1.0));
        self.blanket.borrow_mut().draw(c, &self.snow, &self.frost, self.ground_y() as i32, self.time, self.transparent);
        for f in self.flakes.iter().filter(|f| f.falling) {
            c.dot(f.x, f.y, if f.speed > 13.0 { hex(0xffffff) } else { hex(0xc9d0f2) });
        }
    }

    fn clear_kept(&self, c: &mut Canvas) {
        if let (true, Some((x, y, w, h))) = (self.transparent, self.keep_clear) {
            c.rect(x, y, w, h, CLEAR);
        }
    }

    /// Everything that lives in the world (not the landscape, not the HUD).
    fn draw_world(&self, c: &mut Canvas) {
        let gy = self.ground_y();
        // The glass frosts over as the edges freeze; the snow covers its roots.
        self.glass.draw(c, (self.frost.coverage() / GLASS_FULL_AT).min(1.0));
        // Ground pile and edge snow as one blanket; scenery, so everyone and
        // everything they say stays in front of it.
        self.blanket.borrow_mut().draw(c, &self.snow, &self.frost, gy as i32, self.time, self.transparent);
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
            let (r, x, y) = (fb.spell.radius(), fb.x.round() as i32, fb.y.round() as i32);
            c.glow(fb.x, fb.y, 3.0 * r as f32, 0.8, hex(0xff7a2a));
            c.rect(x - r, y - r, 2 * r, 2 * r, hex(0xffc13d));
            c.rect(x - r / 2, y - r / 2, r.max(1), r.max(1), hex(0xfff4b0));
        }
        for cube in &self.cubes {
            cube.draw(c);
        }
        for ic in &self.icicles {
            ic.draw(c, self.time);
        }
        if let Some(o) = &self.owl {
            o.draw(c);
        }
        for b in &self.snowballs {
            b.draw(c);
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

/// Share of the gap to the target level a weather-only screen closes per second.
const WEATHER_FOLLOW: f32 = 0.5;
/// How hard a weather-only screen's walls frost over while behind the target.
const WEATHER_FROST: f32 = 30.0;

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

/// Once the ground pile's mean height reaches this share of the screen, the
/// edges start to freeze; before that, snow never sticks to them.
pub const ICE_LINE: f32 = 0.30;

/// Edge-snow coverage at which the glass is fully frosted over.
const GLASS_FULL_AT: f32 = 0.6;

/// Pace amounts are pixels on a 270-px-tall scene (1080p at the default pixel
/// scale); scaled by this, the pile rises the same share of any screen.
const PACE_HEIGHT: f32 = 270.0;

fn clear_disc(c: &mut Canvas, (cx, cy): (f32, f32), r: f32) {
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            if (x as f32 - cx).powi(2) + (y as f32 - cy).powi(2) <= r * r {
                c.set(x, y, CLEAR);
            }
        }
    }
}

/// Peaks may rise well past the ice line; it is the mean that counts.
fn snow_cap(h: i32) -> f32 {
    (h as f32 * 0.60).max(8.0)
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
        bury_to_the_line(&mut s);
        run(&mut s, 150.0);
        let (snow, frost) = (s.snow.fill(), s.frost.coverage());
        assert!(frost > 0.0, "friends should have frosted the edges");
        s.celebrate(1.0, Spell::Fireball);
        let melt = Commitment::Relentless.pace().melt_fraction;
        assert!(s.snow.fill() <= snow * (1.0 - melt) + 1e-4);
        assert!(s.frost.coverage() <= frost * (1.0 - melt) + 1e-4);
        assert!(s.snow.fill() > 0.0, "one answer never clears everything");
    }

    #[test]
    fn a_bigger_spell_melts_more_but_never_clears_the_screen() {
        let melted = |spell: Spell| {
            let mut s = Scene::new(240, 135, 2, Commitment::Relentless.pace(), true);
            run(&mut s, 150.0);
            let before = s.snow.fill();
            s.celebrate(1.5, spell); // the most an answer earns: recall on the first try
            assert_eq!(s.last_spell(), Some(spell));
            assert!(s.snow.fill() > 0.0, "{spell:?} cleared everything");
            before - s.snow.fill()
        };
        let (spark, fireball, blaze) = (melted(Spell::Spark), melted(Spell::Fireball), melted(Spell::Blaze));
        assert!(spark > 0.0 && spark < fireball && fireball < blaze, "{spark} {fireball} {blaze}");
    }

    #[test]
    fn during_a_lesson_the_fire_mage_melts_on_impact_not_instantly() {
        let mut s = Scene::new(240, 135, 2, Commitment::Relentless.pace(), true);
        run(&mut s, 150.0);
        s.set_practicing(true);
        run(&mut s, 1.0);
        assert!(s.pyro.visible());
        let before = s.snow.fill();
        s.celebrate(1.0, Spell::Fireball);
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
        assert!(crate::lang::Lines::MageHeld.get(Native::PtBr).contains(&s.mage_bubble().unwrap()), "pissy");
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
        assert!(
            crate::lang::Lines::WarriorHeld
                .get(Native::PtBr)
                .contains(&s.warrior.bubble.as_ref().unwrap().text.as_str())
        );
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

    /// Fills the ground pile up to the 30% line, where the edges start to freeze.
    fn bury_to_the_line(s: &mut Scene) {
        let line = s.h as f32 * ICE_LINE;
        for x in 0..s.w {
            while s.snow.height_at(x as f32) < line && s.snow.add_grain(x as f32) {}
        }
    }

    #[test]
    fn past_the_line_the_pile_stops_rising_and_the_screen_freezes_instead() {
        let mut s = Scene::new(480, 270, 4, Commitment::Relentless.pace(), true);
        bury_to_the_line(&mut s);
        let line = s.h as f32 * ICE_LINE;
        run(&mut s, 600.0);
        assert!(s.snow.mean() <= line + 1.0, "the pile stays at the line: mean {} vs {line}", s.snow.mean());
        assert!(s.frost.coverage() > 0.2, "the edges took the snow: {}", s.frost.coverage());
    }

    #[test]
    fn the_edges_stay_clear_until_the_pile_reaches_the_line() {
        let mut s = Scene::new(480, 270, 9, Commitment::Relentless.pace(), true);
        for _ in 0..(20 * 60 * 10) {
            s.step(0.1);
            if s.frozen_over {
                break;
            }
            assert_eq!(s.frost.coverage(), 0.0, "edge snow at {:.2} of the line, before it", s.pile_level());
        }
        bury_to_the_line(&mut s);
        run(&mut s, 120.0);
        assert!(s.frost.coverage() > 0.0, "past the line, the edges freeze");
    }

    #[test]
    fn a_flake_that_hits_a_wall_before_the_line_slides_down_it_onto_the_pile() {
        let mut s = Scene::new(480, 270, 9, Commitment::Chill.pace(), true);
        let all = s.flakes.len();
        let before = pile(&s);
        let mut landed = 0;
        for _ in 0..(WIND_PERIOD_S as i32 * 2 * 30) {
            landed += s.fall_flakes(1.0 / 30.0, all);
            s.time += 1.0 / 30.0;
        }
        assert_eq!(walls(&s), 0.0, "nothing sticks to the walls yet");
        assert!((pile(&s) - before - landed as f32).abs() < 0.5, "every flake ended on the pile");
        let at_walls = |x: f32| s.snow.height_at(x);
        assert!(at_walls(0.0) > 0.0 && at_walls(s.w as f32 - 1.0) > 0.0, "the ones that slid down a wall too");
    }

    #[test]
    fn past_the_line_the_snow_climbs_both_walls_from_the_pile() {
        let mut s = Scene::new(480, 270, 9, Commitment::Chill.pace(), true);
        bury_to_the_line(&mut s);
        run(&mut s, WIND_PERIOD_S * 4.0);
        let line = s.feet_y(0.0) as i32;
        for e in [Edge::Left, Edge::Right] {
            let at = |y: i32| s.frost.depth_at(e, y as f32);
            assert!(at(line - 1) >= 1.0, "{e:?} wall snow starts on the pile");
            assert!((0..line - 1).all(|y| at(y) <= at(y + 1) + 1.0), "{e:?} wall snow rests on what is under it");
        }
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

    #[test]
    fn peaks_may_rise_past_the_ice_line_it_is_the_mean_that_counts() {
        let mut s = Scene::new(480, 270, 3, Commitment::Chill.pace(), true);
        let line = s.h as f32 * ICE_LINE;
        while s.snow.add_grain(240.0) {}
        assert!(s.snow.height_at(240.0) > line + 10.0, "a peak climbs past the line: {}", s.snow.height_at(240.0));
        assert!(!s.edges_freeze(), "one peak is not the pile at the line");
        for x in 0..s.w {
            while s.snow.height_at(x as f32) < line {
                s.snow.add_grain(x as f32);
            }
        }
        assert!(s.edges_freeze(), "the whole pile at the line is");
    }

    #[test]
    fn an_ice_cube_buries_the_same_share_of_the_screen_at_any_pixel_scale() {
        let share = |w: i32, h: i32| {
            let mut s = Scene::new(w, h, 3, Commitment::Steady.pace(), true);
            s.shatter(w as f32 / 2.0, h as f32 / 2.0);
            (0..w).map(|x| s.snow.height_at(x as f32)).sum::<f32>() / (w * h) as f32
        };
        let (small, big) = (share(480, 270), share(960, 540));
        assert!((big / small - 1.0).abs() < 0.05, "480x270 {small:.5} vs 960x540 {big:.5}");
    }

    /// Buries every edge and the ground as deep as they go.
    fn bury(s: &mut Scene) {
        s.snow.dust(1000.0);
        for y in 0..s.h {
            while s.frost.add_grain(Edge::Left, y as f32) {}
            while s.frost.add_grain(Edge::Right, y as f32) {}
        }
        for x in 0..s.w {
            while s.frost.add_grain(Edge::Top, x as f32) {}
        }
    }

    #[test]
    fn the_in_scene_orb_stays_on_top_of_deep_snow_and_the_hud() {
        let mut s = Scene::new(120, 90, 16, Commitment::Steady.pace(), false);
        s.show_orb = true;
        s.hole = (10.0, 10.0);
        bury(&mut s);
        s.hud.toast("Snowlearner · Constante · ajuda/painel: H", 5.0);
        let mut c = Canvas::new(120, 90);
        s.draw(&mut c);
        let mut orb = Canvas::new(120, 90);
        vortex::draw_orb(&mut orb, s.hole, ORB_R, s.time, false);
        let (mut drawn, mut covered) = (0, 0);
        for y in 0..20 {
            for x in 0..20 {
                if let Some(p) = orb.get(x, y).filter(|p| p[3] != 0) {
                    drawn += 1;
                    covered += usize::from(c.get(x, y) != Some(p));
                }
            }
        }
        assert!(drawn > 100, "orb pixels: {drawn}");
        assert_eq!(covered, 0, "{covered} of {drawn} orb pixels hidden");
        assert_eq!(s.poke(10.0, 10.0), Poke::Orb, "still clickable under the snow");
    }

    #[test]
    fn over_the_desktop_deep_snow_leaves_the_panel_window_uncovered() {
        let mut s = Scene::new(240, 135, 17, Commitment::Steady.pace(), true);
        bury(&mut s);
        s.warrior.say("Uma frase bem comprida para cobrir o painel", 9.0);
        s.keep_clear = Some((150, 60, 80, 70)); // the panel window, in scene pixels
        let mut c = Canvas::new(240, 135);
        s.draw(&mut c);
        assert_eq!(c.opaque_in(150, 60, 80, 70), 0, "snow painted over the panel window");
        assert!(c.opaque_in(0, 100, 140, 35) > 0, "the rest of the screen still has its snow");
        let mut w = Scene::weather(240, 135, 17, Commitment::Steady.pace(), true);
        w.set_freeze_target(1.0);
        w.keep_clear = Some((150, 60, 80, 70));
        for _ in 0..(60 * 30) {
            w.step(1.0 / 30.0);
        }
        let mut cw = Canvas::new(240, 135);
        w.draw(&mut cw);
        assert_eq!(cw.opaque_in(150, 60, 80, 70), 0, "the panel on another monitor stays clear too");
        s.keep_clear = Some((-20, 120, 80, 80)); // partly off screen
        s.draw(&mut c);
    }

    #[test]
    fn over_the_desktop_deep_snow_leaves_the_orb_window_uncovered_and_clickable() {
        let mut s = Scene::new(240, 135, 17, Commitment::Steady.pace(), true);
        s.hole = (226.0, 14.0); // the orb window's default corner
        bury(&mut s);
        s.warrior.x = 220.0;
        s.warrior.say("Uma frase bem comprida para cobrir o canto da tela", 9.0);
        let mut c = Canvas::new(240, 135);
        s.draw(&mut c);
        let r = (s.orb_r * 2.0) as i32;
        let (hx, hy) = (s.hole.0 as i32, s.hole.1 as i32);
        let mut over = 0;
        for y in hy - r..=hy + r {
            for x in hx - r..=hx + r {
                if ((x - hx).pow(2) + (y - hy).pow(2)) <= r * r {
                    over += c.opaque_in(x, y, 1, 1);
                }
            }
        }
        assert_eq!(over, 0, "{over} scene pixels painted over the orb window");
        assert!(s.orb_at(226.0, 14.0), "the orb's spot answers clicks in overlay mode too");
        assert!(!s.orb_at(120.0, 60.0));
    }

    /// Steps 1 s; returns (ever sliding, slide lines said, how far it moved).
    fn ride(s: &mut Scene, mage: bool) -> (bool, usize, f32) {
        let x0 = if mage { s.mage.x } else { s.warrior.x };
        let (mut slid, mut lines) = (false, 0);
        for _ in 0..30 {
            s.step(1.0 / 30.0);
            let (sliding, bubble, line) = if mage {
                (s.mage.sliding, &mut s.mage.bubble, T::MageSlide.get(Native::PtBr))
            } else {
                (s.warrior.sliding, &mut s.warrior.bubble, T::WarriorSlide.get(Native::PtBr))
            };
            slid |= sliding;
            if bubble.as_ref().is_some_and(|b| b.text == line) {
                lines += 1;
                *bubble = None; // a second shout would show up again
            }
        }
        let x1 = if mage { s.mage.x } else { s.warrior.x };
        (slid, lines, (x1 - x0).abs())
    }

    fn on_a_peak(seed: u64, mage: bool, slope: f32) -> Scene {
        let mut s = Scene::new(240, 135, seed, Commitment::Steady.pace(), true);
        let at = if mage { s.mage.x + mage::WIDTH as f32 / 2.0 } else { s.warrior.x + warrior::WIDTH as f32 / 2.0 };
        s.snow = slide::peak(240, s.snow.cap(), at, slope, 28.0);
        s
    }

    #[test]
    fn the_warrior_slides_down_a_steep_pile_and_shouts_once() {
        let (slid, lines, moved) = ride(&mut on_a_peak(18, false, 0.8), false);
        assert!(slid, "slides instead of walking");
        assert_eq!(lines, 1, "one shout per slide");
        assert!(moved > 12.0, "faster than his walk: {moved}");
    }

    #[test]
    fn the_warrior_walks_down_a_gentle_pile() {
        let (slid, lines, moved) = ride(&mut on_a_peak(18, false, 0.25), false);
        assert!(!slid);
        assert_eq!(lines, 0);
        assert!(moved < 8.0, "walking pace: {moved}");
    }

    #[test]
    fn the_frost_mage_slides_down_a_steep_pile_and_shouts_once() {
        let (slid, lines, moved) = ride(&mut on_a_peak(19, true, 0.8), true);
        assert!(slid);
        assert_eq!(lines, 1);
        assert!(moved > 12.0, "{moved}");
    }

    #[test]
    fn the_frost_mage_walks_down_a_gentle_pile() {
        let (slid, lines, _) = ride(&mut on_a_peak(19, true, 0.25), true);
        assert!(!slid);
        assert_eq!(lines, 0);
    }

    #[test]
    fn a_slide_never_talks_over_what_they_are_already_saying() {
        let mut s = on_a_peak(18, false, 0.8);
        s.warrior.say("Dica importante", 9.0);
        let (slid, lines, _) = ride(&mut s, false);
        assert!(slid);
        assert_eq!(lines, 0);
        assert_eq!(s.warrior.bubble.as_ref().unwrap().text, "Dica importante");
    }

    #[test]
    fn a_slide_shout_waits_while_the_other_one_is_talking() {
        let mut s = on_a_peak(18, false, 0.8);
        s.mage.say("Vou congelar tudo!", 9.0);
        let (slid, lines, _) = ride(&mut s, false);
        assert!(slid);
        assert_eq!(lines, 0, "two bubbles at once overlap on screen");
    }

    /// A scene whose pile is `level` of the way to the ice line, owl summon due now.
    fn snowed(seed: u64, level: f32) -> Scene {
        let mut s = Scene::new(240, 135, seed, Commitment::Chill.pace(), true);
        s.snow.dust(s.h as f32 * ICE_LINE * level);
        s.next_owl = 0.0;
        s
    }

    #[test]
    fn the_ice_owl_never_comes_below_the_snow_threshold() {
        let mut s = snowed(20, owl::PILE_LEVEL * 0.5);
        s.cast_skill(Skill::Owl);
        assert_ne!(s.mage_bubble(), Some(T::MageOwl.get(Native::PtBr)), "no summon on demand either");
        for _ in 0..(20 * 30) {
            let level = s.pile_level();
            let had = s.owl.is_some();
            s.step(1.0 / 30.0);
            if s.owl.is_some() && !had {
                assert!(level >= owl::PILE_LEVEL, "owl came at pile level {level}");
            }
        }
        assert!(s.owl.is_none(), "the pile stayed below the threshold: {}", s.pile_level());
    }

    #[test]
    fn above_the_threshold_the_mage_summons_one_ice_owl_at_a_time_and_it_leaves() {
        let mut s = snowed(21, 0.8);
        let (mut seen, mut said, mut max_balls) = (false, false, 0);
        for _ in 0..(5 * 30) {
            s.step(1.0 / 30.0);
            said |= s.mage_bubble() == Some(T::MageOwl.get(Native::PtBr));
            seen |= s.owl.is_some();
            max_balls = max_balls.max(s.snowballs.len());
            if s.owl.is_some() {
                break;
            }
        }
        assert!(said && seen, "the mage summons the owl");
        assert!(!s.mage.busy());
        s.cast_skill(Skill::Owl);
        assert_eq!(s.mage.act, mage::Act::Walk, "no second owl while one flies");
        let mut left = false;
        for _ in 0..(12 * 30) {
            s.step(1.0 / 30.0);
            max_balls = max_balls.max(s.snowballs.len());
            if s.owl.is_none() {
                left = true;
                break;
            }
        }
        assert!(left, "the owl leaves the screen");
        assert!(max_balls <= owl::MAX_SNOWBALLS, "{max_balls} snowballs at once");
        assert!(s.next_owl > owl::EVERY * 0.5, "then a cooldown: {}", s.next_owl);
    }

    #[test]
    fn the_owls_snowballs_raise_the_pile() {
        let mut with = snowed(22, 0.8);
        let mut without = snowed(22, 0.8);
        for s in [&mut with, &mut without] {
            s.stun_t = 1000.0; // the mage himself throws nothing
        }
        with.owl = Some(owl::Owl::new(true, 240.0, 20.0));
        let mut dropped = false;
        for _ in 0..(10 * 30) {
            with.step(1.0 / 30.0);
            without.step(1.0 / 30.0);
            dropped |= !with.snowballs.is_empty();
        }
        assert!(dropped);
        assert!(with.owl.is_none(), "crossed and left");
        let gain = (with.snow.fill() - without.snow.fill()) * with.snow.width() as f32 * with.snow.cap();
        assert!(gain > 5.0, "snowballs added {gain} px of snow");
    }

    #[test]
    fn a_lesson_stops_the_owls_snowballs() {
        let mut s = snowed(23, 0.8);
        s.owl = Some(owl::Owl::new(true, 240.0, 20.0));
        s.set_practicing(true);
        for _ in 0..(3 * 30) {
            s.step(1.0 / 30.0);
            assert!(s.snowballs.is_empty());
        }
    }

    #[test]
    fn a_frozen_pile_gets_no_owl_and_no_snow_from_its_snowballs() {
        let mut s = snowed(24, 1.0);
        s.stun_t = 1000.0;
        s.step(1.0 / 30.0);
        assert!(s.frozen_over, "the pile is at the ice line");
        s.cast_skill(Skill::Owl);
        assert_ne!(s.mage_bubble(), Some(T::MageOwl.get(Native::PtBr)), "no owl over a frozen pile");
        s.owl = Some(owl::Owl::new(true, 240.0, 20.0));
        let before = s.snow.fill();
        let mut dropped = false;
        for _ in 0..(10 * 30) {
            s.step(1.0 / 30.0);
            dropped |= !s.snowballs.is_empty();
        }
        assert!(dropped);
        assert!(s.snow.fill() <= before + 1e-6, "snowballs just burst on the frozen pile");
    }

    // ---- weather-only scenes (the other monitors in overlay mode) ----

    #[test]
    fn pausing_sucks_the_snow_of_a_weather_screen_into_the_black_hole_and_back() {
        let mut s = Scene::weather(240, 135, 5, Commitment::Steady.pace(), true);
        s.set_freeze_target(0.5);
        let run = |s: &mut Scene, secs: f32| {
            for _ in 0..(secs * 30.0) as i32 {
                s.step(1.0 / 30.0);
            }
        };
        let snow = |s: &Scene| {
            let mut c = Canvas::new(240, 135);
            s.draw(&mut c);
            c.opaque_in(0, 0, 240, 135)
        };
        run(&mut s, 60.0);
        let before = snow(&s);
        assert!(before > 1000, "a pile to suck in: {before}");
        s.hole = (-200.0, 20.0); // the black hole sits on the main monitor, to the left
        s.set_paused(true);
        run(&mut s, 4.0);
        assert!(snow(&s) < before / 10, "paused: the snow is gone into the hole ({} of {before})", snow(&s));
        s.set_paused(false);
        run(&mut s, 4.0);
        assert!(snow(&s) > before / 2, "back after the pause: {} of {before}", snow(&s));
    }

    #[test]
    fn a_weather_screen_never_spawns_actors_whatever_the_freeze() {
        let mut s = Scene::weather(320, 180, 13, Commitment::Relentless.pace(), true);
        s.set_freeze_target(0.9);
        run(&mut s, 240.0);
        assert_eq!(s.mobs_out(), 0, "no mob waves");
        assert_eq!(s.friends_out(), 0, "no summoned friends");
        assert_eq!(s.icicles_falling(), 0, "no icicle rain");
        assert_eq!(s.cubes_in_flight(), 0, "no ice cubes thrown");
        assert!(s.fires.is_empty(), "no warrior lighting fires");
        assert!(!s.pyro.visible(), "no fire mage");
    }

    #[test]
    fn a_weather_screen_allocates_nothing_for_actors_effects_or_hud() {
        // Secondary monitors must stay cheap: after minutes of heavy weather the
        // only heap a weather screen holds is the flakes, pile, walls and blanket.
        let mut s = Scene::weather(640, 360, 18, Commitment::Relentless.pace(), true);
        s.set_freeze_target(1.0);
        run(&mut s, 120.0);
        let mut c = Canvas::new(640, 360);
        s.draw(&mut c);
        let heaps = [
            ("cubes", s.cubes.capacity()),
            ("particles", s.particles.capacity()),
            ("friends", s.friends.capacity()),
            ("fires", s.fires.capacity()),
            ("fireballs", s.fireballs.capacity()),
            ("pending_power", s.pending_power.capacity()),
            ("icicles", s.icicles.capacity()),
            ("mobs", s.mobs.capacity()),
            ("tips", s.tips.capacity()),
        ];
        for (what, cap) in heaps {
            assert_eq!(cap, 0, "{what} allocated on a weather screen");
        }
        assert!(s.backdrop.is_none(), "an overlay weather screen paints no landscape");
        let hud = &s.hud;
        assert!(hud.caption.is_none() && hud.summary.is_none() && hud.toast.is_none() && hud.stats.is_none());
        assert!(s.mage.bubble.is_none() && s.warrior.bubble.is_none(), "nobody talks");
    }

    #[test]
    fn a_weather_screen_draws_only_weather_no_characters_or_hud() {
        let mut s = Scene::weather(320, 180, 14, Commitment::Steady.pace(), true);
        s.hud.toast("Snowlearner · não deve aparecer", 5.0);
        s.mage.say("Nem eu!", 5.0);
        run(&mut s, 1.0);
        let mut c = Canvas::new(320, 180);
        s.draw(&mut c);
        let lit = c.opaque_in(0, 0, 320, 180);
        // Flakes in the air plus the odd grain that settled before melting away.
        assert!(lit <= falling(&s) + 40, "{lit} opaque pixels but only {} flakes in the air", falling(&s));
        // The same world as a full scene shows the mage, the warrior and the toast.
        let mut full = Scene::new(320, 180, 14, Commitment::Steady.pace(), true);
        full.hud.toast("Snowlearner · não deve aparecer", 5.0);
        run(&mut full, 1.0);
        let mut f = Canvas::new(320, 180);
        full.draw(&mut f);
        assert!(f.opaque_in(0, 0, 320, 180) > lit + 200, "a full scene draws much more");
    }

    #[test]
    fn a_weather_screen_follows_the_freeze_level_set_from_outside_up_and_down() {
        let mut s = Scene::weather(320, 180, 15, Commitment::Steady.pace(), true);
        assert!(s.freeze_level() < 0.01, "starts clear");
        s.set_freeze_target(0.9);
        run(&mut s, 90.0);
        let up = s.freeze_level();
        assert!((up - 0.9).abs() < 0.08, "climbed to the primary's level: {up}");
        assert!(s.frozen_over && s.frost.coverage() > 0.2, "pile at the line, edges frozen");
        s.set_freeze_target(0.1);
        run(&mut s, 20.0);
        let down = s.freeze_level();
        assert!((down - 0.1).abs() < 0.06, "melted with the primary: {down}");
        assert!(!s.frozen_over, "the pile thawed below the line");
    }

    #[test]
    fn a_weather_screen_freezes_in_two_stages_pile_first_then_the_edges() {
        let mut s = Scene::weather(320, 180, 19, Commitment::Steady.pace(), true);
        s.set_freeze_target(0.45);
        for _ in 0..(60 * 30) {
            s.step(1.0 / 30.0);
            assert_eq!(s.frost.coverage(), 0.0, "edge snow before the pile reached the line");
        }
        assert!(!s.frozen_over);
        assert!((s.pile_level() - 0.75).abs() < 0.08, "the pile alone carries the level: {}", s.pile_level());
        s.set_freeze_target(0.8);
        run(&mut s, 60.0);
        assert!(s.frozen_over, "the pile reached the line");
        let line = s.h as f32 * ICE_LINE;
        assert!(s.snow.mean() <= line * 1.05, "and stopped there: mean {} vs {line}", s.snow.mean());
        assert!(s.frost.coverage() > 0.1, "then the edges froze: {}", s.frost.coverage());
    }

    #[test]
    fn a_weather_screen_snows_as_hard_as_the_primary_level_says() {
        let mut s = Scene::weather(320, 180, 16, Commitment::Steady.pace(), true);
        s.set_freeze_target(0.0);
        run(&mut s, 10.0);
        let light = falling(&s);
        s.set_freeze_target(1.0);
        run(&mut s, 30.0);
        assert!(falling(&s) > light * 5, "heavy snowfall when buried: {} vs {light}", falling(&s));
    }

    #[test]
    fn the_freeze_target_is_ignored_by_a_full_scene_and_clamped_on_a_weather_one() {
        let mut full = Scene::new(240, 135, 17, Commitment::Steady.pace(), true);
        full.set_freeze_target(1.0);
        run(&mut full, 2.0);
        assert!(full.freeze_level() < 0.2, "the primary's level comes from its own mage");
        let mut s = Scene::weather(240, 135, 17, Commitment::Steady.pace(), true);
        s.set_freeze_target(7.0);
        run(&mut s, 60.0);
        assert!(s.freeze_level() <= 1.0);
        s.set_freeze_target(-3.0);
        run(&mut s, 30.0);
        assert!(s.freeze_level() < 0.05, "a negative level means clear: {}", s.freeze_level());
    }
}
