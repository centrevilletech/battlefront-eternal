//! Depth commands for the native instance bank, sharing its geometry and pose.
use crate::{
    actor_depth::Bank,
    core::{Core, MultiDraw},
    instances as I, post, shadow as S,
};
use glow::HasContext;

struct Program {
    id: u32,
    slots: Vec<(usize, u32, usize)>,
}
pub struct Casters {
    programs: [Program; 4],
    skins: Bank,
    ids: Vec<u32>,
    used_slot: Vec<u32>,
    record_skin: Vec<u32>,
    capacity: usize,
    excluded: Option<[u32; I::MAX_RECORDS.div_ceil(32)]>,
    serial: u32,
}
impl Casters {
    pub fn new<C: HasContext, M: MultiDraw>(
        core: &mut Core<C, M>,
        textures: &[u32],
        capacity: usize,
    ) -> Result<Self, String> {
        if textures.is_empty() || textures.len() > crate::actor_layout::limit::SKINS || capacity == 0 {
            return Err("native caster admission capacity".into());
        }
        let mut programs = Vec::new();
        for key in [
            "actors/depth-solid",
            "actors/depth-cutout",
            "actors/depth-skinned",
            "actors/depth-skinned-cutout",
        ] {
            let id = core.add_owned_program(key)?;
            // Resolve names cold. Cascades use the retained numeric uniform
            // slots and the same instance matrices as the color pass.
            let mut slots = Vec::new();
            for (name, ty) in crate::programs::spec(key)?.uniforms {
                let kind = match *name {
                    "modelViewMatrix" => 0,
                    "projectionMatrix" => 1,
                    "bindMatrix" => 2,
                    "bindMatrixInverse" => 3,
                    "boneTexture" => 4,
                    "map" => 5,
                    "cut" => 6,
                    _ => return Err(format!("unknown native caster uniform {name}")),
                };
                if let Some(slot) = core.native_uniform_slot(id, name) {
                    slots.push((slot, *ty, kind));
                }
            }
            programs.push(Program { id, slots });
        }
        let mut skins = Bank::default();
        let mut ids = Vec::with_capacity(textures.len());
        for &texture in textures {
            ids.push(
                skins
                    .skin(texture, capacity)
                    .ok_or("native caster bone capacity")?,
            );
        }
        let used_slot = vec![0; ids.len()];
        Ok(Self {
            programs: programs.try_into().ok().unwrap(),
            skins,
            ids,
            used_slot,
            record_skin: vec![u32::MAX; I::MAX_RECORDS],
            capacity,
            excluded: None,
            serial: 0,
        })
    }
    fn assign_skin<C: HasContext, M: MultiDraw>(
        &mut self,
        core: &Core<C, M>,
        record: usize,
        slot: usize,
        start: usize,
        count: usize,
    ) {
        self.used_slot[slot] = self.serial;
        let id = self.ids[slot];
        self.record_skin[record] = id;
        let base = self.skins.get(id).unwrap().base;
        let source = &core.bones[start..start + count * 16];
        if self.skins.bones[base..base + count * 16] == *source {
            return;
        }
        self.skins.bones[base..base + count * 16].copy_from_slice(source);
        let skin_slot = self.skins.slot(id).unwrap();
        self.skins.input[skin_slot] = self.serial as f64;
    }
    /// The regional light uses a private fixed-size mask. Publishing shadow
    /// views never changes the instance flags used by colour or motion.
    pub fn set_region(&mut self, input: &[f64], records: usize, region: Option<&crate::static_depth::Region>) {
        self.excluded = region.map(|region| {
            let mut bits = [0; I::MAX_RECORDS.div_ceil(32)];
            for (i, row) in input[I::IN_RECORDS..].chunks_exact(I::REC_STRIDE).take(records.min(I::MAX_RECORDS)).enumerate() {
                if row[I::REC_FLAGS] as u32 & I::NO_SHADOW == 0 && !region.instance(row) {
                    bits[i / 32] |= 1 << (i % 32);
                }
            }
            bits
        });
    }

    fn excluded(&self, record: usize) -> bool {
        self.excluded.as_ref().is_some_and(|bits| bits[record / 32] & (1 << (record % 32)) != 0)
    }

    pub fn prepare<C: HasContext, M: MultiDraw>(
        &mut self,
        core: &mut Core<C, M>,
        records: usize,
    ) -> Result<usize, String> {
        self.serial = self.serial.wrapping_add(1);
        if self.serial == 0 {
            self.used_slot.fill(0);
            self.serial = 1;
        }
        let limit = records.min(self.record_skin.len());
        self.record_skin[..limit].fill(u32::MAX);
        let pool_len = self.ids.len();
        let direct_limit = limit.min(pool_len);
        // Keep stable record->slot mapping for records within the pool so a single
        // unit toggling shadow distance never shifts later units' skin caches.
        for record in 0..direct_limit {
            let row = &core.input[I::IN_RECORDS + record * I::REC_STRIDE..][..I::REC_STRIDE];
            if self.excluded(record) || row[I::REC_FLAGS] as u32 & I::NO_SHADOW != 0 || (row[I::REC_KIND] != 1. && row[I::REC_KIND] != I::KIND_BLENDED) {
                continue;
            }
            let count = row[I::REC_BONES] as usize;
            if count > self.capacity {
                return Err("native caster skeleton exceeds admitted capacity".into());
            }
            let start = row[I::REC_BONE_AT] as usize * 16;
            if start + count * 16 > core.bones.len() {
                return Err("native caster bone range".into());
            }
            self.assign_skin(core, record, record, start, count);
        }
        let mut next_free = 0usize;
        let mut dropped_skins = 0usize;
        for record in direct_limit..limit {
            let row = &core.input[I::IN_RECORDS + record * I::REC_STRIDE..][..I::REC_STRIDE];
            if self.excluded(record) || row[I::REC_FLAGS] as u32 & I::NO_SHADOW != 0 || (row[I::REC_KIND] != 1. && row[I::REC_KIND] != I::KIND_BLENDED) {
                continue;
            }
            let count = row[I::REC_BONES] as usize;
            if count > self.capacity {
                return Err("native caster skeleton exceeds admitted capacity".into());
            }
            let start = row[I::REC_BONE_AT] as usize * 16;
            if start + count * 16 > core.bones.len() {
                return Err("native caster bone range".into());
            }
            while next_free < pool_len && self.used_slot[next_free] == self.serial {
                next_free += 1;
            }
            if next_free >= pool_len {
                dropped_skins += 1;
                continue;
            }
            let slot = next_free;
            next_free += 1;
            self.assign_skin(core, record, slot, start, count);
        }
        self.skins.upload(core);
        Ok(dropped_skins)
    }
    pub fn publish<C: HasContext, M: MultiDraw>(
        &self,
        core: &Core<C, M>,
        input: &mut [f64],
        records: usize,
        view: &[f64],
        projection: &[f64],
    ) -> Result<Option<[f64; 4]>, String> {
        let clip = crate::math::mul(projection.try_into().unwrap(), view.try_into().unwrap());
        let frustum = crate::math::Frustum::from_matrix(&clip);
        let mut bounds: Option<[f64; 4]> = None;
        let mut count = 0;
        for record in 0..records.min(self.record_skin.len()) {
            let row = &core.input[I::IN_RECORDS + record * I::REC_STRIDE..][..I::REC_STRIDE];
            if self.excluded(record) || row[I::REC_FLAGS] as u32 & I::NO_SHADOW != 0 || !(1. ..=4.).contains(&row[I::REC_KIND]) {
                continue;
            }
            let skinned = row[I::REC_KIND] == 1. || row[I::REC_KIND] == I::KIND_BLENDED;
            if skinned && self.record_skin[record] == u32::MAX {
                continue;
            }
            let Some(geometry) = core.geometries.get(row[I::REC_GEOMETRY] as usize) else {
                continue;
            };
            let world = row[I::REC_WORLD..I::REC_WORLD + 16].try_into().unwrap();
            let (center, radius) = crate::math::sphere_to_world(world, row[I::REC_SPHERE..I::REC_SPHERE + 3].try_into().unwrap(), row[I::REC_SPHERE + 3]);
            if !frustum.sphere(&center, radius) { continue; }
            let rect = projected_sphere(&clip, center, radius);
            bounds = Some(match bounds {
                None => rect,
                Some([x0, y0, x1, y1]) => [rect[0].min(x0), rect[1].min(y0), rect[2].max(x1), rect[3].max(y1)],
            });
            let model_view = crate::math::mul(view.try_into().unwrap(), world);
            for group in 0..(row[I::REC_GROUPS] as usize).clamp(1, 4) {
                let map = row[I::REC_TEXTURES + group];
                let cutout = row[I::REC_FLAGS] as u32 & 4 != 0 && map >= 0.;
                let program = &self.programs[2 * skinned as usize + cutout as usize];
                let at = S::S_FIRST + count * S::DRAW;
                if at + S::DRAW > input.len() {
                    return Err("native caster command capacity".into());
                }
                let output = &mut input[at..at + S::DRAW];
                output.fill(0.);
                output[S::D_PROGRAM] = program.id as f64;
                output[S::D_GEOMETRY] = row[I::REC_GEOMETRY];
                let (first, indices) = geometry
                    .groups
                    .get(group)
                    .copied()
                    .unwrap_or((0, geometry.index_count));
                output[S::D_START] = first as f64;
                output[S::D_COUNT] = indices as f64;
                output[S::D_SIDE] = (row[I::REC_FLAGS] as u32 & 3) as f64;
                output[S::D_MIRRORED] = crate::math::mirrors(world) as u8 as f64;
                output[S::D_UNIFORMS] = program.slots.len() as f64;
                for (index, &(slot, ty, kind)) in program.slots.iter().enumerate() {
                    let u = &mut output[S::D_FIRST + index * post::UNIFORM..][..post::UNIFORM];
                    u[0] = slot as f64;
                    u[1] = ty as f64;
                    match kind {
                        0 => u[2..18].copy_from_slice(&model_view),
                        1 => u[2..18].copy_from_slice(projection),
                        2 => u[2..18].copy_from_slice(&row[I::REC_BIND..I::REC_BIND + 16]),
                        3 => u[2..18]
                            .copy_from_slice(&row[I::REC_BIND_INVERSE..I::REC_BIND_INVERSE + 16]),
                        4 => u[2] = self.skins.get(self.record_skin[record]).unwrap().texture as f64,
                        5 => u[2] = map,
                        6 => u[2] = 0.5,
                        _ => unreachable!(),
                    }
                }
                count += 1;
            }
        }
        input[S::S_DRAWS] = count as f64;
        Ok(bounds)
    }
}

// Cascades are orthographic: each projected sphere extent is the length of
// one matrix row times its world radius. Unknown bounds conservatively cover
// the whole map, just as a newly positioned cascade does.
fn projected_sphere(clip: &crate::math::Mat4, center: [f64; 3], radius: f64) -> [f64; 4] {
    if clip[3] != 0. || clip[7] != 0. || clip[11] != 0. || !clip[15].is_finite() || clip[15].abs() < 1e-12
        || !radius.is_finite() || radius < 0. || center.iter().any(|x| !x.is_finite()) {
        return [-1., -1., 1., 1.];
    }
    let p = crate::math::transform4(clip, [center[0], center[1], center[2], 1.]);
    // Affine inversion can leave w just above/below one. Divide by its
    // constant value instead of needlessly treating that as perspective.
    let w = clip[15];
    let rx = radius * (clip[0]*clip[0] + clip[4]*clip[4] + clip[8]*clip[8]).sqrt() / w.abs();
    let ry = radius * (clip[1]*clip[1] + clip[5]*clip[5] + clip[9]*clip[9]).sqrt() / w.abs();
    let rect = [p[0]/w-rx, p[1]/w-ry, p[0]/w+rx, p[1]/w+ry];
    if rect.iter().any(|x| !x.is_finite()) { [-1., -1., 1., 1.] } else { rect }
}

#[cfg(test)]
mod tests {
    use super::projected_sphere;
    #[test]
    fn affine_shadow_bounds_ignore_homogeneous_scale() {
        let mut clip = [0.; 16];
        for i in [0, 5, 10, 15] { clip[i] = 2.; }
        assert_eq!(projected_sphere(&clip, [0.25, -0.5, 0.], 0.125), [0.125, -0.625, 0.375, -0.375]);
        clip[3] = 0.1;
        assert_eq!(projected_sphere(&clip, [0.25, -0.5, 0.], 0.125), [-1., -1., 1., 1.]);
    }
}
