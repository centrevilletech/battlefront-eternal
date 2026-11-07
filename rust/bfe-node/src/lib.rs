//! Node lifecycle and numeric block boundary. Production world stepping remains
//! in the WASM instance until the shared simulation exposes an owned world API.
use bfe_sim::abi::context::Context;
use napi::{Env, Error, Result, Status};
use napi_derive::napi;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;
mod blocks;
mod operations;
mod lifecycle;
use lifecycle::{Owner, OwnedContext, Scope, close};

use std::sync::atomic::{AtomicU32, Ordering};
static CREATED: AtomicU32 = AtomicU32::new(0);
static RELEASED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "lifecycle-probe")]
static HOOKS_CREATED: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "lifecycle-probe")]
static HOOKS_RELEASED: AtomicU32 = AtomicU32::new(0);

// Counters contain no owners or IDs and never participate in simulation.
#[napi]
pub fn lifecycle_counts() -> Result<Vec<u32>> {
    boundary(|| Ok(vec![CREATED.load(Ordering::SeqCst), RELEASED.load(Ordering::SeqCst)]))
}

// No raw extern-C simulation export crosses this unwind boundary. Errors stop
// the invocation before N-API sees a result; no Rust borrow survives a call.
fn boundary<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    catch_unwind(AssertUnwindSafe(f))
        .unwrap_or_else(|_| Err(Error::new(Status::GenericFailure, "Native host panic")))
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

// Hosts and still-open owners retain the cleanup registration together. The
// last one removes it; environment teardown closes only weak Context targets.
#[napi]
pub struct NativeHost {
    scope: Rc<Scope>,
}
#[napi]
impl NativeHost {
    #[napi(constructor)]
    pub fn new(env: Env) -> Result<Self> {
        boundary(|| Ok(Self {scope: Scope::new(env)?}))
    }
    #[napi]
    pub fn create(&self, reserve: Option<operations::Reserve>) -> Result<NativeOwner> {
        self.create_owner(reserve, None)
    }
    fn create_owner(&self, reserve: Option<operations::Reserve>, _fail_after: Option<u32>) -> Result<NativeOwner> {
        boundary(|| {
            let owner = Rc::new(RefCell::new(OwnedContext {context: Some(Context::new()), arenas: operations::Arenas::default(), poisoned: false,
                #[cfg(feature = "lifecycle-probe")] panic_on_close: false}));
            // Establish the take-once destructor before registry allocation or
            // diagnostic failure injection. A partial construction owns a drop.
            let native = NativeOwner { owner, scope: RefCell::new(Some(self.scope.clone())) };
            CREATED.fetch_add(1, Ordering::SeqCst);
            #[cfg(feature = "lifecycle-probe")]
            if _fail_after == Some(1) { return Err(Error::from_reason("Context acquisition fixture")); }
            self.scope.insert(&native.owner);
            #[cfg(feature = "lifecycle-probe")]
            if _fail_after == Some(2) { return Err(Error::from_reason("Registry acquisition fixture")); }
            native.invoke(|owner| owner.arenas.reserve(reserve))?;
            #[cfg(feature = "lifecycle-probe")]
            if _fail_after == Some(3) { return Err(Error::from_reason("Arena acquisition fixture")); }
            #[cfg(feature = "lifecycle-probe")]
            if _fail_after == Some(4) {
                native.intern(vec![65])?;
                return Err(Error::from_reason("Cold admission acquisition fixture"));
            }
            Ok(native)
        })
    }
    #[napi]
    pub fn live_owners(&self) -> Result<u32> {
        boundary(|| Ok(self.scope.live()))
    }
}

#[napi]
pub struct NativeOwner {
    owner: Owner,
    scope: RefCell<Option<Rc<Scope>>>,
}
#[napi]
impl NativeOwner {
    #[napi]
    pub fn close(&self) -> Result<bool> {
        let result = boundary(|| Ok(close(&self.owner)));
        // Even a failed destructor may have closed the Context. Release its
        // registration loan then, but retain it if an active borrow rejected close.
        let _ = boundary(|| {
            if self.owner.try_borrow().is_ok_and(|owner| owner.context.is_none()) {
                self.scope.borrow_mut().take();
            }
            Ok(())
        });
        result
    }


}
impl Drop for NativeOwner {
    fn drop(&mut self) { let _ = boundary(|| {close(&self.owner); Ok(())}); }
}

// Exercise the actual N-API unwind boundary in the diagnostic build.
#[cfg(feature = "lifecycle-probe")]
#[napi]
pub fn probe_panic() -> Result<()> {
    boundary(|| panic!("fixture"))
}

#[cfg(feature = "lifecycle-probe")]
mod probe;
