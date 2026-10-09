//! Monitors → one wide scene. In overlay mode every monitor gets a window,
//! and the windows show slices of one scene laid out left to right, so the
//! same characters walk from one screen to the next. Pure: monitors in,
//! slices out.

use super::platform::Rect;

/// The part of the scene one monitor shows, in art pixels: `cols` × `rows`
/// starting at column `col` and row `row` of the scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slice {
    pub monitor: Rect,
    pub col: i32,
    pub row: i32,
    pub cols: i32,
    pub rows: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Scene size in art pixels.
    pub w: i32,
    pub h: i32,
    pub slices: Vec<Slice>,
    pub scale: u32,
}

/// Lays `monitors` side by side, left to right, at `scale` screen pixels per
/// art pixel. The scene is as tall as the tallest monitor; a shorter one shows
/// its bottom part, where the ground and everyone on it are.
pub fn layout(monitors: &[Rect], scale: u32) -> Layout {
    let scale = scale.max(1);
    let mut sorted = monitors.to_vec();
    sorted.sort_by_key(|m| (m.x, m.y));
    let size = |m: &Rect| (m.w.div_ceil(scale) as i32, m.h.div_ceil(scale) as i32);
    let h = sorted.iter().map(|m| size(m).1).max().unwrap_or(1);
    let mut col = 0;
    let slices = sorted
        .iter()
        .map(|m| {
            let (cols, rows) = size(m);
            let slice = Slice { monitor: *m, col, row: h - rows, cols, rows };
            col += cols;
            slice
        })
        .collect();
    Layout { w: col.max(1), h, slices, scale }
}

impl Layout {
    /// Where screen point (`x`, `y`) falls in the scene, if it is on a monitor.
    pub fn to_scene(&self, x: i32, y: i32) -> Option<(f32, f32)> {
        self.slices
            .iter()
            .find(|s| {
                let m = s.monitor;
                (m.x..m.x + m.w as i32).contains(&x) && (m.y..m.y + m.h as i32).contains(&y)
            })
            .map(|s| self.in_slice(s, (x - s.monitor.x) as f64, (y - s.monitor.y) as f64))
    }

    /// Screen rect `win` (the panel window) in scene pixels, through the
    /// monitor its top-left corner is on.
    pub fn rect_to_scene(&self, win: Rect) -> Option<(i32, i32, i32, i32)> {
        let s = self.slices.iter().find(|s| {
            let m = s.monitor;
            (m.x..m.x + m.w as i32).contains(&win.x) && (m.y..m.y + m.h as i32).contains(&win.y)
        })?;
        let (x, y, w, h) = super::platform::to_scene(win, (s.monitor.x, s.monitor.y), self.scale);
        Some((x + s.col, y + s.row, w, h))
    }

    /// The slice showing scene column `x` (the nearest end when off the scene).
    pub fn slice_at(&self, x: f32) -> Option<&Slice> {
        let x = x as i32;
        self.slices.iter().find(|s| x < s.col + s.cols).or(self.slices.last())
    }

    /// Scene point under a position inside `slice`'s window (screen pixels).
    pub fn in_slice(&self, slice: &Slice, x: f64, y: f64) -> (f32, f32) {
        let k = self.scale as f64;
        ((x / k) as f32 + slice.col as f32, (y / k) as f32 + slice.row as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FHD: (u32, u32) = (1920, 1080);

    fn mon(x: i32, y: i32, (w, h): (u32, u32)) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn one_monitor_is_the_whole_scene_as_before() {
        let l = layout(&[mon(0, 0, FHD)], 4);
        assert_eq!((l.w, l.h), (480, 270));
        assert_eq!(l.slices, vec![Slice { monitor: mon(0, 0, FHD), col: 0, row: 0, cols: 480, rows: 270 }]);
    }

    #[test]
    fn monitors_side_by_side_make_one_wide_scene_left_to_right() {
        let l = layout(&[mon(1920, 0, FHD), mon(0, 0, FHD)], 2);
        assert_eq!((l.w, l.h), (1920, 540), "two screens wide, one tall");
        assert_eq!((l.slices[0].monitor.x, l.slices[0].col), (0, 0), "the left screen shows the left half");
        assert_eq!((l.slices[1].monitor.x, l.slices[1].col), (1920, 960));
    }

    #[test]
    fn any_number_of_monitors_even_left_of_the_main_one() {
        let l = layout(&[mon(0, 0, FHD), mon(-1920, 0, FHD), mon(1920, 0, FHD)], 4);
        assert_eq!(l.w, 3 * 480);
        assert_eq!(l.slices.iter().map(|s| s.col).collect::<Vec<_>>(), vec![0, 480, 960]);
    }

    #[test]
    fn a_shorter_monitor_shows_the_bottom_of_the_scene_where_the_ground_is() {
        let l = layout(&[mon(0, 0, (2560, 1440)), mon(2560, 360, FHD)], 4);
        assert_eq!(l.h, 360);
        let short = l.slices[1];
        assert_eq!((short.row, short.rows), (90, 270), "its bottom row is the scene's bottom row");
    }

    #[test]
    fn a_point_on_any_screen_maps_into_the_scene() {
        let l = layout(&[mon(0, 0, FHD), mon(1920, 0, FHD)], 2);
        assert_eq!(l.to_scene(100, 50), Some((50.0, 25.0)));
        assert_eq!(l.to_scene(1920 + 100, 50), Some((1010.0, 25.0)), "the right screen continues the left one");
        assert_eq!(l.to_scene(5000, 50), None, "off every screen");
    }

    #[test]
    fn the_slice_under_a_scene_column_is_the_monitor_showing_it() {
        let l = layout(&[mon(0, 0, FHD), mon(1920, 0, FHD)], 2);
        assert_eq!(l.slice_at(10.0).map(|s| s.monitor.x), Some(0));
        assert_eq!(l.slice_at(1500.0).map(|s| s.monitor.x), Some(1920));
        assert_eq!(l.slice_at(-5.0).map(|s| s.monitor.x), Some(0), "off the scene: the nearest end");
        assert_eq!(l.slice_at(9000.0).map(|s| s.monitor.x), Some(1920));
    }

    #[test]
    fn the_panel_on_the_right_screen_is_kept_clear_in_the_right_part_of_the_scene() {
        let l = layout(&[mon(0, 0, FHD), mon(1920, 0, FHD)], 2);
        let panel = Rect { x: 1920 + 200, y: 100, w: 400, h: 300 };
        assert_eq!(l.rect_to_scene(panel), Some((960 + 100, 50, 200, 150)));
        assert_eq!(l.rect_to_scene(Rect { x: 9000, y: 0, w: 10, h: 10 }), None, "off every screen");
    }
}
