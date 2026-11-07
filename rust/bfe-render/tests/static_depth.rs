// Frozen shadow-casters.js constructors from de43cf0a. No content files or
// browser are needed to prove resource bytes and per-view Morton ranges in CI.
use bfe_render::{math::Frustum,static_depth::DepthGeometry,static_world::{Geometry,Record}};
use serde::Deserialize;
use std::alloc::{GlobalAlloc,Layout,System};
use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
struct Counter;
static TRACK:AtomicBool=AtomicBool::new(false);
static ALLOCATIONS:AtomicUsize=AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self,l:Layout)->*mut u8 {if TRACK.load(Ordering::Relaxed){ALLOCATIONS.fetch_add(1,Ordering::Relaxed);}System.alloc(l)}
    unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8 {if TRACK.load(Ordering::Relaxed){ALLOCATIONS.fetch_add(1,Ordering::Relaxed);}System.realloc(p,l,n)}
    unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
}
#[global_allocator]
static ALLOCATOR:Counter=Counter;
#[derive(Deserialize)]
struct Input {record:Record,vertices:Vec<u32>,index:Vec<u32>}
#[derive(Deserialize)]
struct View {matrix:[f64;16],first:u32,count:u32}
#[derive(Deserialize)]
struct Fixture {mode:String,sources:Vec<Input>,vertices:Vec<u32>,index:Vec<u32>,#[serde(rename="boundsBits")] bounds_bits:[String;10],layer:u32,views:Vec<View>}
#[test]
fn frozen_depth_geometry_and_view_ranges() {
    let fixtures:Vec<Fixture>=serde_json::from_str(include_str!("fixtures/static-depth.json")).unwrap();
    for f in fixtures {
        let sources=f.sources.into_iter().map(|s|Geometry::new(&s.record,&s.vertices.into_iter().map(f32::from_bits).collect::<Vec<_>>(),&s.index).unwrap()).collect::<Vec<_>>();
        let depth=if f.mode=="merged" {DepthGeometry::merged(&sources.iter().collect::<Vec<_>>())} else {DepthGeometry::cutout(&sources[0])};
        assert_eq!(depth.geometry.vertices.iter().map(|v|v.to_bits()).collect::<Vec<_>>(),f.vertices,"{} vertices",f.mode);
        assert_eq!(depth.geometry.index,f.index,"{} indices",f.mode);
        assert_eq!(depth.geometry.bounds.map(f64::to_bits),f.bounds_bits.map(|v|v.parse::<u64>().unwrap()),"{} bounds",f.mode);
        assert_eq!(depth.layer,f.layer);
        assert_eq!(depth.geometry.interleaved,f.mode!="merged");
        assert_eq!(depth.clipped,0);
        for v in f.views {
            let frustum=Frustum::from_matrix(&v.matrix);
            assert_eq!(depth.range(&frustum),(v.first,v.count),"{} range",f.mode);
            // The resource compiler allocates cold. Repeated cascade selection
            // over retained cells must use only stack values and borrowed rows.
            ALLOCATIONS.store(0,Ordering::SeqCst);TRACK.store(true,Ordering::SeqCst);
            for _ in 0..1000 {std::hint::black_box(depth.range(std::hint::black_box(&frustum)));}
            TRACK.store(false,Ordering::SeqCst);
            assert_eq!(ALLOCATIONS.load(Ordering::SeqCst),0);
        }
    }
}
