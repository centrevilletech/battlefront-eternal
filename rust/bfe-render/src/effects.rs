// The battle's effects: bolts, muzzle flashes, the effects-gpu particles,
// debris, lightning, the sniper's trace, jetpack flames and the droideka's
// shield, drawn in the transparent phase after the transparent pass. The
// simulation of each stays in the page. Empty for now: three draws them.
use crate::core::{Core, MultiDraw};
use glow::HasContext;
use std::marker::PhantomData;

pub struct EffectsPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> EffectsPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> EffectsPass<C> {
        EffectsPass { gl: PhantomData }
    }

    pub fn draw<M: MultiDraw>(&mut self, _core: &mut Core<C, M>) {}
}

// The effects as a source of the transparent list's items (transparent.rs):
// none yet.
impl<C: HasContext, M: MultiDraw> crate::transparent::Source<C, M> for EffectsPass<C> {}
