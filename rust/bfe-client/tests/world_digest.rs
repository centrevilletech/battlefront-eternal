//! A fixed-seed local battle without a window: the world steps with no player
//! input, and the test prints a running digest of every publication and the
//! tick's cost. Two builds that print the same digests computed the same
//! battle. Needs prepared game files: `BFE_CONTENT_DIR=... cargo test --release
//! -p bfe-client --test world_digest -- --ignored --nocapture`. A list of
//! MAP[:SIDE] in `BFE_DIGEST_MAPS` (default nab2,tat2,hot1:all) and the tick count in
//! `BFE_DIGEST_TICKS` (default 3600) are optional.
use bfe_client::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_client::local::{launch::Prepared, options::Options, runtime::Runtime, thread::PublicationSlot};
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use std::time::Instant;

struct Allocator;
thread_local! {
    static ALLOCATIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn counted() {
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}
fn allocations() -> u64 {
    ALLOCATIONS.try_with(|count| count.get()).unwrap_or(0)
}
unsafe impl std::alloc::GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        counted();
        unsafe { std::alloc::GlobalAlloc::alloc(&std::alloc::System, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        counted();
        unsafe { std::alloc::GlobalAlloc::alloc_zeroed(&std::alloc::System, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        counted();
        unsafe { std::alloc::GlobalAlloc::realloc(&std::alloc::System, ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::GlobalAlloc::dealloc(&std::alloc::System, ptr, layout) }
    }
}

// FNV-1a over the publication's bytes and block words.
fn mix(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

fn run(root: &str, battle: &str, ticks: usize) {
    let (map, side) = battle.split_once(':').unwrap_or((battle, "rep"));
    let args: Vec<String> = ["--local", map, root, "--side", side, "--seed", "1", "--ai", "classic"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let options = Options::parse(&args, None).unwrap().unwrap();
    let prepared = Prepared::new(&options).unwrap();
    let mut runtime =
        Runtime::new(*prepared.engine, *prepared.admission, CLOCK_START_MS, CLOCK_UNIX_MS).unwrap();
    let mut slot = PublicationSlot::new_preallocated();
    let (command, look, flight) = (ClientCommand::default(), ClientLook::default(), ClientFlightCommand::default());
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut times = Vec::with_capacity(ticks);
    let mut steady_allocations = 0;
    for tick in 1..=ticks {
        let before = allocations();
        let start = Instant::now();
        runtime.step_into_slot(&command, &look, &flight, [0., 0.], 0., &mut slot).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.);
        if tick > 60 {
            steady_allocations += allocations() - before;
        }
        hash = mix(hash, &slot.bytes);
        for word in &slot.block.words {
            hash = mix(hash, &word.to_bits().to_le_bytes());
        }
        if tick % 600 == 0 {
            println!("digest {map} tick {tick} {hash:016x}");
        }
    }
    let mean = times.iter().sum::<f64>() / times.len() as f64;
    times.sort_by(f64::total_cmp);
    let p95 = times[(times.len() - 1) * 95 / 100];
    let max = times[times.len() - 1];
    println!(
        "cost {map} ticks {ticks} mean {mean:.3} p95 {p95:.3} max {max:.3} ms; allocations per tick {:.1}",
        steady_allocations as f64 / (ticks - 60) as f64
    );
    runtime.close();
}

#[test]
#[ignore]
fn world_digest() {
    let root = std::env::var("BFE_CONTENT_DIR").expect("BFE_CONTENT_DIR");
    let maps = std::env::var("BFE_DIGEST_MAPS").unwrap_or_else(|_| "nab2,tat2,hot1:all".into());
    let ticks = std::env::var("BFE_DIGEST_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(3600);
    for map in maps.split(',') {
        run(&root, map, ticks);
    }
}
