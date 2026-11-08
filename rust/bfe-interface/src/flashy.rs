//! Five-stage flashy text reveal animation (`src/interface/flashy.js`).
//!
//! Ports `INTRO`, `INTRO_LENGTH`, and `TitleIntro` from `src/interface/flashy.js`
//! (`interface_util.lua` `ScriptCB_IFObj_CreateHotSpot` / flashy text intro in `common.lvl`).
//!
//! Stage breakdown (`INTRO = [0.1, 0.05, 0.05, 0.15, 0.15]`, total `0.5s`):
//! 1. `0.10s`: A narrow leading bar (`width = 4`) grows vertically from the text midline.
//! 2. `0.05s`: The bar holds full height while a solid block begins fading in.
//! 3. `0.05s`: The bar sweeps across the text width, pulling the solid block behind it.
//! 4. `0.15s`: The block fades out while the text scales vertically from `2.0` down to `1.0`.
//! 5. `0.15s`: The bar fades out while the text holds at full `1.0` alpha.

/// Stage durations in seconds (`INTRO` in `src/interface/flashy.js`).
pub const INTRO: [f64; 5] = [0.1, 0.05, 0.05, 0.15, 0.15];

/// Total intro duration in seconds (`INTRO_LENGTH` in `src/interface/flashy.js`).
pub const INTRO_LENGTH: f64 = 0.5;

/// Normalized `[0.0, 1.0]` stage progress values returned by [`TitleIntro::frame`]
/// (`TitleIntro.frame` in `src/interface/flashy.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyFrame {
    /// Stage 1 progress (`0.0..=1.0`): vertical growth of the leading cursor bar.
    pub bar: f64,
    /// Stage 3 progress (`0.0..=1.0`): horizontal sweep of the block across the label width.
    pub block: f64,
    /// Stage 4 progress (`0.0..=1.0`): vertical text settle and block fade-out.
    pub text: f64,
    /// Stage 5 progress (`0.0..=1.0`): final bar fade-out after the text has settled.
    pub alpha: f64,
}

/// Rectangular bar or sweep block emitted during a [`TitleIntro`] reveal (`src/interface/flashy.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyBar {
    /// Left X offset relative to the text anchor point.
    pub x: f64,
    /// Top Y offset relative to the text top edge.
    pub y: f64,
    /// Horizontal width of the bar or sweep block in interface units.
    pub w: f64,
    /// Vertical height of the bar or sweep block in interface units.
    pub h: f64,
    /// Opacity multiplier (`0.0..=1.0`) for this frame.
    pub alpha: f64,
}

/// Text visibility, vertical scale, and alpha state during a [`TitleIntro`] reveal.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyText {
    /// Whether the glyph quads should be emitted this frame (`f.text > 0.0`).
    pub visible: bool,
    /// Vertical scale factor (`2.0 - f.text`), settling from `2.0` to `1.0` during stage 4.
    pub scale_y: f64,
    /// Opacity multiplier (`0.0..=1.0`) applied to the glyph tint.
    pub alpha: f64,
}

/// Combined bar, block, and text state for a single frame of [`TitleIntro::pieces`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlashyPieces {
    /// Narrow leading vertical bar (`width = 4.0`) visible until stage 5 finishes.
    pub bar: Option<FlashyBar>,
    /// Sweeping rectangular block behind the bar during stages 3 and 4.
    pub block: Option<FlashyBar>,
    /// Glyph visibility, vertical scale, and alpha state for the text string itself.
    pub text: FlashyText,
}

/// Five-stage flashy text reveal state (`TitleIntro` in `src/interface/flashy.js`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TitleIntro {
    /// Initial pre-reveal wait duration in seconds before stage 1 begins.
    pub delay: f64,
    /// Total active animation duration (`INTRO_LENGTH = 0.5` seconds).
    pub duration: f64,
    /// Elapsed time in seconds, starting at `-delay` and clamped at `duration`.
    pub elapsed: f64,
}

impl Default for TitleIntro {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl TitleIntro {
    /// Creates a new `TitleIntro` with optional start `delay` in seconds
    /// (`TitleIntro.start(delay)` in `src/interface/flashy.js`).
    #[inline]
    pub fn new(delay: f64) -> Self {
        // Clamp non-finite or negative delays to 0.0 so `elapsed` never starts past 0.0
        // or gets stuck on NaN when `step` advances the timer.
        let d = if delay.is_finite() && delay > 0.0 {
            delay
        } else {
            0.0
        };
        Self {
            delay: d,
            duration: INTRO_LENGTH,
            elapsed: -d,
        }
    }

    /// Creates a completed `TitleIntro` (`elapsed == duration`) so static headers
    /// and titles render immediately at full width/alpha (`new TitleIntro()` in `flashy.js`).
    #[inline]
    pub fn completed() -> Self {
        Self {
            delay: 0.0,
            duration: INTRO_LENGTH,
            elapsed: INTRO_LENGTH,
        }
    }

    /// Resets the animation back to `-delay` (`TitleIntro.start(delay)` in `src/interface/flashy.js`).
    #[inline]
    pub fn reset(&mut self) {
        self.elapsed = -self.delay;
    }

    /// Advances the intro clock by `dt` seconds (`TitleIntro.update(dt)` in `src/interface/flashy.js`).
    #[inline]
    pub fn step(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.elapsed = (self.elapsed + dt).min(self.duration);
    }

    /// Returns `true` once the reveal animation has completed (`TitleIntro.done` in `flashy.js`).
    #[inline]
    pub fn done(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Returns the `[0.0, 1.0]` normalized stage weights `{ bar, block, text, alpha }`
    /// (`TitleIntro.frame` in `src/interface/flashy.js`).
    #[inline]
    pub fn frame(&self) -> FlashyFrame {
        let [d0, d1, d2, d3, d4] = INTRO;
        let t = self.elapsed;
        // Stage 1 (`0..d0`): leading bar grows; stage 2 (`d0..d0+d1`) holds `bar = 1.0`
        // before stage 3 (`t2 > 0`) begins sweeping `block` across the label width.
        let bar = if t <= 0.0 { 0.0 } else { (t / d0).min(1.0) };
        let t2 = t - d0 - d1;
        let block = if t2 <= 0.0 { 0.0 } else { (t2 / d2).min(1.0) };
        let t3 = t2 - d2;
        let text = if t3 <= 0.0 { 0.0 } else { (t3 / d3).min(1.0) };
        let t4 = t3 - d3;
        let alpha = if t4 <= 0.0 { 0.0 } else { (t4 / d4).min(1.0) };
        FlashyFrame {
            bar,
            block,
            text,
            alpha,
        }
    }

    /// Evaluates the current bar, block, and text pieces for a label of `(text_width, text_height)`
    /// with horizontal alignment `align` (`0` = left, `1` = right, `2` = center).
    pub fn pieces(&self, text_width: f64, text_height: f64, align: u32) -> FlashyPieces {
        let f = self.frame();
        // Resolve the left edge of the text bounding box from the alignment code so
        // the bar and sweep block always travel left-to-right across the rendered glyphs.
        let x0 = match align {
            1 => -text_width,
            2 => -text_width * 0.5,
            _ => 0.0,
        };
        let bar_alpha = 1.0 - f.alpha;
        let block_alpha = (1.0 - f.text) * 0.8;
        // Center the 4-unit-wide leading bar on the sweeping right edge of `block`
        // and expand it vertically from the text midline during stage 1.
        let bar = if f.bar > 0.0 && bar_alpha > 0.0 {
            let h = text_height * f.bar;
            Some(FlashyBar {
                x: x0 + f.block * text_width - 2.0,
                y: (text_height - h) * 0.5,
                w: 4.0,
                h,
                alpha: bar_alpha,
            })
        } else {
            None
        };
        // Trail a translucent block from `x0` to the leading bar's position during
        // stage 3, then fade it out as `f.text` rises from 0 to 1 in stage 4.
        let block = if f.block > 0.0 && block_alpha > 0.0 {
            Some(FlashyBar {
                x: x0,
                y: 0.0,
                w: text_width * f.block,
                h: text_height,
                alpha: block_alpha,
            })
        } else {
            None
        };
        let text = if f.text > 0.0 {
            FlashyText {
                visible: true,
                scale_y: 2.0 - f.text,
                alpha: f.text,
            }
        } else {
            FlashyText {
                visible: false,
                scale_y: 1.0,
                alpha: 0.0,
            }
        };
        FlashyPieces { bar, block, text }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies all five stages of `TitleIntro::frame` and `TitleIntro::pieces`.
    #[test]
    fn title_intro_stages_match_js() {
        let mut intro = TitleIntro::new(0.05);
        assert!(!intro.pieces(120.0, 20.0, 0).text.visible);
        assert!(intro.pieces(120.0, 20.0, 0).bar.is_none());

        intro.step(0.05); // elapsed = 0.0
        intro.step(0.05); // stage 1 (bar growing)
        let s1 = intro.pieces(120.0, 20.0, 0);
        assert!(s1.bar.is_some());
        assert!(s1.block.is_none());
        assert!(!s1.text.visible);

        intro.step(0.07); // elapsed = 0.12 (stage 2: hold)
        let s2 = intro.pieces(120.0, 20.0, 0);
        assert!(s2.bar.is_some());
        assert!(s2.block.is_none());

        intro.step(0.06); // elapsed = 0.18 (stage 3: sweep)
        let s3 = intro.pieces(120.0, 20.0, 0);
        assert!(s3.bar.is_some());
        assert!(s3.block.is_some());
        assert!(!s3.text.visible);

        intro.step(0.10); // elapsed = 0.28 (stage 4: text vertical settle + block fade)
        let s4 = intro.pieces(120.0, 20.0, 0);
        assert!(s4.bar.is_some());
        assert!(s4.block.is_some());
        assert!(s4.text.visible);
        assert!(s4.text.scale_y > 1.0 && s4.text.scale_y < 2.0);

        intro.step(0.30); // elapsed = 0.50 (done)
        assert!(intro.done());
        let done = intro.pieces(120.0, 20.0, 0);
        assert!(done.bar.is_none());
        assert!(done.block.is_none());
        assert!(done.text.visible);
        assert!((done.text.alpha - 1.0).abs() < 1e-9);
    }
}
