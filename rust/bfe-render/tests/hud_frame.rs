//! The complete native HUD update/build path, including non-empty bitmap text,
//! radar, messages, vehicle weapons and layer maps, must remain allocation-free.
use bfe_render::{hud_model::*, interface::*};
use layout::{HudCommand as C, HudState as S, HudTeam as T, HudFrame as H, HudMapDraw as M, HudPost as P, HudUnit as U, HudWeapon as W};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
struct Counter;
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        System.alloc(layout)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        System.realloc(p, l, n)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

fn event(h: &mut HudModel, index: usize, kind: f64, a: f64, b: f64, c: f64, d: f64) {
    let _ = index;
    h.event(kind as u32, a, b, c, d);
}
fn fixture() -> HudModel {
    let mut h = HudModel::new();
    for (i, a) in h.assets.iter_mut().enumerate() {
        *a = Asset { texture: i as i32, width: 32., height: 32., ready: true };
    }
    for f in &mut h.fonts {
        f.height = 18.;
        for c in 32..127 {
            f.glyphs.insert(c, Glyph { page: 17, advance: 6., bounds: [0., 2., 5., 14.], uv: UV });
        }
    }
    let text = h.add_text("TEST MESSAGE", false);
    let f = &mut h.input;
    f[H::HEIGHT] = 720.;
    f[H::DT] = 1. / 60.;
    f[H::COUNTDOWN] = 12.;
    f[H::BATTLE] = 1.;
    f[H::OWN] = 120.;
    f[H::OWN_START] = 150.;
    f[H::FOE] = 90.;
    f[H::FOE_START] = 150.;
    f[H::OWN_BLEED] = 0.5;
    f[H::ARSENAL] = 1.;
    f[H::HEALTH] = 0.35;
    f[H::HEALTH_ICON] = 3.;
    f[H::HULL_ICON] = 4.;
    f[H::HULL_FILL] = 0.4;
    f[H::VEHICLE] = 1.;
    f[H::SEATS..H::SEATS + 5].copy_from_slice(&[1., 2., 3., 0., -1.]);
    f[H::RETICLE] = 1.;
    f[H::RETICLE_X] = 0.5;
    f[H::RETICLE_Y] = 0.45;
    f[H::FOV] = 48.;
    f[H::ASPECT] = 16. / 9.;
    f[H::MARKER] = 1.;
    f[H::MARKER_X] = 0.6;
    f[H::MARKER_Y] = 0.4;
    f[H::MARKER_PROGRESS] = 0.5;
    f[H::MAP] = 1.;
    f[H::MAP_HALF] = 500.;
    f[H::MAP_SCALE] = 2.5;
    f[H::MAP_IMAGE] = 18.;
    f[H::PLAYER] = 1.;
    f[H::SPEED] = 55.;
    f[H::POSTS] = 1.;
    f[POSTS + P::X] = 20.;
    f[POSTS + P::RELATION] = 1.;
    f[POSTS + P::MAPPED] = 1.;
    f[POSTS + P::ACTIVE] = 1.;
    f[POSTS + P::PROGRESS] = 0.5;
    f[POSTS + P::TOWARD] = 2.;
    f[POSTS + P::TOWARD_RELATION] = 2.;
    f[POSTS + P::SCALE] = 2.5;
    f[POSTS + P::LABEL] = text as f64;
    f[H::UNITS] = 2.;
    f[H::HAS_VIEWER] = 1.;
    f[H::HAS_CONTROL] = 1.;
    f[H::VIEWER_TEAM] = 1.;
    f[H::CONTROL_SCAN] = 100.;
    f[H::CONTROLLABLE] = 1.;
    for i in 0..2 {
        let o = UNITS + i * U::WORDS;
        f[o + U::ID] = (i + 1) as f64;
        f[o + U::GENERATION] = 1.;
        f[o + U::ALIVE] = 1.;
        f[o + U::TEAM] = (i + 1) as f64;
        f[o + U::TEXTURE] = 11.;
        f[o + U::SCALE] = 1.4;
        f[o + U::SCAN] = 100.;
        f[TEAMS + (i + 1) * T::WORDS + T::TICKETS] = 150.;
        f[TEAMS + (i + 1) * T::WORDS + T::LIVING] = f64::NAN;
    }
    f[TEAMS + T::WORDS + T::RELATION] = 1.;
    f[UNITS + U::WORDS + U::HAS_POSITION] = 1.;
    f[UNITS + U::WORDS + U::X] = 20.;
    f[UNITS + U::WORDS + U::VISUAL_X] = 21.;
    f[TEAMS + 2 * T::WORDS + T::RELATION] = -1.;
    for i in 0..4 {
        let o = WEAPONS + i * W::WORDS;
        f[o + W::ID] = (i + 1) as f64;
        f[o + W::ICON] = 19.;
        f[o + W::RETICLE] = 20.;
        f[o + W::LOCK_OFF_ANGLE] = 8.;
        f[o + W::LABEL] = text as f64;
        f[o + W::LOADED] = 12.;
        f[o + W::SPARE] = 30.;
        f[o + W::RESERVE] = 3.;
        f[o + W::ROUNDS_PER_CLIP] = 20.;
        f[o + W::RELOADING] = 1.;
        f[o + W::RELOAD_TIME] = 2.;
        f[o + W::RELOAD_LEFT] = 1.;
    }
    event(&mut h, 0, 1., 0., 0., 0., 0.);
    event(&mut h, 1, 8., text as f64, 2., 0., 0.);
    event(&mut h, 2, 7., 1., 0., 0., 0.);
    event(&mut h, 3, 7., 2., 0., 1., 3.);
    event(&mut h, 4, 11., 2., 0., 0., 0.);
    event(&mut h, 5, 6., 0.5, 3., 0., 0.);
    event(&mut h, 6, 13., 2., 0., 0., 0.);
    h
}
#[test]
fn complete_frames_retain_storage_and_emit_golden_geometry() {
    let mut h = fixture();
    h.update();
    h.build(0);
    assert!(h.batch.count > 100, "expected a populated HUD, got {} quads", h.batch.count);
    let initial = h.batch.count;
    assert_eq!(h.batch.draws[0].texture, 18);
    // Frozen minimap at yaw zero: first corner uses the turned texture's
    // (upper-right) UV while its mask remains (0,0).
    assert_eq!(&h.batch.uv[..2], &[0.6875, 0.6875]);
    assert_eq!(&h.batch.mask_uv[..2], &[0., 0.]);
    h.build(0);
    assert!(h.batch.count >= initial);
    ALLOCATIONS.store(0, Ordering::Relaxed);
    TRACK.store(true, Ordering::Relaxed);
    for frame in 0usize..240 {
        // A slow renderer can receive many game updates and loading previews
        // before it draws. Latest-value rows stay fixed; real events are applied
        // immediately, so their volume cannot increase a pending queue.
        for preview in 0..1024 {
            let o = STATES + C::SECOND as usize * S::WORDS;
            h.input[o] = HEALTH as f64;
            h.input[o + 1] = preview as f64 / 1024.;
            h.input[H::STATES] = (1 << C::SECOND) as f64;
        }
        for _ in 0..1024 {
            h.event(C::MESSAGE, 1., 2., 0., 0.);
            h.event(C::KILL, 3., 1., 1., 2.);
            h.event(C::HIT, 2., 0., 0., 0.);
            h.event(C::SHOT, 2., 1., 0.2, 0.);
        }
        for _ in 0..8 { h.update(); }
        event(&mut h, 0, 2., 0.25 + (frame % 10) as f64 * 0.01, 0., 0., 0.);
        event(&mut h, 1, 12., 2., 1., 0.2, 0.);
        if frame.is_multiple_of(20) {
            event(&mut h, 2, 8., 1., 2., 0., 0.);
        }
        h.update();
        h.build(0);
        h.input[H::DEAD] = 1.;
        h.build(0);
        h.input[H::DEAD] = 0.;
        h.input[H::LAYER] = 1.;
        h.input[H::SPAWN_MAP] = 1.;
        h.build(0);
        h.build(1);
        h.input[H::LAYER] = 0.;
        h.input[MAP_DRAW + M::MATRIX..MAP_DRAW + M::MATRIX + 16].copy_from_slice(&IDENTITY);
        h.input[MAP_DRAW + M::SIZE] = 400.;
        h.build(2);
        h.input[H::COVERED] = 1.;
        h.build(0);
        h.input[H::COVERED] = 0.;
    }
    TRACK.store(false, Ordering::Relaxed);
    assert_eq!(ALLOCATIONS.load(Ordering::Relaxed), 0, "native HUD allocated during frames");
}
