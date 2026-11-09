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
    capacity: usize,
    serial: u32,
}
impl Casters {
    pub fn new<C: HasContext, M: MultiDraw>(
        core: &mut Core<C, M>,
        textures: &[u32],
        capacity: usize,
    ) -> Result<Self, String> {
        if textures.len() != I::MAX_RECORDS || capacity == 0 {
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
        Ok(Self {
            programs: programs.try_into().ok().unwrap(),
            skins,
            ids,
            capacity,
            serial: 0,
        })
    }
    pub fn prepare<C: HasContext, M: MultiDraw>(
        &mut self,
        core: &mut Core<C, M>,
        records: usize,
    ) -> Result<(), String> {
        self.serial = self.serial.wrapping_add(1);
        for record in 0..records.min(I::MAX_RECORDS) {
            let row = &core.input[I::IN_RECORDS + record * I::REC_STRIDE..][..I::REC_STRIDE];
            if row[I::REC_KIND] != 1. && row[I::REC_KIND] != I::KIND_BLENDED {
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
            let id = self.ids[record];
            let base = self.skins.get(id).unwrap().base;
            self.skins.bones[base..base + count * 16]
                .copy_from_slice(&core.bones[start..start + count * 16]);
            let slot = self.skins.slot(id).unwrap();
            self.skins.input[slot] = self.serial as f64;
        }
        self.skins.upload(core);
        Ok(())
    }
    pub fn publish<C: HasContext, M: MultiDraw>(
        &self,
        core: &Core<C, M>,
        input: &mut [f64],
        records: usize,
        view: &[f64],
        projection: &[f64],
    ) -> Result<(), String> {
        let mut count = 0;
        for record in 0..records.min(I::MAX_RECORDS) {
            let row = &core.input[I::IN_RECORDS + record * I::REC_STRIDE..][..I::REC_STRIDE];
            if !(1. ..=4.).contains(&row[I::REC_KIND]) {
                continue;
            }
            let Some(geometry) = core.geometries.get(row[I::REC_GEOMETRY] as usize) else {
                continue;
            };
            let world = row[I::REC_WORLD..I::REC_WORLD + 16].try_into().unwrap();
            let model_view = crate::math::mul(view.try_into().unwrap(), world);
            let skinned = row[I::REC_KIND] == 1. || row[I::REC_KIND] == I::KIND_BLENDED;
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
                        4 => u[2] = self.skins.get(self.ids[record]).unwrap().texture as f64,
                        5 => u[2] = map,
                        6 => u[2] = 0.5,
                        _ => unreachable!(),
                    }
                }
                count += 1;
            }
        }
        input[S::S_DRAWS] = count as f64;
        Ok(())
    }
}
