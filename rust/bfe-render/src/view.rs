// The first-person view scene: the weapon in view, drawn last over the near
// scene in a depth range of its own, with its own camera and frame block
// (VIEW_FIRST_PERSON). Empty for now: three draws it.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct ViewPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> ViewPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> ViewPass<C> {
        ViewPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
