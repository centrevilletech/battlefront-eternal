//! Place the current weapon's flash on its displayed rig when its muzzle is visible.
use super::*;

impl Replicas {
    pub(super) fn present_mission_rig_flashes(&mut self, receiver: &Receiver, bridge: &mut Bridge) {
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        for slot in &mut self.slots {
            let (weapon, fired, _, _) = slot.flash;
            if !slot.active || fired < 0. {
                continue;
            }
            let Some(muzzle) = self
                .combat
                .mission_effects
                .as_ref()
                .and_then(|m| m.weapon_muzzle(weapon))
            else {
                continue;
            };
            let Some(opacity) = muzzle.opacity(self.tick - fired) else {
                if self.tick >= fired {
                    slot.flash.1 = -1.;
                }
                continue;
            };
            let eyes = slot.identity == Some(own) && self.first_person.enabled && !slot.dead;
            let matrix = if slot.walker && !eyes {
                let near = if slot.identity == Some(own) {
                    self.walker_flash
                } else {
                    slot.identity
                        .and_then(|id| self.combat.latest_launch(id))
                        .map(|(point, _)| point)
                };
                near.zip(self.walker_guns.get(slot.class))
                    .and_then(|(point, guns)| {
                        let mut matrix = walker_flash(&mut self.units, guns, slot.actor, point)?;
                        let scale = muzzle.radius / FLASH_SCALE;
                        matrix.scale(&three::Vector3::new(scale, scale, scale));
                        Some(matrix)
                    })
            } else {
                let hand = if eyes {
                    self.first_person.hand_node(&self.units).filter(|_| {
                        self.units.header
                            [bfe_presentation::layout::unit_header::FIRST_PERSON_VISIBLE]
                            != 0.
                    })
                } else if self.units.output[slot.actor * o::STRIDE + o::GONE] != -1.
                    && slot.held.is_some()
                    && slot.held == self.weapon_classes.get(weapon).copied().flatten()
                {
                    self.models[slot.model]
                        .hand
                        .map(|hand| self.units.actor_node_start(slot.actor) + hand)
                } else {
                    None
                };
                hand.zip(self.guns.muzzles.get(weapon).copied().flatten())
                    .map(|(hand, point)| {
                        self.units.refresh_actor(slot.actor);
                        let node = hand * bfe_presentation::layout::unit_node::STRIDE
                            + bfe_presentation::layout::unit_node::WORLD;
                        let mut matrix = Matrix4::default();
                        matrix
                            .elements
                            .copy_from_slice(&self.units.nodes[node..node + 16]);
                        let mut local = Matrix4::default();
                        local.compose(
                            &three::Vector3::new(point[0], point[1], point[2]),
                            &three::Quaternion::default(),
                            &three::Vector3::new(muzzle.radius, muzzle.radius, muzzle.radius),
                        );
                        matrix.multiply(&local);
                        matrix
                    })
            };
            let Some(matrix) = matrix else { continue };
            if slot.flash_lit != fired {
                slot.flash_lit = fired;
                if let Some((rgb, radius, duration)) = muzzle.light {
                    let e = &matrix.elements;
                    self.lights
                        .flash([e[12], e[13], e[14]], rgb, radius, duration);
                }
            }
            let shown = if eyes {
                self.combat.particles.flash_view(
                    bridge,
                    &matrix.elements,
                    &muzzle.color[..3],
                    opacity,
                )
            } else {
                self.combat
                    .particles
                    .flash(bridge, &matrix.elements, &muzzle.color[..3], opacity)
            };
            self.presented.shown[10] += u64::from(shown);
        }
    }
}
