//! The whole winter world as a pure, seeded simulation: `step(dt)` advances it,
//! `draw(canvas)` renders it. No window, audio or clock inside, so it runs the
//! same in the app, in `snowlearner snapshot` and in tests.

pub mod backdrop;
pub mod fire;
pub mod friends;
pub mod frost;
pub mod hud;
pub mod ice;
pub mod mage;
pub mod rng;
pub mod snow;
pub mod warrior;

use crate::config::level::Pace;
use crate::render::canvas::{CLEAR, Canvas, hex};
use backdrop::{Backdrop, GROUND_BAND};
use fire::Fire;
use friends::Friend;
use frost::{Edge, Frost};
use hud::Hud;
use ice::{Cube, Kind, Particle};
use mage::Mage;
use rng::Rng;
use snow::Snow;
use warrior::Warrior;

const ICE_COLORS: [u32; 4] = [0xffffff, 0xc8f4ff, 0x8ad8f5, 0x4ea2d8];
const GRAVITY: f32 = 130.0;

struct Flake {
    x: f32,
    y: f32,
    speed: f32,
    phase: f32,
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
        let flakes = (0..(w * h / 500).max(12))
            .map(|_| Flake {
                x: rng.range(0.0, w as f32),
                y: rng.range(0.0, h as f32),
                speed: rng.range(5.0, 16.0),
                phase: rng.range(0.0, 7.0),
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
            practicing: false,
            throws: 0,
            next_throw: 1.5,
            next_summon: pace.summon_every * 0.5,
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

    /// You are practicing: the mage stops to watch, the warrior cheers you on.
    pub fn set_practicing(&mut self, on: bool) {
        if on && !self.practicing {
            self.warrior.say("Vai lá, você consegue!", 3.0);
        }
        self.practicing = on;
        self.mage.watch(on);
    }

    /// A correct phrase: melt snow and frost, stagger the mage, warm the warrior.
    pub fn celebrate(&mut self) {
        let f = self.pace.melt_fraction;
        let gy = self.ground_y();
        for (x, hgt) in self.snow.melt(f) {
            let y = gy - hgt;
            for _ in 0..2 {
                let (vx, vy, life) =
                    (self.rng.range(-8.0, 8.0), self.rng.range(-35.0, -15.0), self.rng.range(0.6, 1.4));
                self.particles.push(Particle::new(Kind::Steam, x, y, vx, vy, life, hex(0xc9d3e8)));
            }
        }
        self.frost.melt(f);
        for _ in 0..40 {
            let x = self.rng.range(0.0, self.w as f32);
            let (vx, vy, life) = (self.rng.range(-10.0, 10.0), self.rng.range(-70.0, -30.0), self.rng.range(0.6, 1.3));
            let col = hex(*self.rng.pick(&[0xfff4b0, 0xffc13d, 0xff7a2a]));
            self.particles.push(Particle::new(Kind::Ember, x, gy - 2.0, vx, vy, life, col));
        }
        self.mage.stagger();
        self.warrior.warm_burst();
        self.warrior.say("Que calor bom! Valeu!", 3.0);
    }

    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        self.hud.step(dt);
        let (w, h) = (self.w as f32, self.h as f32);

        if !self.practicing {
            self.next_throw -= dt;
            self.next_summon -= dt;
            if self.next_summon <= 0.0 && !self.mage.busy() {
                self.mage.start_summon();
                self.next_summon = self.pace.summon_every * self.rng.range(0.8, 1.2);
            } else if self.next_throw <= 0.0 && !self.mage.busy() {
                self.throws += 1;
                self.mage.start_throw(self.throws % 4 == 0);
                self.next_throw = self.pace.throw_every * self.rng.range(0.8, 1.2);
            }
        }

        // Mage.
        let roll = self.rng.f32();
        match self.mage.step(dt, self.pace.walk_speed, 2.0, w - mage::WIDTH as f32 - 2.0, roll) {
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
        if let Some(warrior::Event::FireLit { x }) =
            self.warrior.step(dt, freeze, near_fire, 4.0, w - warrior::WIDTH as f32 - 4.0, roll)
        {
            self.fires.push(Fire::new(x.clamp(6.0, w - 6.0)));
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

        let ground = self.ground_y();
        let snow = &self.snow;
        for p in &mut self.particles {
            p.step(dt, |x| ground - snow.height_at(x));
        }
        self.particles.retain(Particle::alive);
        self.particles.truncate(4000);

        for f in &mut self.flakes {
            f.y += f.speed * dt;
            f.x += ((self.time * 1.3 + f.phase).sin() * 5.0 - 4.0) * dt;
            if f.y > h {
                f.y = -1.0;
                f.x = (f.phase * 997.0 + self.time * 31.0) % (w + 20.0);
            }
            if f.x < -2.0 {
                f.x = w + 1.0;
            }
        }
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
        self.snow.add(x, self.pace.snow_per_cube, 9.0);
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
            for f in self.flakes.iter().filter(|f| f.speed < 9.0) {
                c.dot(f.x, f.y, hex(0x6e74b8));
            }
            b.draw_land(c);
        } else {
            c.clear(CLEAR);
        }
        let gy = self.ground_y();
        self.snow.draw(c, gy as i32, self.time, self.transparent);
        for fire in &self.fires {
            fire.draw(c, self.feet_y(fire.x), self.time);
        }
        for f in &self.friends {
            f.draw(c, self.feet_y(f.x + f.kind.width() as f32 / 2.0), self.time);
        }
        let wx = self.warrior.x + warrior::WIDTH as f32 / 2.0;
        self.warrior.draw(c, self.feet_y(wx), self.time);
        let mx = self.mage.x + mage::WIDTH as f32 / 2.0;
        self.mage.draw(c, self.feet_y(mx), self.time);
        for cube in &self.cubes {
            cube.draw(c);
        }
        for p in &self.particles {
            p.draw(c);
        }
        self.frost.draw(c);
        let near_flakes = self.flakes.iter().filter(|f| f.speed >= 9.0 || self.backdrop.is_none());
        for f in near_flakes {
            c.dot(f.x, f.y, if f.speed > 13.0 { hex(0xffffff) } else { hex(0xc9d0f2) });
        }
        self.hud.draw(c, gy as i32, self.time);
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
        s.celebrate();
        let melt = Commitment::Relentless.pace().melt_fraction;
        assert!(s.snow.fill() <= snow * (1.0 - melt) + 1e-4);
        assert!(s.frost.coverage() <= frost * (1.0 - melt) + 1e-4);
        assert_eq!(s.mage.act, mage::Act::Stagger);
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
}
