//! presentEntityEvent's unit branches (src/runtime/entity-presentation-events.js):
//! a soldier's weapon action over his upper body, a droid's death blast and
//! a droid blown apart. The server decides each; this owner only shows it on
//! the body the client already draws.
use super::{command, f, kind, o, Replicas};
use crate::online::receiver::Receiver;
use bfe_content::Value;
use bfe_presentation::{
    layout::unit_node as node,
    network_layout::{net_kind as k, net_record as r},
};

// presentEntityEvent accepts exactly these actions and postures; the action's
// place in the list is its identity for endWeaponAction.
const ACTIONS: [&str; 7] = ["reload", "charge", "shoot", "shoot2", "shoot3", "shoot4", "shoot5"];
const POSTURES: [&str; 3] = ["stand", "crouch", "prone"];
// game.js droidBlasts: one of these goes off at a bone of the dying droid.
const BLASTS: [&str; 3] = ["explosion", "med_explosion", "small_explosion"];
// decodePresentation: a record nobody could show yet is kept for 120 ticks.
const EXPIRY: f64 = 120.;
const PENDING: usize = 256;

/// What each `Presented::shown` count is, for a battle's summary.
pub const SHOWN: [&str; 11] = [
    "weaponActions",
    "weaponActionEnds",
    "deathBlasts",
    "blownApart",
    "soldierShots",
    "lightningBolts",
    "shieldHits",
    "lightningPrimitives",
    "shieldBubbleDraws",
    "effectDrawsRefused",
    "muzzleFlashDraws",
];

/// One clip per posture and action for a weapon bank, -1 where none plays.
pub(super) type BankClips = [[f64; ACTIONS.len()]; POSTURES.len()];

// What a record asks of the body it names, with every name it carried
// already resolved to a number, so one kept for later holds no text.
#[derive(Clone, Copy)]
enum Act {
    Weapon { action: usize, bank: usize, posture: usize },
    EndWeapon { action: Option<usize> },
    Shot { weapon: usize, repair: bool, melee: bool },
    DeathBlast,
    BlowApart { health: f64 },
    ShieldHit { amount: f64, source: Option<[f64; 3]>, explosion: bool },
}
// One record: the unit's id and generation, the tick it stops being worth
// showing, the record's id (a seed for its effect) and what to show.
#[derive(Clone, Copy)]
struct Event {
    owner: (u32, u32),
    expiry: f64,
    seed: u32,
    act: Act,
}
// A unit-state record kept until its unit is drawn: the unit's id and
// generation, the record's tick, and the two facts read from it.
#[derive(Clone, Copy)]
struct State {
    owner: (u32, u32),
    tick: f64,
    blown: bool,
    throwing: Option<(usize, bool)>,
}
// The payload's own names, borrowed. A field a kind doesn't carry is absent
// or null; both read as None.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Input<'a> {
    kind: &'a str,
    #[serde(default)]
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    #[serde(default)]
    action: Option<&'a str>,
    #[serde(default)]
    bank: Option<&'a str>,
    #[serde(default)]
    posture: Option<&'a str>,
    #[serde(default)]
    amount: Option<f64>,
    #[serde(default)]
    weapon_id: usize,
    #[serde(default)]
    mount_id: u32,
    #[serde(default)]
    repair: bool,
    #[serde(default)]
    melee: bool,
    #[serde(default)]
    source: Option<[f64; 3]>,
    #[serde(default)]
    hit_kind: Option<&'a str>,
}
pub struct Presented {
    // Every bank name a record can carry, with the animator's bank for it
    // (SoldierAnimator.setBank: an unknown bank is the rifle's, 0).
    banks: Vec<(String, usize)>,
    // Each registry weapon's bank (an index into `banks`) and whether it is
    // thrown (src/weapons.js: a grenade, dispenser or remote).
    weapons: Vec<Option<(usize, bool)>>,
    // The same two facts for each weapon definition, with its registry id,
    // in the definitions' own order, which the local player's predicted
    // arsenal names.
    definitions: Vec<(usize, bool, usize)>,
    // The death blasts' effect groups, None where the library lacks one.
    blasts: [Option<usize>; BLASTS.len()],
    // Records whose unit was not drawn yet, oldest first.
    pending: Vec<Event>,
    // Unit-state records waiting for their unit, the newest of each unit.
    states: Vec<State>,
    // The server epoch the kept records belong to.
    epoch: f64,
    // `Math.random()` for the blast and its bone; it decides nothing shared.
    rng: jsrt::rng::ClassicRandom,
    /// Events shown so far, counted under the names in `SHOWN`.
    pub shown: [u64; SHOWN.len()],
}
impl Presented {
    // The bank names come from the animation sets and from every weapon's
    // AnimationBank (`def.AnimationBank || "rifle"`, src/weapons.js), which
    // is all a record can name. `weapons` lists each weapon definition's
    // registry id (0 when the registry lacks it), bank and base.
    pub(super) fn new<'a>(
        sets: Option<&Value>,
        weapons: impl Iterator<Item = (usize, &'a str, &'a str)>,
        registered: usize,
        effects: &[(String, usize)],
    ) -> Self {
        let mut banks = vec![("rifle".to_string(), 0)];
        let named = sets.and_then(|s| s.get("banks")).and_then(Value::as_object);
        for (name, _) in named.iter().flat_map(|b| b.entries()).filter(|(name, _)| *name != "rifle") {
            banks.push((name.to_string(), banks.len()));
        }
        let mut table = vec![None; registered + 1];
        let mut definitions = Vec::new();
        for (id, bank, base) in weapons {
            let known = banks.iter().position(|(name, _)| name == bank).unwrap_or_else(|| {
                banks.push((bank.to_string(), 0));
                banks.len() - 1
            });
            let facts = (known, ["grenade", "dispenser", "remote"].contains(&base));
            definitions.push((facts.0, facts.1, id));
            if let Some(entry) = table.get_mut(id).filter(|entry| id != 0 && entry.is_none()) {
                *entry = Some(facts);
            }
        }
        Self {
            banks,
            weapons: table,
            definitions,
            blasts: BLASTS.map(|name| effects.iter().find(|(n, _)| n == name).map(|(_, id)| *id)),
            pending: Vec::with_capacity(PENDING),
            states: Vec::with_capacity(PENDING),
            epoch: f64::NAN,
            rng: jsrt::rng::ClassicRandom::new(
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0., |t| t.as_nanos() as u32 as f64),
            ),
            shown: [0; SHOWN.len()],
        }
    }
    // SoldierAnimator.weaponAction's clip for every bank, posture and action
    // of one body: the bank's named clip (else the rifle's name for it), else
    // `human_<bank>_<posture>_<action>`, else the grenade bank's standing
    // clip, else the rifle's.
    pub(super) fn clips(&self, sets: Option<&Value>, clips: &[(String, f64)]) -> Vec<BankClips> {
        let id = |name: &str| clips.iter().position(|(clip, _)| clip == name);
        let named = |bank: &str, posture: &str, action: &str| {
            sets.and_then(|s| s.get("banks"))
                .and_then(|b| b.get(bank))
                .and_then(|b| b.get("weapon"))
                .and_then(|w| w.get(posture))
                .and_then(|p| p.get(action))
                .and_then(Value::as_str)
        };
        self.banks
            .iter()
            .map(|(bank, _)| {
                POSTURES.map(|posture| {
                    ACTIONS.map(|action| {
                        let pick = |b: &str, p: &str| id(&format!("human_{b}_{p}_{action}"));
                        named(bank, posture, action)
                            .or_else(|| named("rifle", posture, action))
                            .and_then(id)
                            .or_else(|| pick(bank, posture))
                            .or_else(|| (bank == "grenade").then(|| pick(bank, "stand")).flatten())
                            .or_else(|| pick("rifle", posture))
                            .map_or(-1., |clip| clip as f64)
                    })
                })
            })
            .collect()
    }
    // presentEntityEvent's validation: a weapon action outside the lists is
    // an invalid record and shows nothing.
    fn decode(&self, source: &Input) -> Option<Act> {
        let place = |list: &[&str], name: Option<&str>| name.and_then(|name| list.iter().position(|known| *known == name));
        match source.kind {
            "weaponAction" => Some(Act::Weapon {
                action: place(&ACTIONS, source.action)?,
                bank: self.banks.iter().position(|(name, _)| Some(name.as_str()) == source.bank).unwrap_or(0),
                posture: place(&POSTURES, source.posture)?,
            }),
            // `event.action ?? undefined`: no action ends whichever plays.
            "endWeaponAction" => Some(Act::EndWeapon { action: place(&ACTIONS, source.action) }),
            // A soldier's own shot; a hull's or a turret's (a mount) moves no
            // body. A weapon the registry doesn't name is nobody's.
            "shot" if source.mount_id == 0 => self
                .weapons
                .get(source.weapon_id)
                .copied()
                .flatten()
                .map(|_| Act::Shot { weapon: source.weapon_id, repair: source.repair, melee: source.melee }),
            "deathBlast" => Some(Act::DeathBlast),
            // `event.amount || 0` from `event.source`; a blast flares its own way.
            "shieldHit" => Some(Act::ShieldHit {
                amount: source.amount.unwrap_or(0.),
                source: source.source,
                explosion: source.hit_kind == Some("explosion"),
            }),
            // `event.amount || 0`: the health the droid died at.
            "blowApart" => Some(Act::BlowApart { health: source.amount.unwrap_or(0.) }),
            _ => None,
        }
    }
}
// decodeUnitState (src/runtime/entity-unit-state.js), as far as a drawn body
// needs it: whose state it is, whether a dead droid was blown apart, and
// the selected secondary weapon while a throw is under way (`throwTime >=
// 0`, the replicated part of Weapon.busy). None for a malformed payload.
fn unit_state(payload: &[u8]) -> Option<(u32, bool, Option<(usize, bool)>)> {
    let u8_at = |at: usize| payload.get(at).copied();
    let u16_at = |at: usize| Some(u16::from_le_bytes(payload.get(at..at + 2)?.try_into().ok()?));
    let id = u32::from_le_bytes(payload.get(..4)?.try_into().ok()?);
    let flags = u8_at(4)?;
    let secondary = u16_at(17)? as usize;
    let count = u8_at(19)?;
    let mut at = 21 + u8_at(20)? as usize;
    let mut throwing = None;
    for _ in 0..count {
        let weapon = u16_at(at)? as usize;
        // FLAGS' third bit: the thrown thing has left the hand (`hidden`).
        let hidden = u8_at(at + 3)? & 4 != 0;
        let mask = u8_at(at + 4)?;
        at += 5;
        // Seven optional numbers follow in NUMBERS' order; throwTime is the
        // fourth, and absent it is its default, -1.
        for number in 0..7 {
            if mask & (1 << number) != 0 {
                let value = f32::from_le_bytes(payload.get(at..at + 4)?.try_into().ok()?);
                if number == 3 && value >= 0. && weapon == secondary {
                    throwing = Some((weapon, hidden));
                }
                at += 4;
            }
        }
        if mask & 128 != 0 {
            at += 6;
        }
    }
    (at == payload.len() && flags <= 15).then_some((id, flags & 8 != 0, throwing))
}
impl Replicas {
    // applyUnitState and PlayerCombat.updateHeld for the bodies drawn. A
    // dead droid whose state says it was blown apart is gone even if its
    // blow-apart record never came. The animator's bank is the bank of the
    // weapon in the hand: the secondary for the whole of a throw, otherwise
    // the primary; the local player's throw is read from his own predicted
    // arsenal, everyone else's from their replicated state. A state whose
    // body is not drawn yet is dropped, where the browser keeps it for 120
    // ticks.
    // NetworkEntityGame.apply keeps each unit's newest state record
    // (`pendingState`) and asks applyUnitState again with every packet until
    // the unit is known or the record is more than 120 ticks old.
    pub(super) fn present_states(&mut self, receiver: &Receiver, prediction: &crate::online::replay::Replay, tick: f64) {
        // A new epoch renumbers ticks and bodies: nothing kept still applies.
        if self.presented.epoch != receiver.epoch {
            self.presented.epoch = receiver.epoch;
            self.presented.pending.clear();
            self.presented.states.clear();
        }
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::SUMMARY as f64 || field.row[r::SCHEMA] != 6. {
                continue;
            }
            let Some((id, blown, throwing)) = unit_state(&field.bytes[..field.length]) else {
                continue;
            };
            if field.row[r::ID] != 100000. + id as f64 {
                continue;
            }
            let state = State { owner: (id, field.row[r::GENERATION] as u32), tick: field.row[r::TICK], blown, throwing };
            let states = &mut self.presented.states;
            if let Some(held) = states.iter_mut().find(|held| held.owner.0 == id) {
                *held = state;
            } else {
                if states.len() == PENDING {
                    states.remove(0);
                    self.refused += 1;
                }
                states.push(state);
            }
        }
        let mut at = 0;
        while at < self.presented.states.len() {
            let state = self.presented.states[at];
            if let Some(&slot) = self.bound.get(&state.owner) {
                self.slots[slot].blown = state.blown;
                self.slots[slot].throwing = state
                    .throwing
                    .and_then(|(weapon, hidden)| Some((self.presented.weapons.get(weapon).copied().flatten()?.0, weapon, hidden)));
            } else if tick - state.tick <= EXPIRY {
                at += 1;
                continue;
            }
            self.presented.states.remove(at);
        }
        let own = (receiver.own.id as u32, receiver.own.generation as u32);
        let own_throw = prediction
            .weapons()
            .filter(|weapon| weapon.busy())
            .filter_map(|weapon| Some((self.presented.definitions.get((weapon.def.0 as usize).checked_sub(1)?).copied()?, weapon.hand_empty())))
            .find(|((_, thrown, _), _)| *thrown)
            .map(|((bank, _, id), hidden)| (bank, id, hidden));
        for at in 0..self.slots.len() {
            let slot = &mut self.slots[at];
            if !slot.active {
                continue;
            }
            if slot.dead && slot.blown {
                slot.gone.get_or_insert(0.);
            }
            slot.held = slot.weapon;
            if slot.walker {
                continue;
            }
            let throwing = if slot.identity == Some(own) { own_throw } else { slot.throwing };
            // updateHeld: the grenade is the model in the hand for the
            // whole throw, gone once it has left the hand; the gun waits.
            if let Some((_, weapon, hidden)) = throwing {
                slot.held = if hidden { None } else { self.weapon_classes.get(weapon).copied().flatten() };
            }
            let primary = self.presented.weapons.get(slot.primary).copied().flatten().map_or(0, |(bank, _)| bank);
            let bank = self.presented.banks[throwing.map_or(primary, |(bank, _, _)| bank)].1;
            let actor = slot.actor;
            if self.units.actor_animation(actor).is_some_and(|animator| animator.bank != bank) {
                if let Some(row) = self.command(actor, kind::BANK) {
                    row[command::BANK] = bank as f64;
                }
            }
        }
    }
    // CombatPresentation.receive shows a record as it arrives and
    // CombatPresentation.present asks again, each frame, for the ones whose
    // unit this client did not know yet, until they expire.
    pub(super) fn present_events(&mut self, receiver: &Receiver, tick: f64) {
        for field in receiver.publications.iter() {
            if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
                continue;
            }
            let Ok(source) = serde_json::from_slice::<Input>(&field.bytes[..field.length]) else {
                continue;
            };
            let Some(act) = self.presented.decode(&source) else {
                continue;
            };
            // presentEvent: a blast or a blow-apart naming nobody shows
            // nothing, and is not asked for again.
            if source.owner_id == 0 && matches!(act, Act::DeathBlast | Act::BlowApart { .. } | Act::ShieldHit { .. }) {
                continue;
            }
            let event = Event {
                owner: (source.owner_id, source.owner_generation),
                expiry: field.row[r::TICK] + EXPIRY,
                seed: field.row[r::ID] as u32,
                act,
            };
            if !self.present_event(&event) {
                if self.presented.pending.len() == PENDING {
                    self.presented.pending.remove(0);
                    self.refused += 1;
                }
                self.presented.pending.push(event);
            }
        }
        // The kept ones are asked again after the new ones, oldest first,
        // and dropped once shown or expired.
        let mut at = 0;
        while at < self.presented.pending.len() {
            let event = self.presented.pending[at];
            if event.expiry < tick || self.present_event(&event) {
                self.presented.pending.remove(at);
            } else {
                at += 1;
            }
        }
    }
    // False while the record's unit is not one of the bodies drawn: the
    // browser's `!u || u.generation !== event.ownerGeneration` and its
    // `!combat` (a hull has no animator).
    fn present_event(&mut self, event: &Event) -> bool {
        let Some(&slot) = self.bound.get(&event.owner) else {
            return false;
        };
        let actor = self.slots[slot].actor;
        let model = self.slots[slot].model;
        match event.act {
            // A droideka's animator ignores weapon actions (src/droideka.js).
            Act::Weapon { .. } | Act::EndWeapon { .. } | Act::Shot { .. } if self.slots[slot].walker => {}
            Act::Weapon { action, bank, posture } => {
                // setBank first: changing the bank drops the action under way.
                let animator_bank = self.presented.banks[bank].1;
                if let Some(row) = self.command(actor, kind::BANK) {
                    row[command::BANK] = animator_bank as f64;
                }
                let clip = self.models[model].weapon_clips[bank][posture][action];
                if let Some(row) = self.command(actor, kind::WEAPON) {
                    row[command::ACTION] = action as f64;
                    row[command::CLIP] = clip;
                    row[command::SHOOTING] = ACTIONS[action].starts_with("shoot") as u8 as f64;
                }
                self.presented.shown[0] += 1;
            }
            Act::EndWeapon { action } => {
                if let Some(row) = self.command(actor, kind::END_WEAPON) {
                    row[command::ACTION] = action.map_or(-1., |action| action as f64);
                }
                self.presented.shown[1] += 1;
            }
            // presentEntityEvent "shot" on foot. A repair tick holds the alert
            // stance (presentRepairTick); a round leaving a gun plays the
            // bank's shoot clip at the body's posture and raises the stance
            // (presentProjectileShot; a thrown weapon's clip came with its
            // own weaponAction); a melee swing raises the stance.
            Act::Shot { weapon, repair, melee } => {
                let (bank, thrown) = self.presented.weapons[weapon].unwrap_or_default();
                if !repair && !melee && !thrown {
                    let posture = (self.units.output[actor * o::STRIDE + o::POSTURE] as usize).min(POSTURES.len() - 1);
                    let clip = self.models[model].weapon_clips[bank][posture][2];
                    if let Some(row) = self.command(actor, kind::WEAPON) {
                        row[command::ACTION] = 2.;
                        row[command::CLIP] = clip;
                        row[command::SHOOTING] = 1.;
                    }
                }
                if repair || melee || !thrown {
                    self.command(actor, kind::FIRE);
                }
                // combat.js flash: each round restarts its gun's flash.
                if !repair && !melee {
                    let (length, color) = self.guns.flashes.get(weapon).copied().unwrap_or((0.1, [1.; 4]));
                    self.slots[slot].flash = (weapon, self.tick.floor(), length, color);
                }
                self.presented.shown[4] += 1;
            }
            // game.js presentEvent "deathBlast": one of the three blasts at
            // a bone of the body, both picked at random. The bone is where
            // the last frame posed it, and the effect starts with the next
            // frame's batch, one frame after the browser's.
            Act::DeathBlast => {
                let bones = self.models[model].inverses.len();
                let bone = (self.presented.rng.uniform() * bones as f64) as usize;
                let blast = self.presented.blasts[(self.presented.rng.uniform() * BLASTS.len() as f64) as usize];
                let world = (self.units.actor_node_start(actor) + bone + 2) * node::STRIDE + node::WORLD;
                let position = [self.units.nodes[world + 12], self.units.nodes[world + 13], self.units.nodes[world + 14]];
                self.combat.present_effect(blast, position, event.seed);
                self.presented.shown[2] += 1;
            }
            // game.js presentEvent "shieldHit": the droideka's bubble flares
            // where the hit landed. A unit without a bubble shows nothing.
            Act::ShieldHit { amount, source, explosion } => {
                if let Some(bubble) = self.slots[slot].bubble {
                    let facts = self.shield_facts(slot);
                    self.shields.hit(bubble, &facts, amount, source, explosion);
                    self.presented.shown[6] += 1;
                }
            }
            // game.js presentEvent "blowApart": Debris.blowApart sends each
            // of the class's chunks flying from its bone, away from the
            // body's origin, then Unit.goUp: nothing more of this life is
            // drawn. Origin and velocity are the drawn body's, the same pose
            // the bones are read from.
            Act::BlowApart { health } => {
                let class = self.slots[slot].class;
                let out = &self.units.output[actor * o::STRIDE..];
                let origin: [f64; 3] = std::array::from_fn(|axis| out[o::POSITION + axis]);
                let velocity: [f64; 3] = std::array::from_fn(|axis| out[o::VELOCITY + axis]);
                let start = self.units.actor_node_start(actor);
                for part in 0..self.combat.debris.parts(class).len() {
                    let name = &self.combat.debris.parts(class)[part].node;
                    let Some(bone) = self.models[model].bones.iter().rposition(|bone| bone == name) else {
                        continue;
                    };
                    let world = (start + bone + 2) * node::STRIDE + node::WORLD;
                    let mut matrix = three::Matrix4::default();
                    matrix.elements.copy_from_slice(&self.units.nodes[world..world + 16]);
                    if !self.combat.debris.blow_apart(class, part, matrix, origin, velocity, health) {
                        self.refused += 1;
                    }
                }
                self.slots[slot].gone.get_or_insert(0.);
                self.presented.shown[3] += 1;
            }
        }
        true
    }
    // One unit command for an actor, or None when this frame's batch is full.
    fn command(&mut self, actor: usize, operation: u32) -> Option<&mut [f64]> {
        let at = self.units.input[f::COMMANDS] as usize * command::STRIDE;
        if at + command::STRIDE > self.units.commands.len() {
            self.refused += 1;
            return None;
        }
        self.units.input[f::COMMANDS] += 1.;
        let row = &mut self.units.commands[at..at + command::STRIDE];
        row.fill(0.);
        row[command::ACTOR] = actor as f64;
        row[command::KIND] = operation as f64;
        Some(row)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn sets() -> Value {
        bfe_content::json::parse_bytes(
            br#"{"banks":{"rifle":{"weapon":{"stand":{"reload":"human_rifle_stand_reload"}}},
            "pistol":{"weapon":{"crouch":{"shoot":"human_pistol_crouch_fire"}}},"grenade":{}}}"#,
        )
        .unwrap()
    }
    fn clips() -> Vec<(String, f64)> {
        ["human_rifle_stand_reload", "human_rifle_stand_shoot", "human_pistol_crouch_fire", "human_pistol_stand_shoot", "human_grenade_stand_shoot2"]
            .map(|name| (name.to_string(), 30.))
            .to_vec()
    }
    #[test]
    fn banks_list_the_sets_then_other_weapon_banks_as_the_rifle() {
        let sets = sets();
        let presented = Presented::new(Some(&sets), [(1, "pistol", "cannon"), (2, "bazooka", "launcher"), (3, "grenade", "grenade")].into_iter(), 3, &[]);
        let names: Vec<_> = presented.banks.iter().map(|(name, bank)| (name.as_str(), *bank)).collect();
        assert_eq!(names, [("rifle", 0), ("pistol", 1), ("grenade", 2), ("bazooka", 0)]);
        assert_eq!(presented.weapons, [None, Some((1, false)), Some((3, false)), Some((2, true))]);
    }
    #[test]
    fn weapon_clips_follow_the_animator_fallbacks() {
        let sets = sets();
        let presented = Presented::new(Some(&sets), [(1, "rifle", "cannon")].into_iter(), 1, &[]);
        let table = presented.clips(Some(&sets), &clips());
        // The bank's named clip, then the rifle's name for the same action.
        assert_eq!(table[1][1][2], 2.);
        assert_eq!(table[1][0][0], 0.);
        // `human_<bank>_<posture>_<action>`, then the rifle's clip.
        assert_eq!(table[1][0][2], 3.);
        assert_eq!(table[2][0][2], 1.);
        // The grenade bank throws from any posture with its standing clip.
        assert_eq!(table[2][2][3], 4.);
        assert_eq!(table[0][2][3], -1.);
    }
    #[test]
    fn unit_states_name_the_throw_and_the_blown_apart_flag() {
        // id 7, flags blowApart|targetable, no roster slot, primary 2 and
        // secondary 3 selected, two weapons, no repair lock.
        let mut payload = vec![7, 0, 0, 0, 8 | 2];
        payload.extend([0u8; 8]);
        payload.extend([255, 255, 2, 0, 3, 0, 2, 0]);
        // Weapon 2 idle with heat; weapon 3 0.25 s into its throw, locked.
        payload.extend([2, 0, 0, 0, 1]);
        payload.extend(0.5f32.to_le_bytes());
        payload.extend([3, 0, 0, 0, 8 | 128]);
        payload.extend(0.25f32.to_le_bytes());
        payload.extend([9, 0, 0, 0, 1, 0]);
        assert_eq!(unit_state(&payload), Some((7, true, Some((3, false)))));
        // Its `hidden` flag says the grenade has left the hand.
        let bits = payload.len() - 12;
        payload[bits] = 4;
        assert_eq!(unit_state(&payload), Some((7, true, Some((3, true)))));
        // A throw of a weapon that is not the selected secondary shows nothing.
        payload[17] = 4;
        assert_eq!(unit_state(&payload), Some((7, true, None)));
        assert_eq!(unit_state(&payload[..payload.len() - 1]), None);
    }
    #[test]
    fn records_decode_to_validated_actions() {
        let sets = sets();
        let presented = Presented::new(Some(&sets), [(1, "rifle", "cannon")].into_iter(), 1, &[]);
        let read = |json: &str| presented.decode(&serde_json::from_str::<Input>(json).unwrap());
        assert!(matches!(
            read(r#"{"kind":"weaponAction","action":"shoot2","bank":"grenade","posture":"prone","prepare":true,"ownerId":4,"ownerGeneration":2}"#),
            Some(Act::Weapon { action: 3, bank: 2, posture: 2 })
        ));
        assert!(matches!(read(r#"{"kind":"weaponAction","action":"shoot","bank":"sword","posture":"stand"}"#), Some(Act::Weapon { bank: 0, .. })));
        assert!(read(r#"{"kind":"weaponAction","action":"wave","bank":"rifle","posture":"stand"}"#).is_none());
        assert!(read(r#"{"kind":"weaponAction","action":"shoot","bank":"rifle"}"#).is_none());
        assert!(matches!(read(r#"{"kind":"endWeaponAction","action":null,"ownerId":4}"#), Some(Act::EndWeapon { action: None })));
        assert!(matches!(read(r#"{"kind":"endWeaponAction","action":"reload"}"#), Some(Act::EndWeapon { action: Some(0) })));
        assert!(matches!(
            read(r#"{"kind":"shieldHit","ownerId":5,"ownerGeneration":1,"amount":40,"source":[1,2,3],"hitKind":"explosion"}"#),
            Some(Act::ShieldHit { amount, source: Some([1., 2., 3.]), explosion: true }) if amount == 40.
        ));
        assert!(matches!(
            read(r#"{"kind":"shieldHit","ownerId":5,"amount":12,"source":null,"hitKind":null}"#),
            Some(Act::ShieldHit { source: None, explosion: false, .. })
        ));
        assert!(matches!(read(r#"{"kind":"blowApart","amount":-50,"ownerId":9,"ownerGeneration":1,"source":null,"hitKind":null}"#), Some(Act::BlowApart { health }) if health == -50.));
        assert!(matches!(
            read(r#"{"kind":"shot","ownerId":9,"ownerGeneration":1,"mountId":0,"weaponId":1,"repair":false,"melee":true,"position":null}"#),
            Some(Act::Shot { weapon: 1, repair: false, melee: true })
        ));
        assert!(read(r#"{"kind":"shot","ownerId":9,"mountId":4,"weaponId":1}"#).is_none());
        assert!(read(r#"{"kind":"shot","ownerId":9,"weaponId":7}"#).is_none());
    }
}
