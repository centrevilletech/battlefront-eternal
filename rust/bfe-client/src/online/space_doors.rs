//! Draw space doors from the simulation's previous and current opening fractions.
use super::receiver::Receiver;
use crate::render_bridge::Bridge;
use bfe_content_native::{placed, BattleContent};
use bfe_render::instances as draw;
use bfe_sim::space::SpaceDoorSkin;

pub(super) struct Door {
    pub skin: SpaceDoorSkin,
    pub rows: Vec<[f64; draw::REC_STRIDE]>,
}

impl Door {
    pub fn admit(
        content: &BattleContent,
        bridge: &mut Bridge,
        textures: &[u32],
    ) -> Result<Vec<Self>, String> {
        let Some(space) = content.content.battle.swbf2.as_ref() else {
            return Ok(Vec::new());
        };
        let mut doors = Vec::new();
        for skin in SpaceDoorSkin::admit(&space.map_admission)? {
            let placed = placed::admit_look(content, skin.instance, 0)?;
            let admission = crate::static_scene::props(
                bridge,
                content,
                textures,
                placed,
                bridge.shadow_cascades(),
                false,
            )?;
            doors.push(Self {
                skin,
                rows: admission.records,
            });
        }
        Ok(doors)
    }

    pub fn frame(doors: &mut [Self], receiver: &Receiver, at: f64) {
        let Some((states, alpha)) = receiver.doors.sample(at) else {
            return;
        };
        for door in doors {
            if let Some(state) = states.iter().find(|s| s.instance == door.skin.instance) {
                door.skin.sample(state, alpha);
            }
        }
    }

    pub fn append(
        &self,
        bridge: &mut Bridge,
        mut records: usize,
        mut bones: usize,
    ) -> (usize, usize) {
        let count = self.skin.bone_count();
        for recipe in &self.rows {
            if records >= draw::MAX_RECORDS || bones + count > draw::MAX_BONES {
                break;
            }
            let mut row = *recipe;
            row[draw::REC_BONES] = count as f64;
            row[draw::REC_BONE_AT] = bones as f64;
            bridge.r.core.bones[bones * 16..(bones + count) * 16]
                .copy_from_slice(self.skin.matrices());
            let start = draw::IN_RECORDS + records * draw::REC_STRIDE;
            bridge.r.core.input[start..start + draw::REC_STRIDE].copy_from_slice(&row);
            records += 1;
            bones += count;
        }
        (records, bones)
    }
}
