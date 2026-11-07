//! Synthetic whole-phase fixtures for the water/shore and wake owners. These
//! exercise frame calls and capacity behavior without map assets or a browser.
use bfe_environment::{shore::{Queries, waterline_segment}, surfaces::Phase, surface_math::V8, wakes::Wakes};
use bfe_environment::layout::{surface as lim, surface_config as cfg, surface_frame as frame,
    surface_command as cmd, surface_command_kind as kind, surface_result as out,
    wake_actor as actor, wake_frame as wf, wake_result as wr};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
struct Counting;
thread_local! { static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { ALLOCATIONS.with(|n| n.set(n.get()+1)); System.alloc(layout) }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) { System.dealloc(p,layout) }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, n: usize) -> *mut u8 { ALLOCATIONS.with(|n| n.set(n.get()+1)); System.realloc(p,layout,n) }
}
#[global_allocator] static ALLOCATOR: Counting = Counting;
struct Wall { casts: usize, gathers: usize, dry: bool, excess: bool }
impl Queries for Wall {
    fn triangles(&mut self, _: f64, _: f64, _: f64, out: &mut [f64]) -> usize {
        self.gathers += 1;
        let rows = [0.0,-2.0,-20.0,0.0,2.0,-20.0,0.0,2.0,20.0,
            0.0,-2.0,-20.0,0.0,2.0,20.0,0.0,-2.0,20.0];
        out[..18].copy_from_slice(&rows);
        if self.excess { lim::SHORE_TRIANGLES+100 } else { 2 }
    }
    fn exposed(&mut self, x: f64, _: f64, _: f64) -> bool { self.casts += 1; !self.dry || x >= 0.0 }
}
#[test]
fn intersections_retain_endpoints_and_ignore_coplanar_triangles() {
    let line = waterline_segment(&[[0.0,-2.0,-4.0],[0.0,2.0,-4.0],[0.0,2.0,4.0]],0.0).unwrap();
    assert_eq!(line,[[0.0,0.0,-4.0],[0.0,0.0,0.0]]);
    assert_eq!(waterline_segment(&[[0.0,0.0,-4.0],[1.0,0.0,-4.0],[0.0,0.0,4.0]],0.0),None);
}
#[test]
fn whole_surface_phase_caches_shore_and_does_not_allocate() {
    let mut phase = Phase::new(true); let mut wall = Wall { casts:0,gathers:0,dry:true,excess:false };
    phase.frame[frame::DT] = 1.0/60.0; phase.frame[frame::SHORE_QUALITY] = 1.0;
    phase.frame[frame::EYE] = 2.0; phase.frame[frame::EYE+1] = 3.0;
    phase.config[cfg::BOBBING] = 1.0;
    let before = ALLOCATIONS.with(Cell::get);
    for _ in 0..60 { phase.update::<V8,_>(&mut wall); }
    let gathers = wall.gathers;
    for _ in 0..60 { phase.update::<V8,_>(&mut wall); }
    assert_eq!(gathers,wall.gathers);
    assert!(phase.shore.count > 0 && phase.shore.count <= 192);
    for p in phase.shore.points[..phase.shore.count*4].chunks_exact(4) { assert_eq!(p[0],0.0); assert!(p[2] > 0.99); }
    phase.frame[frame::SHORE_QUALITY] = 0.0; phase.update::<V8,_>(&mut wall);
    assert_eq!(gathers,wall.gathers);
    phase.frame[frame::SHORE_QUALITY] = 2.0;
    for _ in 0..300 { phase.frame[frame::EYE] += 8.0; phase.update::<V8,_>(&mut wall); }
    assert!(phase.shore.tiles() <= lim::SHORE_TILES);
    assert_eq!(ALLOCATIONS.with(Cell::get),before);
}
#[test]
fn paused_bursts_clamp_commands_replace_old_rings_and_never_panic() {
    let mut phase = Phase::new(true); let mut wall = Wall { casts:0,gathers:0,dry:true,excess:true };
    phase.frame[frame::COMMANDS] = (lim::COMMANDS+1000) as f64;
    for i in 0..lim::COMMANDS {
        let at=i*cmd::STRIDE;
        phase.commands[at+cmd::KIND]=kind::RIPPLE;
        phase.commands[at+cmd::LIFE]=4.0;phase.commands[at+cmd::RADIUS]=1.0;
        phase.commands[at+cmd::POSITION]=i as f64*0.001;
    }
    phase.frame[frame::DT]=0.1;phase.frame[frame::SHORE_QUALITY]=2.0;
    let before=ALLOCATIONS.with(Cell::get);
    phase.update::<V8,_>(&mut wall);
    assert_eq!(phase.result[out::QUADS],(lim::RIPPLES*lim::SEGMENTS) as f64);
    assert!(phase.result[out::OVERFLOW] > 0.0);
    phase.frame[frame::DT]=10_000.0;phase.update::<V8,_>(&mut wall);
    assert_eq!(phase.result[out::QUADS],0.0);
    assert_eq!(ALLOCATIONS.with(Cell::get),before);
}
#[test]
fn wake_identity_capacity_and_a_long_pause_keep_fixed_storage() {
    let mut wakes=Wakes::new();wakes.frame[wf::DT]=0.05;
    let before=ALLOCATIONS.with(Cell::get);
    for serial in 0..1200 {
        wakes.frame[wf::ACTORS]=5000.0;
        for i in 0..256 { let at=i*actor::STRIDE;wakes.actors[at+actor::ID]=(1+serial*256+i) as f64;wakes.actors[at+actor::RELEVANT]=1.0; }
        wakes.update(|_|true);
    }
    assert!(wakes.result[wr::OVERFLOW]>0.0);
    wakes.frame[wf::ACTORS]=1.0;wakes.actors[actor::ID]=1.0;wakes.update(|_|true);
    wakes.actors[actor::POSITION]=0.2;wakes.update(|_|true);
    assert_eq!(wakes.result[wr::CURSOR],1.0);
    wakes.clear();wakes.frame[wf::DT]=10000.0;wakes.frame[wf::ACTORS]=0.0;wakes.update(|_|true);
    assert!(wakes.points.chunks_exact(4).all(|p|p[3]==-100.0));
    assert_eq!(ALLOCATIONS.with(Cell::get),before);
}
