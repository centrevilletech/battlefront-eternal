//! Classic spawn screen layout (`src/spawn-screen/classic/layout.js`).
//!
//! Places the Classic spawn screen's pieces in the HUD's `1024xH` interface
//! space inside the 90% safe area (`SAFE = 0.05`, `SAFE_SCALE = 0.9`), checked
//! against the 2004 game's Lua (`ifs_spawndisplay`) and C++ and verified against
//! the retail screenshots:
//! - Five class windows down the left, turned `35 deg` about their vertical axis
//!   (right edge away), `0.35 x 0.13` of the safe area, a title bar's height (`fh`)
//!   plus 10 units apart.
//! - The objectives and spawn point windows at the right, top and bottom,
//!   turned `-20 deg`, `0.3 x 0.15` of the safe area.
//! - `SPAWN` at the bottom middle, `0.2 x 0.12` of the safe area, tipped `45 deg`
//!   about its horizontal axis (top edge away); the word `SPAWN` itself lies flat.
//! - The team logos at the top middle (`0.45` and `0.55` across the safe area,
//!   `0.13` down), the selected team's on the left.
//! - The class's standing soldier body at `(W / 2, H / 1.2)` regardless of safe area.

use std::f64::consts::PI;

use crate::shapes::{GameFamily, GameFamilyError};

pub const W: f64 = 1024.0;
pub const SAFE: f64 = 0.05;
pub const SAFE_SCALE: f64 = 0.9;
pub const CLASS_TILT: f64 = 35.0 * PI / 180.0;
pub const SIDE_TILT: f64 = -20.0 * PI / 180.0;
pub const SPAWN_TILT: f64 = 45.0 * PI / 180.0;
pub const SLOTS: usize = 5;

/// Seconds to wait for the world to realize a committed spawn request before
/// withdrawing it (`src/spawn-screen/index.js` `COMMIT_WAIT = 1`).
pub const COMMIT_WAIT: f64 = 1.0;

/// Safe-area rectangle in interface units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SafeRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// One of the 5 soldier class windows on the left side of the spawn screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassSlotLayout {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub tilt: f64,
    /// C++ hit box `[x0, y0, x1, y1]` relative to `(x, y)` before projection:
    /// `[-0.175 * W, -(0.065 * H + 32), 0.175 * W, 0.065 * H]`.
    pub hit: [f64; 4],
    /// Height available for the weapon lines (`0.08 * sh`).
    pub text_h: f64,
}

/// Tilted window box (`objectives`, `post`, or `spawn`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TiltedBoxLayout {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub tilt: f64,
}

/// Hotspot rectangle centered at `(x, y)` with half-extents `(half_x, half_y)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogoHitBox {
    pub x: f64,
    pub y: f64,
    pub half_x: f64,
    pub half_y: f64,
}

/// Positions, scales, and hotspot of the two spinning team logos.
///
/// The original game's hotspot is a 38.4-unit square between the logos that only
/// reaches their inner halves; this layout uses a wider box covering both logos
/// (`half_x = 0.05 * sw + 60`, `half_y = 55`) while staying clear of the windows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LogosLayout {
    pub own: [f64; 2],
    pub other: [f64; 2],
    pub big: f64,
    pub small: f64,
    pub hit: LogoHitBox,
}

/// Complete Classic spawn screen layout (`src/spawn-screen/classic/layout.js`
/// `classicLayout`).
///
/// Every vertical coordinate scales with interface height `h` (`768` at 4:3,
/// `576` at 16:9) so widescreen aspect ratios preserve the 90% safe-area framing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClassicSpawnLayout {
    pub h: f64,
    pub fh: f64,
    pub safe: SafeRect,
    pub slots: [ClassSlotLayout; SLOTS],
    /// Scale at which the C++ draws weapon lists (`(0.13 * H) / 135`).
    pub info_scale: f64,
    pub objectives: TiltedBoxLayout,
    pub post: TiltedBoxLayout,
    pub spawn: TiltedBoxLayout,
    pub logos: LogosLayout,
    /// Interface coordinates `[x, y]` of the standing preview soldier's feet
    /// (`[W / 2, H / 1.2]`).
    pub preview: [f64; 2],
}

/// Selects the spawn screen layout for `family` (`MULTIPLAYER.md`).
///
/// Only [`GameFamily::Swbf1`] (2004) is implemented; [`GameFamily::Swbf2`] returns an
/// explicit [`GameFamilyError`] and never silently falls back to the 2004 layout.
pub fn spawn_layout_for_family(
    family: GameFamily,
    h: f64,
    fh: f64,
) -> Result<ClassicSpawnLayout, GameFamilyError> {
    family.require_available()?;
    Ok(classic_layout(h, fh))
}

/// Computes the Classic spawn screen layout for interface height `h` and
/// `gamefont_tiny` line height `fh` (`src/spawn-screen/classic/layout.js`
/// `classicLayout`).
pub fn classic_layout(h: f64, fh: f64) -> ClassicSpawnLayout {
    let h = if h > 0.0 { h } else { 768.0 };
    let fh = if fh > 0.0 { fh } else { 18.0 };
    let sw = SAFE_SCALE * W;
    let sh = SAFE_SCALE * h;
    let ox = SAFE * W;
    let oy = SAFE * h;
    let slot_w = 0.35 * sw;
    let slot_h = 0.13 * sh;

    let mut slots = [ClassSlotLayout {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
        tilt: 0.0,
        hit: [0.0; 4],
        text_h: 0.0,
    }; SLOTS];

    for (i, slot) in slots.iter_mut().enumerate() {
        *slot = ClassSlotLayout {
            x: ox + 0.175 * sw,
            y: oy + 0.125 * sh + (i as f64) * (slot_h + fh + 10.0),
            w: slot_w,
            h: slot_h,
            tilt: CLASS_TILT,
            hit: [-0.175 * W, -(0.065 * h + 32.0), 0.175 * W, 0.065 * h],
            text_h: 0.08 * sh,
        };
    }

    let side_w = 0.3 * sw;
    let side_h = 0.15 * sh;

    ClassicSpawnLayout {
        h,
        fh,
        safe: SafeRect {
            x: ox,
            y: oy,
            w: sw,
            h: sh,
        },
        slots,
        info_scale: (0.13 * h) / 135.0,
        objectives: TiltedBoxLayout {
            x: ox + 0.85 * sw,
            y: oy + 0.125 * sh,
            w: side_w,
            h: side_h,
            tilt: SIDE_TILT,
        },
        post: TiltedBoxLayout {
            x: ox + 0.85 * sw,
            y: oy + 0.925 * sh,
            w: side_w,
            h: side_h,
            tilt: SIDE_TILT,
        },
        spawn: TiltedBoxLayout {
            x: ox + 0.5 * sw,
            y: oy + sh - 0.12 * sh + 50.0,
            w: 0.2 * sw,
            h: 0.12 * sh,
            tilt: SPAWN_TILT,
        },
        logos: LogosLayout {
            own: [ox + 0.45 * sw, oy + 0.13 * sh],
            other: [ox + 0.55 * sw, oy + 0.13 * sh],
            big: 0.5 * (sw / 800.0),
            small: 0.25 * (sw / 800.0),
            hit: LogoHitBox {
                x: ox + 0.5 * sw,
                y: oy + 0.13 * sh,
                half_x: 0.05 * sw + 60.0,
                half_y: 55.0,
            },
        },
        preview: [W * 0.5, h / 1.2],
    }
}
