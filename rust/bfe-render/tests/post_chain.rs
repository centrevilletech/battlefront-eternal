//! Full post policy goldens recorded from synthetic pre-migration frames.
//! Active f64 words are hashed in execution order; shader-only replay would
//! miss a planner choosing the wrong pass, target, uniform or history bank.
use bfe_render::{post, post_chain::{PostChain, layout::{facts as F, state as S, resource as R, program as P}}};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counter;
static TRACK: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) { ALLOCATIONS.fetch_add(1,Ordering::Relaxed); }
        System.alloc(layout)
    }
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {
        if TRACK.load(Ordering::Relaxed) { ALLOCATIONS.fetch_add(1,Ordering::Relaxed); }
        System.realloc(p,l,n)
    }
    unsafe fn dealloc(&self,p:*mut u8,l:Layout) {System.dealloc(p,l);}
}
#[global_allocator]
static ALLOCATOR: Counter = Counter;

#[derive(Deserialize)]
struct Camera { projection:[f64;16],view:[f64;16],world:[f64;16] }
#[derive(Deserialize,Debug)]
#[serde(rename_all="camelCase")]
struct Case { mode:usize,aa:usize,blur:usize,bloom:usize,motion:usize,amount:f64,turn:f64,valid:usize,width:f64,height:f64,out_width:f64,out_height:f64,hash:String,steps:usize }
#[derive(Deserialize)]
struct Fixtures {camera:Camera,cases:Vec<Case>}
fn fixture(c:&Case,camera:&Camera)->PostChain {
    let mut p=PostChain::default();let f=&mut p.input;
    f[F::MODE]=c.mode as f64;f[F::AA]=c.aa as f64;f[F::SOFT_FOCUS]=(c.blur==2) as u8 as f64;f[F::BLUR_ENABLED]=(c.blur>0) as u8 as f64;f[F::BLUR_BLEND]=0.3;
    f[F::BLOOM]=c.bloom as f64;f[F::SPEED_ENABLED]=(c.motion>0) as u8 as f64;f[F::SPEED_AMOUNT]=c.amount;f[F::SPEED_TURN]=c.turn;f[F::SHARPNESS]=0.4;
    f[F::INPUT_WIDTH]=c.width;f[F::INPUT_HEIGHT]=c.height;f[F::OUTPUT_WIDTH]=c.out_width;f[F::OUTPUT_HEIGHT]=c.out_height;
    f[F::CANVAS_WIDTH]=c.out_width;f[F::CANVAS_HEIGHT]=c.out_height;
    f[F::PROJECTION..F::PROJECTION+16].copy_from_slice(&camera.projection);f[F::VIEW..F::VIEW+16].copy_from_slice(&camera.view);f[F::WORLD..F::WORLD+16].copy_from_slice(&camera.world);
    for i in 0..P::COUNT {f[F::PROGRAMS+i]=i as f64;}
    for i in 0..R::COUNT {
        let a=F::RESOURCES+i*R::STRIDE;f[a+R::TEXTURE]=(100+i) as f64;f[a+R::TARGET]=i as f64;
        f[a+R::WIDTH]=c.width;f[a+R::HEIGHT]=c.height;
        if i==R::QUARTER {f[a+R::WIDTH]=(c.width*0.25).round();f[a+R::HEIGHT]=(c.height*0.25).round();}
        if i==R::BLOOM||i==R::SCRATCH {f[a+R::WIDTH]=(c.width/4.).ceil();f[a+R::HEIGHT]=(c.height/4.).ceil();}
        if i==R::HISTORY0||i==R::HISTORY1 {f[a+R::WIDTH]=c.out_width;f[a+R::HEIGHT]=c.out_height;}
    }
    let s=&mut p.state;s[S::READ]=1.;s[S::FRAME]=5.;s[S::VALID]=c.valid as f64;s[S::RESET_SERIAL]=0.;s[S::OUTPUT_WIDTH]=c.out_width;s[S::OUTPUT_HEIGHT]=c.out_height;
    s[S::LAST_EYE..S::LAST_EYE+3].copy_from_slice(&[1.,2.,3.]);s[S::LAST_LOOK..S::LAST_LOOK+3].copy_from_slice(&[0.,0.,-1.]);p
}
fn hash(out:&[f64],steps:usize)->String {
    let mut h=Sha256::new();
    for k in 0..steps {
        let r=&out[k*post::STEP..(k+1)*post::STEP];
        for n in &r[..post::STEP_FIRST] {h.update(n.to_le_bytes());}
        for u in 0..r[post::STEP_UNIFORMS] as usize {
            let a=post::STEP_FIRST+u*post::UNIFORM;let ty=r[a+1] as usize;
            let n=if ty>=32||ty==5{1}else{ty};
            for n in &r[a..a+2+n] {h.update(n.to_le_bytes());}
        }
    }
    format!("{:x}",h.finalize())
}
#[test]
fn complete_post_phase_matches_goldens_and_is_bounded_and_allocation_free() {
    let fixtures:Fixtures=serde_json::from_str(include_str!("fixtures/post-chain.json")).unwrap();
    assert_eq!(fixtures.cases.len(),2592);
    let mut out=[0.;post::MAX_STEPS*post::STEP];
    for c in &fixtures.cases {
        let mut p=fixture(c,&fixtures.camera);p.begin();let n=p.plan(&mut out);
        assert_eq!(n,c.steps,"{c:?}");assert_eq!(hash(&out,n),c.hash,"{c:?}");
        let read=p.state[S::READ];p.complete(p.state[S::STEPS] as usize,false);
        assert_eq!(p.state[S::READ],if c.mode==3 {1.-read}else{read});
        if c.mode==3 {assert_eq!(p.state[S::FRAME],6.);assert_eq!(p.state[S::VALID],1.);}
        // Commit is consumed, so a duplicate completion cannot advance jitter.
        let before=p.state;p.complete(p.state[S::STEPS] as usize,false);assert_eq!(before,p.state);
    }
    let c=fixtures.cases.iter().find(|c|c.mode==3&&c.blur==2&&c.bloom==1).unwrap();
    let mut p=fixture(c,&fixtures.camera);p.begin();p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);
    ALLOCATIONS.store(0,Ordering::Relaxed);TRACK.store(true,Ordering::Relaxed);
    for _ in 0..10000 {p.begin();p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);}
    TRACK.store(false,Ordering::Relaxed);assert_eq!(ALLOCATIONS.load(Ordering::Relaxed),0);
    // A clipped view may draw complete preceding steps but cannot commit a
    // history target that never reached its final sharpen operation.
    p.begin();let read=p.state[S::READ];let frame=p.state[S::FRAME];
    assert!(p.plan(&mut out[..post::STEP])<=1);p.complete(p.state[S::STEPS] as usize,false);assert_eq!(p.state[S::READ],read);assert_eq!(p.state[S::FRAME],frame);
    p.begin();p.plan(&mut []);p.complete(p.state[S::STEPS] as usize,false);assert_eq!(p.state[S::FRAME],frame);
    // Loss/cut commands cancel pending completion, including paused frames.
    p.begin();p.plan(&mut out);p.invalidate();p.complete(p.state[S::STEPS] as usize,false);assert_eq!(p.state[S::FRAME],frame);assert_eq!(p.state[S::VALID],0.);
    // A refused draw or loss before the browser event leaves both clocks
    // unchanged, even with a complete plan and an allocated history target.
    p.begin();let draws=p.plan(&mut out);let before=p.state[S::FRAME];let bank=p.state[S::READ];
    p.complete(draws-1,false);assert_eq!(p.state[S::FRAME],before);assert_eq!(p.state[S::READ],bank);assert_eq!(p.state[S::VALID],0.);
    p.begin();let draws=p.plan(&mut out);p.complete(draws,true);assert_eq!(p.state[S::FRAME],before);assert_eq!(p.state[S::READ],bank);
    // Input scaling retains valid output history. Output resizing and camera
    // cuts discard it; reset commands are consumed once even across a pause.
    p.begin();p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);
    p.input[F::INPUT_WIDTH] *= 0.5;p.begin();assert_eq!(p.state[S::VALID],1.);
    assert_eq!(p.state[S::PHASES],32.);p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);
    p.input[F::OUTPUT_WIDTH] += 1.;p.begin();assert_eq!(p.state[S::VALID],0.);
    p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);p.input[F::WORLD+12] += 6.;p.begin();assert_eq!(p.state[S::VALID],0.);
    p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);p.input[F::WORLD+10] = -1.;p.begin();assert_eq!(p.state[S::VALID],0.);
    p.input[F::RESET_SERIAL] += 1.;p.input[F::RESET_FRAME] = 0.;p.input[F::RESET_READ] = 1.;
    p.begin();assert_eq!(p.state[S::FRAME],0.);assert_eq!(p.state[S::READ],1.);
    p.plan(&mut out);p.complete(p.state[S::STEPS] as usize,false);p.begin();assert_eq!(p.state[S::FRAME],1.);assert_eq!(p.state[S::READ],0.);
    p.state[S::READ]=f64::INFINITY;p.state[S::FRAME]=f64::NAN;
    p.input[F::INPUT_WIDTH]=0.;p.input[F::INPUT_HEIGHT]=f64::INFINITY;p.input[F::MODE]=f64::MAX;
    p.begin();assert!(p.plan(&mut out)<=post::MAX_STEPS);p.complete(p.state[S::STEPS] as usize,false);assert!(p.state[S::READ]<=1.);
}
