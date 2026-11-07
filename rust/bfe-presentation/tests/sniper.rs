//! A paused command burst and a busy retained draw must use the storage
//! reserved at construction. A malformed count is only a bounded prefix.
use bfe_presentation::{sniper::Sniper,sniper_layout::{sniper_frame as f,sniper_command as c,sniper_kind as k,sniper_counts as h,sniper_limits as lim}};
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
fn full_command_and_source_stores_do_not_allocate_or_trap(){
 let mut sniper=Sniper::default();
 for i in 0..lim::COMMANDS {let at=f::STRIDE+i*c::STRIDE;
  sniper.input[at+c::KIND]=k::TOUCH as f64;sniper.input[at+c::SOURCE]=i as f64+1.0;
  sniper.input[at+c::DIRECTION+2]=1.0;sniper.input[at+c::HEAD+2]=40.0;
  sniper.input[at+c::COLOR]=255.0;sniper.input[at+c::REACH]=300.0;
 }
 sniper.input[f::TIME]=10.0;sniper.input[f::EYE+1]=1e6;
 sniper.input[f::COMMANDS]=u32::MAX as f64;sniper.input[f::BEGIN]=1.0;
 let mut seed=1u32;let mut random=||{seed=seed.wrapping_mul(1664525).wrapping_add(1013904223);seed as f64/4294967296.0};
 COUNT.with(|c|c.set(0));TRACK.with(|t|t.set(true));
 sniper.collect(&mut random);sniper.frame(&mut random);
 // A second paused block covers source tombstones, a new source generation,
 // completed impacts, head waiting, and spark/burst eviction in one owner.
 for frame in 1..8 {sniper.input[f::TIME]=10.0+frame as f64*0.05;
  for i in 0..lim::COMMANDS {let at=f::STRIDE+i*c::STRIDE;sniper.input[at+c::SOURCE]+=(lim::COMMANDS+1)as f64;sniper.input[at+c::ENDED]=1.0;}
  sniper.collect(&mut random);sniper.frame(&mut random);
 }
 TRACK.with(|t|t.set(false));assert_eq!(COUNT.with(Cell::get),0);
 assert_eq!(sniper.counts[h::RECORDS],48);assert!(sniper.counts[h::STREAKS]<=lim::STREAKS as u32);
 assert!(sniper.counts[h::FLARES]<=lim::FLARES as u32);assert!(sniper.counts[h::SPARKS]<=161);
}
