// CrowdShadows.pack and retained actor descriptor expansion are real frame
// kernels. Admission is outside the counter; imported excess counts must clip
// before a slice or integer conversion can panic on a long paused frame.
use bfe_render::{actor_resources::{Geometry,Record,InstanceDescriptor},actor_crowd::Crowd,actor_layout::{crowd as L,crowd_input as I}};
use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
thread_local!{static TRACK:Cell<(bool,usize)>=const{Cell::new((false,0))};}
struct Counter;
unsafe impl GlobalAlloc for Counter{unsafe fn alloc(&self,l:Layout)->*mut u8{TRACK.with(|v|{let(on,n)=v.get();if on{v.set((on,n+1));}});System.alloc(l)}unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{TRACK.with(|v|{let(on,n)=v.get();if on{v.set((on,n+1));}});System.realloc(p,l,n)}unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}}
#[global_allocator]static ALLOCATOR:Counter=Counter;
#[test]
fn retained_actor_phase_has_no_allocations_and_clips_excess(){
 let record=Record{kind:"plane".into(),..Default::default()};let mut source=Geometry::new(&record,&[],&[]).unwrap();let mut crowd=Crowd::default();let mut target=Crowd::geometry();assert!(crowd.append(&mut target,&[(&source,0,None,0)],1).is_some());
 let d=InstanceDescriptor{program:0,geometry:0,groups:0,textures:[-1;4],flags:8,blend:None,skin:false,scroll:[0.25,-0.5],bounds:source.facts.bounds,body:false};let mut row=[0.;112];
 TRACK.with(|v|v.set((true,0)));for i in 0..10000{crowd.input[I::DIRTY]=1.;crowd.input[I::FLAGS]=(i%2)as f64;assert!(crowd.pack(&mut target));d.publish(&mut row);std::hint::black_box(&target.index);}
 let allocations=TRACK.with(|v|{let(_,n)=v.get();v.set((false,0));n});assert_eq!(allocations,0);
 source.facts.count=L::VERTICES+1;assert!(crowd.append(&mut target,&[(&source,usize::MAX,None,0)],usize::MAX).is_none());assert_eq!(crowd.clipped,1);
 source.facts.count=4;assert!(crowd.append(&mut target,&[(&source,usize::MAX,None,0)],1).is_none());assert!(crowd.append(&mut target,&[(&source,0,None,usize::MAX)],1).is_none());assert_eq!(crowd.clipped,3);
}
