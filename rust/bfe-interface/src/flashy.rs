//! Five-stage flashy text reveal animation (`src/interface/flashy.js`).

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


    /// Advances the intro clock by `dt` seconds (`TitleIntro.update(dt)` in `src/interface/flashy.js`).
    #[inline]
    pub fn step(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.elapsed = (self.elapsed + dt).min(self.duration);
    }



}

#[cfg(test)]
mod tests {
    

}
