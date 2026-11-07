//! UnitFactory owns prepared services and the phase keeps their warmed work.
//! Store owns canonical bodies. Project a service field or detach its existing
//! frame before callbacks; never borrow this enclosing owner across gameplay.
use super::{f3, f3_construction, f3_hulls, live_pose, module, muzzles};

pub(crate) struct Units {
    pub(super) phase: Option<module::Phase>,
    pub(super) state: Option<f3::State>,
    pub(super) factory: Option<f3_construction::Factory>,
    pub(super) hulls: Option<f3_hulls::Hulls>,
    pub(super) body_poses: live_pose::BodyPoses,
    pub(super) muzzles: muzzles::MuzzleRigs,
}
impl Units {
    pub(crate) const fn new() -> Self {
        Self { phase: None, state: None, factory: None, hulls: None,
            body_poses: live_pose::BodyPoses::new(), muzzles: muzzles::MuzzleRigs::new() }
    }
}
// The WASM instance owns this one facade root until its World is constructed.
// No semantic current-world pointer or whole-owner mutable loan is required.
pub(super) static mut UNITS: Units = Units::new();
