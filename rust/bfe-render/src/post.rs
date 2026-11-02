// The frame's last passes over the near scene's image: the composite (the
// soft-focus blur, bloom and speed blur), FXAA, and the bilinear upscale to
// the window. Empty for now: three draws them.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct PostPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> PostPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> PostPass<C> {
        PostPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}
