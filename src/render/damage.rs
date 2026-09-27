//! Redraws only what changed. A full-screen overlay is millions of screen
//! pixels, but from one frame to the next only the snow moves (~0.2% of the
//! canvas), so rescaling the whole frame on the CPU would waste ~20% of a core.
//! Pure: fed the canvas and the age of the buffer being drawn into.

use super::canvas::Canvas;
use super::cpu::{pack, upscale};
use std::collections::VecDeque;

/// Oldest buffer age handled incrementally (swap chains keep 1–3 buffers).
const MAX_AGE: usize = 3;
/// Side of a damage tile, in canvas pixels.
const TILE: usize = 16;
/// Frames sent whole after a window appears (or is invalidated): X11 drops
/// what is drawn before the window is mapped yet reports the buffer as shown.
pub const SETTLE_FRAMES: u64 = 15;
/// A whole frame now and then heals anything the window system lost (an
/// expose without a compositor). ~2 s at 30 fps; costs <1% even full-screen.
pub const REFRESH_EVERY: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redraw {
    /// Rescale the whole canvas (new buffer, resize, or most of it changed).
    Full,
    /// Only these canvas pixel indices (`y * w + x`), sorted, differ.
    Pixels(Vec<u32>),
}

#[derive(Default)]
pub struct Damage {
    prev: Vec<u8>,
    /// Canvas w/h, scale and frame w/h the history belongs to.
    dims: (i32, i32, u32, usize, usize),
    /// Changed pixels of each recent frame, newest first.
    recent: VecDeque<Vec<u32>>,
    /// Frames since the window appeared or was last invalidated.
    frames: u64,
}

impl Damage {
    /// What to repaint in a buffer that shows the frame from `age` presents ago
    /// (`0` = unknown contents). Call once per presented frame.
    pub fn next(&mut self, canvas: &Canvas, scale: u32, w: usize, h: usize, age: u8) -> Redraw {
        self.frames += 1;
        let dims = (canvas.w, canvas.h, scale, w, h);
        if dims != self.dims || self.prev.len() != canvas.bytes().len() {
            self.dims = dims;
            self.prev = canvas.bytes().to_vec();
            self.recent.clear();
            return Redraw::Full;
        }
        let changed = diff(&self.prev, canvas.bytes(), canvas.w as usize);
        self.prev.copy_from_slice(canvas.bytes());
        self.recent.push_front(changed);
        self.recent.truncate(MAX_AGE);

        let age = age as usize;
        if age == 0 || age > self.recent.len() || self.frames <= SETTLE_FRAMES || self.frames % REFRESH_EVERY == 0 {
            return Redraw::Full;
        }
        let mut union: Vec<u32> = self.recent.iter().take(age).flatten().copied().collect();
        if age > 1 {
            union.sort_unstable();
            union.dedup();
        }
        if union.len() * 4 > (canvas.w * canvas.h) as usize {
            return Redraw::Full;
        }
        Redraw::Pixels(union)
    }
}

impl Damage {
    /// The window's contents may be gone (shown again, resized, refocused):
    /// send whole frames for a moment.
    pub fn invalidate(&mut self) {
        self.frames = 0;
    }
}

/// Indices of pixels that differ; rows are compared whole first (memcmp).
fn diff(old: &[u8], new: &[u8], w: usize) -> Vec<u32> {
    let mut out = Vec::new();
    for (y, (a, b)) in old.chunks_exact(w * 4).zip(new.chunks_exact(w * 4)).enumerate() {
        if a == b {
            continue;
        }
        for (x, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
            if pa != pb {
                out.push((y * w + x) as u32);
            }
        }
    }
    out
}

/// Paints the given canvas pixels as `scale`×`scale` blocks, cropped to the frame.
pub fn paint(canvas: &Canvas, scale: u32, pixels: &[u32], dst: &mut [u32], w: usize, h: usize) {
    let (s, cw) = (scale.max(1) as usize, canvas.w as usize);
    let src = canvas.bytes();
    for &i in pixels {
        let i = i as usize;
        let (x0, y0) = ((i % cw) * s, (i / cw) * s);
        if x0 >= w || y0 >= h {
            continue;
        }
        let color = pack(&src[i * 4..i * 4 + 4]);
        let x1 = (x0 + s).min(w);
        for y in y0..(y0 + s).min(h) {
            if let Some(row) = dst.get_mut(y * w + x0..y * w + x1) {
                row.fill(color);
            }
        }
    }
}

/// Screen rectangles `(x, y, width, height)` covering the changed pixels:
/// dirty tiles, merged along each tile row, cropped to the frame.
pub fn rects(canvas_w: i32, scale: u32, pixels: &[u32], w: usize, h: usize) -> Vec<(u32, u32, u32, u32)> {
    let (s, cw) = (scale.max(1) as usize, canvas_w.max(1) as usize);
    let mut tiles: Vec<(usize, usize)> =
        pixels.iter().map(|&i| ((i as usize / cw) / TILE, (i as usize % cw) / TILE)).collect();
    tiles.sort_unstable();
    tiles.dedup();
    let side = TILE * s;
    let mut out = Vec::new();
    let mut run: Option<(usize, usize, usize)> = None; // (tile row, first col, last col)
    let flush = |r: (usize, usize, usize), out: &mut Vec<(u32, u32, u32, u32)>| {
        let (x, y) = (r.1 * side, r.0 * side);
        if x < w && y < h {
            let (rw, rh) = (((r.2 + 1) * side).min(w) - x, (y + side).min(h) - y);
            out.push((x as u32, y as u32, rw as u32, rh as u32));
        }
    };
    for (ty, tx) in tiles {
        run = match run {
            Some((ry, a, b)) if ry == ty && b + 1 == tx => Some((ry, a, tx)),
            Some(r) => {
                flush(r, &mut out);
                Some((ty, tx, tx))
            }
            None => Some((ty, tx, tx)),
        };
    }
    if let Some(r) = run {
        flush(r, &mut out);
    }
    out
}

/// Brings a buffer up to date with `canvas`; returns the screen rectangles to
/// present (`None` = the whole frame, empty = nothing changed).
pub fn update(
    damage: &mut Damage,
    canvas: &Canvas,
    scale: u32,
    dst: &mut [u32],
    w: usize,
    h: usize,
    age: u8,
) -> Option<Vec<(u32, u32, u32, u32)>> {
    match damage.next(canvas, scale, w, h, age) {
        Redraw::Full => {
            upscale(canvas, scale, dst, w, h);
            None
        }
        Redraw::Pixels(px) => {
            paint(canvas, scale, &px, dst, w, h);
            Some(rects(canvas.w, scale, &px, w, h))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::canvas::hex;
    use crate::scene::rng::Rng;

    fn full(c: &Canvas, scale: u32, w: usize, h: usize) -> Vec<u32> {
        let mut out = vec![0; w * h];
        upscale(c, scale, &mut out, w, h);
        out
    }

    /// Past the start-up frames, where everything is still sent whole.
    fn settle(d: &mut Damage, c: &Canvas, scale: u32, w: usize, h: usize) {
        for _ in 0..SETTLE_FRAMES {
            d.next(c, scale, w, h, 1);
        }
    }

    #[test]
    fn a_new_window_is_sent_whole_until_it_has_surely_appeared() {
        // X11 drops what is drawn before the window is mapped, yet reports the
        // buffer as shown: the panel came up black with only the cursor painted.
        let c = Canvas::new(8, 8);
        let mut d = Damage::default();
        for i in 0..SETTLE_FRAMES {
            assert_eq!(d.next(&c, 1, 8, 8, 1), Redraw::Full, "start-up frame {i}");
        }
        assert_eq!(d.next(&c, 1, 8, 8, 1), Redraw::Pixels(vec![]));
    }

    #[test]
    fn the_whole_frame_is_resent_now_and_then_as_a_safety_net() {
        let c = Canvas::new(8, 8);
        let mut d = Damage::default();
        let fulls = (0..REFRESH_EVERY * 3).filter(|_| d.next(&c, 1, 8, 8, 1) == Redraw::Full).count() as u64;
        assert_eq!(fulls, SETTLE_FRAMES + 3, "start-up, then one every {REFRESH_EVERY} frames");
    }

    #[test]
    fn invalidating_resends_everything_for_a_moment() {
        let c = Canvas::new(8, 8);
        let mut d = Damage::default();
        settle(&mut d, &c, 1, 8, 8);
        d.invalidate(); // exposed, resized, refocused…
        assert_eq!(d.next(&c, 1, 8, 8, 1), Redraw::Full);
    }

    #[test]
    fn the_first_frame_and_unknown_buffers_are_drawn_whole() {
        let c = Canvas::new(8, 8);
        let mut d = Damage::default();
        assert_eq!(d.next(&c, 2, 16, 16, 1), Redraw::Full, "no history yet");
        assert_eq!(d.next(&c, 2, 16, 16, 0), Redraw::Full, "age 0 = unspecified contents");
    }

    #[test]
    fn an_unchanged_frame_repaints_nothing_and_one_moved_flake_repaints_one_pixel() {
        let mut c = Canvas::new(8, 8);
        let mut d = Damage::default();
        settle(&mut d, &c, 2, 16, 16);
        assert_eq!(d.next(&c, 2, 16, 16, 1), Redraw::Pixels(vec![]));
        c.set(3, 2, hex(0xffffff));
        assert_eq!(d.next(&c, 2, 16, 16, 1), Redraw::Pixels(vec![2 * 8 + 3]));
    }

    #[test]
    fn an_older_buffer_also_gets_the_frames_it_missed() {
        let mut c = Canvas::new(8, 8);
        let mut d = Damage::default();
        settle(&mut d, &c, 1, 8, 8);
        c.set(1, 0, hex(0xffffff));
        d.next(&c, 1, 8, 8, 1);
        c.set(5, 7, hex(0xffffff));
        assert_eq!(d.next(&c, 1, 8, 8, 2), Redraw::Pixels(vec![1, 7 * 8 + 5]));
        assert_eq!(d.next(&c, 1, 8, 8, 4), Redraw::Full, "older than the history kept");
    }

    #[test]
    fn resizing_or_a_new_scale_starts_over() {
        let c = Canvas::new(8, 8);
        let mut d = Damage::default();
        settle(&mut d, &c, 2, 16, 16);
        assert_eq!(d.next(&c, 2, 15, 16, 1), Redraw::Full, "window resized");
        assert_eq!(d.next(&c, 3, 15, 16, 1), Redraw::Full, "scale changed");
        assert_eq!(d.next(&Canvas::new(9, 8), 3, 15, 16, 1), Redraw::Full, "canvas resized");
    }

    #[test]
    fn a_mostly_changed_frame_is_cheaper_redrawn_whole() {
        let mut c = Canvas::new(8, 8);
        let mut d = Damage::default();
        settle(&mut d, &c, 1, 8, 8);
        c.clear(hex(0x112233));
        assert_eq!(d.next(&c, 1, 8, 8, 1), Redraw::Full);
    }

    #[test]
    fn painting_writes_scaled_blocks_and_crops_at_the_frame_edge() {
        let mut c = Canvas::new(2, 2);
        c.set(1, 1, hex(0xff0000));
        let mut dst = vec![0u32; 3 * 3];
        paint(&c, 2, &[3], &mut dst, 3, 3);
        assert_eq!(dst, vec![0, 0, 0, 0, 0, 0, 0, 0, 0xffff0000], "block cropped to 1×1");
    }

    #[test]
    fn rects_cover_dirty_tiles_merged_along_a_row_and_stay_in_the_frame() {
        let cw = 64;
        // Tiles (0,0) and (0,1) are neighbours; (2,3) stands alone.
        let px = [0, 20, (2 * TILE * cw + 3 * TILE) as u32];
        let r = rects(cw as i32, 2, &px, 100, 1000);
        assert_eq!(r, vec![(0, 0, 64, 32), (96, 64, 4, 32)]);
        assert!(rects(cw as i32, 2, &[], 100, 100).is_empty());
    }

    /// The property that matters: whatever the buffering (1, 2 or 3 buffers
    /// in rotation), incremental updates always leave exactly the full redraw.
    #[test]
    fn incremental_frames_always_match_a_full_redraw() {
        for buffers in 1..=3usize {
            let (cw, ch, scale, w, h) = (23, 17, 3u32, 67, 50); // canvas overhangs the frame
            let mut rng = Rng::new(buffers as u64);
            let mut c = Canvas::new(cw, ch);
            let mut d = Damage::default();
            let mut bufs = vec![vec![0xdeadbeefu32; w * h]; buffers];
            let mut last_drawn: Vec<Option<usize>> = vec![None; buffers];
            for frame in 0..200usize {
                for _ in 0..rng.next_u32() % 6 {
                    let (x, y) = ((rng.next_u32() % cw as u32) as i32, (rng.next_u32() % ch as u32) as i32);
                    c.set(x, y, if rng.chance(0.5) { hex(rng.next_u32() & 0xffffff) } else { [0; 4] });
                }
                let b = frame % buffers;
                let age = last_drawn[b].map_or(0, |f| (frame - f) as u8);
                update(&mut d, &c, scale, &mut bufs[b], w, h, age);
                last_drawn[b] = Some(frame);
                assert_eq!(bufs[b], full(&c, scale, w, h), "{buffers} buffer(s), frame {frame}");
            }
        }
    }
}
