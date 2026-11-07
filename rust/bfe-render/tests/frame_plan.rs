//! Graphics.render's pre-deletion main schedules, captured without content.
//! These compare producer decisions, independently of shader-input replay.
use bfe_render::frame_plan::{FramePlan,layout::{facts as F,state as S,role as R,operation as O,limit as L}};
use serde::Deserialize;
#[derive(Deserialize,Debug)]
struct Case {shadows:usize,frame:usize,dirty:usize,moved:usize,temporal:usize,view:usize,trace:Vec<[usize;3]>}
#[derive(Deserialize)]struct Fixtures{cases:Vec<Case>}
#[test]
fn main_schedule_matches_frozen_graphics_render(){
    let fixture:Fixtures=serde_json::from_str(include_str!("fixtures/frame-main.json")).unwrap();
    assert_eq!(fixture.cases.len(),160);
    for case in fixture.cases {
        let mut p=FramePlan::default();p.facts[F::MAIN]=1.;p.facts[F::TEMPORAL]=case.temporal as f64;p.facts[F::VIEW_SHOWN]=case.view as f64;
        p.facts[F::SHADOW_COUNT]=case.shadows as f64;p.state[S::FRAME]=case.frame as f64;
        for i in 0..4{p.facts[F::SHADOW_DIRTY+i]=case.dirty as f64;p.facts[F::SHADOW_MOVED+i]=case.moved as f64;}
        p.select();p.plan();
        let actual:Vec<_>=p.operations[..p.count].iter().map(|o|[o.kind,o.role,o.argument]).collect();
        assert_eq!(actual,case.trace,"{case:?}");
    }
}
#[test]
fn probe_faces_precede_mipmaps_and_held_mirrors_preserve_prior_maps(){
    let mut p=FramePlan::default();p.facts[F::MAIN]=1.;p.facts[F::PROBE_ENABLED]=1.;p.facts[F::PROBE_TIME]=10.;p.facts[F::PROBE_EYE+1]=20.;
    p.facts[F::MIRROR_ENABLED]=1.;p.facts[F::MIRROR_VISIBLE]=1.;p.facts[F::MIRROR_EYE+1]=10.;p.facts[F::MIRROR_FORWARD+2]=-1.;p.facts[F::MIRROR_CLIP_VALID]=1.;
    p.select();p.plan();
    for i in 0..6{assert_eq!(p.operations[2*i].kind,O::BIND);assert_eq!(p.operations[2*i].role,R::PROBE0+i);assert_eq!(p.operations[2*i+1].kind,O::DRAW);}
    assert_eq!(p.operations[12].kind,O::MIPMAP);
    p.complete(true);p.select();assert_eq!(p.state[S::PROBE_CAPTURE],0.);assert_eq!(p.state[S::MIRROR_HELD],1.);
    p.plan();assert!(p.operations[..p.count].iter().all(|o|o.role!=R::MIRROR0_FAR));
    p.facts[F::PROBE_TIME]=10.5;p.select();assert_eq!(p.state[S::PROBE_CAPTURE],0.);
    p.facts[F::PROBE_EYE+1]+=4.;p.select();assert_eq!(p.state[S::PROBE_CAPTURE],1.);
    p.complete(true);p.facts[F::PROBE_TIME]=11.5;p.select();assert_eq!(p.state[S::PROBE_CAPTURE],1.);
    p.complete(true);p.facts[F::PROBE_ENABLED]=0.;p.select();assert_eq!(p.state[S::PROBE_CAPTURE],0.);
}
#[test]
fn maximum_and_malformed_facts_clip_without_panicking(){
    let mut p=FramePlan::default();p.facts.fill(f64::MAX);p.facts[F::MAIN]=1.;p.facts[F::WATER_EYE_Y]=-100.;p.facts[F::WATER_LEVEL]=0.;
    for i in 0..L::MIRRORS{p.facts[F::MIRROR_LOW+i]=0.;p.facts[F::MIRROR_EYE+3*i+1]=10.;p.facts[F::MIRROR_LEVEL+i]=0.;}
    p.select();p.plan();assert!(p.count<=L::OPERATIONS);
    let frame=p.state[S::FRAME];p.state[S::CLIPPED]=1.;p.complete(true);assert_eq!(p.state[S::FRAME],frame);
    p.facts.fill(f64::NAN);p.select();p.plan();assert!(p.count<=L::OPERATIONS);
}

// A thread-local counter isolates this check from other fixture-test threads.
// It catches accidental allocation from mem::take(Default) as well as sorting.
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell};
thread_local!{static COUNT:Cell<(bool,usize)>=const{Cell::new((false,0))};}
struct CountAlloc;
unsafe impl GlobalAlloc for CountAlloc{
    unsafe fn alloc(&self,l:Layout)->*mut u8{COUNT.with(|c|{let(on,n)=c.get();if on{c.set((true,n+1));}});System.alloc(l)}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{COUNT.with(|c|{let(on,n)=c.get();if on{c.set((true,n+1));}});System.realloc(p,l,n)}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
}
#[global_allocator]static ALLOCATOR:CountAlloc=CountAlloc;
#[test]
fn warm_native_schedule_and_stable_line_merge_allocate_nothing(){
    use bfe_render::transparent::{List,Item};
    let mut p=FramePlan::default();p.facts[F::MAIN]=1.;p.facts[F::TEMPORAL]=1.;
    let mut list=List::reserved();
    // Equal keys preserve line-before-source order, including odd run lengths.
    for i in 0..513{list.items.push(Item{source:(i%5) as u32,..Item::default()});}
    list.order(&[],0);
    COUNT.with(|c|c.set((true,0)));
    for _ in 0..10000{
        let empty=std::mem::take(&mut list);list=empty;
        p.select();p.plan();p.complete(true);list.order(&[],0);
    }
    let allocations=COUNT.with(|c|{let(_,n)=c.get();c.set((false,0));n});
    assert_eq!(allocations,0);assert_eq!(p.state[S::FRAME],10000.);
    for(i,item)in list.items.iter().enumerate(){assert_eq!(item.source,(i%5)as u32);}
}

#[test]
fn ui_execution_uses_semantic_slots_instead_of_publication_order(){
    use bfe_render::frame_plan::layout::{ui as U,kind as K};
    let mut p=FramePlan::default();p.facts[F::MAIN]=1.;
    // The later-prepared "before" view still executes before the native HUD.
    p.facts[F::HUD_KINDS+U::NATIVE]=K::HUD as f64;
    p.facts[F::HUD_KINDS+U::BEFORE]=K::INTERFACE as f64;
    p.select();p.plan();
    let before=p.operations[..p.count].iter().position(|o|o.kind==O::DRAW&&o.role==R::HUD0).unwrap();
    let native=p.operations[..p.count].iter().position(|o|o.kind==O::DRAW&&o.role==R::HUD0+U::NATIVE).unwrap();
    assert!(before<native);assert_eq!(p.operations[before-1].kind,O::BIND);
}

#[test]
fn water_readers_and_fog_take_distinct_surface_snapshots(){
    let mut p=FramePlan::default();p.facts[F::MAIN]=1.;p.facts[F::SURFACE_COPY]=1.;
    p.facts[F::WATER_OVER]=1.;p.facts[F::WATER_SCALE]=1.;p.facts[F::WATER_EYE_Y]=-1.;
    p.select();p.plan();
    let transparent=p.operations[..p.count].iter().position(|o|o.kind==O::DRAW&&o.role==R::TRANSPARENT).unwrap();
    let fog=p.operations[..p.count].iter().position(|o|o.kind==O::DRAW&&o.role==R::FOG).unwrap();
    let copies:Vec<_>=p.operations[..p.count].iter().enumerate().filter(|(_,o)|o.kind==O::COPY_COLOR_DEPTH&&o.argument==R::SURFACE).map(|(i,_)|i).collect();
    assert_eq!(copies.len(),2);assert!(copies[0]<transparent&&transparent<copies[1]&&copies[1]<fog);
    p.facts[F::SURFACE_COPY]=0.;p.select();p.plan();
    assert_eq!(p.operations[..p.count].iter().filter(|o|o.kind==O::COPY_COLOR_DEPTH&&o.argument==R::SURFACE).count(),1);
}
