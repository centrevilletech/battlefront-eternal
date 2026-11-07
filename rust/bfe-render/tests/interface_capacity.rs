// InterfacePass.write now expands the retained native appearance. Admission
// stays outside this counter; oversized draw counts clip before submission.
// Warm camera/pose/value packets reuse their original reservation.

use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
thread_local!{static TRACK:Cell<(bool,usize)>=const{Cell::new((false,0))};}
struct Counter;
unsafe impl GlobalAlloc for Counter{unsafe fn alloc(&self,l:Layout)->*mut u8{TRACK.with(|v|{let(on,n)=v.get();if on{v.set((on,n+1));}});System.alloc(l)}unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{TRACK.with(|v|{let(on,n)=v.get();if on{v.set((on,n+1));}});System.realloc(p,l,n)}unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}}
#[global_allocator]static ALLOCATOR:Counter=Counter;
#[test]
fn warm_interface_descriptor_expansion_allocates_nothing(){
 use bfe_render::{interface_resources::{Descriptor,Style,Policy},interface_resource_layout::{raw as R,look as L},interface_scene::layout as D};
 let style=Style{program:"model".into(),..Default::default()};let policy=Policy::new(&style,true,true).unwrap();let descriptor=Descriptor::new(1,2,400,3,policy,&[7,8,9,0]).unwrap();let mut row=[0.;D::STRIDE];
 TRACK.with(|v|v.set((true,0)));for _ in 0..10000{
  for at in [R::WORLD,R::VIEW,R::PROJECTION,R::BIND,R::INVERSE]{for i in 0..16{row[D::VALUES+at+i]=if i%5==0{1.}else{0.};}}
  for i in 0..L::LEN{row[D::VALUES+R::LOOK+i]=i as f64/13.;}row[D::COUNT]=f64::MAX;descriptor.publish(&mut row);std::hint::black_box(&row);
 }
 let allocations=TRACK.with(|v|{let(_,n)=v.get();v.set((false,0));n});assert_eq!(allocations,0);assert_eq!(row[D::COUNT],400.);
}
