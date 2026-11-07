//! Validate caller arenas before reading memory. Shared backing stores cannot
//! be borrowed as Rust f64 slices while another JS thread can write to them.
use bfe_sim::gen::BlockHeader;
use napi::{sys, Env, Error, JsObject, NapiRaw, Result, Status};
use std::{ffi::c_void, ptr};
struct Arena { data: *mut f64, length: usize }
fn arena(env: Env, object: &JsObject) -> Result<Arena> {
    let (mut kind, mut length, mut data, mut backing, mut offset) =
        (0, 0, ptr::null_mut::<c_void>(), ptr::null_mut(), 0);
    // SAFETY: object is a rooted argument in the current N-API invocation.
    // These APIs inspect engine metadata without invoking JavaScript getters.
    napi::check_status!(unsafe {sys::napi_get_typedarray_info(env.raw(), object.raw(),
        &mut kind, &mut length, &mut data, &mut backing, &mut offset)})?;
    if kind != sys::TypedarrayType::float64_array {return Err(Error::from_reason("Expected Float64Array"));}
    let mut ordinary = false;
    // SAFETY: backing is the engine-provided backing object above. This check
    // uses its internal type, not the replaceable JS .buffer property.
    napi::check_status!(unsafe {sys::napi_is_arraybuffer(env.raw(), backing, &mut ordinary)})?;
    if !ordinary {return Err(Error::from_reason("SharedArrayBuffer arenas are not supported"));}
    if length > isize::MAX as usize / std::mem::size_of::<f64>() || (length != 0 && data.is_null()) {
        return Err(Error::from_reason("Invalid numeric arena"));
    }
    Ok(Arena {data: data.cast(), length})
}
pub(super) fn copy(env: Env, source: JsObject, output: JsObject) -> Result<u32> {
    let source = arena(env, &source)?;
    let output = arena(env, &output)?;
    if source.length == 0 {return Err(Error::from_reason("Invalid numeric block"));}
    // SAFETY: nonempty Float64Array metadata guarantees alignment and extent;
    // ordinary backing stores cannot be concurrently modified by another JS
    // thread. No JS callback/detachment occurs during this synchronous call.
    validate(unsafe {std::slice::from_raw_parts(source.data, source.length)})?;
    if output.length < source.length {return Err(Error::from_reason("Output arena capacity"));}
    // SAFETY: both rooted ordinary arenas remain attached for this call and
    // have checked extents. The validation borrow ended above; no output slice
    // aliases it. ptr::copy permits overlapping source and destination spans.
    unsafe {ptr::copy(source.data, output.data, source.length);}
    Ok(source.length as u32)
}
// Header extents are the generated shared boundary, not schema-specific rows.
fn validate(block: &[f64]) -> Result<()> {
    let invalid = || Error::new(Status::InvalidArg, "Invalid numeric block");
    if block.len() < BlockHeader::WORDS || block.len() > u32::MAX as usize {return Err(invalid());}
    let count = |at: usize| -> Result<usize> {
        let n = block[at];
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > block.len() as f64 {return Err(invalid());}
        Ok(n as usize)
    };
    let rows = count(BlockHeader::ROWS)?;
    let stride = count(BlockHeader::STRIDE)?;
    let heap = count(BlockHeader::HEAP)?;
    let size = count(BlockHeader::HEAP_WORDS)?;
    let retired = count(BlockHeader::RETIRED)?;
    if retired > rows || rows.checked_mul(stride).and_then(|n| n.checked_add(BlockHeader::WORDS)) != Some(heap)
        || heap.checked_add(size) != Some(block.len()) {return Err(invalid());}
    Ok(())
}
