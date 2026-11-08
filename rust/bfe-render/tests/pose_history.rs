//! Golden publications cover the temporal producer before any shader upload.
use bfe_render::{pose_history::PoseHistory, cameras::layout::history_pose as p, motion as m};

use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
struct Count;
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Count{
    unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.alloc(l)}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.realloc(p,l,n)}
}
#[global_allocator]static ALLOCATOR:Count=Count;

const IDENTITY:[f64;16]=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];
fn pose(h:&mut PoseHistory,id:u32,x:f64) {
    let at=(id as usize-1)*p::LEN;
    h.poses[at+p::WORLD..at+p::WORLD+16].copy_from_slice(&IDENTITY);
    h.poses[at+p::WORLD+12]=x;
    h.poses[at+p::INVERSE..at+p::INVERSE+16].copy_from_slice(&IDENTITY);
    h.poses[at+p::INVERSE+12]=-x;
}
fn publication(h:&mut PoseHistory,id:u32)->(Vec<f64>,Vec<f32>,Vec<f32>){
    let mut input=vec![0.;m::INPUT_LEN];let mut bones=vec![0.;32];let mut previous=vec![0.;32];
    h.records[0]=id;h.write(1,&mut input,&mut bones,&mut previous);(input,bones,previous)
}

#[test]
fn fractional_motion_pause_teleport_hide_and_release_keep_exact_history() {
    let mut h=PoseHistory::default();let skin=h.skin(1);let id=h.model(skin,&IDENTITY);
    // A signed zero and noncanonical NaN are preserved as raw f32 skin bits.
    h.current[0]=f32::from_bits(0x80000000);h.current[1]=f32::from_bits(0x7fc00042);
    pose(&mut h,id,0.25);h.selected[0]=id;h.prepare(1);
    let (row,now,before)=publication(&mut h,id);
    assert_eq!(row[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],0.25);
    assert_eq!(now.iter().map(|n|n.to_bits()).collect::<Vec<_>>(),before.iter().map(|n|n.to_bits()).collect::<Vec<_>>());
    h.commit();pose(&mut h,id,0.75);h.current[0]=3.;h.prepare(1);
    let (row,_,before)=publication(&mut h,id);
    assert_eq!(row[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],0.25);
    assert_eq!(row[m::IN_RECORDS+m::REC_PREVIOUS_BIND_INVERSE+12],-0.25);
    assert_eq!(before[0].to_bits(),0x80000000);
    // Preparing repeatedly while paused does not advance the displayed frame.
    h.prepare(1);assert_eq!(publication(&mut h,id).0[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],0.25);
    h.commit();pose(&mut h,id,8.75);h.prepare(1);
    assert_eq!(publication(&mut h,id).0[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],0.75,"exactly eight metres retains history");
    pose(&mut h,id,8.750000000000002);h.prepare(1);
    assert_eq!(publication(&mut h,id).0[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],8.750000000000002);
    h.commit();h.prepare(0);h.commit();pose(&mut h,id,9.);h.prepare(1);
    assert_eq!(publication(&mut h,id).0[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],9.,"hidden to shown resets");
    h.commit();pose(&mut h,id,9.25);h.reset(id);h.prepare(1);
    assert_eq!(publication(&mut h,id).0[m::IN_RECORDS+m::REC_PREVIOUS_WORLD+12],9.25);
    h.release(id);h.prepare(usize::MAX);let (row,_,_)=publication(&mut h,id);
    assert_eq!(row[m::IN_RECORDS+m::REC_WORLD+12],0.,"released identities cannot publish stale history");
}

#[test]
fn shared_skin_commits_once_and_invalid_indices_clip_without_panicking() {
    let mut h=PoseHistory::default();let skin=h.skin(2);let a=h.model(skin,&IDENTITY);let b=h.model(skin,&IDENTITY);
    pose(&mut h,a,1.);pose(&mut h,b,2.);h.selected[0]=a;h.selected[1]=b;h.current.fill(1.);
    h.prepare(2);h.commit();h.current.fill(2.);pose(&mut h,a,1.5);pose(&mut h,b,2.5);h.prepare(2);
    assert_eq!(publication(&mut h,a).2,[1.;32]);assert_eq!(publication(&mut h,b).2,[1.;32]);
    let mut input=vec![0.;m::INPUT_LEN];let mut bones=vec![0.;32];let mut previous=vec![0.;32];
    h.records[0]=a;
    for invalid in [-1.,f64::NAN,f64::INFINITY,0.5,8192.] {
        input[m::IN_RECORDS+m::REC_BONE_AT]=invalid;h.write(1,&mut input,&mut bones,&mut previous);
    }
    assert_eq!(h.clipped,5);assert_eq!(h.skin(usize::MAX),0);assert_eq!(h.model(u32::MAX,&[]),0);
    h.selected[0]=u32::MAX;h.prepare(usize::MAX);h.commit();
}

#[test]
fn retained_source_preserves_words_and_rejects_incomplete_skin_before_mutation(){
    let mut h=PoseHistory::default();let skin=h.skin(2);let id=h.model(skin,&IDENTITY);
    let mut world=IDENTITY;world[12]=0.125;world[13]=-0.;
    let mut inverse=IDENTITY;inverse[12]=-0.125;
    let mut source=[0.;32];source[0]=f32::from_bits(0x80000000);source[31]=f32::from_bits(0x7fc00042);
    assert!(h.source(id,&world,&inverse,2,|i|source.get(i*16..i*16+16)));
    assert_eq!(h.current.iter().map(|n|n.to_bits()).collect::<Vec<_>>(),source.iter().map(|n|n.to_bits()).collect::<Vec<_>>());
    let previous=h.poses.clone();source[0]=4.;world[12]=6.;
    assert!(!h.source(id,&world,&inverse,2,|i|if i==0{Some(&source[..16])}else{None}));
    assert_eq!(h.poses.iter().map(|n|n.to_bits()).collect::<Vec<_>>(),previous.iter().map(|n|n.to_bits()).collect::<Vec<_>>());
    assert_eq!(h.current[0].to_bits(),0x80000000);
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for _ in 0..100{assert!(h.source(id,&world,&inverse,2,|i|source.get(i*16..i*16+16)));}
    TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
}

#[test]
fn sky_motion_consumes_steps_once_and_retains_failed_frame_steps(){
    let mut h=PoseHistory::default();let id=h.flyer();
    assert_eq!(h.flyer_frame(id,false),3);
    assert_eq!(h.flyer_frame(id,true),1,"a second mesh leaves the step pending");
    h.prepare(0);h.commit();assert_eq!(h.flyer_frame(id,true),2);
    assert_eq!(h.flyer_frame(id,false),0,"the same displayed frame keeps its first still value");
    h.prepare(0);h.commit();assert_eq!(h.flyer_frame(id,false),3);
    assert_eq!(h.flyer_frame(u32::MAX,true),1);
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for _ in 0..100{h.flyer_frame(id,true);h.prepare(0);h.commit();}
    TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
}

#[test]
fn larger_than_map_paused_bursts_and_frames_allocate_nothing(){
    let mut h=PoseHistory::default();let skin=h.skin(64);
    for i in 0..20000{let id=h.model(skin,&IDENTITY);pose(&mut h,id,i as f64*0.125);h.selected[i]=id;}
    for i in 0..m::MAX_RECORDS{h.records[i]=(i+1) as u32;}
    let mut input=vec![0.;m::INPUT_LEN];let mut bones=vec![0.;m::MAX_BONES*16];let mut previous=vec![0.;m::MAX_BONES*16];
    ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
    for _ in 0..20{h.prepare(usize::MAX);h.prepare(20000);h.write(usize::MAX,&mut input,&mut bones,&mut previous);h.commit();}
    TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);
}

// Rigid motion wrote identity bind inverses even under a skinned unit parent.
// That publication remains exact although the rigid shader ignores the field.
#[test]
fn rigid_source_retains_identity_inverse_at_the_motion_boundary(){
 use bfe_render::{pose_history::PoseHistory,cameras::layout::history_pose as p};
 let mut history=PoseHistory::default();let identity=[1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.,0.,0.,0.,0.,1.];let id=history.model(0,&identity);let mut inverse=identity;inverse[12]=-7.;
 assert!(history.source(id,&identity,&inverse,0,|_|None));assert_eq!(&history.poses[p::INVERSE..p::INVERSE+16],&identity);
}
