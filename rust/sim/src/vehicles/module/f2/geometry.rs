//! Each posed hull's seat geometry (Vehicle.seatEye/seatFrame and the
//! scalar part of poseOut) was sampled here into delta rows for the
//! JavaScript host. Nothing native reads them, so nothing is sampled; the
//! named bank (F2_POSES) stays empty.
use crate::store::block::Block;
pub(crate) struct Geometry { output: Option<Block> }
impl Geometry {
    pub(in crate::vehicles::module) const fn new() -> Self { Self { output: None } }
}
#[cfg(target_arch = "wasm32")]
pub(super) fn words_in(geometry: &Geometry) -> &[f64] { geometry.output.as_ref().map_or(&[], |block| &block.words[..]) }
/// The same bank for a native host reading by name.
pub(super) fn bank_in(geometry: &Geometry) -> Option<&[f64]> { geometry.output.as_ref().map(|block| &block.words[..]) }
