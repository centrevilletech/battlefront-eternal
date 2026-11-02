// The near scene's foliage (foliage.js, foliage-gpu.js), drawn in the near
// phase. Empty for now: three draws it.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct FoliagePass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> FoliagePass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> FoliagePass<C> {
        FoliagePass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
