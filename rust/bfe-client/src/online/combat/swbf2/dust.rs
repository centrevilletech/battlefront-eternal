//! Camera-relative dust streaks from the world's SpaceDust effect.
use super::{authored_json, BattleContent, Matrix4, Vector3};
use crate::render_bridge::Bridge;
use bfe_render::effects as draw;
use jsrt::Mulberry32;
use serde::Deserialize;

#[derive(Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
struct Settings {
    spawn_distance: f64,
    max_random_side_offset: f64,
    center_dead_zone_radius: f64,
    min_particle_scale: f64,
    max_particle_scale: f64,
    spawn_delay: f64,
    reference_speed: f64,
    dust_particle_speed: f64,
    speed_particle_min_length: f64,
    speed_particle_max_length: f64,
    particle_length_min_speed: f64,
    particle_length_max_speed: f64,
}
struct Mote {
    position: Vector3,
    axis: Vector3,
    width: f64,
    left: f64,
}
struct Field {
    settings: Settings,
    motes: Vec<Mote>,
    previous: Option<Vector3>,
    fraction: f64,
    random: Mulberry32,
}
impl Field {
    fn new(settings: Settings) -> Option<Self> {
        if settings.spawn_distance <= 0.
            || settings.spawn_delay <= 0.
            || settings.reference_speed <= 0.
        {
            return None;
        }
        // Crossing the full field at reference speed bounds the number born before the oldest exit.
        let capacity = (2. * settings.spawn_distance
            / (settings.reference_speed * settings.spawn_delay))
            .ceil()
            .clamp(1., 1024.) as usize;
        Some(Self {
            settings,
            motes: Vec::with_capacity(capacity),
            previous: None,
            fraction: 0.,
            random: Mulberry32::from_seed_u32(1),
        })
    }
    fn reset(&mut self) {
        self.motes.clear();
        self.previous = None;
        self.fraction = 0.;
        self.random = Mulberry32::from_seed_u32(1);
    }
    fn advance(&mut self, eye: Vector3, dt: f64) -> f64 {
        let Some(previous) = self.previous.replace(eye) else {
            return 0.;
        };
        if dt <= 0. || !dt.is_finite() {
            return 0.;
        }
        let s = &self.settings;
        let mut direction = eye;
        direction.sub(&previous);
        let distance = direction.length();
        if distance > s.spawn_distance {
            self.motes.clear();
            self.fraction = 0.;
            return 0.;
        }
        let drift = s.dust_particle_speed.max(0.);
        self.motes.retain_mut(|p| {
            p.left -= dt;
            p.position.add_scaled_vector(&p.axis, drift * dt);
            p.left > 0.
        });
        if distance == 0. {
            return 0.;
        }
        direction.multiply_scalar(1. / distance);
        let speed = distance / dt;
        let interval = s.spawn_delay * s.reference_speed / speed;
        let due = self.fraction + dt / interval;
        self.fraction = due.fract();
        let births = (due.floor() as usize).min(self.motes.capacity() - self.motes.len());
        let mut right = Vector3::default();
        let reference = if direction.y.abs() < 0.99 {
            Vector3::new(0., 1., 0.)
        } else {
            Vector3::new(1., 0., 0.)
        };
        right.cross_vectors(&direction, &reference).normalize();
        let mut up = Vector3::default();
        up.cross_vectors(&right, &direction);
        let outer = s.max_random_side_offset.max(0.);
        let inner = s.center_dead_zone_radius.clamp(0., outer);
        let mut axis = direction;
        axis.multiply_scalar(-1.);
        for i in (0..births).rev() {
            // Backdate each birth within this draw, including the movement of its spawning plane.
            let age = (self.fraction + i as f64) * interval;
            let angle = self.random.next_f64() * std::f64::consts::TAU;
            let radius =
                (inner * inner + self.random.next_f64() * (outer * outer - inner * inner)).sqrt();
            let width = s.min_particle_scale
                + (s.max_particle_scale - s.min_particle_scale) * self.random.next_f64();
            let left = 2. * s.spawn_distance / (speed + drift) - age;
            if left > 0. {
                let mut position = eye;
                position
                    .add_scaled_vector(&direction, s.spawn_distance - (speed + drift) * age)
                    .add_scaled_vector(&right, radius * angle.cos())
                    .add_scaled_vector(&up, radius * angle.sin());
                self.motes.push(Mote {
                    position,
                    axis,
                    width,
                    left,
                });
            }
        }
        speed
    }
    fn length(&self, speed: f64) -> f64 {
        let s = &self.settings;
        let span = s.particle_length_max_speed - s.particle_length_min_speed;
        let t = if span > 0. {
            ((speed - s.particle_length_min_speed) / span).clamp(0., 1.)
        } else {
            0.
        };
        s.speed_particle_min_length
            + t * (s.speed_particle_max_length - s.speed_particle_min_length)
    }
}

pub(super) struct SpaceDust {
    field: Field,
    batch: u32,
    data: usize,
}
impl SpaceDust {
    pub fn new(
        battle: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        at: &mut usize,
    ) -> Result<Option<Self>, String> {
        let Some(image) = battle.map["spaceDustImage"].as_u64() else {
            return Ok(None);
        };
        let raw = battle
            .visual_effects()
            .get("spaceDust")
            .ok_or("missing space dust settings")?;
        let settings = match serde_json::from_value(authored_json(raw)) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!("Skipping mission space dust: {e}");
                return Ok(None);
            }
        };
        let Some(field) = Field::new(settings) else {
            return Ok(None);
        };
        let capacity = field.motes.capacity();
        let texture = *textures
            .get(image as usize)
            .ok_or("invalid space dust image")?;
        let batch = bridge.effect_batch(
            bfe_render::effect_layout::CPU as u32,
            1,
            false,
            texture as i32,
            capacity as u32,
            -1,
            &[],
        )?;
        bridge.fx_batch_interleaved(batch);
        let data = *at;
        *at += capacity * 21;
        bridge.r.effects.reserve(*at);
        Ok(Some(Self { field, batch, data }))
    }
    pub fn reset(&mut self) {
        self.field.reset();
    }
    pub fn frame(&mut self, bridge: &mut Bridge, camera: &[f64], dt: f64) {
        let Some(view) = camera.get(16..32) else {
            return;
        };
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(view);
        world.invert();
        let m = &world.elements;
        let speed = self.field.advance(Vector3::new(m[12], m[13], m[14]), dt);
        let length = self.field.length(speed) as f32;
        let draws = bridge.r.effects.input[draw::IN_DRAWS] as usize;
        let count = self.field.motes.len();
        if count == 0 || draws >= draw::MAX_DRAWS {
            return;
        }
        let data = &mut bridge.r.effects.data[self.data..self.data + count * 21];
        for (out, p) in data.chunks_exact_mut(21).zip(&self.field.motes) {
            out.copy_from_slice(&[
                p.position.x as f32,
                p.position.y as f32,
                p.position.z as f32,
                1.,
                1.,
                1.,
                1.,
                p.width as f32,
                0.,
                length,
                1.,
                p.axis.x as f32,
                p.axis.y as f32,
                p.axis.z as f32,
                1.,
                0.,
                0.,
                0.,
                0.,
                1.,
                1.,
            ]);
        }
        let row = &mut bridge.r.effects.input[draw::DRAW_AT + draws * draw::DRAW_STRIDE..]
            [..draw::DRAW_STRIDE];
        row.fill(0.);
        row[..6].copy_from_slice(&[
            draw::KIND_BATCH as f64,
            self.batch as f64,
            count as f64,
            0.,
            self.data as f64,
            count as f64,
        ]);
        row[draw::DRAW_KEY + 1] = 10.;
        row[draw::DRAW_KEY + 5] = self.batch as f64;
        bridge.r.effects.input[draw::IN_DRAWS] = (draws + 1) as f64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn travel_at_different_display_rates_gives_the_same_dust_before_a_camera_cut() {
        let sample = |frames| {
            let mut field = Field::new(Settings {
                spawn_distance: 95.,
                spawn_delay: 0.125,
                reference_speed: 8.,
                dust_particle_speed: 3.,
                max_random_side_offset: 12.,
                center_dead_zone_radius: 4.,
                ..Default::default()
            })
            .unwrap();
            for i in 0..=frames {
                field.advance(
                    Vector3::new(0., 0., i as f64 * 16. / frames as f64),
                    1. / frames as f64,
                );
            }
            let mut depths: Vec<_> = field
                .motes
                .iter()
                .map(|p| [p.position.x, p.position.y, p.position.z])
                .collect();
            depths.sort_by(|a, b| a[2].total_cmp(&b[2]));
            field.advance(Vector3::new(0., 0., 240.), 1.);
            assert!(field.motes.is_empty());
            depths
        };
        let (a, b) = (sample(8), sample(128));
        assert_eq!(a.len(), 16);
        assert_eq!(a.len(), b.len());
        assert!(a
            .iter()
            .flatten()
            .zip(b.iter().flatten())
            .all(|(a, b)| (a - b).abs() < 1e-9));
    }
}
