// The far scene, drawn into the far target before the near scene: the sky
// dome and its layers, the map's low-detail buckets, the far terrain and the
// far water, with a frame block of its own (VIEW_FAR: the far camera's
// range). Empty for now: three draws all of it.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct FarPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> FarPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> FarPass<C> {
        FarPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
