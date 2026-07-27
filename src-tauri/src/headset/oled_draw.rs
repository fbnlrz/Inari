//! Drawing primitives shared by every OLED screen.
//!
//! Each `oled_mode_*` module (and `clips`) used to carry its own copy of these.
//! The copies drifted, and the drift was visible on the panel: two different
//! `ascii()` mappings rendered the same title as "Don't" on one screen and
//! "Don?t" on the next, and two different centring rules put the same string in
//! two places. One implementation each, so all screens agree.

use super::oled::{Framebuffer, WIDTH};

/// Set one pixel; dashed strokes use a checkerboard so an overlaid trace stays
/// legible against a solid one — and at any slope, unlike a run-length dash.
#[inline]
pub fn plot(fb: &mut Framebuffer, x: isize, y: isize, dashed: bool) {
    if !dashed || (x + y) % 2 == 0 {
        fb.set(x, y, true);
    }
}

/// Bresenham line, optionally dashed.
pub fn line_styled(fb: &mut Framebuffer, x0: isize, y0: isize, x1: isize, y1: isize, dashed: bool) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        plot(fb, x, y, dashed);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Solid Bresenham line — the common case (clock hands, wireframes, glyphs).
pub fn line(fb: &mut Framebuffer, x0: isize, y0: isize, x1: isize, y1: isize) {
    line_styled(fb, x0, y0, x1, y1, false);
}

/// Circle, filled or a 1px outline.
pub fn circle(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize, fill: bool) {
    for dy in -r..=r {
        for dx in -r..=r {
            let d2 = dx * dx + dy * dy;
            let on = if fill { d2 <= r * r } else { (d2 - r * r).abs() <= r };
            if on {
                fb.set(cx + dx, cy + dy, true);
            }
        }
    }
}

/// Filled circle.
pub fn disc(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize) {
    circle(fb, cx, cy, r, true);
}

/// 1px circle outline.
pub fn ring(fb: &mut Framebuffer, cx: isize, cy: isize, r: isize) {
    circle(fb, cx, cy, r, false);
}

/// Fold a string into what the 5x7 font can actually draw.
///
/// External strings — track titles, weather descriptions, app names — are full
/// of typographic punctuation. Mapping the common cases keeps "Don't" readable
/// instead of turning it into "Don?t"; everything else still becomes '?' so a
/// missing glyph reads as a deliberate hole rather than garbage.
pub fn ascii(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' | '\u{02bc}' => '\'',
            '\u{201c}' | '\u{201d}' => '"',
            '\u{2010}'..='\u{2015}' => '-',
            '\u{00a0}' => ' ',
            c if c.is_ascii_graphic() || c == ' ' => c,
            _ => '?',
        })
        .collect()
}

/// Clamp a string to `max` characters, marking the cut with an ASCII ellipsis.
pub fn ellipsize(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        return chars.into_iter().collect();
    }
    if max <= 3 {
        return chars.into_iter().take(max).collect();
    }
    let mut out: String = chars.into_iter().take(max - 3).collect();
    out.push_str("...");
    out
}

/// Left edge that centres `text` on the panel by its *ink* width. Same rule as
/// [`Framebuffer::draw_text_centered`], for screens that need the x rather than
/// the draw.
pub fn centred_x(text: &str, scale: usize) -> isize {
    (WIDTH as isize - Framebuffer::ink_width(text, scale) as isize) / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_folds_typography_and_drops_the_rest() {
        // The apostrophe survives as an apostrophe, not as '?'.
        assert_eq!(ascii("Don\u{2019}t \u{2014} caf\u{e9} \u{1f600}"), "Don't - caf? ?");
    }

    #[test]
    fn ellipsize_marks_the_cut() {
        assert_eq!(ellipsize("short", 10), "short");
        assert_eq!(ellipsize("abcdefghij", 6), "abc...");
        assert_eq!(ellipsize("abcdef", 6), "abcdef");
    }

    #[test]
    fn centring_ignores_the_trailing_gap() {
        // "AB" at scale 2 is 22px of ink (2 glyphs + 1 gap), not 24.
        assert_eq!(centred_x("AB", 2), (WIDTH as isize - 22) / 2);
        assert_eq!(centred_x("", 1), WIDTH as isize / 2);
    }

    #[test]
    fn dashed_strokes_skip_every_other_pixel() {
        let mut solid = Framebuffer::new();
        let mut dashed = Framebuffer::new();
        line(&mut solid, 0, 0, 20, 0);
        line_styled(&mut dashed, 0, 0, 20, 0, true);
        assert!(solid != dashed);
    }
}
