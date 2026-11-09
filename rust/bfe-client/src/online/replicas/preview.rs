//! ClassPreview's independent idle/fidget clock and droideka display cycle.
//! These are menu poses; no random draw or command reaches a battle body.
use super::*;
use bfe_presentation::unit_content::Input;
use jsrt::rng::ClassicRandom;
const CYCLE: [(usize, f64); 8] = [
    (0, 2.5),
    (1, 0.),
    (0, 1.5),
    (2, 0.),
    (0, 2.5),
    (11, 0.),
    (12, 1.2),
    (13, 0.),
];
pub(super) struct Animation {
    lengths: Option<[f64; 14]>,
    emote: Option<(usize, f64)>,
    wait: f64,
    fidget: f64,
    step: usize,
    time: f64,
    held: f64,
    roll: f64,
}
impl Animation {
    pub fn new(input: &Input, emote: Option<usize>) -> Self {
        Self {
            lengths: input
                .walker
                .as_ref()
                .map(|w| w.by_mode.map(|i| i.map_or(0., |i| input.clips[i].duration))),
            emote: emote
                .filter(|i| input.walker.is_none() && *i < input.clips.len())
                .map(|i| (i, input.clips[i].duration)),
            wait: 0.,
            fidget: 0.,
            step: 0,
            time: 0.,
            held: 0.,
            roll: 0.,
        }
    }
    fn command(units: &mut Units, operation: u32) -> &mut [f64] {
        let at = units.input[f::COMMANDS] as usize * command::STRIDE;
        units.input[f::COMMANDS] += 1.;
        let row = &mut units.commands[at..at + command::STRIDE];
        row.fill(0.);
        row[command::KIND] = operation as f64;
        row
    }
    pub fn update(
        &mut self,
        units: &mut Units,
        dt: f64,
        bank: usize,
        reset: bool,
        rng: &mut ClassicRandom,
    ) {
        units.input[f::COMMANDS] = 0.;
        if reset {
            Self::command(units, kind::RESET);
            self.wait = 3. + rng.uniform() * 5.;
            self.fidget = 0.;
            self.step = 0;
            self.time = 0.;
            self.held = 0.;
            self.roll = 0.;
        }
        Self::command(units, kind::BANK)[command::BANK] = bank as f64;
        if let Some(lengths) = self.lengths {
            let (mode, hold) = CYCLE[self.step];
            self.time += dt;
            self.held += dt;
            if mode == 12 {
                self.roll += dt * 6.;
            }
            let length = lengths[mode];
            if if hold > 0. {
                self.held >= hold
            } else {
                self.time >= length
            } {
                self.step = (self.step + 1) % CYCLE.len();
                self.time = 0.;
                self.held = 0.;
            } else if length > 0. && self.time >= length && mode != 12 {
                self.time -= length;
            }
            let row = &mut units.input[f::STRIDE..f::STRIDE + i::STRIDE];
            row[i::WALKER_MODE] = CYCLE[self.step].0 as f64;
            row[i::WALKER_TIME] = self.time;
            row[i::WALKER_ROLL] = self.roll;
        } else {
            self.wait -= dt;
            if self.fidget > 0. {
                self.fidget -= dt;
                if self.fidget <= 0. {
                    Self::command(units, kind::END_FIDGET);
                }
            }
            if self.wait <= 0. && self.fidget <= 0. {
                if let Some((clip, duration)) = self.emote {
                    Self::command(units, kind::FIDGET)[command::CLIP] = clip as f64;
                    self.fidget = duration;
                    self.wait = 3. + rng.uniform() * 5.;
                }
            }
        }
    }
}

impl Replicas {
    /// Page's ClassPreview uses the normal soldier pose and instance banks.
    /// Class changes select prepared rigs; they never borrow a live soldier.
    #[allow(clippy::too_many_arguments)]
    pub fn preview(
        &mut self,
        bridge: &mut Bridge,
        class: usize,
        parent: &[f64; 16],
        dt: f64,
        view: &Matrix4,
        projection: &Matrix4,
        theme: usize,
        build: f64,
    ) -> usize {
        let Some(model) = self.classes.get(class).copied().flatten() else {
            return 0;
        };
        let data = &mut self.models[model];
        let units = &mut data.preview;
        units.input[f::VERSION] = VERSION as f64;
        units.input[f::ACTORS] = 1.;
        units.input[f::FIRST_PERSON] = -1.;
        units.input[f::DT] = dt;
        let input = &mut units.input[f::STRIDE..f::STRIDE + i::STRIDE];
        input[i::ACTOR] = 0.;
        input[i::PRESENT] = 1.;
        input[i::OWN] = 1.;
        input[i::GENERATION] = 1.;
        input[i::PITCH] = (10f64 / 4000.).atan();
        input[i::ALPHA] = 1.;
        input[i::SOURCE_PROP] = -1.;
        input[i::SOURCE_RIG] = -1.;
        input[i::SOURCE_TURRET] = -1.;
        input[i::HELD_CLIP] = -1.;
        input[i::ROOT_QUATERNION + 3] = 1.;
        input[i::HAS_POSE_PARENT] = 1.;
        input[i::HAS_ATTACHMENT_PARENT] = 1.;
        input[i::POSE_PARENT..i::POSE_PARENT + 16].copy_from_slice(parent);
        input[i::ATTACHMENT_PARENT..i::ATTACHMENT_PARENT + 16].copy_from_slice(parent);
        data.preview_animation.update(
            units,
            dt,
            self.preview_banks.get(class).copied().unwrap_or(0),
            self.preview_class != class,
            &mut self.preview_rng,
        );
        self.preview_class = class;
        units.frame();
        units.places[0] = 0;
        units.place_actors(1);
        let out = &units.output[..o::STRIDE];
        // The interface model shader is the same source-owned shader used
        // by Page's browser stage. Only its matrices and skin texture change.
        unsafe {
            use glow::HasContext;
            let rows = (out[o::SKIN_COUNT] as usize * 4).div_ceil(1024).max(1);
            let skin = &units.skin[..rows * 1024 * 4];
            let bytes =
                std::slice::from_raw_parts(skin.as_ptr().cast::<u8>(), std::mem::size_of_val(skin));
            bridge
                .r
                .core
                .gl
                .bind_texture(glow::TEXTURE_2D, Some(self.preview_skin));
            bridge.r.core.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                1024,
                rows as i32,
                glow::RGBA,
                glow::FLOAT,
                glow::PixelUnpackData::Slice(Some(bytes)),
            );
        }
        let mut world = Matrix4::default();
        world
            .elements
            .copy_from_slice(&out[o::ROOT_WORLD..o::ROOT_WORLD + 16]);
        let inverse: [f64; 16] = out[o::BIND_INVERSE..o::BIND_INVERSE + 16]
            .try_into()
            .unwrap();
        let bind: [f64; 16] = data.row[draw::REC_BIND..draw::REC_BIND + 16]
            .try_into()
            .unwrap();
        let mut records = 0;
        let lighting = |part: &Appearance| {
            let mut look = part.look;
            look[UI::AMBIENT] = 0.62;
            look[UI::RIM_POWER] = 3.;
            look[UI::KEY_TINT..UI::KEY_TINT + 3].copy_from_slice(&if theme == 2 {
                [1., 246. / 255., 228. / 255.]
            } else if theme == 0 {
                [236. / 255., 246. / 255., 1.]
            } else {
                [250. / 255., 252. / 255., 1.]
            });
            look[UI::RIM..UI::RIM + 3].copy_from_slice(&if theme == 2 {
                [0.6, 150. / 255. * 0.6, 40. / 255. * 0.6]
            } else if theme == 0 {
                [110. / 255. * 0.7, 190. / 255. * 0.7, 0.7]
            } else {
                [200. / 255. * 0.55, 220. / 255. * 0.55, 0.55]
            });
            let age = (build / 0.65).clamp(0., 1.);
            look[UI::CUT] = if age >= 1. {
                1e9
            } else {
                -0.05 + (1. - (1. - age).powi(3)) * 2.3
            };
            look[UI::CUT_GLOW..UI::CUT_GLOW + 3].copy_from_slice(&if theme == 2 {
                [2., 160. / 255. * 2., 50. / 255. * 2.]
            } else {
                [90. / 255. * 2., 180. / 255. * 2., 2.]
            });
            look
        };
        for part in &data.preview_parts {
            part.push(
                bridge,
                &mut records,
                &world,
                view,
                projection,
                &bind,
                &inverse,
                &lighting(part),
            );
        }
        if let (Some(hand), Some(weapon)) = (
            data.hand,
            self.preview_weapons.get(class).copied().flatten(),
        ) {
            let at = (units.actor_node_start(0) + hand)
                * bfe_presentation::layout::unit_node::STRIDE
                + bfe_presentation::layout::unit_node::WORLD;
            let mut matrix = Matrix4::default();
            matrix.elements.copy_from_slice(&units.nodes[at..at + 16]);
            if let Some(parts) = self.preview_held.get(weapon) {
                for part in parts {
                    part.simple(
                        bridge,
                        &mut records,
                        &matrix,
                        view,
                        projection,
                        &lighting(part),
                    );
                }
            }
        }
        records
    }
}
