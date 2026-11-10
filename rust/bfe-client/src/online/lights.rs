//! The battle's point lights for a frame. The page keeps a pool of eight
//! lights (src/ordnance.js LightPool) that every passing glow asks for: a
//! bolt in flight, a beam, an explosion's flash. Each frame the heaviest
//! requests get the lights. A command post's hologram has a light of its own
//! that is always in the scene. The renderer then packs the scene's lights
//! for the camera (bfe_render::cameras `lights`): those with any colour,
//! within reach of the eye, at most sixteen. Selection and fading are the
//! shared owner's (bfe_presentation::lights); this is its page.
use bfe_presentation::{
    lights::Lights,
    page_layout::{light_command as c, light_frame as f, light_kind as kind, light_limits as limits, light_output as o},
};
use bfe_render::shader::MAX_LIGHTS;

// game.js: `new LightPool(scene, 8)`.
const POOL: usize = 8;
// Lights that stand in the scene on their own: one a hologram.
const FIXED: usize = super::holograms::HOLOGRAMS;
/// Floats of a packed light: position, range, colour times intensity, 0.
pub const LIGHT: usize = 8;

pub struct SceneLights {
    pool: Lights,
    commands: usize,
    serial: f64,
    fixed: [[f64; LIGHT]; FIXED],
    fixed_count: usize,
    /// The frame's lights as the shaders read them, and how many.
    pub packed: [f64; MAX_LIGHTS * LIGHT],
    pub count: usize,
}

impl SceneLights {
    pub fn new() -> Self {
        Self {
            pool: Lights::new(POOL),
            commands: 0,
            serial: 0.,
            fixed: [[0.; LIGHT]; FIXED],
            fixed_count: 0,
            packed: [0.; MAX_LIGHTS * LIGHT],
            count: 0,
        }
    }
    fn command(&mut self, kind: u32, position: [f64; 3], rgb: [f64; 3], radius: f64, scale: f64, duration: f64) {
        // The owner reads a bounded ring; a frame that asks for more than it
        // holds loses its last requests instead of an earlier frame's.
        if self.commands == limits::COMMANDS {
            return;
        }
        let row = &mut self.pool.commands[self.commands * c::STRIDE..][..c::STRIDE];
        row[c::KIND] = kind as f64;
        row[c::SERIAL] = self.serial;
        row[c::POSITION..c::POSITION + 3].copy_from_slice(&position);
        row[c::RGB..c::RGB + 3].copy_from_slice(&rgb);
        row[c::RADIUS] = radius;
        row[c::SCALE] = scale;
        row[c::DURATION] = duration;
        self.commands += 1;
    }
    /// LightPool.add: a light for this frame only. Colours are 0..1.
    pub fn add(&mut self, position: [f64; 3], rgb: [f64; 3], radius: f64, scale: f64) {
        self.command(kind::ADD, position, rgb, radius, scale, 0.);
    }
    /// LightPool.flash: a light that fades out over `duration` seconds.
    #[allow(dead_code)]
    pub fn flash(&mut self, position: [f64; 3], rgb: [f64; 3], radius: f64, duration: f64) {
        self.serial += 1.;
        self.command(kind::FLASH, position, rgb, radius, 1., duration);
    }
    /// A light that is in the scene whether or not it is lit: its colour
    /// times its intensity, and how far it reaches.
    pub fn fixed(&mut self, position: [f64; 3], color: [f64; 3], range: f64) {
        if self.fixed_count < FIXED {
            self.fixed[self.fixed_count] = [position[0], position[1], position[2], range, color[0], color[1], color[2], 0.];
            self.fixed_count += 1;
        }
    }
    /// LightPool.update, then the camera's pack: the pool's lights first (the
    /// page adds them to the scene before it builds the posts), then the
    /// holograms'. A light with no colour, or further from the eye than the
    /// far plane plus its own reach, is left out.
    pub fn frame(&mut self, dt: f64, eye: [f64; 3], far: f64) {
        self.pool.input[f::DT] = dt;
        self.pool.input[f::COMMANDS] = self.commands as f64;
        self.pool.input[f::START] = 0.;
        self.pool.frame();
        self.commands = 0;
        self.count = 0;
        for i in 0..POOL + self.fixed_count {
            let light = if i < POOL {
                let out = &self.pool.output[i * o::STRIDE..][..o::STRIDE];
                let scale = out[o::SCALE];
                [
                    out[o::POSITION],
                    out[o::POSITION + 1],
                    out[o::POSITION + 2],
                    out[o::RADIUS],
                    out[o::RGB] * scale,
                    out[o::RGB + 1] * scale,
                    out[o::RGB + 2] * scale,
                    0.,
                ]
            } else {
                self.fixed[i - POOL]
            };
            if !(light[4] > 0. || light[5] > 0. || light[6] > 0.) {
                continue;
            }
            let away = ((light[0] - eye[0]).powi(2) + (light[1] - eye[1]).powi(2) + (light[2] - eye[2]).powi(2)).sqrt();
            if light[3] > 0. && away > far + light[3] {
                continue;
            }
            self.packed[self.count * LIGHT..][..LIGHT].copy_from_slice(&light);
            self.count += 1;
            if self.count == MAX_LIGHTS {
                break;
            }
        }
        self.fixed_count = 0;
    }
}
