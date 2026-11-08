//! combat.js presentation policy. Rust publishes decisions and numeric values;
//! the browser applies them to retained weapons, animations, lights and HUD.
//!
//! A charge on show, each frame the body is drawn (Unit.present for every
//! soldier, game.js for his own), as the original game draws it: a gun held at
//! a charge keeps the soldier in the alert stance, aim up (but for the tool,
//! sabre and gam banks), where a thrown weapon's windup plays its bank's
//! charge clip; and the weapon's ChargeUpEffect sits at its fire point as
//! drawn (first person, the arms' model), its colour and alpha scaled by
//! the charge. The first-person arms go on with their idle or run.
//! Presentation only: it reads the weapon's charging and held, which the
//! simulation sets, a world worker's page receives with the weapon, and a
//! client online has from the soldier's record (or his own correction,
//! src/runtime/network-entity-game.js), so a bot's charge and another
//! player's show as his own does. dt also runs down how long a repair tool
//! known to be at work only from its ticks stays in the hand.
//!
//! The muzzle flash: the flash mesh for FlashLength, at full alpha for its
//! first half and fading through the second, and a light for
//! FlashLightDuration. Each round restarts its barrel's flash (it sets the
//! barrel's timer): the five rounds of a blast cannon's salvo light the
//! muzzle once, not five lights deep.

//!
//! Held weapon and charge state
//!
//! The secondary is shown for its entire busy throw and for a replicated
//! repair interval. A missing secondary still resolves to an empty hand;
//! selection never substitutes a different resource after Rust chooses it.
//! Dead and mounted bodies publish no held model or first-person weapon.
//! A weapon's handEmpty flag hides its model without hiding its animation.
//!
//! Charge transitions belong to this view's lifetime. They are separate
//! from authoritative charging and held time, which arrive as frame facts.
//! Clearing a life removes the transition latch even when no frame follows.
//! Repair work can hold alert independently of a primary weapon's charge.
//! The tool, sabre and special unalerted banks retain their existing stance.
//!
//! HUD bars preserve priority and authored thresholds. Repair wins before a
//! shield, and a shield wins before jet fuel. Health ratios keep their original
//! arithmetic; only the repair bar clamps its ratio. Low jet fuel pulses while
//! actively jetting and uses a steady low warning otherwise. The browser maps
//! these outputs to art handles and supplies the current camera projection.
//! Scope visibility also uses the current weapon and camera-mode facts.

use crate::gen::CombatView as F;
use v8math::min;
pub struct View {
    pub words: [f64; F::WORDS],
    charge_shown: bool,
}
impl View {
    pub fn new() -> Self {
        Self {
            words: [0.0; F::WORDS],
            charge_shown: false,
        }
    }
    pub fn run(&mut self, op: u32) {
        let mut f = F::take(&self.words, &[]);
        match op {
            // updateHeld: the secondary occupies the hand throughout a throw
            // or the short interval after a replicated repair tick.
            0 => {
                f.slot = u32::from(f.secondary_busy || f.secondary_work > 0.0);
                f.shown = !f.dead && !f.mounted;
                f.model = f.shown
                    && !if f.slot == 1 {
                        f.secondary_empty
                    } else {
                        f.primary_empty
                    };
            }
            // presentCharge advances only the cosmetic tool hold. Weapon
            // charge time itself is a published simulation/prediction fact.
            1 => {
                f.refresh_held = false;
                if f.secondary_work > 0.0 {
                    f.secondary_work -= f.dt;
                    f.refresh_held = f.secondary_work <= 0.0;
                }
                f.repair_alert = f.repair_lock
                    && !f.dead
                    && !f.mounted
                    && (f.primary_repair || f.secondary_repair);
                let charging = !f.dead && !f.mounted && !f.remote && f.primary && f.charging;
                f.charge_action = if charging != self.charge_shown {
                    if charging && f.thrown {
                        1
                    } else if !charging {
                        2
                    } else {
                        0
                    }
                } else {
                    0
                };
                self.charge_shown = charging;
                f.hold_alert = charging && !f.thrown && !f.unalerted;
                f.glow = charging;
                f.tint = min(f.held / f.max_pressed, 1.0);
            }
            // clearCharge must also clear the native transition latch, so a
            // new life with the same weapon still emits its first charge.
            2 => {
                f.charge_action = if self.charge_shown { 2 } else { 0 };
                self.charge_shown = false;
            }
            // updateHud: bar priority and thresholds are game rules. The
            // output names an art resource; the host resolves its texture.
            3 => {
                f.health_fill = f.health / f.max_health;
                f.hull_fill = if f.turret {
                    f.hull_health / f.hull_max
                } else {
                    0.0
                };
                f.second = 0;
                f.fill = 0.0;
                f.look = 0;
                if !f.turret && !f.dead {
                    if f.target {
                        f.second = 1;
                        f.fill = three::clamp(f.target_health / f.target_max, 0.0, 1.0);
                        f.look = u32::from(f.fill <= 0.5);
                    } else if f.shield {
                        f.second = 2;
                        f.fill = if f.shield_max > 0.0 {
                            f.shield_value / f.shield_max
                        } else {
                            0.0
                        };
                        f.look = u32::from(f.fill <= 0.25);
                    } else if f.jet_recharge > 0.0 {
                        f.second = 3;
                        f.fill = f.fuel;
                        if f.fuel <= f.jet_border {
                            f.look = if f.jetting { 2 } else { 1 };
                        }
                    }
                }
                f.eye = f.first_person || (f.zoom > 1.0 && f.zoom_first_person);
                f.scope = !f.turret && f.zoom > 1.0 && (f.first_person || f.zoom_first_person);
            }
            // flash normalizes authored byte colours once per shot. Light
            // colours are premultiplied, unlike the retained flash material.
            4 => {
                for i in 0..3 {
                    f.flash_rgb[i] = f.flash_color[i] / 255.0;
                    f.light_rgb[i] = (f.light_color[i] / 255.0) * (f.light_color[3] / 255.0);
                }
                f.flash_alpha = f.flash_color[3] / 255.0;
            }
            5 => {
                f.secondary_work = f.repair_delay + 0.25;
            }
            // PlayerCombat.zooms resolves immutable levels during admission.
            // A continuous ZoomRate excludes the authored maximum stop.
            6 => {
                f.zooms = [1.0; 3];
                f.zoom_count = 1;
                for value in [
                    f.zoom_min,
                    if f.zoom_rate != 0.0 { 0.0 } else { f.zoom_max },
                ] {
                    if value > f.zooms[f.zoom_count as usize - 1] {
                        f.zooms[f.zoom_count as usize] = value;
                        f.zoom_count += 1;
                    }
                }
            }
            _ => {}
        }
        f.put(&mut self.words, &mut Vec::new());
    }
}
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn combat_view_create() -> *mut View {
        Box::into_raw(Box::new(View::new()))
    }
    #[no_mangle]
    pub unsafe extern "C" fn combat_view_drop(p: *mut View) {
        if !p.is_null() {
            drop(Box::from_raw(p));
        }
    }
    #[no_mangle]
    pub unsafe extern "C" fn combat_view_words(p: *mut View) -> *mut f64 {
        (*p).words.as_mut_ptr()
    }
    #[no_mangle]
    pub unsafe extern "C" fn combat_view_run(p: *mut View, op: u32) {
        (*p).run(op);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn charge_transition_and_new_life_do_not_repeat_or_lose_events() {
        let mut v = View::new();
        let f = F {
            primary: true,
            charging: true,
            thrown: true,
            max_pressed: 1.0,
            held: 0.5,
            ..Default::default()
        };
        f.put(&mut v.words, &mut Vec::new());
        v.run(1);
        assert_eq!(F::take(&v.words, &[]).charge_action, 1);
        v.run(1);
        assert_eq!(F::take(&v.words, &[]).charge_action, 0);
        v.run(2);
        assert_eq!(F::take(&v.words, &[]).charge_action, 2);
        v.run(1);
        assert_eq!(F::take(&v.words, &[]).charge_action, 1);
    }
    #[test]
    fn repair_then_shield_then_jet_preserves_bar_priority_and_thresholds() {
        let mut v = View::new();
        let mut f = F {
            target: true,
            target_health: 50.0,
            target_max: 100.0,
            shield: true,
            shield_value: 20.0,
            shield_max: 100.0,
            jet_recharge: 1.0,
            fuel: 0.1,
            jet_border: 0.2,
            jetting: true,
            ..Default::default()
        };
        f.put(&mut v.words, &mut Vec::new());
        v.run(3);
        let r = F::take(&v.words, &[]);
        assert_eq!((r.second, r.fill, r.look), (1, 0.5, 1));
        f.target = false;
        f.put(&mut v.words, &mut Vec::new());
        v.run(3);
        assert_eq!(F::take(&v.words, &[]).second, 2);
        f.shield = false;
        f.put(&mut v.words, &mut Vec::new());
        v.run(3);
        let r = F::take(&v.words, &[]);
        assert_eq!((r.second, r.fill, r.look), (3, 0.1, 2));
    }
}
