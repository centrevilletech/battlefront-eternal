//! Private infantry and mounted weapon replay. The page supplies commands
//! and mirrors; this owner retains weapons, selections and barrel counters.
//! No gameplay or presentation callback runs inside a command.

use crate::combat::defs::{Js, JsObj, WeaponClass, WeaponDefs, K};
use crate::combat::weapons::{
    Admission, Aim, Arsenal, ArsenalInput, Shot, ShotKind, Trigger, WeaponDraws,
};
use crate::gen::{Posture, Weapon, ClientValueKind, ClientWeapons, ClientArsenalTail, ClientBarrelResult};
use crate::store::DefId;
use jsrt::conv::{js_max, js_mod};
use std::rc::Rc;

const INVALID: &str = "invalid prediction weapon command";

struct Middle;
impl WeaponDraws for Middle {
    fn draw(&mut self, _: u32) -> f64 {
        0.5
    }
    fn legacy(&mut self, _: u32) -> f64 {
        0.5
    }
    fn seeded(&mut self, _: f64, _: f64) {}
    fn admit(&mut self, _: u32) -> Admission {
        Admission::Yes
    }
}

struct Reader<'a> {
    words: &'a [f64],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(words: &'a [f64]) -> Self {
        Self { words, at: 0 }
    }
    fn word(&mut self) -> Result<f64, &'static str> {
        let v = *self.words.get(self.at).ok_or(INVALID)?;
        self.at += 1;
        Ok(v)
    }
    fn count(&mut self) -> Result<usize, &'static str> {
        let v = self.word()?;
        if !v.is_finite() || v < 0.0 || v.fract() != 0.0 || v > u32::MAX as f64 {
            return Err(INVALID);
        }
        Ok(v as usize)
    }
    fn flag(&mut self) -> Result<bool, &'static str> {
        Ok(self.word()? != 0.0)
    }
    fn text(&mut self) -> Result<Rc<str>, &'static str> {
        let n = self.count()?;
        if n > self.words.len() - self.at {
            return Err(INVALID);
        }
        let mut chars = Vec::with_capacity(n);
        for _ in 0..n {
            chars.push(self.count()?.try_into().map_err(|_| INVALID)?);
        }
        Ok(Rc::from(String::from_utf16_lossy(&chars)))
    }
    fn value(&mut self, depth: usize) -> Result<Js, &'static str> {
        if depth > 16 {
            return Err(INVALID);
        }
        let tag = u8::try_from(self.count()?).ok().and_then(ClientValueKind::from_u8).ok_or(INVALID)?;
        Ok(match tag {
            ClientValueKind::Undefined => Js::Undefined,
            ClientValueKind::Null => Js::Null,
            ClientValueKind::False => Js::Bool(false),
            ClientValueKind::True => Js::Bool(true),
            ClientValueKind::Number => Js::Num(self.word()?),
            ClientValueKind::String => Js::Str(self.text()?),
            ClientValueKind::Array => {
                let n = self.count()?;
                if n > self.words.len() - self.at {
                    return Err(INVALID);
                }
                let mut values = Vec::with_capacity(n);
                for _ in 0..n {
                    values.push(self.value(depth + 1)?);
                }
                Js::Arr(Rc::new(values))
            }
            ClientValueKind::Object => {
                let n = self.count()?;
                if n > self.words.len() - self.at {
                    return Err(INVALID);
                }
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    fields.push((self.text()?, self.value(depth + 1)?));
                }
                Js::Obj(Rc::new(JsObj { fields, def: None }))
            }
        })
    }
    fn done(&self) -> Result<(), &'static str> {
        if self.at == self.words.len() {
            Ok(())
        } else {
            Err(INVALID)
        }
    }
}

/// The scalar protocol used by weapon-prediction-sim.js and native twins.
/// 0: definition id, recursive Js value (tags 0..7; strings as UTF-16).
/// 1: weapon count and generated Weapon rows, selections, zoom, barrel count/values.
/// 2: id, generation, primary/secondary pressed+held, reload, selection actions,
/// zoom action, forward/right, posture, air, dive, lock, rolled, underwater,
/// then each press's presence/value.
/// 3: entry count, then weapon/owner slots, advance/tow flags, pressed/held/reload.
/// 4: owner slot, current barrel, shot count; returns the advanced barrel.
/// Commands 1..3 return rows, selections, zoom, barrels with write flags,
/// then ordered shots (weapon, kind, yaw, pitch, charge, target, generation, swing).
#[derive(Default)]
pub struct Session {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    pub(crate) defs: WeaponDefs,
    pub(crate) weapons: Vec<Weapon>,
    pub(crate) selected: [f64; 2],
    pub(crate) zoom: f64,
    barrels: Vec<f64>,
    barrel_writes: Vec<bool>,
    pub(crate) shots: Vec<Shot>,
}

impl Session {
    // Reservations belong to native resource admission; repeated corrections
    // refill the same arsenal, barrel and ordered-shot storage.
    pub(crate) fn reserve_native(&mut self, weapons: usize) {
        self.input.reserve(16384); self.output.reserve(32768);
        self.weapons.reserve(weapons); self.barrels.reserve(128);
        self.barrel_writes.reserve(128); self.shots.reserve(1024);
    }
    pub fn run(&mut self, op: u32) -> Result<(), &'static str> {
        self.output.clear();
        self.shots.clear();
        self.barrel_writes.fill(false);
        let mut r = Reader::new(&self.input);
        match op {
            0 => {
                let id = DefId(r.count()? as u32);
                let Js::Obj(def) = r.value(0)? else {
                    return Err(INVALID);
                };
                r.done()?;
                self.defs.set(id, WeaponClass::new(def));
                return Ok(());
            }
            1 => {
                let n = r.count()?;
                if n > (r.words.len() - r.at) / Weapon::WORDS {
                    return Err(INVALID);
                }
                let rows = r.at;
                for _ in 0..n {
                    let w = Weapon::take(&r.words[r.at..r.at + Weapon::WORDS], &[]);
                    r.at += Weapon::WORDS;
                    if !self.defs.has(w.def)
                        || !w.level.is_finite()
                        || w.level < 0.0
                        || w.level.fract() != 0.0
                        || w.level >= self.defs.class(w.def).levels.len() as f64
                    {
                        return Err(INVALID);
                    }
                }
                let selected = [r.word()?, r.word()?];
                let zoom = r.word()?;
                let count = r.count()?;
                if count > r.words.len() - r.at {
                    return Err(INVALID);
                }
                let barrels = r.at;
                for _ in 0..count { r.word()?; }
                r.done()?;
                // Validate before replacing state, then refill the retained
                // storage instead of allocating it on each correction.
                self.weapons.clear();
                for row in r.words[rows..rows + n * Weapon::WORDS].chunks_exact(Weapon::WORDS) {
                    self.weapons.push(Weapon::take(row, &[]));
                }
                self.selected = selected;
                self.zoom = zoom;
                self.barrels.clear();
                self.barrels.extend_from_slice(&r.words[barrels..barrels + count]);
                self.barrel_writes.resize(count, false);
                self.barrel_writes.fill(false);
            }
            2 => {
                let id = r.word()?;
                let generation = r.word()?;
                let primary = Trigger {
                    pressed: r.flag()?,
                    held: r.flag()?,
                    ..Trigger::default()
                };
                let secondary = Trigger {
                    pressed: r.flag()?,
                    held: r.flag()?,
                    ..Trigger::default()
                };
                let mut input = ArsenalInput {
                    primary,
                    secondary,
                    reload: r.flag()?,
                };
                let next = r.flag()?;
                let previous = r.flag()?;
                let next_secondary = r.flag()?;
                let zoom = r.flag()?;
                let stick = js_max(r.word()?.abs(), r.word()?.abs());
                let posture = match r.word()? {
                    0.0 => Some(Posture::Stand),
                    1.0 => Some(Posture::Crouch),
                    2.0 => Some(Posture::Prone),
                    _ => None,
                };
                let air = r.word()?;
                let dive = r.flag()?;
                let lock = r.word()?;
                let rolled = r.flag()?;
                let underwater = r.flag()?;
                for (channel, trigger) in [&mut input.primary, &mut input.secondary]
                    .into_iter()
                    .enumerate()
                {
                    let has = r.flag()?;
                    let press = r.word()?;
                    trigger.held |= trigger.pressed;
                    if has && trigger.pressed {
                        trigger.seed = Some(jsrt::rng::spread_seed_f64(
                            id,
                            generation,
                            channel as f64,
                            press,
                        ));
                    }
                }
                r.done()?;
                let airborne = air >= 0.0 && air != 3.0;
                let blocked = airborne
                    || dive
                    || lock > 0.0
                    || rolled
                    || (posture == Some(Posture::Prone) && stick > 0.1)
                    || underwater;
                let mut arsenal = Arsenal {
                    weapons: &mut self.weapons,
                    selected: &mut self.selected,
                };
                if next && arsenal.next(&self.defs, 0, 1.0) {
                    self.zoom = 0.0;
                }
                if previous && arsenal.next(&self.defs, 0, -1.0) {
                    self.zoom = 0.0;
                }
                if next_secondary {
                    arsenal.next(&self.defs, 1, 1.0);
                }
                let primary = arsenal.current(&self.defs, 0);
                if zoom {
                    if let Some(index) = primary {
                        let weapon = &arsenal.weapons[index];
                        let def = weapon.level(self.defs.class(weapon.def));
                        let mut last = 1.0;
                        let mut count = 1.0;
                        for value in [
                            def.get(K::ZoomMin).or(1.0),
                            if def.get(K::ZoomRate).truthy() {
                                0.0
                            } else {
                                def.get(K::ZoomMax).or(1.0)
                            },
                        ] {
                            if value > last {
                                last = value;
                                count += 1.0;
                            }
                        }
                        self.zoom = js_mod(self.zoom + 1.0, count);
                    }
                }
                if airborne
                    || dive
                    || primary.is_some_and(|index| arsenal.weapons[index].reloading())
                {
                    self.zoom = 0.0;
                }
                for (channel, trigger) in [&mut input.primary, &mut input.secondary]
                    .into_iter()
                    .enumerate()
                {
                    if let Some(index) = arsenal.current(&self.defs, channel) {
                        let weapon = &arsenal.weapons[index];
                        let def = weapon.level(self.defs.class(weapon.def));
                        if (weapon.deployed && def.get(K::Droid).truthy())
                            || def.get(K::Base).is_str("shield")
                        {
                            *trigger = Trigger::default();
                        }
                    }
                }
                let aim = Aim {
                    stick,
                    posture,
                    zoomed: self.zoom > 0.0,
                    repair_target: false,
                };
                arsenal.update(
                    &self.defs,
                    1.0 / 60.0,
                    &input,
                    &aim,
                    blocked,
                    &mut Middle,
                    &mut self.shots,
                );
            }
            3 => {
                let n = r.count()?;
                let entries = r.at;
                for _ in 0..n {
                    let weapon = r.count()?;
                    let owner = r.count()?;
                    for _ in 0..5 { r.flag()?; }
                    if weapon >= self.weapons.len() || owner >= self.barrels.len() {
                        return Err(INVALID);
                    }

                }
                r.done()?;
                let aim = Aim {
                    stick: 0.0,
                    posture: Some(Posture::Stand),
                    zoomed: false,
                    repair_target: false,
                };
                for row in self.input[entries..entries + n * 7].chunks_exact(7) {
                    let (index, owner) = (row[0] as usize, row[1] as usize);
                    let (advance, tow, pressed, held, reload) = (row[2] != 0.0, row[3] != 0.0, row[4] != 0.0, row[5] != 0.0, row[6] != 0.0);
                    let weapon = &mut self.weapons[index];
                    let random = weapon.random;
                    let admission = weapon.admit_shot;
                    weapon.random = true;
                    weapon.admit_shot = false;
                    let before = self.shots.len();
                    let trigger = Trigger {
                        pressed: !tow && pressed,
                        held: !tow && (pressed || held),
                        ..Trigger::default()
                    };
                    weapon.update(
                        index as u32,
                        self.defs.class(weapon.def),
                        1.0 / 60.0,
                        true,
                        &trigger,
                        reload,
                        &aim,
                        &mut Middle,
                        &mut self.shots,
                    );
                    weapon.random = random;
                    weapon.admit_shot = admission;
                    if advance {
                        self.barrels[owner] += (self.shots.len() - before) as f64;
                        self.barrel_writes[owner] = true;
                    }
                }
            }
            4 => {
                let owner = r.count()?;
                let barrel = r.word()?;
                let count = r.word()?;
                r.done()?;
                if owner >= self.barrels.len() {
                    return Err(INVALID);
                }
                self.barrels[owner] = barrel + count;
                self.output.push(self.barrels[owner]);
                return Ok(());
            }
            _ => return Err(INVALID),
        }
        self.write();
        Ok(())
    }

    fn write(&mut self) {
        let count = ClientWeapons { count: self.weapons.len() as u32 };
        self.output.resize(ClientWeapons::WORDS, 0.0);
        count.put(&mut self.output, &mut Vec::new());
        for weapon in &self.weapons {
            let at = self.output.len();
            self.output.resize(at + Weapon::WORDS, 0.0);
            weapon.put(&mut self.output[at..], &mut Vec::new());
        }
        // client-prediction.js reads the same generated selection/zoom tail.
        // Preserve the old Session.write word order for frozen checkpoints.
        let tail = ClientArsenalTail { primary: self.selected[0], secondary: self.selected[1],
            zoom_step: self.zoom, barrels_count: self.barrels.len() as u32 };
        let at = self.output.len(); self.output.resize(at + ClientArsenalTail::WORDS, 0.0);
        tail.put(&mut self.output[at..], &mut Vec::new());
        for (value, written) in self.barrels.iter().zip(&self.barrel_writes) {
            let row = ClientBarrelResult { barrel: *value, written: *written };
            let at = self.output.len(); self.output.resize(at + ClientBarrelResult::WORDS, 0.0);
            row.put(&mut self.output[at..], &mut Vec::new());
        }
        self.output.push(self.shots.len() as f64);
        for shot in &self.shots {
            let mut out = [0.0; 8];
            out[0] = shot.weapon as f64;
            match shot.kind {
                ShotKind::Round {
                    yaw,
                    pitch,
                    charge,
                    target_id,
                    target_generation,
                    ..
                } => {
                    out[2] = yaw;
                    out[3] = pitch;
                    out[4] = charge;
                    out[5] = target_id;
                    out[6] = target_generation;
                }
                ShotKind::Repair => out[1] = 1.0,
                ShotKind::MeleeStart { swing } => {
                    out[1] = 2.0;
                    out[7] = swing;
                }
                ShotKind::MeleeHit => out[1] = 3.0,
                ShotKind::MeleeEnd => out[1] = 4.0,
            }
            self.output.extend(out);
        }
    }
}

// ClientPrediction shares the existing cold tagged-definition reader with
// recon preparation; mutable frames never parse authored property trees.
pub(crate) fn read_definition(words: &[f64]) -> Result<Js, &'static str> {
    let mut reader = Reader::new(words);
    let value = reader.value(0)?;
    reader.done()?;
    Ok(value)
}
