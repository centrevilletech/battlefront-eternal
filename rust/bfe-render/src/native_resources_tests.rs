//! Exercise the real owner through GL's name-allocation boundary. The fake
//! device tracks names only; references, cache keys and recycling stay in the
//! production GpuResources implementation and run without a GPU in CI.
use super::*;
use std::{cell::RefCell, collections::HashSet, ffi::c_void};
#[derive(Default)]
struct Device {
    next: u32,
    live: HashSet<u32>,
    deleted: usize,
    double_delete: usize,
}
thread_local! { static DEVICE:RefCell<Device>=RefCell::default(); }
unsafe extern "system" fn string(_: u32) -> *const u8 {
    c"3.3 synthetic".as_ptr().cast()
}
unsafe extern "system" fn integer(name: u32, out: *mut i32) {
    // SAFETY: glow passes one writable integer for GetIntegerv; these queries
    // have scalar results and the stub never retains its borrowed pointer.
    unsafe {
        *out = if name == glow::MAX_TEXTURE_SIZE {
            4096
        } else {
            0
        };
    }
}
unsafe extern "system" fn generate(n: i32, out: *mut u32) {
    DEVICE.with_borrow_mut(|d| {
        for i in 0..n as usize {
            d.next += 1;
            d.live.insert(d.next);
            // SAFETY: GenTextures gives the driver exactly n writable names.
            unsafe {
                *out.add(i) = d.next;
            }
        }
    });
}
unsafe extern "system" fn delete(n: i32, names: *const u32) {
    DEVICE.with_borrow_mut(|d| {
        for i in 0..n as usize {
            // SAFETY: DeleteTextures lends exactly n initialized names. Nothing
            // retains that storage after the synchronous API returns.
            let name = unsafe { *names.add(i) };
            if !d.live.remove(&name) {
                d.double_delete += 1;
            }
            d.deleted += 1;
        }
    });
}
unsafe extern "system" fn bind(_: u32, _: u32) {}
unsafe extern "system" fn parameter(_: u32, _: u32, _: i32) {}
fn owner() -> GpuResources {
    DEVICE.with_borrow_mut(|d| *d = Device::default());
    // SAFETY: each provided address has the system ABI/signature expected by
    // glow. Owner tests use only these entry points, on this same test thread.
    let gl = unsafe {
        glow::Context::from_loader_function(|name| match name {
            "glGetString" => string as *const c_void,
            "glGetIntegerv" => integer as *const c_void,
            "glGenTextures" => generate as *const c_void,
            "glDeleteTextures" => delete as *const c_void,
            "glBindTexture" => bind as *const c_void,
            "glTexParameteri" => parameter as *const c_void,
            _ => std::ptr::null(),
        })
    };
    GpuResources::new(gl)
}
fn image(r: &mut GpuResources) -> u32 {
    r.source(r#"{"width":1,"height":1,"depth":1,"target":3553,"format":1023,"dataType":1009,"srgb":false}"#).unwrap()
}
fn settings(wrap: u32) -> String {
    serde_json::json!({"wrapS":wrap,"wrapT":1001,"wrapR":1001,"min":1006,"mag":1006,"anisotropy":1,"flip":false,"premultiply":false,"alignment":1,"mipmaps":false,"compare":null}).to_string()
}
#[test]
fn shared_storage_retires_only_after_its_pinned_alias_and_reuses_physical_rows() {
    let mut r = owner();
    let s = image(&mut r);
    let a = r.sample(s, &settings(1001)).unwrap();
    let b = r.sample(s, &settings(1001)).unwrap();
    let pinned = r.retain_sample(a).unwrap();
    let texture = r.texture(a).unwrap();
    assert_eq!(r.texture(b).unwrap(), texture);
    r.release_source(s);
    assert!(r.texture(a).is_err());
    assert!(r.texture(b).is_err());
    assert_eq!(r.texture(pinned).unwrap(), texture);
    assert_eq!(DEVICE.with_borrow(|d| d.deleted), 0);
    r.dispose_sample(pinned).unwrap();
    r.dispose_sample(pinned).unwrap();
    assert_eq!(DEVICE.with_borrow(|d| d.deleted), 1);
    assert!(r.cache.is_empty());
    assert_eq!(r.free_physical.len(), 1);
    assert!(r.source_ref(s).is_err());
    // A long stream of births/removals must not grow physical rows. Retired
    // logical ids stay invalid even when a new source occupies their old row.
    for _ in 0..1000 {
        let source = image(&mut r);
        let next = r.sample(source, &settings(1001)).unwrap();
        assert!(next > pinned);
        assert_ne!(r.texture(next).unwrap(), texture);
        r.release_source(source);
        assert!(r.cache.is_empty());
        assert_eq!(r.physical.len(), 1);
        assert!(r.texture(pinned).is_err());
    }
    drop(r);
    DEVICE.with_borrow(|d| {
        assert!(d.live.is_empty());
        assert_eq!(d.deleted, 1001);
        assert_eq!(d.double_delete, 0);
    });
}
#[test]
fn sampler_merges_release_the_old_storage_and_sole_revisions_keep_their_name() {
    let mut r = owner();
    let s = image(&mut r);
    let a = r.sample(s, &settings(1001)).unwrap();
    let b = r.sample(s, &settings(1000)).unwrap();
    let old = r.texture(a).unwrap();
    let retirement_capacity = r.free_physical.capacity();
    r.sampling(a, &settings(1000)).unwrap();
    assert_eq!(r.texture(a).unwrap(), r.texture(b).unwrap());
    assert_ne!(r.texture(a).unwrap(), old);
    assert_eq!(r.free_physical.len(), 1);
    assert_eq!(r.cache.len(), 1);
    r.dispose_sample(b).unwrap();
    let current = r.texture(a).unwrap();
    r.sampling(a, &settings(1002)).unwrap();
    assert_eq!(r.texture(a).unwrap(), current);
    r.dispose_sample(a).unwrap();
    assert!(r.cache.is_empty());
    assert_eq!(r.free_physical.len(), 2);
    assert_eq!(r.free_physical.capacity(), retirement_capacity);
    assert!(r.dispose_sample(u32::MAX).is_err());
    drop(r);
    DEVICE.with_borrow(|d| {
        assert_eq!(d.deleted, 2);
        assert_eq!(d.double_delete, 0);
    });
}
#[test]
fn inconsistent_reference_counts_are_reported_instead_of_saturating() {
    let mut r = owner();
    let s = image(&mut r);
    let a = r.sample(s, &settings(1001)).unwrap();
    r.physical[r.samples[a as usize].physical].users = 0;
    assert!(r.dispose_sample(a).unwrap_err().contains("underflow"));
    drop(r);
    DEVICE.with_borrow(|d| assert_eq!(d.double_delete, 0));
}
#[test]
fn a_released_image_keeps_its_texture_and_changes_sampling_in_place() {
    let mut r = owner();
    let s = image(&mut r);
    r.source_pixels(s, &[1, 2, 3, 4]).unwrap();
    let a = r.sample(s, &settings(1001)).unwrap();
    let b = r.sample(s, &settings(1001)).unwrap();
    assert!(r.release_pixels(s).is_err(), "not uploaded yet");
    let version = r.source_ref(s).unwrap().version;
    r.physical[r.samples[a as usize].physical].seen = version;
    r.release_pixels(s).unwrap();
    assert!(matches!(r.source_ref(s).unwrap().pixels, Pixels::Released(_)));
    let texture = r.texture(a).unwrap();
    // Shared or not, a sampling change goes to the one uploaded texture.
    r.sampling(a, &settings(1000)).unwrap();
    assert_eq!(r.texture(a).unwrap(), texture);
    assert_eq!(r.texture(b).unwrap(), texture);
    assert!(r.upload(a).is_ok(), "nothing to upload again");
    assert!(r.sample(s, &settings(1002)).is_err());
    let mipmapped = settings(1000).replace("\"mipmaps\":false", "\"mipmaps\":true");
    assert!(r.sampling(a, &mipmapped).is_err());
    // New pixels make it an ordinary image again.
    r.source_pixels(s, &[5, 6, 7, 8]).unwrap();
    assert!(matches!(r.source_ref(s).unwrap().pixels, Pixels::Raw(_)));
    drop(r);
    DEVICE.with_borrow(|d| assert_eq!(d.double_delete, 0));
}
