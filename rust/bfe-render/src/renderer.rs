// The battle renderer: the near scene's static world and its soldiers (their
// bodies and held guns), drawn with few GL calls:
//
// - one uniform-block upload a view and frame for everything it shares;
// - the map's buckets culled by spatial cell as render-culling.js does, the
//   visible runs drawn by one multi-draw a bucket (world.rs);
// - every visible soldier's body part (and gun) drawn instanced, a draw a
//   model, texture and program, its per-object values in a texture row and
//   its bones in one texture holding every drawn soldier's (instances.rs).
//
// It is made of passes, each a module of its own, sharing the core
// (core.rs: the context, programs, geometries, textures, inputs, GL state).
// A frame is drawn in phases, each a call from the page at its point in
// three's frame, which draws its passes in order: the far scene (far_frame),
// the near scene (frame), what it blends (transparent_frame), the
// first-person view (view_frame) and the post passes (post_frame). A mirror's
// render of the near scene (water.js) draws the near phase's passes for its
// own camera (mirror_frame).
//
// The page writes the frame's inputs (camera, lights, each soldier) into
// `input` and `bones` before a phase (web/, src/rust-render/).
use crate::core::{Core, MultiDraw, Stats, VIEW_FIRST_PERSON, VIEW_NEAR, VIEW_SPARE};
use crate::effects::EffectsPass;
use crate::far::FarPass;
use crate::foliage::FoliagePass;
use crate::instances::{InstancePass, BONES_LEN, INPUT_LEN};
use crate::post::PostPass;
use crate::terrain::TerrainPass;
use crate::transparent::TransparentPass;
use crate::view::ViewPass;
use crate::world::WorldPass;
use glow::HasContext;

pub struct Renderer<C: HasContext, M: MultiDraw> {
    pub core: Core<C, M>,
    pub world: WorldPass<C>,
    pub instances: InstancePass<C>,
    pub far: FarPass<C>,
    pub terrain: TerrainPass<C>,
    pub transparent: TransparentPass<C>,
    pub effects: EffectsPass<C>,
    pub foliage: FoliagePass<C>,
    pub view: ViewPass<C>,
    pub post: PostPass<C>,
}

impl<C: HasContext, M: MultiDraw> Renderer<C, M> {
    pub fn new(gl: C, multi: M) -> Renderer<C, M> {
        let mut core = Core::new(gl, multi, INPUT_LEN, BONES_LEN);
        let world = WorldPass::new(&mut core);
        let instances = InstancePass::new(&mut core);
        let far = FarPass::new(&mut core);
        let terrain = TerrainPass::new(&mut core);
        let transparent = TransparentPass::new(&mut core);
        let effects = EffectsPass::new(&mut core);
        let foliage = FoliagePass::new(&mut core);
        let view = ViewPass::new(&mut core);
        let post = PostPass::new(&mut core);
        Renderer { core, world, instances, far, terrain, transparent, effects, foliage, view, post }
    }

    // The far scene's phase, drawn into the far target before the near scene.
    pub fn far_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw(&mut self.core);
        Ok(self.core.end_phase())
    }

    // The near scene's phase, inside three's near-scene pass right after the
    // far scene's image: the static world, then the soldiers (records: how
    // many the page wrote), then the terrain and foliage.
    pub fn frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_NEAR);
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.world.draw(core, &camera);
        self.instances.draw(core, &camera, records);
        self.terrain.draw(core);
        self.foliage.draw(core);
        Ok(core.end_phase())
    }

    // What the near scene blends, at the start of three's transparent list:
    // the transparent pass, then the effects.
    pub fn transparent_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.transparent.draw(&mut self.core);
        self.effects.draw(&mut self.core);
        Ok(self.core.end_phase())
    }

    // The first-person view scene's phase, after the depth clear three makes
    // for it: its parts (records: how many the page wrote), read with the
    // view camera's frame block.
    pub fn view_frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_FIRST_PERSON);
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.view.draw(core, &camera, &mut self.instances, records);
        Ok(core.end_phase())
    }

    // The post passes' phase, once the scenes are drawn.
    pub fn post_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.post.draw(&mut self.core);
        Ok(self.core.end_phase())
    }

    // The near phase from a mirror (water.js's reflection of the near scene,
    // drawn before the frame's own): the same passes for the mirror camera,
    // its frame block in VIEW_SPARE's buffer.
    pub fn mirror_frame(&mut self, records: usize) -> Result<Stats, String> {
        self.core.swap_views(VIEW_NEAR, VIEW_SPARE);
        let stats = self.frame(records);
        self.core.swap_views(VIEW_NEAR, VIEW_SPARE);
        stats
    }
}
