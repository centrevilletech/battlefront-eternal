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
// A frame is drawn in phases, each a call from the client at its point in
// three's frame, which draws its passes in order: the far scene (far_frame;
// far_mirror_frame from a water's mirror camera and far_probe_frame for a
// sky probe's faces, both before the frame's own),
// the near scene (frame), what it blends (transparent_frame, in segments
// around what three still blends itself: transparent_segment), the
// first-person view (view_frame), the temporal upscaler's motion of the
// bodies that move on their own (motion_frame), the post passes (post_frame)
// and, over the finished frame, the HUD (hud_frame). A mirror's render of the near scene
// draws the near phase's passes for its own camera
// (mirror_frame), then what it blends, as the frame's own render does
// (mirror_transparent_frame, and its segments).
//
// The client writes the frame's inputs (camera, lights, each soldier) into
// `input` and `bones` before a phase (web/, src/rust-render/).
use crate::canopies::CanopiesPass;
use crate::core::{Core, MultiDraw, Stats, VIEW_FAR, VIEW_FAR_MIRROR, VIEW_FIRST_PERSON, VIEW_NEAR, VIEW_SPARE};
use crate::effects::EffectsPass;
use crate::far::FarPass;
use crate::foliage::FoliagePass;
use crate::hud::HudPass;
use crate::interface_scene::InterfacePass;
use crate::instances::{InstancePass, BONES_LEN, INPUT_LEN};
use crate::motion::MotionPass;
use crate::post::PostPass;
use crate::shadow::ShadowPass;
use crate::transparent::{self, Source, TransparentPass};
use crate::view::ViewPass;
use crate::water::WaterPass;
use crate::world::WorldPass;
use glow::HasContext;

pub struct Renderer<C: HasContext, M: MultiDraw> {
    pub core: Core<C, M>,
    pub prepare: crate::prepare::Prepare,
    pub frames: crate::frame::FrameBank<C>,
    pub scene: crate::scene::SceneRegistry,
    pub cameras: crate::cameras::Cameras,
    pub world: WorldPass<C>,
    pub instances: InstancePass<C>,
    pub far: FarPass<C>,
    pub transparent: TransparentPass<C>,
    pub effects: EffectsPass<C>,
    pub foliage: FoliagePass<C>,
    pub view: ViewPass<C>,
    pub post: PostPass<C>,
    pub post_chain: crate::post_chain::PostChain,
    pub hud: HudPass<C>,
    pub interface: InterfacePass,
    pub water: WaterPass<C>,
    pub shadow: ShadowPass<C>,
    pub motion: MotionPass<C>,
    pub canopies: CanopiesPass<C>,
    /// The near fade (start, end) of units, vehicles, held weapons and props when it differs from the
    /// static world's: at the SHORT view distance the map's own geometry hands over to the far scene at the
    /// 2004 game's short range, while everything instanced stays drawn out to the authored range.
    pub actor_fade: Option<[f64; 2]>,
}

impl<C: HasContext, M: MultiDraw> Renderer<C, M> {
    pub fn new(gl: C, multi: M) -> Renderer<C, M> {
        let mut core = Core::new(gl, multi, INPUT_LEN, BONES_LEN);
        let world = WorldPass::new(&mut core);
        let instances = InstancePass::new(&mut core);
        let far = FarPass::new(&mut core);
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
        Renderer { prepare: crate::prepare::Prepare::default(), cameras: crate::cameras::Cameras::default(), scene: crate::scene::SceneRegistry::default(), frames: crate::frame::FrameBank::default(), core, world, instances, far, transparent, effects, foliage, view, post, post_chain: crate::post_chain::PostChain::default(), hud, interface: InterfacePass::default(), water, shadow, motion, canopies, actor_fade: None }
    }

    // Replace a pending image's sampler in every retained draw binding.
    pub fn replace_sample_texture(&mut self, id: usize, texture: C::Texture) -> Result<C::Texture, String> {
        let old = self.core.replace_sample_texture(id, texture)?;
        self.world.replace_texture(old, texture);
        self.effects.replace_texture(old, texture);
        self.transparent.replace_texture(old, texture);
        self.foliage.replace_texture(old, texture);
        Ok(old)
    }

    // The resource owner is about to delete this texture: drop what holds it.
    pub fn forget_texture(&mut self, id: u32) {
        self.core.forget_texture(id);
        self.post.forget_texture(&self.core, id);
    }

    // The far scene's phase, drawn into the far target before the near scene.
    pub(crate) fn far_frame(&mut self, records: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        let timer=self.core.profile.begin(&self.core.gl);
        self.core.bind_light2();
        if records > 0 {
            // Static far world keeps its real fade range in VIEW_FAR while far
            // instanced actors use an unconstrained fade in VIEW_ACTORS. Draw
            // opaque far instances before transparent static far meshes so far
            // shields blend over opaque hulls rather than being overwritten.
            let has_blended_world = self.far.draw_opaque_only(&mut self.core, VIEW_FAR);
            self.core.upload_shadow();
            let camera = self.core.camera();
            let fade_at = crate::shader::FRAME_OFFSETS[18];
            let world_fade = [self.core.input[fade_at], self.core.input[fade_at + 1]];
            self.core.input[fade_at] = 1e9;
            self.core.input[fade_at + 1] = 2e9;
            self.core.upload_frame(crate::core::VIEW_ACTORS);
            self.core.opaque();
            self.instances.draw(&mut self.core, &camera, records);
            self.core.input[fade_at..fade_at + 2].copy_from_slice(&world_fade);
            self.far.draw_blended_with_instances(&mut self.core, &mut self.instances, &camera, has_blended_world);
        } else {
            self.far.draw(&mut self.core, VIEW_FAR);
        }
        self.core.state.front_face(&self.core.gl, &mut self.core.stats, glow::CCW);
        self.core.profile.end(&self.core.gl,"far terrain/sky",timer);
        self.core.end_phase()
    }

    // The far scene from a water's mirror (reflect, before the
    // frame's own far scene), into the mirror's target: the far pass for the
    // mirror camera, its frame block in VIEW_FAR_MIRROR's buffer.
    pub(crate) fn far_mirror_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw(&mut self.core, VIEW_FAR_MIRROR);
        self.core.end_phase()
    }

    // A face of a sky probe's capture, into the cube face
    // three bound: the far pass for the face's camera, with a frame block of
    // the probe's own for the face (far.rs draw_probe).
    pub(crate) fn far_probe_frame(&mut self, face: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.far.draw_probe(&mut self.core, face);
        self.core.end_phase()
    }

    // The near scene's phase, inside three's near-scene pass right after the
    // far scene's image: the tree canopies, then the static world and the
    // soldiers (records: how many the client wrote), terrain and foliage, and
    // the Enhanced ocean's surface (the far pass's items from the near scene).
    pub(crate) fn frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_NEAR);
        core.upload_shadow();
        core.bind_lamps();
        core.bind_light2();
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.world.draw(core, &camera);
        // Solid scenery rejects the hidden crown fragments before their
        // leaf texture and coverage work. Both passes still write depth.
        let timer=core.profile.begin(&core.gl);
        self.canopies.draw(core);
        core.profile.end(&core.gl,"canopies",timer);
        let timer=core.profile.begin(&core.gl);
        let fade = crate::shader::FRAME_MEMBERS.iter().position(|(n, _)| *n == "fade").map(|i| crate::shader::FRAME_OFFSETS[i]);
        let actors = self.actor_fade.zip(fade).filter(|(f, at)| core.input[*at] != f[0] || core.input[*at + 1] != f[1]);
        if let Some((f, at)) = actors {
            let world = [core.input[at], core.input[at + 1]];
            core.input[at..at + 2].copy_from_slice(&f);
            core.upload_frame(crate::core::VIEW_ACTORS);
            self.instances.draw(core, &camera, records);
            core.input[at..at + 2].copy_from_slice(&world);
            core.bind_frame(VIEW_NEAR);
        } else {
            self.instances.draw(core, &camera, records);
        }
        core.profile.end(&core.gl,"skinned units",timer);
        let timer=core.profile.begin(&core.gl);
        self.foliage.draw(core);
        core.profile.end(&core.gl,"foliage",timer);
        self.far.draw_near(core);
        core.end_phase()
    }

    // What the near scene blends (transparent.rs), at the start of three's
    // transparent list: every source's items sorted as three sorts, three's
    // own remaining items (`three` keys the client wrote) merged in, and the
    // first segment drawn; transparent_segment draws the others. image: the
    // post steps the client wrote for the far scene's image (graphics.background,
    // the last of the near scene's opaque objects, which three then leaves
    // out), drawn first: right after three's last opaque item.
    pub(crate) fn transparent_frame(&mut self, three: usize, image: usize) -> Result<Stats, String> {
        self.transparent_list(three, VIEW_NEAR, image)
    }

    // A batch of the effects drawn into a texture of its own before the
    // transparent phase reads it (the steam's volumes into their target), with
    // the near view's frame block as the client wrote it for that phase. The
    // page has three bind its own framebuffer again after.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fx_into_frame(&mut self, batch: usize, target: u32, width: i32, height: i32, count: usize, at: usize, capacity: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_NEAR);
        self.effects.draw_into(core, batch, target, width, height, count, at, capacity);
        core.end_phase()
    }

    // What a mirror's render of the near scene blends (water.js's
    // reflection, drawn before the frame's own): the transparent phase for
    // the mirror camera, read with its frame block (VIEW_SPARE's), which its
    // later segments bind again; the mirror's far image first, as above.
    pub(crate) fn mirror_transparent_frame(&mut self, three: usize, image: usize) -> Result<Stats, String> {
        self.transparent_list(three, VIEW_SPARE, image)
    }

    // The transparent phase for the camera whose values the client wrote last
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
            use crate::frame_plan::layout::{limit as L,line as V};
            let view_index=self.frames.active_lines.min(2);let count=self.frames.line_counts[view_index].min(L::LINES);
            let at=view_index*L::LINES*V::LEN;let values=self.frames.lines.get(at..at+count*V::LEN).unwrap_or(&[]);
            let mut lines=crate::frame::LineSource{post:&mut self.post,values,count:count.min(values.len()/V::LEN)};
            // A line preceded equal keys at the old segment boundary. Emit it
            // first so the stable native sort preserves precisely that tie.
            lines.emit(core,&view,4,&mut list.items);
            let mut sources: [&mut dyn Source<C, M>; 4] = [&mut self.transparent, &mut self.effects, &mut self.water, &mut self.instances];
            for (s,source) in sources.iter_mut().enumerate(){source.emit(core,&view,s as u32,&mut list.items);}
        }
        list.order(&self.transparent.keys, three.min(transparent::MAX_THREE));
        list.frame = frame;
        if list.items.is_empty() {
            // Nothing to draw: no GL call at all (but the image's), so three's
            // state stands (the client skips its reset).
            list.view = Some(view);
            self.transparent.list = list;
            return if image > 0 { core.end_phase() } else { Ok(core.stats) };
        }
        core.upload_frame(frame);
        core.upload_shadow();
        core.bind_lamps();
        core.bind_light2();
        core.bind_far_map();
        {
            use crate::frame_plan::layout::{limit as L,line as V};
            let index=self.frames.active_lines.min(2);let count=self.frames.line_counts[index].min(L::LINES);let at=index*L::LINES*V::LEN;
            let values=self.frames.lines.get(at..at+count*V::LEN).unwrap_or(&[]);
            let mut lines=crate::frame::LineSource{post:&mut self.post,values,count:count.min(values.len()/V::LEN)};
            let mut sources: [&mut dyn Source<C,M>;5]=[&mut self.transparent,&mut self.effects,&mut self.water,&mut self.instances,&mut lines];
            transparent::draw_segment(core,&view,list.segment(0),&mut sources);
        }
        list.view = Some(view);
        self.transparent.list = list;
        core.end_phase()
    }


    // The first-person view scene's phase, after the depth clear three makes
    // for it: its parts (records: how many the client wrote), read with the
    // view camera's frame block.
    pub(crate) fn view_frame(&mut self, records: usize) -> Result<Stats, String> {
        let core = &mut self.core;
        core.begin_phase();
        core.upload_frame(VIEW_FIRST_PERSON);
        core.bind_lamps();
        core.bind_light2();
        let camera = core.camera();
        core.opaque();
        core.bind_far_map();
        self.view.draw(core, &camera, &mut self.instances, &mut self.effects, records);
        core.end_phase()
    }



    pub(crate) fn post_chain_draw(&mut self) -> Result<Stats,String> {
        let steps=self.post_chain.plan(&mut self.post.input);
        self.core.begin_phase();self.post.draw(&mut self.core,steps);
        match self.core.end_phase() {
            Ok(stats)=>{self.post_chain.stats(stats);Ok(stats)},
            Err(error)=>{self.post_chain.invalidate();Err(error)},
        }
    }

    // A full-screen pass outside the post chain, drawn on its own (the
    // storm's cloud panorama into its target, the underwater tint over the
    // finished frame): the steps the client wrote, each with its own state
    // (post.rs draw_screen).
    pub(crate) fn screen_frame(&mut self, steps: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        self.post.draw_screen(&mut self.core, steps);
        self.core.end_phase()
    }

    // The near phase from a mirror (water.js's reflection of the near scene,
    // drawn before the frame's own): the same passes for the mirror camera,
    // its frame block in VIEW_SPARE's buffer.
    pub(crate) fn mirror_frame(&mut self, records: usize) -> Result<Stats, String> {
        self.core.swap_views(VIEW_NEAR, VIEW_SPARE);
        let stats = self.frame(records);
        self.core.swap_views(VIEW_NEAR, VIEW_SPARE);
        stats
    }

    // A shadow cascade's pass (shadow.rs), in three's shadow pass's place: the
    // map cleared or given its cache's depth, then the casters the client wrote.
    pub(crate) fn shadow_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        let drawn = self.shadow.draw(&mut self.core);
        let stats = self.core.end_phase();
        drawn.and(stats)
    }

    // The temporal upscaler's motion phase, once the scenes are drawn: the
    // moving bodies' proxies and the first-person view's locked mask into the
    // velocity target (motion.rs; records: how many the client wrote).
    pub(crate) fn motion_frame(&mut self, records: usize) -> Result<Stats, String> {
        self.core.begin_phase();
        let timer=self.core.profile.begin(&self.core.gl);
        self.motion.draw(&mut self.core, records);
        self.core.profile.end(&self.core.gl,"motion vectors",timer);
        self.core.end_phase()
    }

    // The HUD's phase, over the finished frame on the canvas (hud.rs).
    pub(crate) fn hud_frame(&mut self) -> Result<Stats, String> {
        self.core.begin_phase();
        self.hud.draw(&mut self.core);
        self.core.end_phase()
    }
}
