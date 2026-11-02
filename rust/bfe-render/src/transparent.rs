// What the near scene blends: the map's blended buckets, holograms,
// sprites and basic materials, in three's back-to-front order (its
// transparent list, which this pass must own to keep that order). Empty for
// now: three draws all of it.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct TransparentPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> TransparentPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> TransparentPass<C> {
        TransparentPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
