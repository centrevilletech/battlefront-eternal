//! A paused command burst and a busy retained draw must use the storage
//! reserved at construction. A malformed count is only a bounded prefix.
use bfe_presentation::{feet::{Feet,Content},layout::{foot_frame as f,foot_input as i,foot_event as o}};
use std::alloc::{GlobalAlloc,Layout,System};
use std::cell::Cell;
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static COUNT:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting {
 unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){COUNT.with(|n|n.set(n.get()+1));}System.alloc(l)}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){COUNT.with(|c|c.set(c.get()+1));}System.realloc(p,l,n)}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
#[test]
fn a_full_walker_foot_phase_does_not_allocate(){
 let mut feet=Feet::default();
 for _ in 0..256 {
  let content:Content=serde_json::from_str(r#"{"bones":[],"clips":[],"root":-1,"length":0,"feet":[{"bone":-1,"point":[0,0,0],"turn":null}],"detection":false,"threshold":0.15,"hydraulic":1,"centred":true,"rate":1}"#).unwrap();feet.add(content);
 }
 for k in 0..256 {let at=f::STRIDE+k*i::STRIDE;feet.input[at+i::PRESENT]=1.0;feet.input[at+i::GENERATION]=1.0;for n in [0,5,10,15]{feet.input[at+i::MATRIX+n]=1.0;}}
 feet.input[f::DT]=0.1;COUNT.with(|c|c.set(0));TRACK.with(|t|t.set(true));
 for frame in 0..30 {for k in 0..256 {let at=f::STRIDE+k*i::STRIDE;feet.input[at+i::MATRIX+13]=if frame%3==0{2.0}else{1.0};}feet.frame();}
 TRACK.with(|t|t.set(false));assert_eq!(COUNT.with(Cell::get),0);assert_eq!(feet.output.len(),256*o::STRIDE);
}
