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
    for p in phase.shore.points[..phase.shore.count*4].as_chunks::<4>().0 { assert_eq!(p[0],0.0); assert!(p[2] > 0.99); }
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
    assert!(wakes.points.as_chunks::<4>().0.iter().all(|p|p[3]==-100.0));
    assert_eq!(ALLOCATIONS.with(Cell::get),before);
}

// Classic Debris.update is a retained presentation phase too. A paused burst
// must not grow its bank, and ordinary motion/retirement allocates nothing.
#[test]
fn classic_debris_full_burst_and_view_queries_do_not_allocate() {
    use bfe_environment::{debris::{Phase as Debris,Host,Hit},surface_policy::Policy};
    use bfe_environment::layout::{debris as limits,debris_launch as launch,water_view as view};
    use three::{Vector3,Quaternion,Matrix4};
    struct Scene;
    impl Host for Scene {
        fn cast(&mut self,_:usize,_:Vector3,_:Vector3,_:f64)->Option<Hit>{None}
        fn low(&mut self,_:usize,_:Quaternion)->f64{0.}
        fn event(&mut self,_:usize,_:f64,_:&[f64]){}
    }
    let mut debris=Debris::new();let mut scene=Scene;
    debris.input[launch::MATRIX..launch::MATRIX+16].copy_from_slice(&Matrix4::default().elements);
    debris.input[launch::HEALTH] = -50.;debris.input[launch::SPEED]=8.;
    let mut policy=Policy::default();policy.configure(&serde_json::json!({"grid":1,"cell":32,"height":0,"offset":[0,0],"cells":[[0,0]],"look":{}}));
    policy.input[view::FAR]=1500.;policy.input[view::FADE]=150.;policy.input[view::FADE+1]=200.;policy.input[view::FOV]=50.;policy.input[view::ASPECT]=1.5;policy.input[view::NEAR]=0.5;
    policy.input[view::INVERSE..view::INVERSE+16].copy_from_slice(&Matrix4::default().elements);
    let before=ALLOCATIONS.with(Cell::get);
    for id in 0..limits::CAPACITY {debris.launch(id,&mut scene);}
    debris.launch(usize::MAX,&mut scene);
    for _ in 0..2400{debris.update(1./60.,&mut scene);policy.update::<V8>();}
    assert_eq!(ALLOCATIONS.with(Cell::get)-before,0);
    assert_eq!(debris.free(),0);
}

// Admission may allocate, but publication and live rain mode changes reuse
// retained storage. Exercise all producers in a long frame sequence, rather
// than testing a helper independently of its real phase outputs.
#[test]
fn admitted_effects_and_draw_publication_do_not_allocate(){
    use bfe_environment::{admission,publication::Publication,WeatherPhase,water_draw::Draws,surface_policy::Policy};
    use bfe_environment::layout::{weather_mode as mode,sky_frame as sf,water_view as view};
    use serde_json::json;
    let mut rows=Publication::new();
    let rain_data=json!({"precipitation":{"Range":[12.5],"ParticleDensity":[256],"Velocity":[9.]},"wind":[1.,2.]});
    rows.weather(&rain_data,true);let m=rows.weather_mode;
    let mut rain=WeatherPhase::new(m[mode::CAPACITY] as usize,true,m[mode::RANGE],m[mode::SIZE] as usize);
    let(mut snow,snow_recipe)=admission::snow(&json!({"precipitation":{"ParticleDensity":[8],"Range":[4.]}}));rows.snow(&snow_recipe,snow.snow.as_ref().unwrap().cover.cell);
    let(mut atmosphere,atmosphere_recipe)=admission::atmosphere(&json!({"data":{"dust":{"classes":[],"instances":[]}},"available":{},"fog":false}));rows.atmosphere(&atmosphere_recipe);
    let(mut sky,_)=admission::sky(&json!({"data":{"flyers":[]},"models":{}}));
    let(mut sources,_)=admission::sources(&json!({"instances":[],"basins":[],"effects":[]}));rows.fountain(&json!({"basins":[],"tier":true}));
    let mut water=Phase::new(true);let mut draws=Draws::default();let mut policy=Policy::default();let mut wall=Wall{casts:0,gathers:0,dry:false,excess:false};
    rain.frame[bfe_environment::layout::weather_frame::DT]=1./60.;snow.view[bfe_environment::layout::view::DT]=1./60.;sky.frame[sf::DT]=1./60.;
    let before=ALLOCATIONS.with(Cell::get);
    for i in 0..2400{
        if i%120==0{rows.mode(i%240==0);let m=rows.weather_mode;rain.weather.set_mode(m[mode::ENHANCED]!=0.,m[mode::RANGE],m[mode::SIZE] as usize);}
        rain.update(&mut |_,_,_|0.,&mut |_,_,_|bfe_environment::weather::Impact{y:0.,normal:[0.,1.,0.],water:true});rows.weather_frame(&rain.result);
        snow.update(&mut |_,_,_|0.);rows.snow_frame(&snow.result);atmosphere.update(&mut |_,_|0.);rows.atmosphere_frame(&atmosphere.result);
        sky.update();rows.sky_frame(&sky);assert_eq!(rows.sky_motion(i as f64),0.);assert_eq!(rows.sky_motion(i as f64),0.);assert_eq!(rows.sky_motion(i as f64+0.5),1.);
        sources.update();rows.fountain_frame(&sources);water.update::<V8,_>(&mut wall);rows.shore_frame(&water);
        policy.input[view::SCALE]=0.5;policy.input[view::MAX_SIZE]=1024.;policy.input[view::SIZE]=1200.;policy.input[view::SIZE+1]=800.;policy.update::<V8>();
        for slot in 0..6{draws.prepare(slot,&water.result,&policy.result);}
    }
    assert_eq!(ALLOCATIONS.with(Cell::get),before);
}
