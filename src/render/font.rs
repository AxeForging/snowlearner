//! Tiny 5x7 bitmap font with composable diacritics, so pt-BR and Spanish text
//! (ã, ç, é, ñ, ¿, ¡ …) renders in the same pixel style without a font file.
//!
//! Line box (top → bottom): 3 rows accent room, 7 rows cap height, 2 rows
//! descender. `y` passed to the draw functions is the top of that box.

use super::canvas::{Canvas, Rgba};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

pub const ADVANCE: i32 = 6;
pub const ASCENT: i32 = 3;
pub const LINE_H: i32 = 13;

fn glyph(c: char) -> Option<&'static str> {
    Some(match c {
        ' ' => "",
        '!' => "..#..|..#..|..#..|..#..|..#..|.....|..#..",
        '"' => ".#.#.|.#.#.",
        '#' => ".#.#.|.#.#.|#####|.#.#.|#####|.#.#.|.#.#.",
        '$' => "..#..|.####|#.#..|.###.|..#.#|####.|..#..",
        '%' => "##...|##..#|...#.|..#..|.#...|#..##|...##",
        '&' => ".##..|#..#.|#.#..|.#...|#.#.#|#..#.|.##.#",
        '\'' => "..#..|..#..",
        '(' => "...#.|..#..|.#...|.#...|.#...|..#..|...#.",
        ')' => ".#...|..#..|...#.|...#.|...#.|..#..|.#...",
        '*' => ".....|..#..|#.#.#|.###.|#.#.#|..#..",
        '+' => ".....|..#..|..#..|#####|..#..|..#..",
        ',' => ".....|.....|.....|.....|.....|..#..|..#..|.#...",
        '-' => ".....|.....|.....|#####",
        '.' => ".....|.....|.....|.....|.....|.....|..#..",
        '/' => "....#|....#|...#.|..#..|.#...|#....|#....",
        '0' => ".###.|#...#|#..##|#.#.#|##..#|#...#|.###.",
        '1' => "..#..|.##..|..#..|..#..|..#..|..#..|.###.",
        '2' => ".###.|#...#|....#|...#.|..#..|.#...|#####",
        '3' => "#####|...#.|..#..|...#.|....#|#...#|.###.",
        '4' => "...#.|..##.|.#.#.|#..#.|#####|...#.|...#.",
        '5' => "#####|#....|####.|....#|....#|#...#|.###.",
        '6' => "..##.|.#...|#....|####.|#...#|#...#|.###.",
        '7' => "#####|....#|...#.|..#..|.#...|.#...|.#...",
        '8' => ".###.|#...#|#...#|.###.|#...#|#...#|.###.",
        '9' => ".###.|#...#|#...#|.####|....#|...#.|.##..",
        ':' => ".....|..#..|.....|.....|.....|..#..",
        ';' => ".....|..#..|.....|.....|.....|..#..|..#..|.#...",
        '<' => "...#.|..#..|.#...|#....|.#...|..#..|...#.",
        '=' => ".....|.....|#####|.....|#####",
        '>' => ".#...|..#..|...#.|....#|...#.|..#..|.#...",
        '?' => ".###.|#...#|....#|...#.|..#..|.....|..#..",
        '@' => ".###.|#...#|#.###|#.#.#|#.###|#....|.###.",
        'A' => ".###.|#...#|#...#|#####|#...#|#...#|#...#",
        'B' => "####.|#...#|#...#|####.|#...#|#...#|####.",
        'C' => ".###.|#...#|#....|#....|#....|#...#|.###.",
        'D' => "###..|#..#.|#...#|#...#|#...#|#..#.|###..",
        'E' => "#####|#....|#....|####.|#....|#....|#####",
        'F' => "#####|#....|#....|####.|#....|#....|#....",
        'G' => ".###.|#...#|#....|#.###|#...#|#...#|.####",
        'H' => "#...#|#...#|#...#|#####|#...#|#...#|#...#",
        'I' => ".###.|..#..|..#..|..#..|..#..|..#..|.###.",
        'J' => "..###|...#.|...#.|...#.|...#.|#..#.|.##..",
        'K' => "#...#|#..#.|#.#..|##...|#.#..|#..#.|#...#",
        'L' => "#....|#....|#....|#....|#....|#....|#####",
        'M' => "#...#|##.##|#.#.#|#.#.#|#...#|#...#|#...#",
        'N' => "#...#|#...#|##..#|#.#.#|#..##|#...#|#...#",
        'O' => ".###.|#...#|#...#|#...#|#...#|#...#|.###.",
        'P' => "####.|#...#|#...#|####.|#....|#....|#....",
        'Q' => ".###.|#...#|#...#|#...#|#.#.#|#..#.|.##.#",
        'R' => "####.|#...#|#...#|####.|#.#..|#..#.|#...#",
        'S' => ".####|#....|#....|.###.|....#|....#|####.",
        'T' => "#####|..#..|..#..|..#..|..#..|..#..|..#..",
        'U' => "#...#|#...#|#...#|#...#|#...#|#...#|.###.",
        'V' => "#...#|#...#|#...#|#...#|#...#|.#.#.|..#..",
        'W' => "#...#|#...#|#...#|#.#.#|#.#.#|#.#.#|.#.#.",
        'X' => "#...#|#...#|.#.#.|..#..|.#.#.|#...#|#...#",
        'Y' => "#...#|#...#|.#.#.|..#..|..#..|..#..|..#..",
        'Z' => "#####|....#|...#.|..#..|.#...|#....|#####",
        '[' => ".###.|.#...|.#...|.#...|.#...|.#...|.###.",
        '\\' => "#....|#....|.#...|..#..|...#.|....#|....#",
        ']' => ".###.|...#.|...#.|...#.|...#.|...#.|.###.",
        '^' => "..#..|.#.#.|#...#",
        '_' => ".....|.....|.....|.....|.....|.....|#####",
        '`' => ".#...|..#..",
        'a' => ".....|.....|.###.|....#|.####|#...#|.####",
        'b' => "#....|#....|####.|#...#|#...#|#...#|####.",
        'c' => ".....|.....|.###.|#....|#....|#...#|.###.",
        'd' => "....#|....#|.####|#...#|#...#|#...#|.####",
        'e' => ".....|.....|.###.|#...#|#####|#....|.###.",
        'f' => "..##.|.#..#|.#...|###..|.#...|.#...|.#...",
        'g' => ".....|.....|.####|#...#|#...#|.####|....#|.###.",
        'h' => "#....|#....|####.|#...#|#...#|#...#|#...#",
        'i' => "..#..|.....|.##..|..#..|..#..|..#..|.###.",
        'ı' => ".....|.....|.##..|..#..|..#..|..#..|.###.",
        'j' => "...#.|.....|..##.|...#.|...#.|...#.|#..#.|.##..",
        'k' => "#....|#....|#..#.|#.#..|##...|#.#..|#..#.",
        'l' => ".##..|..#..|..#..|..#..|..#..|..#..|.###.",
        'm' => ".....|.....|##.#.|#.#.#|#.#.#|#.#.#|#.#.#",
        'n' => ".....|.....|####.|#...#|#...#|#...#|#...#",
        'o' => ".....|.....|.###.|#...#|#...#|#...#|.###.",
        'p' => ".....|.....|####.|#...#|#...#|####.|#....|#....",
        'q' => ".....|.....|.####|#...#|#...#|.####|....#|....#",
        'r' => ".....|.....|#.##.|##..#|#....|#....|#....",
        's' => ".....|.....|.####|#....|.###.|....#|####.",
        't' => ".#...|.#...|###..|.#...|.#...|.#..#|..##.",
        'u' => ".....|.....|#...#|#...#|#...#|#..##|.##.#",
        'v' => ".....|.....|#...#|#...#|#...#|.#.#.|..#..",
        'w' => ".....|.....|#...#|#...#|#.#.#|#.#.#|.#.#.",
        'x' => ".....|.....|#...#|.#.#.|..#..|.#.#.|#...#",
        'y' => ".....|.....|#...#|#...#|#...#|.####|....#|.###.",
        'z' => ".....|.....|#####|...#.|..#..|.#...|#####",
        '{' => "...#.|..#..|..#..|.#...|..#..|..#..|...#.",
        '|' => "..#..|..#..|..#..|..#..|..#..|..#..|..#..",
        '}' => ".#...|..#..|..#..|...#.|..#..|..#..|.#...",
        '~' => ".....|.....|.#...|#.#.#|...#.",
        '¿' => "..#..|.....|..#..|.#...|#....|#...#|.###.",
        '¡' => "..#..|.....|..#..|..#..|..#..|..#..|..#..",
        '…' => ".....|.....|.....|.....|.....|.....|#.#.#",
        '°' => ".##..|#..#.|.##..",
        '·' => ".....|.....|.....|..#..",
        '→' => ".....|..#..|...#.|#####|...#.|..#..",
        '←' => ".....|..#..|.#...|#####|.#...|..#..",
        '↑' => "..#..|.###.|#.#.#|..#..|..#..|..#..|..#..",
        '↓' => "..#..|..#..|..#..|..#..|#.#.#|.###.|..#..",
        _ => return None,
    })
}

/// Combining marks → (pattern, placed above?). Above marks sit in the accent
/// rows for capitals and just above the x-height for lowercase.
fn mark(m: char) -> Option<(&'static str, bool)> {
    Some(match m {
        '\u{301}' => ("...#.|..#..", true),  // acute
        '\u{300}' => (".#...|..#..", true),  // grave
        '\u{302}' => ("..#..|.#.#.", true),  // circumflex
        '\u{303}' => (".##.#|#..#.", true),  // tilde
        '\u{308}' => (".#.#.", true),        // diaeresis
        '\u{30A}' => ("..#..|.#.#.", true),  // ring (approx.)
        '\u{327}' => ("..#..|.##..", false), // cedilla
        _ => return None,
    })
}

fn normalize_char(c: char) -> char {
    match c {
        '’' | '‘' | '´' => '\'',
        '“' | '”' => '"',
        '—' | '–' => '-',
        other => other,
    }
}

/// One drawable cell: a base glyph and the marks stacked on it.
struct Cell {
    base: char,
    marks: Vec<char>,
}

fn cells(text: &str) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::new();
    for c in text.nfd() {
        if is_combining_mark(c) {
            if let Some(last) = out.last_mut() {
                last.marks.push(c);
            }
            continue;
        }
        out.push(Cell { base: normalize_char(c), marks: Vec::new() });
    }
    for cell in &mut out {
        // Accented i/j lose their dot.
        if cell.base == 'i' && cell.marks.iter().any(|m| mark(*m).is_some_and(|(_, above)| above)) {
            cell.base = 'ı';
        }
    }
    out
}

pub fn text_width(text: &str) -> i32 {
    let n = cells(text).len() as i32;
    if n == 0 { 0 } else { n * ADVANCE - 1 }
}

/// True when every character has a glyph (unknown chars render as a box).
pub fn supports(text: &str) -> bool {
    cells(text).iter().all(|c| glyph(c.base).is_some() && c.marks.iter().all(|m| mark(*m).is_some()))
}

fn stamp(canvas: &mut Canvas, pattern: &str, x: i32, y: i32, color: Rgba) {
    for (j, row) in pattern.split('|').enumerate() {
        for (i, ch) in row.chars().enumerate() {
            if ch == '#' {
                canvas.set(x + i as i32, y + j as i32, color);
            }
        }
    }
}

/// Draws `text` with its line box top at `y`. Returns the drawn width.
pub fn draw(canvas: &mut Canvas, x: i32, y: i32, text: &str, color: Rgba) -> i32 {
    let base_y = y + ASCENT;
    let mut cx = x;
    for cell in cells(text) {
        match glyph(cell.base) {
            Some(p) => stamp(canvas, p, cx, base_y, color),
            None => stamp(canvas, "#####|#...#|#...#|#...#|#...#|#...#|#####", cx, base_y, color),
        }
        let lower = cell.base.is_lowercase() || cell.base == 'ı';
        for m in &cell.marks {
            if let Some((p, above)) = mark(*m) {
                let my = match (above, lower) {
                    (true, true) => base_y,
                    (true, false) => base_y - 3,
                    (false, _) => base_y + 7,
                };
                stamp(canvas, p, cx, my, color);
            }
        }
        cx += ADVANCE;
    }
    (cx - x - 1).max(0)
}

/// Text with a 1px dark outline — readable over any background (overlay mode).
pub fn draw_outlined(canvas: &mut Canvas, x: i32, y: i32, text: &str, color: Rgba, outline: Rgba) -> i32 {
    for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
        draw(canvas, x + dx, y + dy, text, outline);
    }
    draw(canvas, x, y, text, color)
}

/// Greedy word wrap to `max_w` pixels. Words longer than a line are kept whole.
pub fn wrap(text: &str, max_w: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
        if text_width(&candidate) > max_w && !line.is_empty() {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::canvas::hex;

    #[test]
    fn every_printable_ascii_char_has_a_glyph() {
        for c in ' '..='~' {
            assert!(glyph(c).is_some(), "missing glyph for {c:?}");
        }
    }

    #[test]
    fn glyph_rows_are_five_wide_and_fit_the_line_box() {
        for c in (' '..='~').chain("¿¡…°ı".chars()) {
            let p = glyph(c).unwrap();
            let rows: Vec<&str> = if p.is_empty() { vec![] } else { p.split('|').collect() };
            assert!(rows.len() <= 9, "{c:?} too tall");
            for r in rows {
                assert_eq!(r.chars().count(), 5, "{c:?} row {r:?}");
            }
        }
    }

    #[test]
    fn portuguese_and_spanish_text_is_fully_supported() {
        assert!(supports("Não, obrigado! Você está com fome? Ação, pão, avó"));
        assert!(supports("¿Dónde está el baño? ¡Mañana! pingüino"));
        assert!(!supports("日本語"));
        assert!(supports("trabalho · A2 · de memória → ok ↑↓←"));
    }

    #[test]
    fn accented_char_is_one_cell_wide() {
        assert_eq!(text_width("ã"), text_width("a"));
        assert_eq!(text_width("ação"), text_width("acao"));
    }

    #[test]
    fn accent_draws_extra_pixels_above_the_letter() {
        let mut plain = Canvas::new(8, LINE_H);
        let mut accented = Canvas::new(8, LINE_H);
        draw(&mut plain, 0, 0, "a", hex(0xffffff));
        draw(&mut accented, 0, 0, "á", hex(0xffffff));
        let above = |c: &Canvas| c.opaque_in(0, 0, 8, ASCENT + 2);
        assert_eq!(above(&plain), 0);
        assert!(above(&accented) > 0);
        // The letter body itself is identical.
        assert_eq!(plain.opaque_in(0, ASCENT + 2, 8, 7), accented.opaque_in(0, ASCENT + 2, 8, 7));
    }

    #[test]
    fn cedilla_draws_below_the_baseline() {
        let mut c = Canvas::new(8, LINE_H);
        draw(&mut c, 0, 0, "ç", hex(0xffffff));
        assert!(c.opaque_in(0, ASCENT + 7, 8, 2) > 0);
    }

    #[test]
    fn unknown_chars_render_a_placeholder_box_instead_of_nothing() {
        let mut c = Canvas::new(8, LINE_H);
        draw(&mut c, 0, 0, "語", hex(0xffffff));
        assert!(c.opaque_in(0, 0, 8, LINE_H) > 10);
    }

    #[test]
    fn wrap_respects_width_and_keeps_all_words() {
        let text = "Para dizer que estou com fome devo dizer I'm hungry";
        let lines = wrap(text, 60);
        assert!(lines.len() > 1);
        for l in &lines {
            assert!(text_width(l) <= 60 || !l.contains(' '), "{l:?} overflows");
        }
        assert_eq!(lines.join(" "), text);
    }

    #[test]
    fn wrap_of_empty_text_is_empty() {
        assert!(wrap("   ", 50).is_empty());
    }
}
