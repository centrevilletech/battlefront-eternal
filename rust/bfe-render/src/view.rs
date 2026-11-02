// The first-person view scene: the arms and the weapon in view, drawn last
// over the near scene after a depth clear, with its own camera (near 0.3 m,
// far 10 m) and frame block (VIEW_FIRST_PERSON: the view camera, and the
// point lights packed for the view scene, which holds a copy of each of the
// near scene's: Graphics.syncViewLights).
//
// Each part is a rigid mesh hung from a bone of the arms' rig (or the gun in
// its hand), drawn as the soldiers' guns are (instances.rs): a record a part
// the page wrote, instanced by model, texture and program, its matrices,
// look and lights in a row of the instance texture. Its draws count as this
// pass's.
use crate::core::{Camera, Core, MultiDraw, PASS_INSTANCES, PASS_VIEW};
use crate::instances::InstancePass;
use glow::HasContext;
use std::marker::PhantomData;

pub struct ViewPass<C: HasContext> {
    gl: PhantomData<C>,
}

impl<C: HasContext> ViewPass<C> {
    pub fn new<M: MultiDraw>(_core: &mut Core<C, M>) -> ViewPass<C> {
        ViewPass { gl: PhantomData }
    }

    // records: how many the page wrote for the view's parts.
    pub fn draw<M: MultiDraw>(&mut self, core: &mut Core<C, M>, camera: &Camera, instances: &mut InstancePass<C>, records: usize) {
        if records == 0 {
            return;
        }
        let before = core.stats.pass_draws[PASS_INSTANCES];
        instances.draw(core, camera, records);
        let drawn = core.stats.pass_draws[PASS_INSTANCES] - before;
        core.stats.pass_draws[PASS_INSTANCES] = before;
        core.stats.pass_draws[PASS_VIEW] += drawn;
    }
}
