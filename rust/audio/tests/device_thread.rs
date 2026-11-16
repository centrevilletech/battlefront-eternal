//! What the device thread does with streamed and handed-over samples must not
//! allocate or free memory (a real-time callback can block in the allocator),
//! and a streamed segment of any length plays to its end.
use audio::voices::{Engine, Sample};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
fn count() { COUNT.with(|c| if let Some(n) = c.get() { c.set(Some(n + 1)); }); }
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { count(); System.alloc(layout) }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { count(); System.alloc_zeroed(layout) }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 { count(); System.realloc(pointer, layout, size) }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) { count(); System.dealloc(pointer, layout) }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
fn counted<T>(f: impl FnOnce() -> T) -> (T, usize) {
    COUNT.with(|c| c.set(Some(0)));
    let out = f();
    let n = COUNT.with(|c| c.replace(None)).unwrap();
    (out, n)
}

// Windows decoded beforehand (as the decode-ahead thread hands them over):
// producing one only lends a slice. A ramp, mono.
struct Windows { pcm: Vec<f32>, at: usize, window: usize }
impl audio::streaming::Producer for Windows {
    fn produce(&mut self, frames: usize) -> Option<(&[f32], usize)> {
        let n = frames.min(self.window).min(self.pcm.len() - self.at);
        if n == 0 { return None; }
        self.at += n;
        Some((&self.pcm[self.at - n..self.at], n))
    }
}
fn ramp(total: usize, window: usize) -> Box<Windows> {
    Box::new(Windows { pcm: (0..total).map(|i| i as f32).collect(), at: 0, window })
}
// Plays `sample` frame by frame as a source does, feeding between quanta.
fn play(sample: &mut Sample, total: usize) -> usize {
    let mut index = 0;
    for _ in 0..(total * 4 + 1000) {
        if index == total { break; }
        if sample.available(index) {
            assert_eq!(sample.value(0, index), index as f32, "frame {index}");
            index += 1;
        } else {
            sample.consumed(index);
            sample.feed();
        }
    }
    sample.consumed(index);
    sample.feed();
    index
}

#[test]
fn feeding_a_stream_to_its_end_neither_allocates_nor_frees() {
    let total = 50_000;
    let mut sample = Sample::codec(48000., 1, total, ramp(total, 2048)).unwrap();
    let (played, n) = counted(|| play(&mut sample, total));
    assert_eq!(played, total);
    assert!(sample.ring.as_ref().unwrap().eof);
    assert_eq!(n, 0, "the device thread allocated or freed {n} times");
}

#[test]
fn a_stream_shorter_than_a_window_plays_and_ends() {
    for total in [1, 100, 1000, 2047, 2048, 2049, 2050, 2051, 4097] {
        // The decoder may give more than the header says: its last window is longer.
        let mut sample = Sample::codec(48000., 1, total, ramp(total, 2048)).unwrap();
        assert_eq!(play(&mut sample, total), total, "{total} frames");
        assert!(sample.ring.as_ref().unwrap().eof && sample.frames == total, "{total} frames end");
    }
}

#[test]
fn handing_a_sample_over_frees_nothing_on_the_device_thread() {
    let mut engine = Engine::new(48000., 4, -3.0103, false);
    engine.sample_storage(3, 1000, 48000.).fill(0.5);
    let replacement = Some(Sample::codec(48000., 1, 5000, ramp(5000, 2048)).unwrap());
    let (old, n) = counted(|| engine.set_sample(3, replacement));
    assert_eq!(n, 0, "replacing a sample freed {n} times");
    drop(old);
    let (old, n) = counted(|| engine.set_sample(3, None));
    assert_eq!(n, 0, "releasing a sample freed {n} times");
    drop(old);
}

#[test]
fn a_silent_output_skipping_its_blocks_neither_allocates_nor_frees() {
    let mut out = audio::output::Output::new(48000., 2, true, false, false, false, false);
    out.voices.sample_channels(1, 48000, 1, 48000.).fill(0.1);
    out.commands = vec![audio::commands::BIND as f64, 0., 1., audio::commands::START as f64, 0., 1., 0., 1., 1., 20000., 0.];
    let len = out.commands.len();
    out.apply(len);
    out.volume(0., 0., 0., true);
    out.process();
    let (_, n) = counted(|| for _ in 0..400 { out.process(); });
    assert_eq!(n, 0, "skipping allocated or freed {n} times");
    assert!(out.output.iter().all(|&x| x == 0.));
}
