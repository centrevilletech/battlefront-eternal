//! Optional page resources share the simulation lifetime without starting on
//! a dedicated host. Admission reserves each private replay or sound bank;
//! ordinary frames borrow its existing buffers through a qualified identity.
use crate::store::handles::Pool;
pub struct Resources {
    pub prediction:Pool<super::ClientPrediction>,
    pub sound:Pool<::audio_control::runtime::Runtime>,
}
impl Default for Resources {fn default()->Self {Self::new()}}
impl Resources {pub const fn new()->Self {Self {prediction:Pool::new(13),sound:Pool::new(14)}}}
