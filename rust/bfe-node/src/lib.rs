//! Node lifecycle and numeric block boundary. Production world stepping remains
//! in the WASM instance until the shared simulation exposes an owned world API.
use bfe_sim::abi::context::Context;
use bfe_sim::gen::BlockHeader;
use napi::{bindgen_prelude::Float64Array, Env, Error, Result, Status};
use napi_derive::napi;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::{Rc, Weak};

#[cfg(feature = "lifecycle-probe")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(feature = "lifecycle-probe")]
static CREATED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "lifecycle-probe")]
static RELEASED: AtomicU32 = AtomicU32::new(0);

// Test counters contain no owners or IDs and never participate in simulation.
#[cfg(feature = "lifecycle-probe")]
#[napi]
pub fn lifecycle_counts() -> Vec<u32> {
    vec![CREATED.load(Ordering::SeqCst), RELEASED.load(Ordering::SeqCst)]
}

type Owner = Rc<RefCell<Option<Context>>>;
type Registry = Rc<RefCell<Vec<Weak<RefCell<Option<Context>>>>>>;

// No raw extern-C simulation export crosses this unwind boundary. Errors stop
// the invocation before N-API sees a result; no Rust borrow survives a call.
fn boundary<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(Error::new(Status::GenericFailure, "Native host panic")))
}
fn close(owner: &Owner) -> bool {
    if let Some(context) = owner.borrow_mut().take() {
        // No callbacks or active invocations exist in this foundation API.
        let _ = context.dispose();
        #[cfg(feature = "lifecycle-probe")]
        RELEASED.fetch_add(1, Ordering::SeqCst);
        true
    } else {
        false
    }
}

#[napi(object)]
pub struct Handshake {
    pub addon_abi: u32,
    pub store_abi: u32,
    pub schema: String,
    pub engine_build: String,
    pub napi_minimum: u32,
    pub native_world: bool,
    pub native_publication: bool,
}
#[napi]
pub fn handshake() -> Handshake {
    Handshake {
        addon_abi: 1,
        store_abi: 1,
        schema: env!("BFE_SCHEMA_FINGERPRINT").into(),
        engine_build: env!("BFE_ENGINE_BUILD").into(),
        napi_minimum: 8,
        native_world: false,
        native_publication: false,
    }
}

// One registration per host, scoped to its Node environment. Only weak owner
// references are retained, so collecting a battle releases its Context too.
#[napi]
pub struct NativeHost {
    registry: Registry,
}
#[napi]
impl NativeHost {
    #[napi(constructor)]
    pub fn new(mut env: Env) -> Result<Self> {
        boundary(|| {
            let registry: Registry = Rc::default();
            env.add_env_cleanup_hook(registry.clone(), |registry| {
                for entry in registry.borrow().iter() {
                    if let Some(owner) = entry.upgrade() { close(&owner); }
                }
            })?;
            Ok(Self { registry })
        })
    }
    #[napi]
    pub fn create(&self) -> Result<NativeOwner> {
        boundary(|| {
            let owner = Rc::new(RefCell::new(Some(Context::new())));
            let mut entries = self.registry.borrow_mut();
            entries.retain(|entry| entry.strong_count() != 0);
            entries.push(Rc::downgrade(&owner));
            #[cfg(feature = "lifecycle-probe")]
            CREATED.fetch_add(1, Ordering::SeqCst);
            Ok(NativeOwner { owner })
        })
    }
    #[napi]
    pub fn live_owners(&self) -> u32 {
        self.registry.borrow().iter().filter_map(Weak::upgrade)
            .filter(|owner| owner.borrow().is_some()).count() as u32
    }
}

#[napi]
pub struct NativeOwner {
    owner: Owner,
}
#[napi]
impl NativeOwner {
    #[napi]
    pub fn close(&self) -> Result<bool> { boundary(|| Ok(close(&self.owner))) }

    // Copy into a caller-owned reusable arena. JS owns both buffers, and Rust
    // retains neither. Byte-preserving copies keep signed zero and NaN payloads.
    // This operation validates the common header, not schema-specific records.
    #[napi]
    pub fn copy_block(&self, source: Float64Array, mut output: Float64Array) -> Result<u32> {
        boundary(|| {
            if self.owner.borrow().is_none() { return Err(Error::from_reason("Native owner closed")); }
            validate(&source)?;
            if output.len() < source.len() { return Err(Error::from_reason("Output arena capacity")); }
            // The caller may pass overlapping views of its arena. memmove keeps
            // that legal without forming simultaneous aliased Rust slices.
            unsafe { std::ptr::copy(source.as_ptr(), output.as_mut_ptr(), source.len()); }
            Ok(source.len() as u32)
        })
    }
}
impl Drop for NativeOwner {
    fn drop(&mut self) { close(&self.owner); }
}

// Never cast an unchecked floating count or a native address into an index.
// Checked extents reject malformed/capacity inputs before any output is written.
fn validate(block: &[f64]) -> Result<()> {
    let invalid = || Error::new(Status::InvalidArg, "Invalid numeric block");
    if block.len() < BlockHeader::WORDS || block.len() > u32::MAX as usize { return Err(invalid()); }
    let count = |at: usize| -> Result<usize> {
        let n = block[at];
        if !n.is_finite() || n < 0.0 || n.fract() != 0.0 || n > block.len() as f64 { return Err(invalid()); }
        Ok(n as usize)
    };
    let rows = count(BlockHeader::ROWS)?;
    let stride = count(BlockHeader::STRIDE)?;
    let heap = count(BlockHeader::HEAP)?;
    let size = count(BlockHeader::HEAP_WORDS)?;
    let retired = count(BlockHeader::RETIRED)?;
    if retired > rows || rows.checked_mul(stride).and_then(|n| n.checked_add(BlockHeader::WORDS)) != Some(heap)
        || heap.checked_add(size) != Some(block.len()) { return Err(invalid()); }
    Ok(())
}

// Exercise the actual N-API unwind boundary in the diagnostic build.
#[cfg(feature = "lifecycle-probe")]
#[napi]
pub fn probe_panic() -> Result<()> {
    boundary(|| panic!("fixture"))
}
