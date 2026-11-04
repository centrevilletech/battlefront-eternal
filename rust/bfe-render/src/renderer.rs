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
// three's frame, which draws its passes in order: the far scene (far_frame;
// far_mirror_frame from a water's mirror camera and far_probe_frame for a
// sky probe's faces, both before the frame's own),
// the near scene (frame), what it blends (transparent_frame, in segments
// around what three still blends itself: transparent_segment), the
// first-person view (view_frame), the temporal upscaler's motion of the
// bodies that move on their own (motion_frame), the post passes (post_frame)
// and, over the finished frame, the HUD (hud_frame). A mirror's render of the near scene
// (water.js) draws the near phase's passes for its own camera
// (mirror_frame), then what it blends, as the frame's own render does
// (mirror_transparent_frame, and its segments).
//
// The page writes the frame's inputs (camera, lights, each soldier) into
// `input` and `bones` before a phase (web/, src/rust-render/).
use crate::canopies::CanopiesPass;
use crate::core::{Core, MultiDraw, Stats, VIEW_FAR, VIEW_FAR_MIRROR, VIEW_FIRST_PERSON, VIEW_NEAR, VIEW_SPARE};
use crate::effects::EffectsPass;
use crate::far::FarPass;
use crate::foliage::FoliagePass;
use crate::hud::HudPass;
use crate::instances::{InstancePass, BONES_LEN, INPUT_LEN};
use crate::motion::MotionPass;
use crate::post::PostPass;
use crate::shadow::ShadowPass;
use crate::terrain::TerrainPass;
use crate::transparent::{self, Source, TransparentPass};
use crate::view::ViewPass;
use crate::water::WaterPass;
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
    pub hud: HudPass<C>,
    pub water: WaterPass<C>,
    pub shadow: ShadowPass<C>,
    pub motion: MotionPass<C>,
    pub canopies: CanopiesPass<C>,
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
        let hud = HudPass::new(&mut core);
        let water = WaterPass::new(&mut core);
        let shadow = ShadowPass::new(&mut core);
        let motion = MotionPass::new(&mut core);
        let canopies = CanopiesPass::new(&mut core);
        Renderer { core, world, instances, far, terrain, transparent, effects, foliage, view, post, hud, water, shadow, motion, canopies }
    }

    // The far scene's phase, drawn into the far target before the near scene.
    pub fn far_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw(&mut self.core, VIEW_FAR);
        Ok(self.core.end_phase())
    }

    // The far scene from a water's mirror (water.js reflect, before the
    // frame's own far scene), into the mirror's target: the far pass for the
    // mirror camera, its frame block in VIEW_FAR_MIRROR's buffer.
    pub fn far_mirror_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw(&mut self.core, VIEW_FAR_MIRROR);
        Ok(self.core.end_phase())
    }

    // A face of a sky probe's capture (sky-probe.js), into the cube face
    // three bound: the far pass for the face's camera, with a frame block of
    // the probe's own for the face (far.rs draw_probe).
    pub fn far_probe_frame(&mut self, face: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw_probe(&mut self.core, face);
        Ok(self.core.end_phase())
    }

    // The near scene's phase, inside three's near-scene pass right after the
    // far scene's image: the static world, then the soldiers (records: how
    // many the page wrote), then the terrain, foliage and tree canopies, and
    // the Enhanced ocean's surface (the far pass's items from the near scene).
    pub fn frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_NEAR);
        core.upload_shadow();
        core.bind_lamps();
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.world.draw(core, &camera);
        self.instances.draw(core, &camera, records);
        self.terrain.draw(core);
        self.foliage.draw(core);
        self.canopies.draw(core);
        self.far.draw_near(core);
        Ok(core.end_phase())
    }

    // What the near scene blends (transparent.rs), at the start of three's
    // transparent list: every source's items sorted as three sorts, three's
    // own remaining items (`three` keys the page wrote) merged in, and the
    // first segment drawn; transparent_segment draws the others. image: the
    // post steps the page wrote for the far scene's image (graphics.background,
    // the last of the near scene's opaque objects, which three then leaves
    // out), drawn first: right after three's last opaque item.
    pub fn transparent_frame(&mut self, three: usize, image: usize) -> Result<Stats, String> {
        self.transparent_list(three, VIEW_NEAR, image)
    }

    // A batch of the effects drawn into a texture of its own before the
    // transparent phase reads it (the steam's volumes into their target), with
    // the near view's frame block as the page wrote it for that phase. The
    // page has three bind its own framebuffer again after.
    #[allow(clippy::too_many_arguments)]
    pub fn fx_into_frame(&mut self, batch: usize, target: u32, width: i32, height: i32, count: usize, at: usize, capacity: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_NEAR);
        self.effects.draw_into(core, batch, target, width, height, count, at, capacity);
        Ok(core.end_phase())
    }

    // What a mirror's render of the near scene blends (water.js's
    // reflection, drawn before the frame's own): the transparent phase for
    // the mirror camera, read with its frame block (VIEW_SPARE's), which its
    // later segments bind again; the mirror's far image first, as above.
    pub fn mirror_transparent_frame(&mut self, three: usize, image: usize) -> Result<Stats, String> {
        self.transparent_list(three, VIEW_SPARE, image)
    }

    // The transparent phase for the camera whose values the page wrote last
    // (the frame's own, or a mirror's), with its frame block in `frame`'s
    // buffer.
    fn transparent_list(&mut self, three: usize, frame: usize, image: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        // The far scene's image, at the far plane and depth tested, as three
        // draws it last of the opaque objects (post.rs draw_screen).
        self.post.draw_screen(core, image);
        let view = transparent::view(core);
        let mut list = std::mem::take(&mut self.transparent.list);
        list.items.clear();
        {
            let mut sources: [&mut dyn Source<C, M>; 4] = [&mut self.transparent, &mut self.effects, &mut self.water, &mut self.instances];
            for (s, source) in sources.iter_mut().enumerate() {
                source.emit(core, &view, s as u32, &mut list.items);
            }
        }
        list.order(&self.transparent.keys, three.min(transparent::MAX_THREE));
        list.frame = frame;
        if list.items.is_empty() {
            // Nothing to draw: no GL call at all (but the image's), so three's
            // state stands (the page skips its reset).
            list.view = Some(view);
            self.transparent.list = list;
            return Ok(if image > 0 { core.end_phase() } else { core.stats });
        }
        core.upload_frame(frame);
        core.upload_shadow();
        core.bind_lamps();
        core.bind_far_map();
        {
            let mut sources: [&mut dyn Source<C, M>; 4] = [&mut self.transparent, &mut self.effects, &mut self.water, &mut self.instances];
            transparent::draw_segment(core, &view, list.segment(0), &mut sources);
        }
        list.view = Some(view);
        self.transparent.list = list;
        Ok(core.end_phase())
    }

    // Segment k (from 1) of the frame's transparent list, right after three
    // drew its item k - 1.
    pub fn transparent_segment(&mut self, k: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        let list = std::mem::take(&mut self.transparent.list);
        if let Some(view) = &list.view {
            if !list.segment(k).is_empty() {
                core.bind_frame(list.frame);
                core.bind_shadow();
                core.bind_lamps();
                core.bind_far_map();
                let mut sources: [&mut dyn Source<C, M>; 4] = [&mut self.transparent, &mut self.effects, &mut self.water, &mut self.instances];
                transparent::draw_segment(core, view, list.segment(k), &mut sources);
            }
        }
        self.transparent.list = list;
        Ok(core.end_phase())
    }

    // The first-person view scene's phase, after the depth clear three makes
    // for it: its parts (records: how many the page wrote), read with the
    // view camera's frame block.
    pub fn view_frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_FIRST_PERSON);
        core.bind_lamps();
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.view.draw(core, &camera, &mut self.instances, records);
        Ok(core.end_phase())
    }

    // The near scene's start when the page draws all of it in place of
    // three's render (near.js): the target cleared as three clears it
    // (clear: rgba), then the far scene's image drawn into it (the steps the
    // page wrote for the post pass, into the target bound now).
    pub fn near_begin(&mut self, clear: &[f64], steps: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        if clear.len() >= 4 {
            crate::post::clear(&mut self.core, clear);
        }
        self.post.draw(&mut self.core, steps);
        Ok(self.core.end_phase())
    }

    // The post passes' phase, once the scenes are drawn: the steps the page
    // wrote (post.rs), in order.
    pub fn post_frame(&mut self, steps: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.post.draw(&mut self.core, steps);
        Ok(self.core.end_phase())
    }

    // A full-screen pass outside the post chain, drawn on its own (the
    // storm's cloud panorama into its target, the underwater tint over the
    // finished frame): the steps the page wrote, each with its own state
    // (post.rs draw_screen).
    pub fn screen_frame(&mut self, steps: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.post.draw_screen(&mut self.core, steps);
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

    // A shadow cascade's pass (shadow.rs), in three's shadow pass's place: the
    // map cleared or given its cache's depth, then the casters the page wrote.
    pub fn shadow_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        let drawn = self.shadow.draw(&mut self.core);
        let stats = self.core.end_phase();
        drawn.map(|_| stats)
    }

    // The temporal upscaler's motion phase, once the scenes are drawn: the
    // moving bodies' proxies and the first-person view's locked mask into the
    // velocity target (motion.rs; records: how many the page wrote).
    pub fn motion_frame(&mut self, records: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.motion.draw(&mut self.core, records);
        Ok(self.core.end_phase())
    }

    // The HUD's phase, over the finished frame on the canvas (hud.rs).
    pub fn hud_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.hud.draw(&mut self.core);
        Ok(self.core.end_phase())
    }
}
