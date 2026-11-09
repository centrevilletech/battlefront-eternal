//! Cold recon definitions from F3Recon.prepare and prepareStrike. A droid's
//! destruct weapon carries salvo geometry separately from its strike ordnance.
//! Id allocation stays with the enclosing admission's shared content tables.
use super::{Call, units::{call, error, weapon_levels}};
use crate::gen::*;
use bfe_content::{defs::identity, Value};
use super::super::{AdmitError, Stage};

type Calls = Vec<(Stage, &'static str, Call)>;

/// These are the same tables CombatSim and OrdnanceSim have already populated.
/// Reusing them avoids assigning different handles when two weapons share data.
pub trait Definitions {
    fn intern(&mut self, text: &str) -> StrId;
    fn weapon(&mut self, def: &Value) -> Result<DefId, AdmitError>;
    fn ordnance(&mut self, def: &Value) -> Result<DefId, AdmitError>;
    fn explosion(&mut self, def: &Value) -> Result<u32, AdmitError>;
    /// Observe each completed leaf before later lazy definitions can intern.
    fn emit(&mut self, _name: &'static str, _call: &Call) {}
}

/// weapon-content-sim.js valueWords, including depth 12 and JavaScript's own
/// property order. None is undefined; it must remain different from null.
pub fn value_words(value: Option<&Value>, intern: &mut impl FnMut(&str) -> StrId) -> Vec<f64> {
    fn write(value: Option<&Value>, depth: usize, intern: &mut impl FnMut(&str) -> StrId, out: &mut Vec<f64>) {
        match value {
            None => out.push(0.0), Some(Value::Null) => out.push(1.0),
            Some(Value::Bool(false)) => out.push(2.0), Some(Value::Bool(true)) => out.push(3.0),
            Some(Value::Number(n)) => out.extend([4.0,*n]),
            Some(Value::String(s)) => out.extend([5.0,intern(s).0 as f64]),
            Some(Value::Array(a)) if depth < 12 => {
                out.extend([6.0,a.len() as f64]);
                for v in a.iter() { write(Some(v),depth+1,intern,out); }
            }
            Some(Value::Object(o)) if depth < 12 => {
                out.extend([7.0,o.entries().count() as f64]);
                for (key,v) in o.entries() { out.push(intern(key).0 as f64); write(Some(v),depth+1,intern,out); }
            }
            Some(_) => out.push(9.0),
        }
    }
    let mut out = Vec::new(); write(value,0,intern,&mut out); out
}

/// First-use identity maps are retained across class preparation. Content
/// values use Rc identity, as the JS Map does, rather than class-name equality.
#[derive(Default)]
pub struct Classes { definitions: Vec<Value>, strikes: Vec<Value> }
impl Classes {
    pub fn prepare(&mut self, weapon: &Value, definitions: &mut impl Definitions,
        muzzle: Option<Vec3>) -> Result<Calls, AdmitError> {
        let mut calls = Vec::new();
        let Some(def) = weapon.get("droid").filter(|v| v.truthy()) else { return Ok(calls) };
        let def_identity = identity(def).ok_or_else(|| error("recon droid class is not an object"))?;
        let id = match self.definitions.iter().position(|known| identity(known) == Some(def_identity)) {
            Some(i) => i as u32,
            None => {
                let id = self.definitions.len() as u32; self.definitions.push(def.clone());
                let mut loadout = Vec::new();
                if let Some(guns) = def.get("weapons").filter(|v| v.truthy()) {
                    for gun in guns.as_array().ok_or_else(|| error("recon weapons are not an array"))? {
                        let weapon_id = definitions.weapon(gun)?; loadout.push(weapon_id);
                        for (index, level) in weapon_levels(gun)?.iter().enumerate() {
                            let ordnance = level.get("ordnance").filter(|v| !matches!(v, Value::Null))
                                .map(|v| definitions.ordnance(v)).transpose()?;
                            let strike = if let Some(strike) = level.get("strike").filter(|v| v.truthy()) {
                                let known = self.strikes.iter().position(|v| identity(v) == identity(level));
                                let id = if let Some(id) = known { id as u32 } else {
                                    let id = self.strikes.len() as u32;
                                    // Merged charge objects are fresh identities,
                                    // while a shared base gun keeps its strike id.
                                    self.strikes.push(level.clone());
                                    let ordnance = definitions.ordnance(strike)?;
                                    let value = value_words(Some(level), &mut |text| definitions.intern(text));
                                    let row = F3ReconStrike { id, ordnance, value };
                                    let encoded = call(F3ReconStrike::WORDS,|r,h| row.put(r,h));
                                    definitions.emit("f3_recon_strike", &encoded);
                                    calls.push((Stage::Units,"f3_recon_strike",encoded));
                                    id
                                };
                                Some(id)
                            } else { None };
                            let value = value_words(Some(level), &mut |text| definitions.intern(text));
                            let row = F3ReconLevel { weapon: weapon_id, level: index as u32, value, ordnance, strike };
                            let encoded = call(F3ReconLevel::WORDS,|r,h| row.put(r,h));
                            definitions.emit("f3_recon_level", &encoded);
                            calls.push((Stage::Units,"f3_recon_level",encoded));
                        }
                    }
                }
                // The class value includes its full authored weapons. The loadout
                // beside it contains the separate combat definition handles.
                let value = value_words(Some(def), &mut |text| definitions.intern(text));
                let explosion = def.get("explosion").filter(|v| v.truthy()).map(|v| definitions.explosion(v)).transpose()?;
                let row = F3ReconClass { id, value, loadout, explosion, muzzle: muzzle.unwrap_or([0.0;3]) };
                let encoded = call(F3ReconClass::WORDS,|r,h| row.put(r,h));
                definitions.emit("f3_recon_class", &encoded);
                calls.push((Stage::Units,"f3_recon_class",encoded));
                id
            }
        };
        // Different remote weapons may launch the same droid class and still
        // require independent weapon-to-class bindings.
        let row = F3ReconWeapon { weapon: definitions.weapon(weapon)?, class: id,
            immediate: weapon.get("SwitchImmediately").is_some_and(Value::truthy) };
        let encoded = call(F3ReconWeapon::WORDS,|r,h| row.put(r,h));
        definitions.emit("f3_recon_weapon", &encoded);
        calls.push((Stage::Units,"f3_recon_weapon",encoded));
        Ok(calls)
    }
}

/// F3Recon.bind initializes all command scratch fields. No private random draw
/// happens here: the caller supplies reconRandom's initial seed for this key.
pub fn binding(action: F3ReconAction, key: u32, owner: F3UnitKey, seed: u32) -> F3ReconBinding {
    F3ReconBinding { action, key, owner, seed, droid: None, dx: 0.0, dy: 0.0, sensitivity: 1.0, flag: false }
}

/// reconRandom uses a private named stream, without registering that stream
/// in the world's ordinary RNG table or drawing from it at admission.
pub fn seed(world_seed: u32, connection_key: &str) -> u32 {
    jsrt::hash::stream_seed(world_seed,&format!("network-recon:{connection_key}"))
}

/// createNetworkRemotes configures the server's bound before any owner binds.
/// This is a scalar export argument, not a generated argument-block field.
pub fn owner_capacity(capacity: u32) -> u32 { capacity }


/// createNetworkRemotes merges each side's fire points, then the played
/// side's again. Last assignment wins, including a duplicate model name in
/// a different side. A model-less droid still uses its authored muzzle.
pub fn muzzle(battle: &bfe_content::battle::Battle, def: &Value) -> Vec3 {
    let name = def.get("GeometryName").and_then(Value::as_str).unwrap_or("undefined");
    let point = battle.sides.first().and_then(|side|side.fire_points.get(name))
        .or_else(||battle.sides.iter().rev().find_map(|side|side.fire_points.get(name)))
        .filter(|value|value.truthy());
    match point {
        None => [0.0;3],
        Some(value) => std::array::from_fn(|i|bfe_content::js::to_number(value.at(i))),
    }
}

/// The shared sequence fills runtime identities before this owner is built.
pub fn build(_cx: &mut super::Cx<'_>) -> Result<super::Calls, AdmitError> {
    // F3Recon's constructor allocates host mirrors only. Definitions arrive
    // with Weapons::prepare; owner capacity and bindings arrive at Session.
    Ok(Vec::new())
}

/// Session admission supplies the local server's actual player capacity.
/// Recon has no owner or stream before that explicit configuration call.
pub fn configure(cx: &mut super::Cx<'_>, capacity: u32) -> Result<(Stage,&'static str,Call),AdmitError> {
    super::units::in_context(cx,|_,_|Ok(super::units::Entry::ReconOwners(capacity)))
}

/// Binding keys, owner lives and the private stream seed come from the
/// connection's Cx entries; attach/rebind clear all unrelated command fields.
pub fn bind(cx: &mut super::Cx<'_>, action: F3ReconAction, key: u32,
    owner: F3UnitKey, seed: u32) -> Result<(Stage,&'static str,Call),AdmitError> {
    super::units::in_context(cx,|_,_|Ok(super::units::Entry::ReconCommand(binding(action,key,owner,seed))))
}

impl Classes {
    /// Session/spawn consumers borrow the very objects registered above, in
    /// their assigned id order. Rebuilding by GeometryName would alias two
    /// distinct class objects which happen to use the same model.
    // A context may already contain this prefix from an earlier remote.
    // Validate it by allocation identity before appending any newly seen class.
    pub fn publish(&self, cx: &mut super::Cx<'_>) -> Result<(),AdmitError> {
        if cx.definitions.recon.len()>self.definitions.len() {
            return Err(error("recon definition context is ahead of its owner"));
        }
        for ((_,prior),source) in cx.definitions.recon.iter().zip(&self.definitions) {
            if !prior.same(source) {return Err(error("recon definition identity order changed"));}
        }
        for (id,source) in self.definitions.iter().enumerate() {
            // Session resolves these paths to the same Rc, with the already
            // allocated recon index. No model or class-name id is inferred.
            let key=format!("recon:{}",definition_path(cx.battle,source)?);
            if let Some((_,prior))=cx.ids.classes.iter().find(|(prior,_)|prior==&key) {
                if *prior!=id as f64 {return Err(error("recon path has a different assigned id"));}
            }else{cx.ids.classes.push((key.clone(),id as f64));}
            if id>=cx.definitions.recon.len() {cx.definitions.recon.push((key,source.clone()));}
        }
        Ok(())
    }
    pub fn definitions(&self) -> &[Value] {&self.definitions}
    pub fn strikes(&self) -> &[Value] {&self.strikes}
}

impl Classes {
    /// Restore the published class prefix. Base guns retain source identity;
    /// merged charges retain their own allocations for strike numbering.
    pub fn from_published(values: &[Value]) -> Result<Self,AdmitError> {
        // Replaying prior class registrations must reserve exactly the old
        // strike count without emitting exports or advancing string ids.
        // Fresh charge allocations remain distinct from later charge merges.
        let mut cache=Self::default();
        for def in values {
            cache.definitions.push(def.clone());
            if let Some(guns)=def.get("weapons").and_then(Value::as_array) {
                for gun in guns {
                    for level in weapon_levels(gun)? {
                        if level.get("strike").is_some_and(Value::truthy)
                            && !cache.strikes.iter().any(|known|known.same(&level)) {
                            cache.strikes.push(level);
                        }
                    }
                }
            }
        }
        Ok(cache)
    }
}

// Combat's path helpers are private to its module. Use the same root order for
// this owner's recon receipts; paths describe identity, never assign ids.
// Shared subtrees choose their first occurrence, including played-side data.
fn definition_path(battle:&bfe_content::battle::Battle,target:&Value)->Result<String,AdmitError> {
    fn visit(value:&Value,target:&Value,path:&mut Vec<String>,seen:&mut std::collections::HashSet<usize>)->bool {
        let Some(key)=identity(value) else{return false};
        if value.same(target) {return true}
        if path.len()>=64||!seen.insert(key) {return false}
        match value {
            Value::Object(object)=>for (name,child) in object.entries() {
                path.push(name.into());if visit(child,target,path,seen) {return true}path.pop();
            },
            Value::Array(array)=>for (index,child) in array.iter().enumerate() {
                path.push(index.to_string());if visit(child,target,path,seen) {return true}path.pop();
            },
            _=>{},
        }
        false
    }
    // Resolve metadata against source objects, not flattened string names.
    // The first two roots of each side match the combat table's traversal.
    let mut roots=Vec::new();
    for side in &battle.sides {
        roots.push((vec!["sides".into(),side.name.clone(),"units".into()],&side.units));
        roots.push((vec!["sides".into(),side.name.clone(),"items".into()],&side.items));
    }
    if let Some(value)=&battle.vehicle_def {roots.push((vec!["vehicles".into()],value));}
    if let Some(value)=&battle.props {roots.push((vec!["props".into()],value));}
    roots.extend([(vec!["common".into()],&battle.common),(vec!["effects".into()],&battle.effects),
        (vec!["world".into()],&battle.world),(vec!["soldier".into()],&battle.soldier_def)]);
    // Keep one visited set across roots. A shared subtree would already
    // have been searched at its earlier path, bounding repeated traversal.
    let mut seen=std::collections::HashSet::new();
    for (mut path,root) in roots {
        if visit(root,target,&mut path,&mut seen) {return serde_json::to_string(&path).map_err(|e|error(e.to_string()));}
    }
    Err(error("recon definition is not retained by the shared battle"))
}
