//! Immutable combat content and cold scalar commands (f3-combat.js).
//!
//! Registration follows object identity, not class names: two loadouts can
//! contain equal weapon records with different native handles. Keep these
//! tables across fleet, unit and spawn preparation, and share the instance's
//! Strings, so a late class never renumbers earlier content.
use super::Call;
use crate::gen::{F3CombatField, F3WeaponField};
use crate::store::{DefId, Strings};
use crate::world::admit::{AdmitError, Stage};
use bfe_content::defs::identity;
use bfe_content::value::Value;
use std::collections::{HashMap, HashSet};

/// Scalar export arguments are separate from the argument block. For example,
/// cmb_class receives an id directly and consumes valueWords from the block.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportCall {
    pub name: &'static str,
    pub call: Call,
    /// End of the shared string prefix when this call was encoded. This
    /// retains interns made before a nested definition call can execute.
    pub string_count: Option<usize>,
}
impl ExportCall {
    pub fn new(name: &'static str, arguments: Vec<f64>, row: Vec<f64>) -> Self {
        Self { name, call: Call { args: arguments, row, heap: Vec::new(), ..Call::default() }, string_count: None }
    }
    pub fn with_strings(mut self, strings: &Strings) -> Self {
        self.string_count=Some(strings.list.len());
        self
    }
}

/// DefTable.id in bridge-core.js. Retaining values keeps allocation identities
/// alive for the table's entire lifetime, even for merged charge definitions.
#[derive(Default)]
pub struct Definitions {
    pub values: Vec<Value>,
    ids: HashMap<usize, DefId>,
}
impl Definitions {
    pub fn get(&self, value: &Value) -> Option<DefId> {
        identity(value).and_then(|key| self.ids.get(&key).copied())
    }
    // Registration keeps a strong reference before publishing the handle.
    // A later shallow charge merge therefore cannot reuse an old allocation key.
    pub fn id(&mut self, value: &Value, stage: Stage) -> Result<DefId, AdmitError> {
        let key = identity(value).ok_or_else(|| AdmitError::Content { stage, message: "not a definition object".into() })?;
        if let Some(id) = self.ids.get(&key) { return Ok(*id); }
        let id = u32::try_from(self.values.len()).map_err(|_| AdmitError::Content { stage, message: "too many definitions".into() })?;
        self.values.push(value.clone());
        self.ids.insert(key, DefId(id));
        Ok(DefId(id))
    }
}

/// weapon-content-sim.js valueWords. Missing and null are distinct; numbers
/// are copied without arithmetic to preserve signed zero and NaN payloads.
/// The depth guard applies only to containers, just as in JavaScript.
pub fn value_words(value: Option<&Value>, strings: &mut Strings, weapons: Option<&Definitions>) -> Vec<f64> {
    fn write(value: Option<&Value>, strings: &mut Strings, weapons: Option<&Definitions>, depth: usize, out: &mut Vec<f64>) {
        match value {
            None => out.push(0.0),
            Some(Value::Null) => out.push(1.0),
            Some(Value::Bool(false)) => out.push(2.0),
            Some(Value::Bool(true)) => out.push(3.0),
            Some(Value::Number(number)) => out.extend([4.0, *number]),
            Some(Value::String(text)) => out.extend([5.0, strings.intern_str(text).0 as f64]),
            Some(value) => {
                if let Some(id) = weapons.and_then(|table| table.get(value)) {
                    out.extend([8.0, id.0 as f64]);
                } else if depth >= 12 {
                    out.push(9.0);
                } else {
                    match value {
                        Value::Array(items) => {
                            out.extend([6.0, items.len() as f64]);
                            for item in items.iter() { write(Some(item), strings, weapons, depth + 1, out); }
                        }
                        Value::Object(object) => {
                            out.extend([7.0, object.len() as f64]);
                            for (name, child) in object.entries() {
                                out.push(strings.intern_str(name).0 as f64);
                                write(Some(child), strings, weapons, depth + 1, out);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
    let mut out = Vec::new();
    write(value, strings, weapons, 0, &mut out);
    out
}

/// WeaponFacade's `[def, ...charges.map(level => ({...def, ...level}))]`.
/// A shallow merge preserves the ordnance/item objects shared across levels,
/// while each merged charge level gets a distinct construction identity.
pub fn weapon_levels(def: &Value) -> Result<Vec<Value>, AdmitError> {
    let base = def.as_object().ok_or_else(|| AdmitError::Content { stage: Stage::Combat, message: "weapon is not an object".into() })?;
    let mut levels = vec![def.clone()];
    if let Some(charges) = def.get("charges").filter(|v| v.truthy()) {
        let charges = charges.as_array().ok_or_else(|| AdmitError::Content { stage: Stage::Combat, message: "weapon charges are not an array".into() })?;
        for charge in charges {
            let mut merged = base.clone();
            if let Some(fields) = charge.as_object() {
                for (name, value) in fields.entries() { merged.set(std::rc::Rc::from(name), value.clone()); }
            } else if !charge.is_null() {
                return Err(AdmitError::Content { stage: Stage::Combat, message: "weapon charge is not an object".into() });
            }
            levels.push(Value::Object(std::rc::Rc::new(merged)));
        }
    }
    Ok(levels)
}

/// F3CombatStore's shared immutable tables. Methods return only new calls;
/// requesting an already prepared class cannot publish it a second time.
#[derive(Default)]
pub struct CombatRows {
    pub weapons: Definitions,
    pub classes: Definitions,
    pub explosions: Definitions,
    pub calls: Vec<ExportCall>,
    sent_weapons: HashSet<u32>,
    sent_classes: HashSet<u32>,
}
impl CombatRows {
    /// registerWeaponClass. The weapon table can already contain handles from
    /// generated fleet rows; only this method sends cmb_weapon content.
    pub fn weapon(&mut self, def: &Value, strings: &mut Strings) -> Result<DefId, AdmitError> {
        let id = self.weapons.id(def, Stage::Combat)?;
        // JS tracks sent objects separately from DefTable. Gaps are legal:
        // another owner may have reserved a later handle before sending it.
        if !self.sent_weapons.contains(&id.0) {
            self.calls.push(ExportCall::new("cmb_weapon", vec![id.0 as f64], value_words(Some(def), strings, None)).with_strings(strings));
            self.sent_weapons.insert(id.0);
        }
        Ok(id)
    }

    /// F3CombatStore.unitClass sends only AddHealth, base and weapons, in
    /// literal-property order, after registering every weapon it references.
    pub fn class(&mut self, def: &Value, strings: &mut Strings) -> Result<DefId, AdmitError> {
        let id = self.classes.id(def, Stage::Combat)?;
        if self.sent_classes.contains(&id.0) { return Ok(id); }
        // The object literal still includes AddHealth when it is missing.
        // Only the loadout uses the host's truthy empty-array default.
        let empty = Value::Array(Default::default());
        let weapons = def.get("weapons").filter(|value| value.truthy()).unwrap_or(&empty);
        let list = weapons.as_array().ok_or_else(|| AdmitError::Content { stage: Stage::Combat, message: "class weapons are not an array".into() })?;
        for weapon in list { self.weapon(weapon, strings)?; }
        let mut row = vec![7.0, 3.0];
        for (key, value) in [("AddHealth", def.get("AddHealth")), ("base", def.get("base")), ("weapons", Some(weapons))] {
            row.push(strings.intern_str(key).0 as f64);
            row.extend(value_words(value, strings, Some(&self.weapons)));
        }
        self.calls.push(ExportCall::new("cmb_class", vec![id.0 as f64], row).with_strings(strings));
        self.sent_classes.insert(id.0);
        Ok(id)
    }

    /// F3CombatStore.explosion has its own identity table, separate from F1's
    /// explosion handles. The blast binding joins the two tables later.
    pub fn explosion(&mut self, def: &Value, strings: &mut Strings) -> Result<DefId, AdmitError> {
        if let Some(id) = self.explosions.get(def) { return Ok(id); }
        let id = self.explosions.id(def, Stage::Combat)?;
        self.calls.push(ExportCall::new("cmb_explosion", vec![id.0 as f64], value_words(Some(def), strings, None)).with_strings(strings));
        Ok(id)
    }

    /// CombatFacade.attach. Native unit ids are supplied by the registry,
    /// including the temporary source id used while staging pooled bodies.
    pub fn attach(&mut self, id: u32, def: &Value, player: bool, strings: &mut Strings) -> Result<DefId, AdmitError> {
        let class = self.class(def, strings)?;
        self.calls.push(new_combat(id, class, def, player, strings)?);
        Ok(class)
    }
}

/// CombatFacade's construction arguments. MaxHealth uses || rather than ??;
/// heroes keep their numeric health, while authored zero falls back to 300.
pub fn new_combat(id: u32, class: DefId, def: &Value, player: bool, strings: &mut Strings) -> Result<ExportCall, AdmitError> {
    let health = def.get("MaxHealth").filter(|value| value.truthy()).map(|v| bfe_content::js::to_number(Some(v))).unwrap_or(300.0);
    // Optional `.length` is a JS property read, not an Array-only test.
    // Keep string and authored object lengths distinct from missing length.
    let has_chunks = match def.get("chunks") {
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::String(text)) => !text.is_empty(),
        Some(value) => value.get("length").is_some_and(Value::truthy),
        None => false,
    };
    let frequency = def.get("ChunkFrequency").filter(|v| !v.is_null()).map(|v| bfe_content::js::to_number(Some(v))).unwrap_or(-1.0);
    let chunks = if has_chunks && frequency > 0.0 { frequency } else { 0.0 };
    let health_type = def.get("HealthType").filter(|v| v.truthy());
    // The host calls toLowerCase directly. A truthy non-string is an error
    // at construction, rather than an instruction to stringify a new scale.
    let health_type = match health_type {
        None => "person",
        Some(Value::String(text)) => text,
        _ => return Err(AdmitError::Content { stage: Stage::Combat, message: "HealthType is not a string".into() }),
    };
    let health_type = health_type.to_lowercase();
    let scale_index = crate::ordnance::defs::HEALTH_TYPES.iter().position(|name| *name == health_type).unwrap_or(0);
    let scale = crate::ordnance::defs::SCALE_FIELDS[scale_index];
    Ok(ExportCall::new("cmb_new", vec![id as f64, class.0 as f64, f64::from(player), health, chunks, strings.intern_str(scale).0 as f64], Vec::new()).with_strings(strings))
}

/// WeaponFacade.set and keep before adoption. The generated field enum
/// defines this scalar boundary; its value is never converted through JSON.
pub fn weapon_set(id: u32, index: u32, field: F3WeaponField, value: f64) -> ExportCall {
    ExportCall::new("cmb_weapon_set", vec![id as f64, index as f64, field as u32 as f64, value], Vec::new())
}

/// CombatFacade.attach publishes these six settings after cmb_new. Inputs
/// are the retained pre-adoption settings, already converted with Number.
pub fn combat_settings(id: u32, values: [f64; 6]) -> Vec<ExportCall> {
    [F3CombatField::Mounted, F3CombatField::GunUnderwater, F3CombatField::Team, F3CombatField::DeferMuzzles, F3CombatField::Random, F3CombatField::Remotes]
        .into_iter().zip(values).map(|(field, value)| ExportCall::new("cmb_set", vec![id as f64, field as u32 as f64, value], Vec::new())).collect()
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::module::{cmb_class_in, cmb_new_in, cmb_weapon_in, cmb_weapon_set_in};
    use crate::world::owner::World;

    #[test]
    fn marshaled_loadout_constructs_native_combat_and_accepts_cold_settings() {
        let class = bfe_content::json::parse(r#"{"base":"soldier","MaxHealth":125,"weapons":[{"base":"launcher","class":"rifle","channel":0,"RoundsPerClip":12,"ammo":36,"ShotDelay":0.15}]}"#).unwrap();
        let mut rows = CombatRows::default();
        let mut strings = Strings::new();
        rows.attach(4,&class,true,&mut strings).unwrap();
        let setting = weapon_set(4,0,F3WeaponField::Clip,-0.0);
        let mut world = World::new();
        world.sim.strings = strings;
        // Exercise the actual owner readers, not a second decoder in the test.
        // The unit class must resolve its weapon reference before cmb_new arms it.
        for call in rows.calls {
            world.sim.calls.result(call.call.row.len(),|row,_| row.copy_from_slice(&call.call.row));
            match call.name {
                "cmb_weapon" => cmb_weapon_in(&mut world,call.call.args[0] as u32),
                "cmb_class" => cmb_class_in(&mut world,call.call.args[0] as u32),
                "cmb_new" => { let a=&call.call.args; assert!(cmb_new_in(&mut world,a[0] as u32,a[1] as u32,a[2] as u32,a[3],a[4],a[5] as u32).is_some()); },
                _ => panic!("unexpected construction call"),
            }
        }
        let a=&setting.call.args;
        assert_eq!(cmb_weapon_set_in(&mut world,a[0] as u32,a[1] as u32,a[2] as u32,a[3]),1);
        let (_,unit)=world.sim.store.unit.at(4).unwrap();
        assert_eq!(unit.combat.health,125.0);
        assert_eq!(unit.combat.weapons.len(),1);
        assert_eq!(unit.combat.weapons[0].clip.to_bits(),(-0.0f64).to_bits());

        // The policy source reaches the real native parser unchanged. Later
        // sides overwrite a shared weapon class, matching loadAIWeaponData.
        let source=super::super::weapon_policies::source_with_data(&[DefId(0)],[
            Some(br#"{ "weapons": {"rifle": {"ai": {"minRange":5}}}}
"#.to_vec()),
            Some(br#"{"weapons":{"rifle":{"ai":{"minRange":17}}}}"#.to_vec()),
        ]);
        let call=super::super::weapon_policies::call(&source);
        world.sim.calls.result(call.row.len(),|row,heap| {row.copy_from_slice(&call.row);heap.extend_from_slice(&call.heap);});
        let block=crate::ai::owned::policies::ai_weapon_policies_in(&mut world).unwrap();
        let (row,heap)=block.read(0);
        let policies=crate::gen::AiWeaponPolicyContent::take(row,heap);
        assert_eq!(policies.weapons[0].weapon,DefId(0));
        assert_eq!(policies.weapons[0].levels[0].min_range,17.0);
    }
}

/// Retained across admission stages: fleet, units and spawn construction all
/// share these identities. Strings remain the enclosing context's table.
#[derive(Default)]
pub struct Bindings {
    pub combat: CombatRows,
    pub ordnance: super::ordnance::OrdnanceRows,
    pub items: super::items::ItemRows,
    // combat-sources.js derivedFrom: keep the authored provenance separate
    // from the derived object's own immutable fields and native identity.
    sources: HashMap<usize,(Value,Value)>,
}
impl Bindings {
    pub fn derived_from(&mut self,value:&Value,source:&Value)->Result<(),AdmitError> {
        let id=identity(value).ok_or_else(||AdmitError::Content {stage:Stage::Combat,message:"derived definition is not an object".into()})?;
        if identity(source).is_none() {return Err(AdmitError::Content {stage:Stage::Combat,message:"definition source is not an object".into()});}
        let source=identity(source).and_then(|id|self.sources.get(&id)).map(|(_,source)|source).unwrap_or(source).clone();
        self.sources.insert(id,(value.clone(),source));
        Ok(())
    }
}

// Cx's identity entries use full content paths, rather than class names.
// Distinct loadout objects can share a class name. The path is only a lookup
// key: ids always come from the earlier entry or first registration order.
// Each root borrows the original Battle allocations. The temporary path
// catalog adds no definition objects and does not parse fetched data again.
// Shared subtrees resolve to their first path in this deterministic order.
fn content_roots(battle: &bfe_content::battle::Battle) -> Vec<(Vec<String>, &Value)> {
    let mut roots=Vec::new();
    for side in &battle.sides {
        roots.push((vec!["sides".into(),side.name.clone(),"units".into()],&side.units));
        roots.push((vec!["sides".into(),side.name.clone(),"items".into()],&side.items));
    }
    if let Some(value)=&battle.vehicle_def {roots.push((vec!["vehicles".into()],value));}
    if let Some(value)=&battle.props {roots.push((vec!["props".into()],value));}
    roots.extend([(vec!["common".into()],&battle.common),(vec!["effects".into()],&battle.effects),(vec!["world".into()],&battle.world),(vec!["soldier".into()],&battle.soldier_def)]);
    roots
}
// A qualified path is the retained identity receipt in Cx. Comparing Rc
// identity before walking children distinguishes identical authored records.
// The visited set bounds repeated walks of shared charge and effect data.
fn content_key(battle: &bfe_content::battle::Battle, target: &Value) -> Result<String,AdmitError> {
    fn visit(value:&Value,target:&Value,path:&mut Vec<String>,seen:&mut HashSet<usize>)->bool {
        let Some(key)=identity(value) else {return false;};
        if value.same(target) {return true;}
        if path.len()>=64 || !seen.insert(key) {return false;}
        match value {
            Value::Object(object) => for (name,child) in object.entries() {
                path.push(name.into());if visit(child,target,path,seen) {return true;}path.pop();
            },
            Value::Array(array) => for (index,child) in array.iter().enumerate() {
                if identity(child).is_none() {continue;}
                path.push(index.to_string());if visit(child,target,path,seen) {return true;}path.pop();
            },
            _ => {},
        }
        false
    }
    let mut seen=HashSet::new();
    for (mut path,value) in content_roots(battle) {
        if visit(value,target,&mut path,&mut seen) {
            return serde_json::to_string(&path).map_err(|e|AdmitError::Content {stage:Stage::Content,message:e.to_string()});
        }
    }
    Err(AdmitError::Content {stage:Stage::Content,message:"definition is not retained by the loaded battle".into()})
}
// The stored scalar is authoritative. Checking registration order catches
// missing entries, duplicate identities and invalid numeric handles; it never
// silently renumbers content after another owner has already referenced it.
fn load_ids(entries:&[(String,f64)],values:&[(String,Value)],kind:&str,stage:Stage)->Result<Definitions,AdmitError> {
    let mut table=Definitions::default();
    for (key,id) in entries {
        if !key.starts_with(kind) {continue;}
        let def=values.iter().find(|(name,_)|name==key).map(|(_,value)|value)
            .ok_or_else(||AdmitError::Content {stage,message:"registered definition object is absent".into()})?;
        let actual=table.id(&def,stage)?;
        if *id!=actual.0 as f64 {return Err(AdmitError::Content {stage,message:"definition ids are out of registration order".into()});}
    }
    Ok(table)
}
// Append only newly registered objects. Untouched side classes must not
// consume handles before the construction seam that first uses them.
fn save_ids(battle:&bfe_content::battle::Battle,entries:&mut Vec<(String,f64)>,values:&mut Vec<(String,Value)>,kind:&str,table:&Definitions,sources:&HashMap<usize,(Value,Value)>)->Result<(),AdmitError> {
    let count=entries.iter().filter(|(key,_)|key.starts_with(kind)).count();
    for (id,value) in table.values.iter().enumerate().skip(count) {
        let path=if let Some((_,source))=identity(value).and_then(|id|sources.get(&id)) {
            // A derived object has a fresh native id for each construction.
            // Its source path remains the peer-stable network identity.
            serde_json::to_string(&["$derived".to_string(),content_key(battle,source)?,id.to_string()])
                .map_err(|e|AdmitError::Content {stage:Stage::Content,message:e.to_string()})?
        } else {content_key(battle,value)?};
        let key=format!("{kind}{path}");
        entries.push((key.clone(),id as f64));
        values.push((key,value.clone()));
    }
    Ok(())
}

// DefTable registration and sending content are separate in the host. AI
// facts can reserve an item or class before a body needs its native content.
// Sent markers retain that distinction without changing any definition id.
// A sent marker names both the existing id and its original object. This
// keeps class/weapon sets independent from F1's contiguous sent prefixes.
fn load_sent(entries:&[(String,f64)],kind:&str,table:&Definitions)->Result<HashSet<u32>,AdmitError> {
    let error=||AdmitError::Content {stage:Stage::Content,message:"invalid sent-definition marker".into()};
    let mut sent=HashSet::new();
    for (key,id) in entries {
        let Some(path)=key.strip_prefix(kind) else {continue;};
        if !id.is_finite() || *id<0.0 || id.fract()!=0.0 || *id>=table.values.len() as f64 {return Err(error());}
        let id=*id as u32;
        let registered_kind=kind.replace("-sent:",":");
        if !entries.iter().any(|(key,number)|*number==id as f64&&key.strip_prefix(&registered_kind)==Some(path)) || !sent.insert(id) {return Err(error());}
    }
    Ok(sent)
}
// OrdnanceSim.defId and ItemsSim.defId drain every reserved predecessor.
// Their sent sets consequently have a prefix shape that weapon sets do not.
fn contiguous_sent(sent:HashSet<u32>)->Result<usize,AdmitError> {
    if (0..sent.len() as u32).any(|id|!sent.contains(&id)) {
        return Err(AdmitError::Content {stage:Stage::Store,message:"sent definition prefix has a gap".into()});
    }
    Ok(sent.len())
}
// Callers enumerate ids in order, so hash bucket order never reaches Cx.
// An existing marker stays in place, just as the host's sent set does.
fn save_sent(entries:&mut Vec<(String,f64)>,kind:&str,table:&Definitions,sent:impl Iterator<Item=u32>)->Result<(),AdmitError> {
    for id in sent {
        if id as usize>=table.values.len() {return Err(AdmitError::Content {stage:Stage::Content,message:"sent definition is absent".into()});}
        let registered_kind=kind.replace("-sent:",":");
        let path=entries.iter().find_map(|(key,number)|if *number==id as f64 {key.strip_prefix(&registered_kind)} else {None})
            .ok_or_else(||AdmitError::Content {stage:Stage::Content,message:"sent definition receipt is absent".into()})?;
        let key=format!("{kind}{path}");
        if !entries.iter().any(|(existing,_)|*existing==key) {entries.push((key,id as f64));}
    }
    Ok(())
}

/// Restore the actual previously assigned tables from Cx, run one construction
/// seam, and publish its added ids and original objects. Every later owner
/// reads the same Value allocations from the shared Content/Defs lifetime.
pub fn with_bindings<T>(cx:&mut super::Cx,stage:Stage,prepare:impl FnOnce(&mut Bindings,&mut Strings)->Result<(T,Vec<ExportCall>),AdmitError>)->Result<(T,super::Calls),AdmitError> {
    let weapons=load_ids(&cx.ids.defs,&cx.definitions.weapons,"weapon:",Stage::Combat)?;
    let classes=load_ids(&cx.ids.defs,&cx.definitions.classes,"class:",Stage::Combat)?;
    let explosions=load_ids(&cx.ids.defs,&cx.definitions.explosions,"blast:",Stage::Combat)?;
    let sent_weapons=load_sent(&cx.ids.defs,"weapon-sent:",&weapons)?;
    let sent_classes=load_sent(&cx.ids.defs,"class-sent:",&classes)?;
    let ordnance=load_ids(&cx.ids.ordnance,&cx.definitions.ordnance,"ordnance:",Stage::Store)?;
    let ordnance_sent=contiguous_sent(load_sent(&cx.ids.ordnance,"ordnance-sent:",&ordnance)?)?;
    let items=load_ids(&cx.ids.items,&cx.definitions.items,"item:",Stage::Store)?;
    let items_sent=contiguous_sent(load_sent(&cx.ids.items,"item-sent:",&items)?)?;
    let mut sources=HashMap::new();
    // Derived provenance can cross more than one construction seam. Restore
    // its ultimate authored source before another shallow derivation runs.
    for kind in [DefinitionKind::Class,DefinitionKind::Weapon,DefinitionKind::CombatExplosion,DefinitionKind::Ordnance,DefinitionKind::Explosion,DefinitionKind::Item] {
        for ((_,value),(_,source)) in registered_definitions(cx,kind)?.into_iter().zip(source_definitions(cx,kind)?) {
            if !value.same(&source) {if let Some(id)=identity(&value) {sources.insert(id,(value,source));}}
        }
    }
    let mut bindings=Bindings {
        combat:CombatRows {weapons,classes,explosions,calls:Vec::new(),sent_weapons,sent_classes},
        ordnance:super::ordnance::OrdnanceRows::from_definitions(
            ordnance,load_ids(&cx.ids.ordnance,&cx.definitions.explosions,"explosion:",Stage::Store)?,ordnance_sent)?,
        items:super::items::ItemRows::from_definitions(items,items_sent)?,
        sources,
    };
    // The hook may register nested explosions and combat blast definitions.
    // Capture the shared string prefix before any of those observers run.
    let start=cx.strings.list.len();
    let (value,exports)=prepare(&mut bindings,&mut cx.strings)?;
    save_ids(cx.battle,&mut cx.ids.defs,&mut cx.definitions.weapons,"weapon:",&bindings.combat.weapons,&bindings.sources)?;
    save_ids(cx.battle,&mut cx.ids.defs,&mut cx.definitions.classes,"class:",&bindings.combat.classes,&bindings.sources)?;
    save_ids(cx.battle,&mut cx.ids.defs,&mut cx.definitions.explosions,"blast:",&bindings.combat.explosions,&bindings.sources)?;
    save_ids(cx.battle,&mut cx.ids.ordnance,&mut cx.definitions.ordnance,"ordnance:",&bindings.ordnance.definitions,&bindings.sources)?;
    save_ids(cx.battle,&mut cx.ids.ordnance,&mut cx.definitions.explosions,"explosion:",&bindings.ordnance.explosions,&bindings.sources)?;
    save_ids(cx.battle,&mut cx.ids.items,&mut cx.definitions.items,"item:",&bindings.items.definitions,&bindings.sources)?;
    save_sent(&mut cx.ids.defs,"weapon-sent:",&bindings.combat.weapons,
        (0..bindings.combat.weapons.values.len() as u32).filter(|id|bindings.combat.sent_weapons.contains(id)))?;
    save_sent(&mut cx.ids.defs,"class-sent:",&bindings.combat.classes,
        (0..bindings.combat.classes.values.len() as u32).filter(|id|bindings.combat.sent_classes.contains(id)))?;
    save_sent(&mut cx.ids.ordnance,"ordnance-sent:",&bindings.ordnance.definitions,0..bindings.ordnance.sent_count() as u32)?;
    save_sent(&mut cx.ids.items,"item-sent:",&bindings.items.definitions,0..bindings.items.sent_count() as u32)?;
    // Each export retained its interner position when it was encoded.
    // Moving parent-field interns past a nested call would change later ids.
    // Replay these exact prefixes even when a text is not in that child row.
    let mut calls=Vec::with_capacity(exports.len());
    let mut cursor=start;
    for export in exports {
        let end=export.string_count.unwrap_or(cursor);
        if end<cursor || end>cx.strings.list.len() {
            return Err(AdmitError::Content {stage,message:"definition intern order is inconsistent".into()});
        }
        let mut call=export.call;
        call.strings.extend_from_slice(&cx.strings.list[cursor..end]);
        cursor=end;
        calls.push((stage,export.name,call));
    }
    if cursor!=cx.strings.list.len() {
        return Err(AdmitError::Content {stage,message:"definition call did not retain its string interns".into()});
    }
    Ok((value,calls))
}

/// registerWeaponClass at a fleet, unit or recon construction seam.
pub fn prepare_weapon(cx:&mut super::Cx,def:&Value,stage:Stage)->Result<(DefId,super::Calls),AdmitError> {
    with_bindings(cx,stage,|bindings,strings| {
        let id=bindings.combat.weapon(def,strings)?;
        Ok((id,std::mem::take(&mut bindings.combat.calls)))
    })
}

/// CombatFacade.attach needs the registry id assigned by the earlier owner.
/// Returning it explicitly avoids deriving a unit slot from the class's place.
pub fn attach_unit(cx:&mut super::Cx,id:u32,def:&Value,player:bool,stage:Stage)->Result<super::Calls,AdmitError> {
    let (_,calls)=with_bindings(cx,stage,|bindings,strings| {
        bindings.combat.attach(id,def,player,strings)?;
        let mut calls=std::mem::take(&mut bindings.combat.calls);
        calls.extend(combat_settings(id,[0.0,0.0,0.0,if def.get("base").and_then(Value::as_str)==Some("walkerdroid") {1.0} else {0.0},0.0,0.0]));
        Ok(((),calls))
    })?;
    Ok(calls)
}

/// game.js's initial viewpoint attaches before the fleet. Its registry id
/// must already be supplied by the registry owner, as Units.add supplied it.
pub fn build(cx:&mut super::Cx)->Result<super::Calls,AdmitError> {
    let id=cx.ids.registry.iter().find(|(key,_)|key=="viewpoint").map(|(_,id)|*id)
        .filter(|id|id.is_finite()&&*id>=0.0&&*id<=u32::MAX as f64&&id.fract()==0.0)
        .ok_or_else(||AdmitError::Content {stage:Stage::Combat,message:"viewpoint registry id is not assigned".into()})? as u32;
    attach_unit(cx,id,&cx.battle.soldier_def.clone(),true,Stage::Combat)
}

/// F1 explosions and combat blast packets have independent handle tables,
/// even when both retain the same original definition object.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum DefinitionKind { Class, Weapon, CombatExplosion, Ordnance, Explosion, Item }
fn registered_table(cx:&super::Cx,kind:DefinitionKind)->Result<Definitions,AdmitError> {
    let (ids,values,prefix,stage)=match kind {
        DefinitionKind::Class=>(&cx.ids.defs,&cx.definitions.classes,"class:",Stage::Combat),
        DefinitionKind::Weapon=>(&cx.ids.defs,&cx.definitions.weapons,"weapon:",Stage::Combat),
        DefinitionKind::CombatExplosion=>(&cx.ids.defs,&cx.definitions.explosions,"blast:",Stage::Combat),
        DefinitionKind::Ordnance=>(&cx.ids.ordnance,&cx.definitions.ordnance,"ordnance:",Stage::Store),
        DefinitionKind::Explosion=>(&cx.ids.ordnance,&cx.definitions.explosions,"explosion:",Stage::Store),
        DefinitionKind::Item=>(&cx.ids.items,&cx.definitions.items,"item:",Stage::Store),
    };
    load_ids(ids,values,prefix,stage)
}

/// Read the producer's registered objects with their previously assigned ids.
/// A namespace's order is its own DefTable order; callers do not enumerate
/// the combined explosion receipts as though they shared one numeric table.
pub fn registered_definitions(cx:&super::Cx,kind:DefinitionKind)->Result<Vec<(DefId,Value)>,AdmitError> {
    Ok(registered_table(cx,kind)?.values.into_iter().enumerate().map(|(id,value)|(DefId(id as u32),value)).collect())
}

/// Resolve an existing handle by object identity, including unsent reservations.
/// This is a read of an earlier owner's receipt and cannot allocate a handle.
pub fn definition_id(cx:&super::Cx,kind:DefinitionKind,value:&Value)->Result<Option<DefId>,AdmitError> {
    Ok(registered_table(cx,kind)?.get(value))
}

/// Network source identities follow combat-sources.js, while the native
/// definition tables retain the distinct per-construction derived objects.
pub fn source_definitions(cx:&super::Cx,kind:DefinitionKind)->Result<Vec<(DefId,Value)>,AdmitError> {
    let prefix=match kind {DefinitionKind::Class=>"class:",DefinitionKind::Weapon=>"weapon:",DefinitionKind::CombatExplosion=>"blast:",DefinitionKind::Ordnance=>"ordnance:",DefinitionKind::Explosion=>"explosion:",DefinitionKind::Item=>"item:"};
    let entries=match kind {DefinitionKind::Ordnance|DefinitionKind::Explosion=>&cx.ids.ordnance,DefinitionKind::Item=>&cx.ids.items,_=>&cx.ids.defs};
    let mut values=registered_definitions(cx,kind)?;
    for (key,id) in entries {
        let Some(path)=key.strip_prefix(prefix) else {continue;};
        let path:Vec<String>=serde_json::from_str(path).map_err(|e|AdmitError::Content {stage:Stage::Content,message:e.to_string()})?;
        if path.first().map(String::as_str)!=Some("$derived") {continue;}
        let source:Vec<String>=path.get(1).and_then(|s|serde_json::from_str(s).ok()).ok_or_else(||AdmitError::Content {stage:Stage::Content,message:"derived source path is absent".into()})?;
        let (root,mut value)=content_roots(cx.battle).into_iter().find(|(root,_)|source.starts_with(root)).ok_or_else(||AdmitError::Content {stage:Stage::Content,message:"derived source root is absent".into()})?;
        for key in &source[root.len()..] {
            value=match value {Value::Array(_)=>key.parse::<usize>().ok().and_then(|i|value.at(i)),_=>value.get(key)}
                .ok_or_else(||AdmitError::Content {stage:Stage::Content,message:"derived source is absent".into()})?;
        }
        values[*id as usize].1=value.clone();
    }
    Ok(values)
}

/// weapon-targeting.js vehicleMissileDefinition runs before policy loading.
/// It shallow-copies only the three authored tank launchers; every hull gets
/// fresh weapon and ordnance identities, retaining all other child aliases.
pub fn vehicle_missile_definition(def:&Value)->Result<Value,AdmitError> {
    if !matches!(def.get("class").and_then(Value::as_str),Some("rep_weap_hover_fightertank_missile"|"imp_weap_hover_fightertank_missile"|"all_weap_hover_combatspeeder_missile")) {return Ok(def.clone());}
    let mut strings=Strings::new();
    let words=value_words(Some(def),&mut strings,None);
    let crate::combat::defs::Js::Obj(obj)=crate::combat::module::read_js(&words,&mut 0,&strings) else {return Err(AdmitError::Content {stage:Stage::Combat,message:"tank weapon is not an object".into()});};
    let ai=crate::ai::combat::weapons_ai::WeaponAiTable::default().weapon_ai(&obj);
    // Most policy fields are numbers after fround. Nullish-coalesced fields
    // retain their original JS type and bits, including strings and -0.
    let own=def.get("ai").filter(|v|v.truthy());
    let raw=|key:&str,fallback:Value|own.and_then(|v|v.get(key)).filter(|v|!matches!(v,Value::Null)).cloned().unwrap_or(fallback);
    let fields=vec![
        ("minRange",Value::Number(ai.min_range)),("optimalRange",Value::Number(ai.optimal_range)),("maxRange",Value::Number(ai.max_range)),
        ("implicitRange",raw("implicitRange",Value::Bool(ai.implicit_range))),("extremeRange",Value::Number(150.0)),
        ("relationMask",Value::Number(1.0)),("categoryMask",Value::Number(8.0)),
        ("barrageMin",raw("barrageMin",Value::Number(ai.barrage_min))),("barrageMax",raw("barrageMax",Value::Number(ai.barrage_max))),
        ("barrageDelay",Value::Number(ai.barrage_delay)),("triggerSingle",if def.get("base").and_then(Value::as_str)==Some("melee") {Value::Bool(true)} else {raw("triggerSingle",Value::Bool(ai.trigger_single))}),
        ("zoomMax",Value::Number(ai.zoom_max)),("action",raw("action",Value::Number(ai.action)))];
    let ai=Value::Object(std::rc::Rc::new(bfe_content::value::Object::from_written(fields.into_iter().map(|(k,v)|(std::rc::Rc::from(k),v)).collect())));
    let mut weapon=def.as_object().ok_or_else(||AdmitError::Content {stage:Stage::Combat,message:"tank weapon is not an object".into()})?.clone();
    let mut ordnance=def.get("ordnance").and_then(Value::as_object).cloned().unwrap_or_default();
    for (key,value) in [("base",Value::str("missile")),("TurnRate",Value::Number(6.0)),("ScaleTime",Value::Number(1.0))] {ordnance.set(std::rc::Rc::from(key),value);}
    weapon.set(std::rc::Rc::from("base"),Value::str("launcher"));
    for (key,value) in [("LockOnRange",150.0),("LockOnAngle",6.0),("LockOffAngle",10.0),("LockTime",1.0),("ClearLockOnFire",1.0),("TargetEnemy",1.0),("TargetFriendly",0.0),("TargetNeutral",0.0),("TargetPerson",0.0),("TargetAnimal",0.0),("TargetDroid",0.0),("TargetVehicle",1.0),("TargetBuilding",0.0)] {weapon.set(std::rc::Rc::from(key),Value::Number(value));}
    weapon.set(std::rc::Rc::from("ai"),ai);
    weapon.set(std::rc::Rc::from("ordnance"),Value::Object(std::rc::Rc::new(ordnance)));
    Ok(Value::Object(std::rc::Rc::new(weapon)))
}

/// F2Firing.adopt registers the level-zero weapon before its ordnance. Return
/// the derived object as well as the handles so the fleet can retain it for
/// later services, without deriving an id from a hull or seat index.
pub fn prepare_vehicle_weapon(cx:&mut super::Cx,authored:&Value,stage:Stage)->Result<((Value,DefId,Option<DefId>),super::Calls),AdmitError> {
    let def=vehicle_missile_definition(authored)?;
    with_bindings(cx,stage,|bindings,strings| {
        if !def.same(authored) {
            bindings.derived_from(&def,authored)?;
            if let (Some(value),Some(source))=(def.get("ordnance"),authored.get("ordnance")) {bindings.derived_from(value,source)?;}
        }
        let weapon=bindings.combat.weapon(&def,strings)?;
        let mut calls=std::mem::take(&mut bindings.combat.calls);
        let ordnance=def.get("ordnance").filter(|value|value.truthy()).map(|def|bindings.ordnance.definition(def,strings)).transpose()?;
        calls.append(&mut bindings.ordnance.calls);
        Ok(((def,weapon,ordnance),calls))
    })
}
