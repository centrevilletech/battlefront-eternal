//! Placed light beams use the existing additive vertex-colored mesh program.
use super::{authored_json, BattleContent, Matrix4, Vector3};
use crate::render_bridge::Bridge;
use bfe_render::effects as draw;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Shape {
    length: f64,
    start: [f64; 2],
    end: [f64; 2],
    color: [f64; 4],
    beam: f64,
    flare: f64,
    fade: f64,
    fade_length: f64,
    distance: f64,
}
struct Beam {
    shape: Shape,
    origin: Vector3,
    x: Vector3,
    y: Vector3,
    z: Vector3,
}
const VERTICES: usize = 13;
const INDICES: [u32; 30] = [
    0, 3, 1, 1, 3, 4, 1, 4, 2, 2, 4, 5, 3, 6, 4, 4, 6, 7, 4, 7, 5, 5, 7, 8, 9, 10, 11, 11, 10, 12,
];

pub(super) struct Beams {
    placed: Vec<Beam>,
    mesh: u32,
    data: usize,
}
impl Beams {
    pub(super) fn new(
        battle: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
        at: &mut usize,
    ) -> Result<Option<Self>, String> {
        let Some(image) = battle.map["lightBeamImage"].as_u64() else {
            return Ok(None);
        };
        let table = battle
            .visual_effects()
            .get("beams")
            .ok_or("missing beam definitions")?;
        let mut shapes = BTreeMap::new();
        for (name, value) in table.as_object().into_iter().flat_map(|o| o.entries()) {
            match serde_json::from_value::<Vec<Shape>>(authored_json(value)) {
                Ok(group) => {
                    shapes.insert(name, group);
                }
                Err(e) => eprintln!("Skipping mission beam {name}: {e}"),
            }
        }
        let mut placed = Vec::new();
        for instance in &battle.environment_instances {
            let Some(shapes) = shapes.get(instance.effect.as_str()) else {
                continue;
            };
            let m = &instance.matrix;
            let axis = |i| Vector3::new(m[i], m[i + 1], m[i + 2]);
            for shape in shapes {
                placed.push(Beam {
                    shape: shape.clone(),
                    origin: axis(12),
                    x: axis(0),
                    y: axis(4),
                    z: axis(8),
                });
            }
        }
        if placed.is_empty() {
            return Ok(None);
        }
        let active = placed.len();
        let vertices = active
            .checked_mul(VERTICES)
            .filter(|&v| v <= u16::MAX as usize)
            .ok_or("too many placed beam vertices")?;
        let indices = active * INDICES.len();
        let texture = *textures.get(image as usize).ok_or("invalid beam image")?;
        let mesh = bridge.effect_mesh(
            bfe_render::effect_layout::LIGHTNING as u32,
            bridge.shadow_cascades() > 0,
            texture as i32,
            vertices as u32,
            indices as u32,
            &[1., 0., 0., 0., 1., 0., 0., 0., 1.],
        )?;
        let data = *at;
        *at += vertices * 9 + indices;
        bridge.r.effects.reserve(*at);
        Ok(Some(Self { placed, mesh, data }))
    }
    pub(super) fn draw(&self, bridge: &mut Bridge, camera: &[f64]) {
        if camera.len() < 32 {
            return;
        }
        let mut world = Matrix4::default();
        world.elements.copy_from_slice(&camera[16..32]);
        world.invert();
        let m = &world.elements;
        let axis = |i| Vector3::new(m[i], m[i + 1], m[i + 2]);
        let (right, up, eye) = (axis(0), axis(4), axis(12));
        let visible = |beam: &&Beam| {
            beam.shape.distance <= 0.
                || beam.origin.distance_to_squared(&eye) <= beam.shape.distance.powi(2)
        };
        let count = self.placed.iter().filter(visible).count();
        let draws = bridge.r.effects.input[draw::IN_DRAWS] as usize;
        if count == 0 || draws >= draw::MAX_DRAWS {
            return;
        }
        let vertices = count * VERTICES;
        let indices = count * INDICES.len();
        let data = &mut bridge.r.effects.data[self.data..self.data + vertices * 9 + indices];
        let (positions, data) = data.split_at_mut(vertices * 3);
        let (uv, data) = data.split_at_mut(vertices * 2);
        let (colors, indices_out) = data.split_at_mut(vertices * 4);
        for (i, beam) in self.placed.iter().filter(visible).enumerate() {
            let s = &beam.shape;
            let mut towards = eye;
            towards.sub(&beam.origin);
            let mut cross = Vector3::default();
            cross.cross_vectors(&beam.y, &towards).normalize();
            if cross.length_sq() == 0. {
                cross = right;
            }
            let width =
                |w: [f64; 2]| 0.5 * (cross.dot(&beam.x) * w[0]).hypot(cross.dot(&beam.z) * w[1]);
            let w0 = width(s.start);
            let w1 = width(s.end);
            let mut vertex = |v: usize, p: Vector3, tex: [f32; 2], alpha: f64| {
                let at = i * VERTICES + v;
                positions[at * 3..at * 3 + 3]
                    .copy_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
                uv[at * 2..at * 2 + 2].copy_from_slice(&tex);
                let rgba = &mut colors[at * 4..at * 4 + 4];
                for j in 0..3 {
                    rgba[j] = (s.color[j] / 255.) as f32;
                }
                rgba[3] = (s.color[3] / 255. * alpha) as f32;
            };
            // The side facing the camera subtends the projected width of the elliptical cross section.
            for (row, (t, intensity)) in [
                (0., s.beam),
                (s.fade_length.clamp(0., 1.), s.beam * s.fade),
                (1., 0.),
            ]
            .into_iter()
            .enumerate()
            {
                let mut center = beam.origin;
                center.add_scaled_vector(&beam.y, t * s.length);
                for col in 0..3 {
                    let mut p = center;
                    p.add_scaled_vector(&cross, (col as f64 - 1.) * (w0 + (w1 - w0) * t));
                    vertex(
                        row * 3 + col,
                        p,
                        [0.5; 2],
                        if col == 1 { intensity } else { 0. },
                    );
                }
            }
            for (corner, (x, y)) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)]
                .into_iter()
                .enumerate()
            {
                let mut p = beam.origin;
                p.add_scaled_vector(&right, x * s.start[0] * 0.5)
                    .add_scaled_vector(&up, y * s.start[1] * 0.5);
                vertex(
                    9 + corner,
                    p,
                    [(x as f32 + 1.) * 0.5, (y as f32 + 1.) * 0.5],
                    s.flare,
                );
            }
            for (dest, index) in indices_out[i * INDICES.len()..].iter_mut().zip(INDICES) {
                *dest = f32::from_bits((i * VERTICES) as u32 + index);
            }
        }
        let row = &mut bridge.r.effects.input[draw::DRAW_AT + draws * draw::DRAW_STRIDE..]
            [..draw::DRAW_STRIDE];
        row.fill(0.);
        row[..6].copy_from_slice(&[
            draw::KIND_MESH as f64,
            self.mesh as f64,
            vertices as f64,
            indices as f64,
            self.data as f64,
            0.,
        ]);
        row[draw::DRAW_KEY + 1] = 5.;
        row[draw::DRAW_KEY + 5] = self.mesh as f64;
        bridge.r.effects.input[draw::IN_DRAWS] = (draws + 1) as f64;
    }
}
