//! The spawn launcher's class body in a 2005 battle: the class's model standing
//! in its idle stance on the stage's projector, drawn through the stage's
//! interface program with the same light as a 2004 body. The class's own
//! animation player poses it at the fixed 60 Hz clock and drawing interpolates;
//! the body is skinned here and streamed into its retained vertex buffers.
use super::assets::{Loadout, Part};
use crate::{
    app::page_battle::appearance::{self, Appearance},
    render_bridge::Bridge,
};
use bfe_content_native::{
    animation2::{
        player::{Arena, Player},
        state::State,
        Class,
    },
    geometry::{Geometry, Group, Scalar},
    loader::{Blend, Material},
};
use bfe_render::interface_resources::Style;
use three::Matrix4;

/// A skinned mesh of the stage: its rest pose and its draw parts. Positions and
/// normals are skinned into `posed` and `turned` and streamed each frame.
struct Mesh {
    geometry: u32,
    slots: [u32; 2],
    rest: Vec<f32>,
    normals: Vec<f32>,
    joints: Vec<u16>,
    weights: Vec<f32>,
    posed: Vec<f32>,
    turned: Vec<f32>,
    parts: Vec<Appearance>,
}

/// One model's stage parts: its skinned meshes, and its primary weapon's rigid
/// parts with the joint that holds them.
#[derive(Default)]
struct Staged {
    meshes: Vec<Mesh>,
    held: Vec<(u32, Vec<Appearance>)>,
}

pub(crate) struct Preview {
    models: Vec<Staged>,
    arena: Arena,
    player: Player,
    model: Option<u32>,
    clock: f64,
    tick: u32,
    /// Each joint's skinning matrix in the model's own space, this frame.
    bones: Vec<[f32; 16]>,
}

fn style(material: &Material) -> Option<Style> {
    if material.blend == Blend::Additive {
        return None;
    }
    let mut style = appearance::style("model");
    style.depth_test = Some(true);
    style.depth_write = Some(material.blend == Blend::Opaque);
    style.cutoff = Some(if material.cutout { 0.5 } else { -1. });
    if material.blend != Blend::Opaque {
        style.transparent = Some(true);
    }
    Some(style)
}

fn map(material: &Material, textures: &[u32]) -> i32 {
    material.texture.map_or(-1, |t| textures[t] as i32)
}

/// The values of a tightly or strided packed attribute, `size` per vertex.
fn values<T>(geometry: &Geometry, name: &str, scalar: Scalar, size: u32, read: impl Fn(&[u8]) -> T) -> Option<(u32, Vec<T>)> {
    let a = geometry.attributes.iter().find(|a| a.name == name)?;
    if a.scalar != scalar || a.size != size {
        return None;
    }
    let width = match scalar {
        Scalar::Float => 4,
        Scalar::U16 => 2,
        _ => return None,
    };
    let bytes = geometry.buffers.get(a.buffer)?;
    let stride = if a.stride == 0 { width * size as usize } else { a.stride };
    let mut out = Vec::with_capacity(geometry.vertices * size as usize);
    for v in 0..geometry.vertices {
        for k in 0..size as usize {
            let at = v * stride + a.offset + k * width;
            out.push(read(bytes.get(at..at + width)?));
        }
    }
    Some((u32::try_from(a.buffer).ok()?, out))
}

fn float(b: &[u8]) -> f32 {
    f32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// Blends each vertex's position and normal by up to four joint matrices
/// (column-major), normals renormalised, into `posed` and `turned`.
fn skin(
    rest: &[f32],
    normals: &[f32],
    joints: &[u16],
    weights: &[f32],
    bones: &[[f32; 16]],
    posed: &mut [f32],
    turned: &mut [f32],
) {
    for v in 0..rest.len() / 3 {
        let (p, n) = (&rest[v * 3..v * 3 + 3], &normals[v * 3..v * 3 + 3]);
        let mut out_p = [0f32; 3];
        let mut out_n = [0f32; 3];
        for k in 0..4 {
            let w = weights[v * 4 + k];
            if w == 0. {
                continue;
            }
            let Some(m) = bones.get(usize::from(joints[v * 4 + k])) else {
                continue;
            };
            for r in 0..3 {
                out_p[r] += w * (m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r]);
                out_n[r] += w * (m[r] * n[0] + m[4 + r] * n[1] + m[8 + r] * n[2]);
            }
        }
        let length = (out_n[0] * out_n[0] + out_n[1] * out_n[1] + out_n[2] * out_n[2]).sqrt();
        if length > 0. {
            out_n.iter_mut().for_each(|c| *c /= length);
        }
        posed[v * 3..v * 3 + 3].copy_from_slice(&out_p);
        turned[v * 3..v * 3 + 3].copy_from_slice(&out_n);
    }
}

impl Mesh {
    fn new(bridge: &mut Bridge, geometry: &Geometry) -> Result<Option<Self>, String> {
        let (Some((p, rest)), Some((n, normals)), Some((_, joints)), Some((_, weights))) = (
            values(geometry, "position", Scalar::Float, 3, float),
            values(geometry, "normal", Scalar::Float, 3, float),
            values(geometry, "skinIndex", Scalar::U16, 4, |b| u16::from_le_bytes([b[0], b[1]])),
            values(geometry, "skinWeight", Scalar::Float, 4, float),
        ) else {
            return Ok(None);
        };
        // The position and normal streams must be the attributes' own buffers.
        if p == n {
            return Ok(None);
        }
        let id = geometry.register(&mut bridge.r, "interface/model")?;
        Ok(Some(Self {
            geometry: id,
            slots: [p, n],
            posed: rest.clone(),
            turned: normals.clone(),
            rest,
            normals,
            joints,
            weights,
            parts: Vec::new(),
        }))
    }

    /// Skins the rest pose by `bones` and streams it to the mesh's buffers.
    fn pose(&mut self, bridge: &mut Bridge, bones: &[[f32; 16]]) {
        skin(&self.rest, &self.normals, &self.joints, &self.weights, bones, &mut self.posed, &mut self.turned);
        let core = &mut bridge.r.core;
        core.geometry_buffer_sub_data(self.geometry, self.slots[0], 0, appearance::float_bytes(&self.posed));
        core.geometry_buffer_sub_data(self.geometry, self.slots[1], 0, appearance::float_bytes(&self.turned));
    }
}

impl Preview {
    pub fn new(joints: usize) -> Result<Self, String> {
        Ok(Self {
            models: Vec::new(),
            arena: Arena::new(1, joints.max(1))?,
            player: Player::default(),
            model: None,
            clock: 0.,
            tick: 0,
            bones: vec![[0.; 16]; joints.max(1)],
        })
    }

    /// Admits one model's parts, in the order of the characters' models.
    pub fn admit(
        &mut self,
        bridge: &mut Bridge,
        textures: &[u32],
        geometry: &Geometry,
        materials: &[(Group, Material)],
        parts: &[Part],
        gear: &[Loadout],
    ) -> Result<(), String> {
        let mut staged = Staged::default();
        if let Some(mut mesh) = Mesh::new(bridge, geometry)? {
            for (group, material) in materials {
                let Some(style) = style(material) else { continue };
                let mut part = Appearance::new(
                    bridge,
                    mesh.geometry,
                    geometry.indices.len(),
                    style,
                    false,
                    &[map(material, textures)],
                )?;
                part.first = group.first;
                part.count = group.count;
                mesh.parts.push(part);
            }
            staged.meshes.push(mesh);
        }
        for p in parts {
            let Some(style) = style(&p.material) else { continue };
            if let Some(mut mesh) = Mesh::new(bridge, &p.geometry)? {
                mesh.parts.push(Appearance::new(
                    bridge,
                    mesh.geometry,
                    p.geometry.indices.len(),
                    style,
                    false,
                    &[map(&p.material, textures)],
                )?);
                staged.meshes.push(mesh);
            }
        }
        if let Some(first) = gear.first() {
            for mount in &first.mounts {
                let mut held = Vec::with_capacity(mount.parts.len());
                for p in &mount.parts {
                    let Some(style) = style(&p.material) else { continue };
                    let id = p.geometry.register(&mut bridge.r, "interface/model")?;
                    held.push(Appearance::new(
                        bridge,
                        id,
                        p.geometry.indices.len(),
                        style,
                        false,
                        &[map(&p.material, textures)],
                    )?);
                }
                staged.held.push((u32::try_from(mount.joint).unwrap_or(u32::MAX), held));
            }
        }
        self.models.push(staged);
        Ok(())
    }

    /// Poses `class` (model `model`) idle at the stage's `parent` and submits
    /// its records to the interface pass; returns how many.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        bridge: &mut Bridge,
        model: usize,
        class: &Class,
        parent: &[f64; 16],
        dt: f64,
        view: &Matrix4,
        projection: &Matrix4,
        theme: usize,
        build: f64,
    ) -> usize {
        let Some(staged) = self.models.get_mut(model) else {
            return 0;
        };
        let joints = class.animation.rig.joints.len();
        let mut pose = self.arena.slot(0, joints);
        let id = u32::try_from(model).ok();
        if self.model != id {
            self.model = id;
            self.player.reset();
            self.clock = 0.;
            self.tick = 0;
        }
        // The stance is stepped at the fixed 60 Hz clock and drawn between ticks.
        self.clock += dt.max(0.);
        let due = ((self.clock * 60.).floor() as u32).saturating_add(1);
        while self.tick < due {
            self.tick += 1;
            let state = State {
                tick: self.tick,
                ..State::default()
            };
            self.player.publish(class, state, &mut pose);
        }
        let world = self.player.place_at(class, (self.clock * 60.).max(1.), false, &mut pose);
        let mut inverse = world;
        inverse.invert();
        // The palette places the joints in the world; the stage wants them in the model's own space.
        let count = class.animation.rig.mesh_joints.min(self.bones.len());
        for (b, bone) in self.bones.iter_mut().enumerate().take(count) {
            let mut m = Matrix4::default();
            for (e, v) in m.elements.iter_mut().zip(&pose.palette[b * 16..b * 16 + 16]) {
                *e = f64::from(*v);
            }
            let mut local = inverse;
            local.multiply(&m);
            for (o, e) in bone.iter_mut().zip(local.elements) {
                *o = e as f32;
            }
        }
        let mut stage = Matrix4::default();
        stage.elements.copy_from_slice(parent);
        let mut records = 0;
        for mesh in &mut staged.meshes {
            mesh.pose(bridge, &self.bones[..count]);
            for part in &mesh.parts {
                part.simple(
                    bridge,
                    &mut records,
                    &stage,
                    view,
                    projection,
                    &appearance::body_look(part.look, theme, build),
                );
            }
        }
        for (joint, held) in &staged.held {
            let Some(hand) = pose.world.get(*joint as usize) else { continue };
            let mut at = inverse;
            at.multiply(hand);
            let mut placed = stage;
            placed.multiply(&at);
            for part in held {
                part.simple(
                    bridge,
                    &mut records,
                    &placed,
                    view,
                    projection,
                    &appearance::body_look(part.look, theme, build),
                );
            }
        }
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved(x: f32, y: f32, z: f32) -> [f32; 16] {
        let mut m = [0.; 16];
        m[0] = 1.;
        m[5] = 1.;
        m[10] = 1.;
        m[15] = 1.;
        m[12..15].copy_from_slice(&[x, y, z]);
        m
    }

    #[test]
    fn a_vertex_follows_its_weighted_joints_and_keeps_a_unit_normal() {
        let mut quarter = moved(0., 0., 0.);
        // A quarter turn about y: x goes to -z.
        quarter[0] = 0.;
        quarter[2] = -1.;
        quarter[8] = 1.;
        quarter[10] = 0.;
        let bones = [moved(2., 0., 0.), moved(0., 4., 0.), quarter];
        let rest = [1., 0., 0., 1., 0., 0.];
        let normals = [1., 0., 0., 1., 0., 0.];
        let joints = [0, 1, 0, 0, 2, 0, 0, 0];
        let weights = [0.25, 0.75, 0., 0., 1., 0., 0., 0.];
        let (mut posed, mut turned) = ([0.; 6], [0.; 6]);
        skin(&rest, &normals, &joints, &weights, &bones, &mut posed, &mut turned);
        assert_eq!(&posed[..3], &[1.5, 3., 0.]);
        assert_eq!(&turned[..3], &[1., 0., 0.]);
        assert_eq!(&posed[3..], &[0., 0., -1.]);
        assert_eq!(&turned[3..], &[0., 0., -1.]);
    }

    #[test]
    fn a_strided_attribute_is_read_per_vertex() {
        let mut bytes = Vec::new();
        for v in [[1f32, 2., 3., 9.], [4., 5., 6., 9.]] {
            bytes.extend(v.iter().flat_map(|f| f.to_le_bytes()));
        }
        let geometry = Geometry {
            buffers: vec![bytes],
            attributes: vec![bfe_content_native::geometry::Attribute {
                name: "position".into(),
                buffer: 0,
                size: 3,
                scalar: Scalar::Float,
                normalized: false,
                stride: 16,
                offset: 0,
            }],
            indices: Vec::new(),
            vertices: 2,
            groups: Vec::new(),
        };
        let (buffer, read) = values(&geometry, "position", Scalar::Float, 3, float).unwrap();
        assert_eq!((buffer, read), (0, vec![1., 2., 3., 4., 5., 6.]));
        assert!(values(&geometry, "position", Scalar::U16, 3, float).is_none());
        assert!(values(&geometry, "normal", Scalar::Float, 3, float).is_none());
    }
}
