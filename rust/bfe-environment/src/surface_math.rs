//! Pixel-visible arithmetic from Water.update, Water.updateRings and
//! WaterShore.update. Electron supplies only standard scalar primitives,
//! following the camera phase's host-math boundary.

// Native replay uses the server's V8 arithmetic, matching Node captures.
// Electron binds its own primitives to preserve the page's rounding; its
// output is checked separately by the cross-build GPU comparisons.
pub trait Math {
    fn sin(x: f64) -> f64;
    fn cos(x: f64) -> f64;
    fn tan(x: f64) -> f64;
    fn atan2(y: f64, x: f64) -> f64;
    fn hypot(x: f64, y: f64) -> f64;
}
pub struct V8;
impl Math for V8 {
    fn sin(x: f64) -> f64 { v8math::sin(x) }
    fn cos(x: f64) -> f64 { v8math::cos(x) }
    fn tan(x: f64) -> f64 { v8math::tan(x) }
    fn atan2(y: f64, x: f64) -> f64 { v8math::atan2(y, x) }
    fn hypot(x: f64, y: f64) -> f64 { v8math::hypot(x, y) }
}

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "surface_math")]
extern "C" {
    fn surface_sin(x: f64) -> f64;
    fn surface_cos(x: f64) -> f64;
    fn surface_tan(x: f64) -> f64;
    fn surface_atan2(y: f64, x: f64) -> f64;
    fn surface_hypot(x: f64, y: f64) -> f64;
}
#[cfg(target_arch = "wasm32")]
pub struct Page;
#[cfg(target_arch = "wasm32")]
impl Math for Page {
    // SAFETY: these imports are the page's scalar Math builtins. They cannot
    // touch phase storage or re-enter any environment export.
    fn sin(x: f64) -> f64 { unsafe { surface_sin(x) } }
    fn cos(x: f64) -> f64 { unsafe { surface_cos(x) } }
    fn tan(x: f64) -> f64 { unsafe { surface_tan(x) } }
    fn atan2(y: f64, x: f64) -> f64 { unsafe { surface_atan2(y, x) } }
    fn hypot(x: f64, y: f64) -> f64 { unsafe { surface_hypot(x, y) } }
}
