//! Camera-facing blade strips and a tick-sampled ribbon at each authored emitter.
use super::{assets, emit, material_row, Bridge, Row};
use bfe_content_native::{
    geometry::{Geometry, Group},
    loader::{Blend, Material, MaterialKind},
};
use bfe_render::instances as draw;
use glow::HasContext;
use three::{Matrix4, Vector3};

// One ribbon per emitter. The pool is shared by all visible characters.
const LIFETIME: f64 = 36.;
const SEGMENTS: usize = 37;
const HISTORY: usize = 48;
const WIDTH: usize = 32;
const HEIGHT: usize = 64;
pub(super) const MAX_RIBBONS: usize = 64;
pub(super) const EDGE_PARTS: usize = 2;
const PROVISIONAL_IGNITE: f64 = 0.16;
const PROVISIONAL_RETRACT: f64 = 0.12;
pub(super) struct Edge {
    socket: Matrix4,
    width: f64,
    length: f64,
    blade: Row,
    trail: Option<u32>,
}
fn mesh(vertices: &[f32], indices: &[u32]) -> Result<Geometry, String> {
    let mut g = Geometry::model(
        &serde_json::json!({"vertexCount":vertices.len()/8,"vertexOffset":0,"indexOffset":0,"indexCount":indices.len()}),
        crate::session::bytes(vertices),
        crate::session::bytes(indices),
    )?;
    g.groups = vec![Group {
        first: 0,
        count: indices.len(),
        material: 0,
    }];
    Ok(g)
}
fn blade_mesh() -> Result<Geometry, String> {
    // The glow is symmetric across v; the narrow tip is at u=0.
    mesh(
        &[
            -0.5, 0., 0., 0., 0., 1., 1., 0., 0.5, 0., 0., 0., 0., 1., 1., 1., -0.5, 1., 0., 0.,
            0., 1., 0., 0., 0.5, 1., 0., 0., 0., 1., 0., 1.,
        ],
        &[0, 1, 2, 1, 3, 2],
    )
}
fn texture(bridge: &mut Bridge, width: i32, height: i32, pixels: &[u8]) -> Result<u32, String> {
    unsafe {
        let gl = &bridge.r.core.gl;
        let native = bridge.r.core.hold_texture(gl.create_texture()?);
        gl.bind_texture(glow::TEXTURE_2D, Some(native));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(Some(pixels)),
        );
        for p in [glow::TEXTURE_MIN_FILTER, glow::TEXTURE_MAG_FILTER] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, glow::LINEAR as i32);
        }
        for p in [glow::TEXTURE_WRAP_S, glow::TEXTURE_WRAP_T] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, glow::CLAMP_TO_EDGE as i32);
        }
        Ok(bridge.r.core.add_texture(native))
    }
}
impl Edge {
    pub fn new(
        data: assets::Edge,
        bridge: &mut Bridge,
        textures: &[u32],
        pool: &mut Pool,
    ) -> Result<Self, String> {
        let diffuse = match data.texture {
            Some(i) => textures[i],
            None => texture(bridge, 1, 1, &data.color)?,
        };
        let material = Material {
            extended: None,
            kind: MaterialKind::Unlit,
            blend: Blend::Additive,
            cutout: false,
            double_sided: true,
            flags: 0,
            far: false,
            texture: Some(0),
            bump: None,
            record: serde_json::json!({"glow":true}),
        };
        let (mut blade, key) = material_row(bridge, &[diffuse], material, false)?;
        blade[draw::REC_FLAGS] = (2 | draw::NO_SHADOW) as f64;
        blade[draw::REC_GEOMETRY] = f64::from(blade_mesh()?.register(&mut bridge.r, &key)?);
        let trail = if let Some(color) = data.trail {
            pool.reserve(bridge, &key)?;
            let mut pixels = [0; WIDTH * HEIGHT * 4];
            for (i, rgba) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let radius = (i % WIDTH) as f64 / (WIDTH - 1) as f64;
                let age = (i / WIDTH) as f64 / (HEIGHT - 1) as f64;
                let edge = (radius * 12.).min(1.) * ((1. - radius) * 14.).min(1.);
                rgba.copy_from_slice(&color);
                rgba[3] = (f64::from(color[3])
                    * (1. - age).powi(4)
                    * edge
                    * (0.15 + 0.85 * radius.sqrt()))
                .round() as u8;
            }
            Some(texture(bridge, WIDTH as i32, HEIGHT as i32, &pixels)?)
        } else {
            None
        };
        Ok(Self {
            socket: data.socket,
            width: data.width,
            length: data.length,
            blade,
            trail,
        })
    }
    pub fn socket(&self) -> &Matrix4 {
        &self.socket
    }
    fn ends(&self, hand: &Matrix4, length: f64) -> [Vector3; 2] {
        let mut m = *hand;
        m.multiply(&self.socket);
        let e = m.elements;
        let base = Vector3::new(e[12], e[13], e[14]);
        let mut dir = Vector3::new(e[8], e[9], e[10]);
        dir.normalize()
            .multiply_scalar(self.length * length)
            .add(&base);
        [base, dir]
    }
}
#[derive(Clone, Copy, Default)]
struct Point {
    ends: [Vector3; 2],
    active: bool,
    tick: u32,
    on: bool,
    length: f64,
}
impl Point {
    fn length_at(self, tick: f64) -> f64 {
        let seconds = (tick - f64::from(self.tick)).max(0.) / 60.;
        if self.on {
            (self.length + seconds / PROVISIONAL_IGNITE).min(1.)
        } else {
            (self.length - seconds / PROVISIONAL_RETRACT).max(0.)
        }
    }
}
#[derive(Clone, Copy)]
pub(super) struct History {
    points: [Point; HISTORY],
    next: usize,
    count: usize,
    last_attack: Option<u32>,
}
impl Default for History {
    fn default() -> Self {
        Self {
            points: [Point::default(); HISTORY],
            next: 0,
            count: 0,
            last_attack: None,
        }
    }
}
impl History {
    fn newest_first(&self) -> impl Iterator<Item = &Point> {
        (0..self.count).map(|n| &self.points[(self.next + HISTORY - n - 1) % HISTORY])
    }
    pub fn budget(&self, edge: &Edge, tick: f64) -> (usize, usize) {
        let mut past = self.newest_first().skip_while(|p| f64::from(p.tick) > tick);
        let Some(current) = past.next().or_else(|| self.newest_first().last()) else {
            return (0, 0);
        };
        let blade = usize::from(current.length_at(tick) > 0.);
        let mut newer = current;
        if edge.trail.is_some() && self.recent_attack(tick) {
            if current.active && f64::from(current.tick) < tick {
                return (blade, 1);
            }
            for p in past {
                if tick - f64::from(newer.tick) >= LIFETIME {
                    break;
                }
                if p.active && newer.active {
                    return (blade, 1);
                }
                newer = p;
            }
        }
        (blade, 0)
    }
    fn recent_attack(&self, tick: f64) -> bool {
        self.last_attack
            .is_some_and(|t| tick - f64::from(t) <= LIFETIME)
    }
    pub fn publish(&mut self, tick: u32, hand: &Matrix4, edge: &Edge, on: bool, attack: bool) {
        let last = self.newest_first().next().copied();
        if last.is_some_and(|p| p.tick >= tick) {
            return;
        }
        if attack && on {
            self.last_attack = Some(tick);
        }
        let length = last.map_or(f64::from(u8::from(on)), |p| p.length_at(tick.into()));
        self.points[self.next] = Point {
            ends: edge.ends(hand, length),
            active: attack && on,
            tick,
            on,
            length,
        };
        self.next = (self.next + 1) % HISTORY;
        self.count = (self.count + 1).min(HISTORY);
    }
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        edge: &Edge,
        hand: &Matrix4,
        tick: f64,
        eye: &[f64; 3],
        bridge: &mut Bridge,
        count: &mut usize,
        id: (u32, u32),
        part: usize,
        pool: &mut Pool,
    ) -> bool {
        let mut past = self.newest_first().skip_while(|p| f64::from(p.tick) > tick);
        let Some(current) = past
            .next()
            .copied()
            .or_else(|| self.newest_first().last().copied())
        else {
            return false;
        };
        let length = current.length_at(tick);
        let [base, tip] = edge.ends(hand, length);
        let mut axis = tip;
        axis.sub(&base);
        let mut sight = Vector3::new(eye[0], eye[1], eye[2]);
        sight.sub(&base);
        let mut width = Vector3::default();
        width.cross_vectors(&axis, &sight);
        let mut m = Matrix4::default();
        let blade = length > 0. && width.length_sq() >= 1e-14 && *count < draw::MAX_RECORDS;
        if blade {
            width.normalize().multiply_scalar(edge.width);
            let mut normal = Vector3::default();
            normal.cross_vectors(&width, &axis).normalize();
            m.make_basis(&width, &axis, &normal).set_position_v3(&base);
            emit(bridge, count, &edge.blade, &m, None, id, part);
        }
        if let Some(texture) = edge
            .trail
            .filter(|_| pool.jobs.len() < MAX_RIBBONS && self.recent_attack(tick))
        {
            let mut ribbon = Job::new(edge.blade, texture, base, id, part + 1);
            let mut last = [base, tip];
            let mut last_age = 0.;
            let mut active = current.active;
            for p in std::iter::once(&current).chain(past) {
                let age = (tick - f64::from(p.tick)).max(0.);
                if age > last_age && active && p.active {
                    let mut older = p.ends;
                    if age > LIFETIME {
                        for (dst, from) in older.iter_mut().zip(last) {
                            let mut end = from;
                            end.lerp(dst, (LIFETIME - last_age) / (age - last_age));
                            *dst = end;
                        }
                    }
                    ribbon.segment(last, older, last_age, age.min(LIFETIME));
                }
                if age >= LIFETIME {
                    break;
                }
                last = p.ends;
                last_age = age;
                active = p.active;
            }
            if ribbon.segments > 0 {
                pool.jobs.push(ribbon);
            }
        }
        blade
    }
}
struct Job {
    row: Row,
    origin: Vector3,
    id: (u32, u32),
    part: usize,
    vertices: [[f32; 8]; 4 * SEGMENTS],
    segments: usize,
    radius: f64,
}
impl Job {
    fn new(mut row: Row, texture: u32, origin: Vector3, id: (u32, u32), part: usize) -> Self {
        row[draw::REC_TEXTURES] = texture.into();
        Self {
            row,
            origin,
            id,
            part,
            vertices: [[0.; 8]; 4 * SEGMENTS],
            segments: 0,
            radius: 0.,
        }
    }
    fn segment(&mut self, newer: [Vector3; 2], older: [Vector3; 2], young: f64, old: f64) {
        let first = self.segments * 4;
        for (i, (point, age)) in [
            (newer[0], young),
            (newer[1], young),
            (older[0], old),
            (older[1], old),
        ]
        .into_iter()
        .enumerate()
        {
            let mut local = point;
            local.sub(&self.origin);
            self.radius = self.radius.max(local.length_sq());
            let u = (0.5 + (i % 2) as f64 * (WIDTH - 1) as f64) / WIDTH as f64;
            let v = (0.5 + age / LIFETIME * (HEIGHT - 1) as f64) / HEIGHT as f64;
            self.vertices[first + i] = [
                local.x as f32,
                local.y as f32,
                local.z as f32,
                0.,
                0.,
                1.,
                u as f32,
                v as f32,
            ];
        }
        self.segments += 1;
    }
}
#[derive(Default)]
pub(super) struct Pool {
    geometries: Vec<u32>,
    jobs: Vec<Job>,
}
impl Pool {
    fn reserve(&mut self, bridge: &mut Bridge, key: &str) -> Result<(), String> {
        if !self.geometries.is_empty() {
            return Ok(());
        }
        let indices: Vec<_> = (0..SEGMENTS as u32)
            .flat_map(|i| [0, 1, 2, 1, 3, 2].map(|n| 4 * i + n))
            .collect();
        let geometry = mesh(&[0.; 4 * 8 * SEGMENTS], &indices)?;
        for _ in 0..MAX_RIBBONS {
            self.geometries.push(geometry.register(&mut bridge.r, key)?);
        }
        self.jobs.reserve_exact(MAX_RIBBONS);
        Ok(())
    }
    pub fn emit(&mut self, bridge: &mut Bridge, records: &mut usize) -> (usize, usize) {
        let rows =
            &bridge.r.core.input[draw::IN_RECORDS..draw::IN_RECORDS + *records * draw::REC_STRIDE];
        let blended = rows
            .as_chunks::<{ draw::REC_STRIDE }>()
            .0
            .iter()
            .filter(|r| {
                matches!(
                    r[draw::REC_KIND],
                    draw::KIND_BLENDED | draw::KIND_BLENDED_RIGID
                )
            })
            .count();
        let room = draw::MAX_BLENDED
            .saturating_sub(blended)
            .min(draw::MAX_RECORDS - *records);
        let mut triangles = 0;
        let ribbons = self.jobs.len().min(room);
        for (job, &geometry) in self.jobs.iter_mut().take(room).zip(&self.geometries) {
            let vertex = &job.vertices[..job.segments * 4];
            bridge.r.core.geometry_buffer_sub_data(
                geometry,
                0,
                0,
                crate::session::bytes(vertex.as_flattened()),
            );
            bridge.r.core.geometry_count(geometry, job.segments * 6);
            job.row[draw::REC_GEOMETRY] = geometry.into();
            job.row[draw::REC_SPHERE..draw::REC_SPHERE + 3].fill(0.);
            job.row[draw::REC_SPHERE + 3] = job.radius.sqrt();
            let mut world = Matrix4::default();
            world.set_position_v3(&job.origin);
            emit(bridge, records, &job.row, &world, None, job.id, job.part);
            triangles += 2 * job.segments;
        }
        self.jobs.clear();
        (ribbons, triangles)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adjacent_segments_sample_the_same_fade_at_the_join() {
        let point = [Vector3::default(), Vector3::new(0., 1., 0.)];
        let mut job = Job::new([0.; draw::REC_STRIDE], 0, point[0], (0, 0), 0);
        for i in [0., 0.125, 0.5, 0.875, 1.] {
            job.segments = 0;
            job.segment(point, point, i, 11. + i);
            job.segment(point, point, 11. + i, 24. + i);
            assert_eq!(job.vertices[2][7], job.vertices[4][7]);
            assert_eq!(job.vertices[3][7], job.vertices[5][7]);
            assert!(job.vertices[6][7] > job.vertices[4][7]);
        }
    }
}
