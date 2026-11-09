//! Cold definitions and borrowed own-player weapon checkpoints. All replay
//! behavior remains in ClientPrediction; this host only packs generated rows.
use bfe_content::{defs::Defs, Value};
use bfe_sim::{
    client::ClientPrediction,
    gen::{ClientValueKind as Tag, Weapon},
    prediction::corrections::{own_weapons, restore_arsenal_weapon},
    store::DefId,
};
// client-prediction.js configure serializes authored definitions once. Preserve
// property order and IEEE values instead of passing them through JSON again.
pub(super) fn value(v: &Value, out: &mut Vec<f64>) {
    match v {
        Value::Null => out.push(Tag::Null as u8 as f64),
        Value::Bool(b) => out.push(if *b { Tag::True } else { Tag::False } as u8 as f64),
        Value::Number(n) => out.extend([Tag::Number as u8 as f64, *n]),
        Value::String(s) => {
            out.push(Tag::String as u8 as f64);
            text(s, out);
        }
        Value::Array(a) => {
            out.extend([Tag::Array as u8 as f64, a.len() as f64]);
            for v in a.iter() {
                value(v, out);
            }
        }
        Value::Object(o) => {
            out.push(Tag::Object as u8 as f64);
            let fields = o.entries().collect::<Vec<_>>();
            out.push(fields.len() as f64);
            for (k, v) in fields {
                text(k, out);
                value(v, out);
            }
        }
    }
}
fn text(s: &str, out: &mut Vec<f64>) {
    out.push(s.encode_utf16().count() as f64);
    out.extend(s.encode_utf16().map(|c| c as f64));
}
// Arsenal checkpoints contain only differences from each weapon's initial
// state. Keep those initial rows beside the class-to-slot table: restoring a
// checkpoint against another class's defaults would silently change ammo,
// heat and reload progress even when both classes name the same weapon.
// These records are prerequisites, not an independently advanced arsenal.
pub struct Arsenal {
    initial: Vec<Weapon>,
    classes: Vec<Vec<usize>>,
    pub rows: Vec<Weapon>,
    pub selected: [f64; 2],
    pub zoom: f64,
    pub kick: [f64; 3],
}
impl Arsenal {
    pub fn new(defs: &Defs, owner: &mut ClientPrediction) -> Result<Self, String> {
        let mut initial = Vec::new();
        let mut words = Vec::new();
        for (i, w) in defs.weapons.iter().enumerate() {
            words.clear();
            words.push((i + 1) as f64);
            value(&w.record, &mut words);
            owner.configure_weapon(&words).map_err(str::to_string)?;
            initial.push(owner.initial_weapon(DefId((i + 1) as u32)).ok_or("weapon definition not installed")?);
        }
        let mut classes = vec![vec![]; defs.registry.classes.len() + 1];
        for c in &defs.classes {
            if let Some(id) = c.registry_id {
                classes[id as usize] = c.weapons.clone();
            }
        }
        Ok(Self { initial, classes, rows: Vec::with_capacity(32), selected: [0.; 2], zoom: 0., kick: [0.; 3] })
    }
    // Wire ids identify weapon names, but class slots identify the actual
    // authored definition, including class-specific ammunition and upgrades.
    // restorePrediction validates the entire own-player envelope first. The
    // borrowed spans below then restore each weapon in authored slot order.
    // No JSON object, payload Vec or facade survives this observation.
    // The canonical correction parser supplies the defaults and field rules.
    pub fn restore(&mut self, bytes: &[u8]) -> Result<(), &'static str> {
        let (head, weapons) = own_weapons(bytes)?;
        let class = self.classes.get(head.class_id as usize).ok_or("unknown own arsenal class")?;
        self.rows.clear();
        for (slot, entry) in weapons.enumerate() {
            let entry = entry?;
            if slot >= 32 {
                return Err("own arsenal exceeds reservation");
            }
            let at = *class.get(slot).ok_or("own arsenal slot mismatch")?;
            let initial = self.initial.get(at).ok_or("missing own weapon definition")?;
            self.rows.push(restore_arsenal_weapon(entry.payload.get(bytes).ok_or("truncated own weapon")?, initial)?);
        }
        self.selected = head.selected.map(|n| n as f64);
        self.zoom = head.zoom_step as f64;
        self.kick = head.kick;
        Ok(())
    }
    pub fn write(&self, out: &mut Vec<f64>) -> (u32, u32) {
        let at = out.len();
        out.push(self.rows.len() as f64);
        let mut heap = Vec::new();
        for w in &self.rows {
            let row = out.len();
            out.resize(row + Weapon::WORDS, 0.);
            w.put(&mut out[row..], &mut heap);
        }
        out.extend([self.selected[0], self.selected[1], self.zoom, 0.]);
        (at as u32, (out.len() - at) as u32)
    }
}

// MountedWeaponPrediction resolves a lease once per display frame. Definitions
// are admitted while loading; correction spans supply mutable weapon state.
// A seat's weapon definition order is independent of its wire class id.
// Vehicle entries and armed buildings can share a weapon class but retain
// different mutable barrels and timers. Qualified leases select this table;
// a registry index alone cannot select the current occupant's continuation.
struct Seat {
    kind: u8,
    class: usize,
    index: usize,
    definitions: Vec<Weapon>,
    advance: Vec<bool>,
}
pub struct Mounted {
    seats: Vec<Seat>,
    pub rows: Vec<Weapon>,
    barrels: [f64; 2],
    advance: [bool; 2],
}
impl Mounted {
    pub fn new(defs: &Defs, owner: &mut ClientPrediction, only: Option<usize>) -> Result<Self, String> {
        let mut seats = Vec::new();
        let mut id = defs.weapons.len() + 1;
        let mut words = Vec::new();
        for vehicle in &defs.vehicles {
            let Some(class) = vehicle.registry_id else {
                continue;
            };
            if only.is_some_and(|wanted| wanted != class as usize) {
                id += vehicle.seats.iter().map(|s| s.weapons.iter().filter(|w| w.get("def").is_some()).count()).sum::<usize>();
                continue;
            }
            for (index, seat) in vehicle.seats.iter().enumerate() {
                let mut definitions = Vec::new();
                let mut advance = Vec::new();
                for weapon in &seat.weapons {
                    let Some(def) = weapon.get("def") else {
                        continue;
                    };
                    words.clear();
                    words.push(id as f64);
                    value(def, &mut words);
                    owner.configure_weapon(&words).map_err(str::to_string)?;
                    definitions.push(owner.initial_weapon(DefId(id as u32)).ok_or("missing mounted definition")?);
                    advance.push(weapon.get("muzzles").and_then(Value::as_array).is_some_and(|a| !a.is_empty()));
                    id += 1;
                }
                seats.push(Seat { kind: 3, class: class as usize, index, definitions, advance });
            }
        }
        if only.is_none() {
            for prop in &defs.props {
                if prop.base.map(|name| defs.names.text(name)) != Some("armedbuilding") {
                    continue;
                }
                let Some(def) = prop.record.get("weapon") else {
                    continue;
                };
                words.clear();
                words.push(id as f64);
                value(def, &mut words);
                owner.configure_weapon(&words).map_err(str::to_string)?;
                let weapon = owner.initial_weapon(DefId(id as u32)).ok_or("missing turret definition")?;
                id += 1;
                seats.push(Seat { kind: 4, class: prop.index, index: 0, definitions: vec![weapon], advance: vec![true] });
            }
        }
        Ok(Self { seats, rows: Vec::with_capacity(2), barrels: [0.; 2], advance: [false; 2] })
    }
    pub fn restore(&mut self, receiver: &super::receiver::Receiver) -> Result<(), &'static str> {
        use bfe_presentation::network_layout::{net_kind as k, net_record as r};
        self.rows.clear();
        let bytes = &receiver.checkpoint[..receiver.checkpoint_length];
        let Some(mount) = bfe_sim::prediction::corrections::own_mounted(bytes)? else {
            return Ok(());
        };
        let kind = if mount.mode == 3 { k::TURRET } else { k::VEHICLE };
        let Some(entity) = receiver
            .active()
            .find(|row| row[r::KIND] == kind as f64 && row[r::ID] == mount.id as f64 && row[r::GENERATION] == mount.generation as f64)
        else {
            return Ok(());
        };
        let class = entity[if mount.mode == 3 { r::BUILDING } else { r::CLASS_ID }] as usize;
        let Some(seat) = self
            .seats
            .iter()
            .find(|seat| seat.kind == if mount.mode == 3 { 4 } else { 3 } && seat.class == class && seat.index == mount.index as usize)
        else {
            return Ok(());
        };
        for (i, entry) in mount.weapons[..mount.weapon_count].iter().enumerate() {
            let initial = seat.definitions.get(i).ok_or("mounted arsenal slot mismatch")?;
            let payload = bytes.get(entry.offset..entry.offset + entry.length).ok_or("truncated mounted weapon")?;
            self.rows.push(restore_arsenal_weapon(payload, initial)?);
            self.barrels[i] = entry.barrel;
            self.advance[i] = seat.advance.get(i).copied().unwrap_or(false);
        }
        Ok(())
    }
    // MountedWeaponPrediction packs one arsenal and one mount row per gun.
    // Offsets refer to the retained frame bank, so the prediction owner can
    // borrow all weapons and commands in one call. Empty optional heaps here
    // hold no dynamic fields; generated Weapon and ClientMount are fixed rows.
    pub fn write(&self, out: &mut Vec<f64>, active: bool) -> (u32, u32, u32, u32) {
        // Landed craft keep their checkpoint rows for display while the
        // prediction packet omits them, preserving Vehicle.fireSeat's freeze.
        let rows = if active { self.rows.as_slice() } else { &[] };
        let at = out.len();
        out.push(rows.len() as f64);
        let mut heap = Vec::new();
        for weapon in rows {
            let at = out.len();
            out.resize(at + Weapon::WORDS, 0.);
            weapon.put(&mut out[at..], &mut heap);
        }
        out.extend([0., 0., 0., rows.len() as f64]);
        out.extend_from_slice(&self.barrels[..rows.len()]);
        let len = out.len() - at;
        let mounts = out.len();
        for i in 0..rows.len() {
            let at = out.len();
            out.resize(at + bfe_sim::gen::ClientMount::WORDS, 0.);
            bfe_sim::gen::ClientMount { advance: self.advance[i], tow: false }.put(&mut out[at..], &mut heap);
        }
        (at as u32, len as u32, mounts as u32, rows.len() as u32)
    }
}

#[cfg(test)]
mod mounted_display_tests {
    use super::*;

    #[test]
    fn frozen_prediction_packet_retains_checkpoint_weapons_for_the_hud() {
        let weapon = Weapon {
            clip: 0.75,
            timer: 0.25,
            ..Default::default()
        };
        let mounted = Mounted {
            seats: vec![],
            rows: vec![weapon.clone()],
            barrels: [0.; 2],
            advance: [true; 2],
        };
        let mut words = Vec::new();
        let (at, _, _, count) = mounted.write(&mut words, false);
        assert_eq!(words[at as usize], 0.);
        assert_eq!(count, 0);
        assert_eq!(mounted.rows, [weapon.clone()]);
        // Takeoff restores prediction from the same preserved checkpoint,
        // including heat/reload timers and ammunition shown while landed.
        words.clear();
        let (at, _, _, count) = mounted.write(&mut words, true);
        assert_eq!(count, 1);
        assert_eq!(Weapon::take(&words[at as usize + 1..], &[]), weapon);
    }
}
