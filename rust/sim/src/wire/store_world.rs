//! captureUnitState reads canonical F3 bodies and their complete arsenals at
//! the world's publication tick. Presentation never samples a body facade.
use crate::{gen::{Member, Store, UnitBody}, store::Strings};
use super::{codec::Result, presentation, stagger::Stagger, store_capture::Capture};
use crate::combat::defs::WeaponDefs;

pub struct Units { payload: Vec<u8>, repair: Vec<u8> }
impl Default for Units {
    fn default() -> Self { Self { payload: Vec::with_capacity(617), repair: Vec::with_capacity(512) } }
}
impl Units {
    /// The stagger asks before any weapon or string work. Removed bodies do
    /// not offer schema 6, matching captureSnapshot's omission and retention.
    pub fn take(&mut self, tick: f64, store: &Store, strings: &Strings, defs: &WeaponDefs,
        maps: &Capture, stagger: &mut Stagger,
        visible: impl Fn(crate::store::Ref<crate::gen::Unit>, &crate::gen::Unit) -> bool) -> Result<()> {
        for member in &store.units.list {
            let Member::Unit(reference) = *member else { continue };
            let Some(unit) = store.unit.get(reference).filter(|u| !u.removed) else { continue };
            let id = 100000. + reference.slot as f64;
            if !stagger.due(tick, 6., id) { continue; }
            // captureUnitState omits removed registry lives but keeps dead bodies.
            // The roster field uses the Character's stable slot, not this loop index.
            // Visibility is the native owning phase's predicate at this completed tick.
            let combat = &unit.combat;
            if combat.weapons.len() > 12 { return Err("Unit weapon count exceeds presentation limit"); }
            // The nine header words and twelve words per weapon are internal
            // arguments to the shared writer, never another boundary layout.
            let mut words = [0.; presentation::UNIT_HEADER + 12 * presentation::WEAPON_WORDS];
            words[0] = reference.slot as f64;
            words[1] = (u8::from(unit.hero) | (u8::from(unit.targetable.unwrap_or(false)) << 1)
                | (u8::from(visible(reference, unit)) << 2) | (u8::from(combat.blow_apart) << 3)) as f64;
            words[2] = jsrt::num_or(unit.dead_time, 0.);
            words[3] = jsrt::num_or(match &unit.soldier { UnitBody::Soldier(s) => s.state_time, UnitBody::Droideka(d) => d.state_time }, 0.);
            words[4] = unit.character.and_then(|r| store.character.get(r)).and_then(|c| c.roster_slot).unwrap_or(65535.);
            for channel in 0..2 {
                words[5 + channel] = super::store_capture::current(&combat.weapons, combat.selected[channel], channel as f64, defs)
                    .and_then(|weapon| maps.weapon_id(weapon.def.0)).unwrap_or(0.);
            }
            words[7] = combat.repair_lock.0 as f64; words[8] = combat.weapons.len() as f64;
            for (index, weapon) in combat.weapons.iter().enumerate() {
                let row = &mut words[presentation::UNIT_HEADER + index * presentation::WEAPON_WORDS..][..presentation::WEAPON_WORDS];
                row[0] = maps.weapon_id(weapon.def.0).ok_or("Unknown unit weapon")?; row[1] = weapon.level;
                row[2] = (u8::from(weapon.overheated) | u8::from(weapon.released) << 1 | u8::from(weapon.hidden) << 2
                    | u8::from(weapon.charging) << 3 | u8::from(weapon.lock_complete) << 4) as f64;
                row[3..10].copy_from_slice(&[weapon.heat, weapon.wait, weapon.reload_left, weapon.throw_time,
                    weapon.charge, weapon.held, weapon.lock_elapsed]);
                row[10] = weapon.lock_target_id; row[11] = weapon.lock_generation;
            }
            presentation::unit_payload_into(&words[..presentation::UNIT_HEADER + combat.weapons.len() * presentation::WEAPON_WORDS],
                strings, &mut self.payload, &mut self.repair).map_err(|error| match error {
                    presentation::Error::WeaponCount => "Unit weapon count exceeds presentation limit",
                    presentation::Error::RepairLength => "Repair identity too long",
                    presentation::Error::Weapon => "Unknown unit weapon",
                    _ => "Invalid unit presentation number",
                })?;
            stagger.offer_unit(tick, id, unit.generation, &self.payload)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct Building { pub turret: bool, pub state: u8, pub health: f64, pub team: f64, pub facing: f64, pub yaw: f64, pub station: Option<Station> }
#[derive(Clone, Copy)]
pub struct Station { pub active_mode: bool, pub active: f64, pub pulse: f64, pub served_health: bool, pub served_ammo: bool }
/// F4 supplies scalar views in authored order; the native rigs are never
/// advanced, copied or exported as a side effect of network capture.
pub trait Props {
    fn building_count(&self) -> usize;
    fn building(&self, index: usize) -> Option<Building>;
    fn animation_count(&self) -> usize;
    fn animation(&self, index: usize) -> Option<[f64; 7]>;
    fn trap_count(&self) -> usize;
    fn trap(&self, index: usize) -> Option<[f64; 3]>;
}
// captureWorldPresentation encodes small JSON arrays in authored order.
// The token scratch is internal to the retained writer, not an ABI block.
// Its numeric formatter preserves JSON.stringify rounding and null rules.
struct Json { words: [f64; 64], count: usize }
impl Json {
    fn new(length: usize) -> Self { let mut words = [0.; 64]; words[0] = 4.; words[1] = length as f64; Self { words, count: 2 } }
    fn pair(&mut self, kind: f64, value: f64) { self.words[self.count] = kind; self.words[self.count + 1] = value; self.count += 2; }
    fn number(&mut self, value: f64) { self.pair(1., value); }
    fn boolean(&mut self, value: bool) { self.pair(2., u8::from(value) as f64); }
    fn string(&mut self, value: crate::store::StrId) { self.pair(3., value.0 as f64); }
    fn null(&mut self) { self.words[self.count] = 0.; self.count += 1; }
    // captureWorldPresentation writes into one retained 900-byte payload.
    // A formatter overflow faults before any stagger field is replaced.
    fn bytes(&self, strings: &Strings, out: &mut Vec<u8>) -> Result<()> {
        presentation::world_payload_into(&self.words[..self.count], strings, out).map_err(|_| "World presentation record too large")
    }
}
pub struct WorldFields { payload: Vec<u8> }
impl Default for WorldFields { fn default() -> Self { Self { payload: Vec::with_capacity(900) } } }
impl WorldFields {
    /// captureWorldPresentation's JSON record order and held checks. The
    /// native rows include cold affiliation and headings alongside health,
    /// station clocks, grabs, traps and tethers. No prop value is imported.
    pub fn take(&mut self, tick: f64, epoch: f64, store: &Store, strings: &Strings,
        props: &impl Props, names: [crate::store::StrId; 5], stagger: &mut Stagger) -> Result<()> {
        for index in 0..props.building_count() {
            let Some(building) = props.building(index).filter(|b| !b.turret) else { continue };
            let name = names.get(building.state as usize).copied().ok_or("invalid canonical building state")?;
            let team = jsrt::num_or(building.team, 0.);
            // JSON normalizes both signed zeros to 0, and all nonfinite team
            // numbers to null. The immediate-change identity does the same.
            let key = (building.state, if team == 0. { 0 } else if team.is_finite() { team.to_bits() } else { u64::MAX });
            let id = 200000. + index as f64;
            if !stagger.needs_native(tick, 7., id, Some(key)) { continue; }
            let mut json = Json::new(11); json.number(index as f64); json.string(name); json.number(building.health); json.number(team);
            if let Some(station) = building.station { json.string(names[if station.active_mode { 4 } else { 3 }]); }
            else { json.null(); }
            json.number(building.station.map_or(0., |s| jsrt::num_or(s.active, 0.)));
            json.number(building.station.map_or(0., |s| jsrt::num_or(s.pulse, 0.)));
            json.number(jsrt::num_or(building.facing, 0.)); json.number(jsrt::num_or(building.yaw, 0.));
            json.boolean(building.station.is_some_and(|s| s.served_health)); json.boolean(building.station.is_some_and(|s| s.served_ammo));
            json.bytes(strings, &mut self.payload)?; stagger.offer_native(tick, 7., id, epoch, &self.payload, Some(key))?;
        }
        // captureWorldPresentation rounds creature time with toFixed(4), then
        // converts it back to a JSON number. Binary rounding followed by a decimal
        // formatter is observably different at ties and for signed zero.
        for index in 0..props.animation_count() {
            let Some(mut state) = props.animation(index) else { continue };
            state[2] = jsrt::numfmt::fixed_four_number(state[2]).ok_or("invalid creature publication clock")?;
            let mut json = Json::new(8); json.number(index as f64); for value in state { json.number(value); }
            json.bytes(strings, &mut self.payload)?;
            stagger.offer_native(tick, 9., 400000. + index as f64, epoch, &self.payload, None)?;
        }
        // captureWorldPresentation places traps after creature animation rows.
        // Their schema has its own stagger clock and retained insertion order.
        // A short publication cannot shift ids of authored traps that still exist.
        for index in 0..props.trap_count() {
            let Some(state) = props.trap(index) else { continue };
            let mut json = Json::new(4); json.number(index as f64); for value in state { json.number(value); }
            json.bytes(strings, &mut self.payload)?;
            stagger.offer_native(tick, 10., 500000. + index as f64, epoch, &self.payload, None)?;
        }
        // captureWorldPresentation keeps tows in canonical fleet order.
        // Only live source hulls offer rows; a dead target can still be referenced.
        // The target generation is serialized even though its current pose is not.
        for reference in &store.vehicles.vehicles {
            let Some(hull) = store.vehicle.get(*reference).filter(|v| v.alive) else { continue };
            let id = 300000. + hull.target_id;
            if !stagger.needs_native(tick, 8., id, None) { continue; }
            // A tow may name a dead hull still present in the fleet. Array
            // indexing rejects fractional, negative and NaN keys before cast.
            let target = if hull.tow_target.is_finite() && hull.tow_target >= 0. && hull.tow_target.fract() == 0. {
                store.vehicles.vehicles.get(hull.tow_target as usize).and_then(|r| store.vehicle.get(*r))
            } else { None };
            let mut json = Json::new(7); json.number(hull.target_id);
            json.number(target.map_or(0., |v| v.target_id)); json.number(target.map_or(0., |v| v.target_generation));
            json.pair(4., 3.); for value in hull.tow_local { json.number(value); }
            json.string(hull.tow_leg); json.number(jsrt::num_or(hull.tow_angle, 0.)); json.number(jsrt::num_or(hull.tow_winding, 0.));
            json.bytes(strings, &mut self.payload)?;
            stagger.offer_native(tick, 8., id, hull.target_generation, &self.payload, None)?;
        }
        Ok(())
    }
}
