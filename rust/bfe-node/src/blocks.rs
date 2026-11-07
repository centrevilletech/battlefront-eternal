//! Validate caller arenas before reading memory. Shared backing stores cannot
//! be borrowed as Rust f64 slices while another JS thread can write to them.
use bfe_sim::gen::BlockHeader;
use napi::{sys, Env, Error, JsObject, NapiRaw, Result, Status};
use std::{ffi::c_void, ptr};
pub(super) struct Arena { data: *mut f64, pub length: usize }
pub(super) fn arena(env: Env, object: &JsObject) -> Result<Arena> {
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
    let mut detached = false;
    // SAFETY: only an ordinary engine-provided ArrayBuffer reaches this API.
    napi::check_status!(unsafe {sys::napi_is_detached_arraybuffer(env.raw(), backing, &mut detached)})?;
    if detached {return Err(Error::new(Status::InvalidArg, "Detached numeric arena"));}
    if length > u32::MAX as usize || length > isize::MAX as usize / std::mem::size_of::<f64>() || (length != 0 && data.is_null()) {
        return Err(Error::from_reason("Invalid numeric arena"));
    }
    Ok(Arena {data: data.cast(), length})
}
#[cfg(feature = "lifecycle-probe")]
pub(super) fn copy(env: Env, source: JsObject, output: JsObject) -> Result<u32> {
    let source = arena(env, &source)?;
    let output = arena(env, &output)?;
    if source.length == 0 {return Err(Error::from_reason("Invalid numeric block"));}
    source.read(validate)?;
    if output.length < source.length {return Err(Error::from_reason("Output arena capacity"));}
    // SAFETY: both rooted ordinary arenas remain attached for this call and
    // have checked extents. The validation borrow ended above; no output slice
    // aliases it. ptr::copy permits overlapping source and destination spans.
    unsafe {ptr::copy(source.data, output.data, source.length);}
    Ok(source.length as u32)
}
// Header extents are the generated shared boundary, not schema-specific rows.
pub(super) fn validate(block: &[f64]) -> Result<()> {
    let invalid = || Error::new(Status::InvalidArg, "Invalid numeric block");
    if block.len() < BlockHeader::WORDS || block.len() > u32::MAX as usize {return Err(invalid());}
    let count = |at: usize| -> Result<usize> {
        let n = block[at];
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {return Err(invalid());}
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

impl Arena {
    // The borrow ends before any write to another arena. No caller may invoke
    // JavaScript inside this closure; all production users only call Context.
    pub fn read<T>(&self, read: impl FnOnce(&[f64]) -> Result<T>) -> Result<T> {
        if self.length == 0 { return read(&[]); }
        // SAFETY: arena() checked the rooted ordinary backing and its extent.
        // No JS runs between metadata validation and completion of the copy.
        read(unsafe {std::slice::from_raw_parts(self.data, self.length)})
    }
    pub fn write(&self, words: &[f64]) -> u32 {
        if self.length >= words.len() && !words.is_empty() {
            // SAFETY: checked ordinary backing, no callbacks, sufficient extent;
            // Context owns words, which cannot alias the caller's output arena.
            unsafe {ptr::copy_nonoverlapping(words.as_ptr(), self.data, words.len());}
        }
        words.len() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn header_validation_checks_extents_but_never_normalizes_payloads() {
        let block = [1., 2., 7., 0., 0., -0., f64::from_bits(0x7ff8_0000_0000_0042)];
        validate(&block).unwrap();
        validate(&[0., 48., 5., 0., 0.]).unwrap();
        assert_eq!(block[5].to_bits(), (-0.0f64).to_bits());
        assert_eq!(block[6].to_bits(), 0x7ff8_0000_0000_0042);
        for at in 0..BlockHeader::WORDS {
            let mut bad = block; bad[at] = f64::NAN;
            assert!(validate(&bad).is_err());
        }
        for bad in [&[][..], &[1., 1., 5., 0., 0.], &[0., 0., 5., 0., 1.]] {
            assert!(validate(bad).is_err());
        }
    }
}
