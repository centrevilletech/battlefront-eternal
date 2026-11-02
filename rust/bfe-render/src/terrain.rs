// The near scene's terrain: its patches and their painted layers, drawn in
// the near phase. Empty for now: three draws it.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct TerrainPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> TerrainPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> TerrainPass<C> {
        TerrainPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
