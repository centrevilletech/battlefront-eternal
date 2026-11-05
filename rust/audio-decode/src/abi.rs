//! A whole decode step crosses the worker boundary. The worker fills a
//! bounded input window and copies only produced planar PCM out of it.
use crate::{layout::*, Decoder, Step};
// Each handle belongs to one codec job on the loading worker. No audio
// thread can dereference it. Parser and output allocations are permitted
// here; the render module receives copied PCM through a separate port.
pub struct Host {
    decoder: Decoder,
    input: Vec<u8>,
    meta: [f64; LENGTH],
    error: Vec<u8>,
}
// The JS wrapper owns this Box until codec_free and calls it serially.
// The lifetime expresses an opaque C handle, not shared access: callers
// must never use a freed handle or overlap calls on the same handle.
fn host(h: *mut Host) -> &'static mut Host {
    unsafe { &mut *h }
}
#[no_mangle]
// Rate, asset gain and impulse compensation are fixed for this job.
// Zero rate requests source PCM; it does not consult an AudioContext.
pub extern "C" fn codec_new(rate: u32, gain: f64, compensate: u32) -> *mut Host {
    Box::into_raw(Box::new(Host {
        decoder: Decoder::new(rate, gain, compensate != 0),
        input: Vec::new(),
        meta: [0.0; LENGTH],
        error: Vec::new(),
    }))
}
#[no_mangle]
// Release all compressed, decoder and PCM storage together. Cancellation
// uses the same ownership rule as successful completion.
pub extern "C" fn codec_free(h: *mut Host) {
    if !h.is_null() {
        unsafe {
            drop(Box::from_raw(h));
        }
    }
}
#[no_mangle]
// Reserve a bounded upload window before JS creates its typed view.
// A reserve may grow WASM memory, invalidating older JS views. The pointer
// is for this upload only and is not retained across decoder calls.
pub extern "C" fn codec_input(h: *mut Host, length: usize) -> *mut u8 {
    let h = host(h);
    h.input.resize(length.min(INPUT_CHUNK), 0);
    h.input.as_mut_ptr()
}
#[no_mangle]
// Copy the uploaded window into parser-owned storage. NEED_INPUT here
// means the bytes were accepted; codec_step decides whether more are needed.
pub extern "C" fn codec_feed(h: *mut Host, eof: u32) -> usize {
    let h = host(h);
    match h.decoder.push(&h.input, eof != 0) {
        Ok(()) => NEED_INPUT,
        Err(e) => {
            h.error = e.into_bytes();
            ERROR
        }
    }
}
#[no_mangle]
// One synchronous worker phase either publishes metadata, a planar PCM
// window, completion, or a request for bytes. It never waits for network I/O.
// Errors are retained as UTF-8 until the wrapper reads them or frees the job.
pub extern "C" fn codec_step(h: *mut Host, maximum: usize) -> usize {
    let h = host(h);
    let status = match h.decoder.step(maximum.clamp(1, PCM_CHUNK)) {
        Ok(Step::NeedInput) => NEED_INPUT,
        Ok(Step::Metadata) => METADATA,
        Ok(Step::Output(n)) => {
            h.meta[OUTPUT_FRAMES] = n as f64;
            OUTPUT
        }
        Ok(Step::Done) => DONE,
        Err(e) => {
            h.error = e.into_bytes();
            ERROR
        }
    };
    // Metadata uses f64 cells so frame counts and rates cross JS without
    // integer coercion. A negative frame count means duration is not known yet;
    // OUTPUT_FRAMES describes this window, not the whole asset.
    if let Some(info) = &h.decoder.info {
        h.meta[SOURCE_RATE] = info.source_rate as f64;
        h.meta[RATE] = info.rate as f64;
        h.meta[CHANNELS] = info.channels as f64;
        h.meta[BITS] = info.bits as f64;
        h.meta[FRAMES] = info.frames.map_or(-1.0, |n| n as f64);
        h.meta[FORMAT] = info.format as f64;
    }
    status
}
#[no_mangle]
pub extern "C" fn codec_meta(h: *mut Host) -> *const f64 {
    host(h).meta.as_ptr()
}
#[no_mangle]
// PCM is channel-major, with OUTPUT_FRAMES samples in each lane. JS must
// copy it before the next step can resize or overwrite this vector.
pub extern "C" fn codec_pcm(h: *mut Host) -> *const f32 {
    host(h).decoder.output.as_ptr()
}
#[no_mangle]
pub extern "C" fn codec_error(h: *mut Host) -> *const u8 {
    host(h).error.as_ptr()
}
#[no_mangle]
pub extern "C" fn codec_error_len(h: *mut Host) -> usize {
    host(h).error.len()
}
#[no_mangle]
// The worker can read a small compressed tail independently of its forward
// cursor. Publish the final Opus duration before scheduling the first source;
// this does not decode or retain the remainder of the track.
pub extern "C" fn codec_tail(h: *mut Host) -> usize {
    let h = host(h);
    match h.decoder.tail(&h.input) {
        Ok(()) => {
            h.meta[FRAMES] = h.decoder.info.as_ref().unwrap().frames.unwrap() as f64;
            METADATA
        }
        Err(e) => {
            h.error = e.into_bytes();
            ERROR
        }
    }
}
