//! The recorded fixture uses production SoundControl, synthetic bank PCM and
//! native generated exchange layouts. No device or content is needed to render.
use bfe_client::{audio::Status,sound::SoundOutput};
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,path::PathBuf};
thread_local!{static COUNT:Cell<Option<usize>>=const{Cell::new(None)};}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self,l:Layout)->*mut u8{COUNT.with(|c|if let Some(n)=c.get(){c.set(Some(n+1));});System.alloc(l)}
    unsafe fn alloc_zeroed(&self,l:Layout)->*mut u8{COUNT.with(|c|if let Some(n)=c.get(){c.set(Some(n+1));});System.alloc_zeroed(l)}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{COUNT.with(|c|if let Some(n)=c.get(){c.set(Some(n+1));});System.realloc(p,l,n)}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
}
#[global_allocator]static ALLOCATOR:Allocator=Allocator;
#[test]
#[ignore="requires the production-host recorded sound fixture"]
fn controls_and_partial_device_quanta_allocate_nothing_including_the_first_frame(){
    let dir=PathBuf::from(std::env::var_os("BFE_SOUND_FIXTURE").expect("sound fixture path"));
    let mut output=SoundOutput::load(&dir).unwrap();assert!(output.checks>=300);
    let status=Status::default();let mut buffer=[0.0f32;257];
    COUNT.with(|c|c.set(Some(0)));
    for _ in 0..1000{output.fill(&mut buffer,&status);}
    let allocations=COUNT.with(|c|c.replace(None).unwrap());assert_eq!(allocations,0);
    assert!(status.blocks.load(std::sync::atomic::Ordering::Relaxed)>750);
    assert!(f32::from_bits(status.peak.load(std::sync::atomic::Ordering::Relaxed))>0.01);
}
