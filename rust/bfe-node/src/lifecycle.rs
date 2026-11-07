//! A host and its still-open owners share one environment registration. The
//! registration owns only weak Context references, never a world lifetime.
use super::{boundary, Env, Error, Result};
use bfe_sim::abi::context::Context;
use napi::sys;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::rc::{Rc, Weak};

pub(super) struct OwnedContext {
    pub context: Option<Context>,
    pub arenas: super::operations::Arenas,
    pub publication: super::world::Packets,
    pub poisoned: bool,
    #[cfg(feature = "lifecycle-probe")]
    pub panic_on_close: bool,
}
pub(super) type Owner = Rc<RefCell<OwnedContext>>;
type Registry = RefCell<Vec<Weak<RefCell<OwnedContext>>>>;
struct Registration { registry: Registry, registered: Cell<bool> }
pub(super) struct Scope { env: Env, data: Rc<Registration> }

impl Scope {
    pub fn new(env: Env) -> Result<Rc<Self>> {
        let scope = Rc::new(Self {env, data: Rc::new(Registration {
            registry: RefCell::default(), registered: Cell::new(false),
        })});
        let data = Rc::into_raw(scope.data.clone()) as *mut c_void;
        // SAFETY: the extra Rc belongs to Node until cleanup or successful
        // removal. The callback and pointer are the exact removal identity.
        let status = unsafe {sys::napi_add_env_cleanup_hook(env.raw(), Some(cleanup), data)};
        if status != sys::Status::napi_ok {
            // SAFETY: registration failed, so Node never acquired this Rc.
            unsafe {drop(Rc::from_raw(data as *const Registration));}
            return Err(Error::from_reason("Cannot register environment cleanup"));
        }
        scope.data.registered.set(true);
        #[cfg(feature = "lifecycle-probe")]
        super::HOOKS_CREATED.fetch_add(1, super::Ordering::SeqCst);
        Ok(scope)
    }
    pub fn insert(&self, owner: &Owner) {
        let mut entries = self.data.registry.borrow_mut();
        entries.retain(|entry| entry.strong_count() != 0);
        entries.push(Rc::downgrade(owner));
    }
    pub fn live(&self) -> u32 {
        self.data.registry.borrow().iter().filter_map(Weak::upgrade)
            .filter(|owner| owner.borrow().context.is_some()).count() as u32
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        // Finalizers cannot propagate Rust unwinds through Node. Cleanup marks
        // this false before touching Contexts, so a later finalizer uses no Env.
        let _ = boundary(|| {
            if !self.data.registered.get() {return Ok(());}
            let data = Rc::as_ptr(&self.data) as *mut c_void;
            // SAFETY: this is the live Env, callback and still-registered Rc
            // from new(). Node runs hooks/finalizers on this same thread.
            let status = unsafe {sys::napi_remove_env_cleanup_hook(self.env.raw(), Some(cleanup), data)};
            if status != sys::Status::napi_ok {
                // Node retains its Rc and will reclaim it during cleanup.
                return Err(Error::from_reason("Cannot remove environment cleanup"));
            }
            self.data.registered.set(false);
            #[cfg(feature = "lifecycle-probe")]
            super::HOOKS_RELEASED.fetch_add(1, super::Ordering::SeqCst);
            // SAFETY: successful removal transfers Node's one Rc back to us.
            // Our own data Rc keeps the registration alive through this drop.
            unsafe {drop(Rc::from_raw(data as *const Registration));}
            Ok(())
        });
    }
}
unsafe extern "C" fn cleanup(data: *mut c_void) {
    let _ = boundary(|| {
        // SAFETY: Node invokes this registration once. Ordinary removal takes
        // the same Rc only after Node has promised not to invoke its callback.
        let data = unsafe {Rc::from_raw(data as *const Registration)};
        data.registered.set(false);
        #[cfg(feature = "lifecycle-probe")]
        super::HOOKS_RELEASED.fetch_add(1, super::Ordering::SeqCst);
        for entry in data.registry.borrow().iter() {
            if let Some(owner) = entry.upgrade() {
                // One owner's failure must neither unwind through the C hook
                // nor prevent another owner's resources from being released.
                let _ = boundary(|| {close(&owner); Ok(())});
            }
        }
        Ok(())
    });
}
pub(super) fn close(owner: &Owner) -> bool {
    // Return the RefCell loan before disposal: a resource destructor may call
    // into another owner, and no loan should span that callback point.
    let context = {
        let mut owner = owner.borrow_mut();
        owner.arenas.release();
        owner.publication = super::world::Packets::default();
        owner.context.take()
    };
    if let Some(context) = context {
        let _ = context.dispose();
        drop(context);
        super::RELEASED.fetch_add(1, super::Ordering::SeqCst);
        #[cfg(feature = "lifecycle-probe")]
        {
            let panic = std::mem::take(&mut owner.borrow_mut().panic_on_close);
            if panic {panic!("Context close fixture");}
        }
        true
    } else {false}
}
