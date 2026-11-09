//! Command-post remaining time and Hologram.update, including procedural art.
//! Capture still belongs to the simulation. These rules only decide what the
//! drawn logo, beam, flare and light show from published post facts.
//!
//! Command posts, following the 2004 game.
//!
//! Soldiers inside a post's capture region count for their team's capture
//! team (src/registry.js captureTeam: an army for itself, a local team for the
//! one army it is friendly with, for nobody when it is friendly with both, for
//! itself when with neither). N soldiers of a team capture at 1 + 1/2 + ...
//! (N terms) times the base rate. An owned post that other teams stand in is
//! neutralized over NeutralizeTime at the sum of their rates (defenders don't
//! slow it); when they leave, that progress is lost at once. A neutral post
//! fills toward one team over CaptureTime; another team must first drain it;
//! two teams present freezes it; an empty post keeps its progress. A post
//! without a capture region (Tatooine's Tusken campfires, Hoth's probe droid
//! drop) never changes hands. The hologram shows the owner's logo, coloured as
//! the viewer's team sees the owner, and goes dark while a change is about to
//! land; a post whose class has no HoloOdf has none, and one without a
//! MapTexture (or a MapScale of 0) isn't drawn on the maps. A command vehicle
//! (the AT-AT, the AT-TE) is a post too while it lives (src/mobile-posts.js):
//! all() and owned() count it after the placed posts; it has no capture
//! region or hologram, and the maps draw it as the vehicle it is.
//!
//! A destructible post (a class with MaxHealth: Hoth's shield generator,
//! Geonosis' Techno Union ships and forward command center, Endor's bunker
//! control panel) is also a building (src/props.js ObjectivePost). It has
//! no capture region; when the building is destroyed the post goes inactive
//! for the rest of the battle: no spawns, no bleed
//! value, no AI objective, off the maps, its hologram dark, and it is
//!
//! Post.remaining observes published capture counts; it does not capture or
//! neutralize a command post. Owned posts sum every opposing team's rate.
//! A neutral post can finish only with one present team and compatible progress.
//! Defenders do not subtract from neutralization, and an incompatible capture
//! team cannot predict a landing while another team's progress remains.
//!
//! Hologram.update keeps fade and flicker independently for each stable post id.
//! A changed owner resets fade before the first approach to the new target.
//! An inactive post hides its scene group without advancing retained flicker.
//! Approaching a capture or neutralization landing dims the logo in advance.
//! The frame receives dt separately from the absolute rotation/scroll clock.
//!
//! One random test decides whether the frame changes flicker. A successful test
//! consumes one additional value for alpha. The supplied cosmetic source must
//! therefore be called conditionally, in post order, rather than prefilled with
//! a fixed random tape that would change subsequent presentation seeds.
//!
//! Colour is the viewer's published relation colour, normalized before output.
//! Distance reach only affects the logo, beam, flare and emission halo. The
//! point light retains its original owner/fade response independently of reach.
//! Turning bloom off hides the halo resource but does not alter fade history.
//! The logo texture's scrolling phase follows the original command-post clock.
//!
//! Procedural art is admitted once. Halo texels sample pixel centers and round
//! only when assigning bytes. The beam gradient preserves its vertical direction
//! and the texture host retains flipY. Beam positions pass through float32 before
//! the authored translation, and its rounded normals are normalized once more,
//! matching the original BufferGeometry transform instead of combining the two
//! operations in higher precision. Hosts bind these arrays without geometry math.

use crate::{
    unit_effects::vector,
    visual_effects_layout::{
        post_frame as f, post_state as s, post_team as t, visual_limits as limits,
    },
};
use serde::Deserialize;
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Look {
    pub height: f64,
    pub fade_in_time: f64,
    pub flicker_rate: f64,
    pub flicker_alpha: [f64; 2],
    pub visible_distance: f64,
    pub rotate_rate: f64,
    pub image_intensity: f64,
    pub beam_intensity: f64,
    pub flare_intensity: Option<f64>,
    pub light_intensity: f64,
}
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Descriptor {
    pub look: Look,
    pub posts: Vec<serde_json::Value>,
}
pub struct Posts {
    pub input: Vec<f64>,
    pub counts: Vec<f64>,
    pub art: Vec<u8>,
    pub beam_positions: Vec<f32>,
    pub beam_normals: Vec<f32>,
    pub beam_uv: Vec<f32>,
    pub beam_indices: Vec<u16>,
    look: Look,
    shown: Vec<Option<f64>>,
}
impl Posts {
    pub fn new(descriptor: Descriptor) -> Self {
        let look = descriptor.look;
        let mut input = vec![0.; limits::POSTS * f::STRIDE];
        for row in input.as_chunks_mut::<{ f::STRIDE }>().0 {
            row[f::FLICKER] = 1.;
            row[f::HALO_SCALE..f::HALO_SCALE + 3].copy_from_slice(&[
                2.6,
                (look.height * 1.4).max(2.6),
                1.,
            ]);
            row[f::HALO_HEIGHT] = look.height * 0.45;
            row[f::BEAM] = 0.35 * look.beam_intensity;
            row[f::FLARE] = 0.6 * look.flare_intensity.unwrap_or(1.);
        }
        for (k, post) in descriptor.posts.iter().take(limits::POSTS).enumerate() {
            let texture = post.get("mapTexture");
            let present = texture.is_some();
            let has = texture.is_some_and(|t| t.as_str().is_some_and(|s| !s.is_empty()));
            let scale = post.get("mapScale").and_then(|s| s.as_f64()).unwrap_or(1.);
            input[k * f::STRIDE + f::MAPPED] = (!present || (has && scale > 0.)) as u8 as f64;
        }
        // Hologram's open CylinderGeometry and translate retain both original
        // float32 boundaries. Translation also renormalizes rounded normals.
        let mut beam_positions = vec![0.; 34 * 3];
        let mut beam_normals = vec![0.; 34 * 3];
        let mut beam_uv = vec![0.; 34 * 2];
        let mut beam_indices = vec![0; 16 * 6];
        let mut translation = three::Matrix4::default();
        translation.make_translation(0., look.height / 2., 0.);
        for y in 0..2 {
            let v = y as f64;
            let radius = v * (0.05 - 0.35) + 0.35;
            for x in 0..17 {
                let u = x as f64 / 16.;
                let theta = u * (std::f64::consts::PI * 2.) + 0.;
                let sin = v8math::sin(theta);
                let cos = v8math::cos(theta);
                let mut point = three::Vector3::new(
                    (radius * sin) as f32 as f64,
                    (-v * look.height + look.height / 2.) as f32 as f64,
                    (radius * cos) as f32 as f64,
                );
                point.apply_matrix4(&translation);
                let mut normal = three::Vector3::new(sin, (0.05 - 0.35) / look.height, cos);
                normal.normalize();
                normal
                    .set(
                        normal.x as f32 as f64,
                        normal.y as f32 as f64,
                        normal.z as f32 as f64,
                    )
                    .apply_normal_matrix(&three::Matrix3::default());
                let index = y * 17 + x;
                beam_positions[index * 3..index * 3 + 3].copy_from_slice(&[
                    point.x as f32,
                    point.y as f32,
                    point.z as f32,
                ]);
                beam_normals[index * 3..index * 3 + 3].copy_from_slice(&[
                    normal.x as f32,
                    normal.y as f32,
                    normal.z as f32,
                ]);
                beam_uv[index * 2..index * 2 + 2].copy_from_slice(&[u as f32, (1. - v) as f32]);
            }
        }
        for x in 0..16 {
            beam_indices[x * 6..x * 6 + 6].copy_from_slice(&[
                x as u16,
                (x + 17) as u16,
                (x + 1) as u16,
                (x + 17) as u16,
                (x + 18) as u16,
                (x + 1) as u16,
            ]);
        }
        let mut art = vec![0; 128 * 128 * 4 + 4 * 64 * 4];
        // haloTexture and gradientTexture preserve byte rounding at texel centers.
        for y in 0..128 {
            for x in 0..128 {
                let radius = v8math::hypot(
                    (x as f64 + 0.5) / 128. * 2. - 1.,
                    (y as f64 + 0.5) / 128. * 2. - 1.,
                );
                let alpha = (v8math::exp(-radius * radius * 5.) - v8math::exp(-5.)).max(0.);
                let at = (y * 128 + x) * 4;
                art[at..at + 3].fill(255);
                art[at + 3] = v8math::round(alpha * 255.) as u8;
            }
        }
        for y in 0..64 {
            for x in 0..4 {
                let at = 128 * 128 * 4 + (y * 4 + x) * 4;
                art[at..at + 3].fill(255);
                art[at + 3] = v8math::round((y as f64 + 0.5) / 64. * 255.) as u8;
            }
        }
        Self {
            input,
            counts: vec![0.; limits::POSTS * s::STRIDE],
            art,
            beam_positions,
            beam_normals,
            beam_uv,
            beam_indices,
            look,
            shown: vec![None; limits::POSTS],
        }
    }
    // Post.remaining: counts are pairs of team id and population, bounded at
    // the input. An excess team cannot become a false single-team capture.
    pub fn remaining(&mut self, count: usize) {
        for index in 0..count.min(limits::POSTS) {
            let r = &self.counts[index * s::STRIDE..][..s::STRIDE];
            let count = (r[s::COUNT] as usize).min(256);
            let owner = r[s::OWNER];
            let neutralize = r[s::NEUTRALIZE];
            let progress = r[s::PROGRESS];
            let toward = r[s::TOWARD];
            let neutralize_time = r[s::NEUTRALIZE_TIME];
            let capture_time = r[s::CAPTURE_TIME];
            let mut attack = 0.;
            let mut teams = 0;
            let mut team = 0.;
            let mut number = 0.;
            for k in 0..count {
                let t = r[s::TEAMS + k * t::STRIDE + t::TEAM];
                let n = r[s::TEAMS + k * t::STRIDE + t::POPULATION];
                if n > 0. {
                    teams += 1;
                    team = t;
                    number = n;
                    if t != owner {
                        attack += 2. - v8math::pow_legacy(2., 1. - n);
                    }
                }
            }
            self.input[index * f::STRIDE + f::REMAINING] = if owner != 0. {
                if attack != 0. {
                    ((1. - neutralize) * neutralize_time) / attack
                } else {
                    f64::INFINITY
                }
            } else if teams != 1 || (team != toward && progress > 0.) {
                f64::INFINITY
            } else {
                ((1. - progress) * capture_time) / (2. - v8math::pow_legacy(2., 1. - number))
            };
        }
    }
    pub fn frame(&mut self, count: usize, random: &mut impl FnMut() -> f64) {
        self.remaining(count);
        self.input[f::LOGO_OFFSET] = (self.input[f::TIME] * 0.25) % 1.;
        for index in 0..count.min(limits::POSTS) {
            let x = &mut self.input[index * f::STRIDE..][..f::STRIDE];
            if x[f::ACTIVE] == 0. {
                continue;
            }
            let look = &self.look;
            let owner = x[f::OWNER];
            if self.shown[index] != Some(owner) {
                self.shown[index] = Some(owner);
                x[f::FADE] = 0.;
            }
            let target = if x[f::REMAINING] <= x[f::TURN_ON_TIME] {
                0.
            } else {
                1.
            };
            let delta = target - x[f::FADE];
            let sign = if delta == 0. {
                0.
            } else if delta < 0. {
                -1.
            } else {
                1.
            };
            x[f::FADE] += sign * delta.abs().min(x[f::DT] / look.fade_in_time);
            if random() < look.flicker_rate * 60. * x[f::DT] {
                x[f::FLICKER] = look.flicker_alpha[0]
                    + random() * (look.flicker_alpha[1] - look.flicker_alpha[0]);
            }
            for k in 0..3 {
                x[f::COLOR + k] /= 255.;
            }
            let distance = vector(x, f::CAMERA).distance_to(&vector(x, f::POSITION));
            let reach =
                ((look.visible_distance - distance) / (look.visible_distance * 0.1)).clamp(0., 1.);
            x[f::REACH] = reach;
            x[f::ROTATION] = x[f::TIME] * look.rotate_rate * std::f64::consts::PI * 2.;
            x[f::IMAGE] = look.image_intensity * x[f::FLICKER] * x[f::FADE] * reach;
            let emission = x[f::FADE] * x[f::FLICKER] * reach;
            x[f::HALO] = 0.48 * emission;
            x[f::BEAM] = 0.35 * look.beam_intensity * emission;
            x[f::FLARE] = 0.6 * look.flare_intensity.unwrap_or(1.) * emission;
            x[f::LIGHT] = look.light_intensity
                * if owner != 0. {
                    0.4 + 0.6 * x[f::FADE]
                } else {
                    1.
                };
            x[f::LOGO_OFFSET] = (x[f::TIME] * 0.25) % 1.;
        }
    }
}
