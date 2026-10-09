//! Sliding down a steep snow pile: the warrior and the frost mage stop
//! walking and slide on their boots, shouting once as the slide starts
//! (unless they are already saying something: lesson lines and tips win).

use super::snow::Snow;

/// Drop per column (px/px) over `REACH` columns ahead that counts as steep.
pub const SLOPE: f32 = 0.5;
/// Columns ahead the slope is measured over (so one bumpy column isn't a slide).
const REACH: f32 = 4.0;
/// Snow under the feet needed to slide at all: bare ground never slides.
pub const MIN_DEPTH: f32 = 4.0;
/// Sliding speed, px/s, on top of their walk.
pub const SPEED: f32 = 26.0;

pub const WARRIOR_LINE: &str = "Uhuu! Escorregando!";
pub const MAGE_LINE: &str = "Aaah! Escorrega!";

/// Is the pile steep going down from `x` in direction `dir` (±1)?
pub fn steep(snow: &Snow, x: f32, dir: f32) -> bool {
    let here = snow.height_at(x);
    here >= MIN_DEPTH && (here - snow.height_at(x + dir.signum() * REACH)) / REACH >= SLOPE
}

/// Test pile: a peak `height` px tall at column `at` of a `width`-column
/// pile, sides dropping `slope` px per column (whole grains).
#[cfg(test)]
pub(crate) fn peak(width: i32, cap: f32, at: f32, slope: f32, height: f32) -> Snow {
    let mut s = Snow::new(width, cap);
    for i in 0..width {
        let h = (height - (i as f32 - at).abs() * slope).max(0.0).round() as i32;
        for _ in 0..h {
            s.add_grain(i as f32);
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::font;

    #[test]
    fn downhill_on_a_steep_pile_is_steep_uphill_is_not() {
        let s = peak(100, 40.0, 50.0, 0.8, 30.0);
        assert!(steep(&s, 45.0, -1.0), "going down the left side");
        assert!(steep(&s, 55.0, 1.0), "going down the right side");
        assert!(!steep(&s, 45.0, 1.0), "climbing never slides");
    }

    #[test]
    fn a_gentle_pile_or_bare_ground_is_not_steep() {
        assert!(!steep(&peak(100, 40.0, 50.0, 0.25, 30.0), 45.0, -1.0), "gentle slope");
        assert!(!steep(&peak(100, 40.0, 50.0, 0.8, 3.0), 50.0, 1.0), "too little snow under the feet");
        assert!(!steep(&Snow::new(100, 40.0), 50.0, 1.0));
    }

    #[test]
    fn slide_lines_are_short_pt_br_the_font_can_draw() {
        for line in [WARRIOR_LINE, MAGE_LINE] {
            assert!(font::supports(line), "{line}");
            assert!(line.len() <= 24, "{line}");
        }
    }
}
