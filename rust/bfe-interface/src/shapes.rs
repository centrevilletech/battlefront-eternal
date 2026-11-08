//! Nine-slice frame decomposition, greedy word wrapping, and convex quad hit-testing (`src/interface/shapes.js`).
//!
//! Ports `nineSlice`, `wrapLines`, and `insideQuad` from `src/interface/shapes.js`
//! for decomposing 9-slice window borders (`border_3_pieces`, `listbox_cursor`), wrapping popup
//! text into fixed-width columns, and testing pointer hits against perspective-projected quads.

use crate::space::Point;

/// A single textured quad piece from a 3x3 nine-slice grid (`nineSlice` in `src/interface/shapes.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NineSlicePiece {
    /// Left X coordinate in local widget space (centered around `0.0`).
    pub x0: f64,
    /// Top Y coordinate in local widget space (centered around `0.0`).
    pub y0: f64,
    /// Right X coordinate in local widget space.
    pub x1: f64,
    /// Bottom Y coordinate in local widget space.
    pub y1: f64,
    /// Left texture U coordinate (`0.0`, `0.25`, or `0.75`).
    pub u0: f64,
    /// Top texture V coordinate (`0.0`, `0.25`, or `0.75`).
    pub v0: f64,
    /// Right texture U coordinate (`0.25`, `0.75`, or `1.0`).
    pub u1: f64,
    /// Bottom texture V coordinate (`0.25`, `0.75`, or `1.0`).
    pub v1: f64,
}

/// Computes up to 9 non-degenerate pieces of a centered `w x h` nine-slice frame with `corner`
/// inset and UV splits `[0.0, 0.25, 0.75, 1.0]` without heap allocation (`nineSlice` in `shapes.js`).
pub fn nine_slice_fixed(w: f64, h: f64, corner: f64) -> ([NineSlicePiece; 9], usize) {
    let mut out = [NineSlicePiece::default(); 9];
    if w <= 0.0 || h <= 0.0 {
        return (out, 0);
    }
    // Clamp the corner size to half of `w` and `h` so rectangles narrower or shorter than
    // `2 * corner` collapse their middle column or row cleanly (`xs[1] == xs[2]` / `ys[1] == ys[2]`).
    let cx = corner.max(0.0).min(w * 0.5);
    let cy = corner.max(0.0).min(h * 0.5);
    let x0 = -w * 0.5;
    let x3 = w * 0.5;
    let y0 = -h * 0.5;
    let y3 = h * 0.5;
    let xs = [x0, x0 + cx, x3 - cx, x3];
    let ys = [y0, y0 + cy, y3 - cy, y3];
    // The 2004 `border_3_pieces` and `listbox_cursor` textures reserve the outer 25% on each
    // axis for the 16-unit corner caps and stretch the middle 50% (`0.25..0.75`) across the body.
    let us = [0.0, 0.25, 0.75, 1.0];
    let vs = [0.0, 0.25, 0.75, 1.0];

    let mut count = 0;
    for r in 0..3 {
        for c in 0..3 {
            let (xa, xb) = (xs[c], xs[c + 1]);
            let (ya, yb) = (ys[r], ys[r + 1]);
            // Skip zero-area middle strips when `w <= 2 * corner` or `h <= 2 * corner`.
            if xb > xa && yb > ya {
                out[count] = NineSlicePiece {
                    x0: xa,
                    y0: ya,
                    x1: xb,
                    y1: yb,
                    u0: us[c],
                    v0: vs[r],
                    u1: us[c + 1],
                    v1: vs[r + 1],
                };
                count += 1;
            }
        }
    }
    (out, count)
}

/// Decomposes a centered `w x h` rectangle into a 3x3 grid of [`NineSlicePiece`] quads
/// (`nineSlice(w, h, corner)` in `src/interface/shapes.js`).
pub fn nine_slice(w: f64, h: f64, corner: f64) -> Vec<NineSlicePiece> {
    let (pieces, count) = nine_slice_fixed(w, h, corner);
    pieces[..count].to_vec()
}

/// Wraps `text` so each line fits within `max_width` as measured by `measure`
/// (`wrapLines(text, width, measure)` in `src/interface/shapes.js`).
///
/// Preserves explicit `\n` line breaks and never splits inside a single word.
pub fn wrap_lines<F>(text: &str, max_width: f64, mut measure: F) -> Vec<String>
where
    F: FnMut(&str) -> f64,
{
    if text.is_empty() {
        return Vec::new();
    }
    let paragraphs: Vec<&str> = text.split('\n').collect();
    if max_width <= 0.0 {
        return paragraphs.into_iter().map(str::to_owned).collect();
    }
    let mut lines = Vec::new();
    for para in paragraphs {
        // Split each paragraph on whitespace runs, preserving blank lines when a paragraph
        // contains no words, and greedily append words until `measure` exceeds `max_width`.
        let words: Vec<&str> = para
            .split(|c: char| c.is_whitespace())
            .filter(|s| !s.is_empty())
            .collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }
        // A single word longer than `max_width` stands alone on its own line (`shapes.js`).
        let mut current = words[0].to_owned();
        for &w in &words[1..] {
            let candidate = format!("{current} {w}");
            if measure(&candidate) <= max_width {
                current = candidate;
            } else {
                lines.push(current);
                current = w.to_owned();
            }
        }
        lines.push(current);
    }
    lines
}

/// Tests whether `(px, py)` lies inside or on the boundary of a 4-corner convex quad
/// (`insideQuad(x, y, corners)` in `src/interface/shapes.js`).
pub fn inside_quad(px: f64, py: f64, corners: &[Point; 4]) -> bool {
    // A point is inside a convex quad (with either clockwise or counter-clockwise winding)
    // iff the 2D cross product `edge x (p - a)` has the same non-zero sign for all four edges.
    let mut sign = 0i32;
    for i in 0..4 {
        let [ax, ay] = corners[i];
        let [bx, by] = corners[(i + 1) & 3];
        let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
        if cross.abs() <= 1e-5 {
            continue;
        }
        let s = if cross > 0.0 { 1 } else { -1 };
        if sign == 0 {
            sign = s;
        } else if sign != s {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies `nineSlice`, `wrapLines`, and `insideQuad` against `src/interface/shapes.js`.
    #[test]
    fn nine_slice_and_quad_hit_test_match_js() {
        let pieces = nine_slice(200.0, 100.0, 16.0);
        assert_eq!(pieces.len(), 9);
        assert!((pieces[0].x0 - -100.0).abs() < 1e-6);
        assert!((pieces[0].u1 - 0.25).abs() < 1e-6);
        assert!((pieces[4].u0 - 0.25).abs() < 1e-6);
        assert!((pieces[4].u1 - 0.75).abs() < 1e-6);

        let wrapped = wrap_lines("ONE TWO THREE FOUR", 8.0, |s| s.len() as f64);
        assert_eq!(wrapped, vec!["ONE TWO", "THREE", "FOUR"]);

        let q = [[10.0, 10.0], [110.0, 10.0], [110.0, 50.0], [10.0, 50.0]];
        assert!(inside_quad(50.0, 30.0, &q));
        assert!(!inside_quad(5.0, 30.0, &q));
    }
}
