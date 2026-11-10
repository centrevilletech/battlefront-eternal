//! Dump unit rows from loaded content with the earlier owners' explicit
//! bindings. Each stdin JSON line names an export and its resolved ids; stdout
//! uses the admission recorder's arguments/args hex-word shape. Strings may be
//! seeded with `strings: [text,...]` to reproduce the preceding call prefix.
//!
//! admission-rows-units CONTENT_DIR [--map nab2] [--side rep] [--seed 1]
//! Requests name a class, a zero-based weapon and charge level where relevant.
//! Body requests supply key:{slot,generation,life}; droideka requests supply
//! clips:{name:id}; impact/item requests supply their native definition id.
//! --check instead reads a complete gunzipped admission recording and compares
//! supported payloads using earlier imported owner facts as explicit bindings.
//! The summary lists checked and skipped exports separately; it is not a
//! replacement for the whole native admission bank comparison.
use bfe_content::{battle::{Battle, Loader}, content::Content, defs::Defs, files::DirFiles, Value as ContentValue};
use bfe_sim::{gen::*, store::Strings, world::admit::{Options, rows::{Call, units,props,recon}}};
use serde_json::{json,Value};
use std::io::BufRead;

type Output = (&'static str,Vec<f64>,Call);
fn hex(words: &[f64]) -> Vec<String> { words.iter().map(|w|format!("{:016x}",w.to_bits())).collect() }
fn number(value: Option<&Value>) -> Result<f64,String> {
    match value {
        Some(Value::Number(n)) => n.as_f64().ok_or("number cannot be represented".into()),
        Some(Value::String(s)) if s.len()==16 => u64::from_str_radix(s,16).map(f64::from_bits).map_err(|e|e.to_string()),
        _ => Err("number or hexadecimal f64 word required".into()),
    }
}
fn integer(value: Option<&Value>) -> Result<u32,String> {
    let n = number(value)?;
    if n.is_finite() && n>=0.0 && n<=u32::MAX as f64 && n.fract()==0.0 { Ok(n as u32) }
    else { Err("unsigned integer required".into()) }
}
fn optional_id(value: Option<&Value>) -> Result<Option<u32>,String> {
    value.filter(|v|!v.is_null()).map(|v|integer(Some(v))).transpose()
}
fn vector<const N: usize>(value: &Value) -> Result<[f64;N],String> {
    let list = value.as_array().ok_or("numeric array required")?;
    if list.len()!=N {return Err(format!("array of {N} words required"))}
    let mut out = [0.0;N]; for (at,v) in list.iter().enumerate() {out[at]=number(Some(v))?;} Ok(out)
}
fn body(value: &Value) -> Result<Ref<Body>,String> {
    Ok(Ref::new(integer(value.get("slot"))?,integer(value.get("generation"))?))
}
fn member(value: &Value) -> Result<Option<Member>,String> {
    if value.is_null() {return Ok(None)}
    let slot=integer(value.get("slot"))?; let generation=integer(value.get("generation"))?;
    Ok(Some(match value["kind"].as_str().ok_or("member kind required")? {
        "unit"=>Member::Unit(Ref::new(slot,generation)),"vehicle"=>Member::VehicleTarget(Ref::new(slot,generation)),
        "turret"=>Member::TurretTarget(Ref::new(slot,generation)),"objective"=>Member::ObjectiveTarget(Ref::new(slot,generation)),
        _=>return Err("unknown member kind".into()),
    }))
}
fn key(request: &Value) -> Result<F3UnitKey,String> {
    let k = &request["key"];
    Ok(F3UnitKey {slot:integer(k.get("slot"))?,generation:integer(k.get("generation"))?,life:number(k.get("life"))?})
}
fn typed(words: usize,put:impl FnOnce(&mut [f64],&mut Vec<f64>))->Call {units::call(words,put)}

// Native ids depend on the whole preceding sequence. The dump requires those
// bindings explicitly; its content-derived payload never substitutes an index
// from Defs for an identity allocated by CombatSim or the mixed registry.
fn request(input: &Value, content: &Content, defs: &Defs, strings: &mut Strings,
    groups: &mut units::ItemGroups, buildings: &mut props::Buildings) -> Result<Vec<Output>,String> {
    let export = input["export"].as_str().ok_or("export required")?;
    match export {
        "f3_init" => {
            let water = content.battle.world.get("water").filter(|v|v.truthy())
                .map(|v|bfe_content::js::to_number(v.get("height"))+0.0);
            let init = units::Init::new(integer(input.get("world"))?,water,integer(input.get("bodySerial"))?);
            Ok(vec![("f3_init",init.call().args,Call::default())])
        }
        "f3_missile_classes" => Ok(units::missile_classes(|s|strings.intern_str(s)).iter().map(|row|
            ("f3_missile_classes",vec![1.0],typed(F3MissileClass::WORDS,|r,h|row.put(r,h)))).collect()),
        "f3_registry_prepare" => Ok(vec![("f3_registry_prepare",vec![],Call::default())]),
        "f3_registry_add" => Ok(vec![("f3_registry_add",units::registry_add(integer(input.get("kind"))? as u16)
            .map_err(|e|e.to_string())?.args,Call::default())]),
        "f3_impact_rules" => {
            let setting = input.get("friendlyFire").map(|v|bfe_content::json::parse(&v.to_string()).map_err(|e|e.to_string())).transpose()?;
            let row = units::impact_rules(setting.as_ref());
            Ok(vec![("f3_impact_rules",vec![],typed(F3ImpactRules::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_blast_binding" => {
            let row = units::blast_binding(integer(input.get("explosion"))?,integer(input.get("combat"))?);
            Ok(vec![("f3_blast_binding",vec![],typed(F3BlastBinding::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_body_names" => {
            let mut names=Vec::new();
            for value in input["bodies"].as_array().ok_or("bodies required")? {
                names.push((body(value)?,integer(value.get("kernel"))?));
            }
            let row=units::Entry::BodyNames(units::body_names(names)).encode().map_err(|e|e.to_string())?;
            Ok(vec![(row.name,row.arguments,row.call)])
        }
        "f3_world_blast" => {
            let owner=&input["owner"]; let slot=optional_id(owner.get("slot"))?.unwrap_or(0);
            let generation=optional_id(owner.get("generation"))?.unwrap_or(0);
            let owner=match owner["kind"].as_str().unwrap_or("null") {
                "null"=>OrdnanceOwner::Null,"player"=>OrdnanceOwner::Player,"unit"=>OrdnanceOwner::Unit(Ref::new(slot,generation)),
                "vehicle"=>OrdnanceOwner::Vehicle(Ref::new(slot,generation)),"snapshot"=>OrdnanceOwner::OwnerSnapshot(Ref::new(slot,generation)),
                _=>return Err("unknown ordnance owner kind".into()),
            };
            let row=units::world_blast(integer(input.get("explosion"))?,vector::<3>(&input["position"])?,owner);
            Ok(vec![("f3_world_blast",vec![],typed(F3BlastCall::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_hull" => {
            let identity=units::HullIdentity {hull:Ref::new(integer(input.get("slot"))?,integer(input.get("generation"))?),
                member:member(&input["member"])?,category:integer(input.get("category"))?,kind:number(input.get("kind"))?,
                key:strings.intern_str(input["key"].as_str().ok_or("hull key required")?),health:integer(input.get("health"))?};
            let row=units::hull(identity,vector::<3>(&input["size"])?,vector::<3>(&input["center"])?,
                input.get("sphere").filter(|v|!v.is_null()).map(vector::<4>).transpose()?,
                optional_id(input.get("explosion"))?,optional_id(input.get("finalExplosion"))?);
            Ok(vec![("f3_hull",vec![],typed(F3HullSetup::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_building" => {
            let index=integer(input.get("source"))? as usize;
            let data=content.battle.props.as_ref().ok_or("battle has no props")?;
            let desc=data.get("instances").and_then(|v|v.at(index)).ok_or("prop source out of range")?;
            let models=data.get("models").ok_or("prop models missing")?;
            let target_slot=integer(input.get("slot"))?; let generation=integer(input.get("generation"))?;
            let target=if desc.get("base").and_then(ContentValue::as_str)==Some("armedbuilding") {
                RepairTarget::Turret(Ref::new(target_slot,generation))
            } else {RepairTarget::Building(Ref::new(target_slot,generation))};
            let mut bodies=[None;3];
            let refs=input["bodies"].as_array().ok_or("three building bodies required")?;
            if refs.len()!=3 {return Err("three building bodies required".into())}
            for (dest,value) in bodies.iter_mut().zip(refs) {if !value.is_null() {*dest=Some(body(value)?);}}
            let trap=input.get("trap").filter(|v|!v.is_null()).map(|v|->Result<_,String> {Ok(props::Trap {
                pad_sphere:Bounds {center:vector::<3>(&v["center"])?,radius:number(v.get("radius"))?},
                point:vector::<3>(&v["point"])?,effect_at:vector::<3>(&v["effectAt"])?,
                build_frames:v.get("buildFrames").filter(|v|!v.is_null()).map(|v|number(Some(v))).transpose()?,
                trigger_frames:v.get("triggerFrames").filter(|v|!v.is_null()).map(|v|number(Some(v))).transpose()?,
            })}).transpose()?;
            let binding=props::Binding {index:integer(input.get("index"))?,target,member:member(&input["member"])?,bodies,
                generation:input.get("life").filter(|v|!v.is_null()).map(|v|number(Some(v))).transpose()?,
                team:number(input.get("team"))?,target_team:number(input.get("targetTeam"))?,category:integer(input.get("category"))?,kind:number(input.get("kind"))?,
                damaged:input["damaged"].as_bool().unwrap_or(false),crew:input["crew"].as_bool().unwrap_or(false),trap};
            let row=buildings.prepare(desc,models,binding,optional_id(input.get("explosion"))?,|text|strings.intern_str(text)).map_err(|e|e.to_string())?;
            Ok(vec![("f3_building",vec![],typed(F3BuildingSetup::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_recon_owners" => Ok(vec![("f3_recon_owners",vec![integer(input.get("capacity"))? as f64],Call::default())]),
        "f3_recon_command" => {
            let action=F3ReconAction::NAMES.iter().position(|name|Some(*name)==input["action"].as_str()).ok_or("recon action required")?;
            let row=recon::binding(F3ReconAction::from_u8(action as u8).ok_or("invalid recon action")?,integer(input.get("ownerKey"))?,key(input)?,
                integer(input.get("seed"))?);
            Ok(vec![("f3_recon_command",vec![],typed(F3ReconBinding::WORDS,|r,h|row.put(r,h)))])
        }
        "f3_item_class" => {
            let source = integer(input.get("source"))? as usize;
            let item = defs.items.get(source).ok_or("item source out of range")?;
            let mesh = item.record.get("GeometryName").filter(|v|v.truthy()).and_then(ContentValue::as_str)
                .and_then(|name|content.weapon_models.get(name)).is_some();
            let model = ModelAnswer {mesh,radius:item.radius,center:item.pickup_center.map(|p|[p.x,p.y,p.z])};
            let row = groups.prepare(&item.record,DefId(integer(input.get("definition"))?),
                input.get("pool").filter(|v|!v.is_null()).map(|v|number(Some(v))).transpose()?,model);
            Ok(vec![("f3_item_class",vec![],typed(F3ItemClass::WORDS,|r,h|row.put(r,h)))])
        }
        _ => {
            let name = input["class"].as_str().unwrap_or(&content.battle.unit);
            let class = defs.classes.iter().find(|c|defs.names.text(c.name)==name).ok_or("unit class not found")?;
            match export {
                "f3_body_skeleton" => {
                    let row = units::class_skeleton(content,class,key(input)?,|s|strings.intern_str(s)).map_err(|e|e.to_string())?;
                    Ok(vec![("f3_body_skeleton",vec![],typed(F3BodySkeleton::WORDS,|r,h|row.put(r,h)))])
                }
                "units_droideka_class" => {
                    let clips = input["clips"].as_object().ok_or("resolved clips required")?;
                    let mut ids = Vec::new();
                    for (name,id) in clips { ids.push((name.as_str(),integer(Some(id))?)); }
                    let row = units::droideka_class(&class.record,|name|ids.iter().find(|(n,_)|*n==name).map(|(_,id)|*id));
                    Ok(vec![("units_droideka_class",vec![],typed(F3DroidekaClass::WORDS,|r,h|row.put(r,h)))])
                }
                "f3_unit_items" => {
                    let id = DefId(integer(input.get("definition"))?);
                    let explosion = optional_id(input.get("explosion"))?;
                    let mut items = Vec::new();
                    if let Some(bindings) = input["items"].as_object() {
                        for (name,id) in bindings {items.push((name.as_str(),DefId(integer(Some(id))?)));}
                    }
                    let row = units::unit_items(&class.record,id,explosion,|name|Ok(items.iter().find(|(n,_)|*n==name).map(|(_,id)|*id)),|s|strings.intern_str(s))
                        .map_err(|e|e.to_string())?;
                    Ok(vec![("f3_unit_items",vec![],typed(F3UnitItems::WORDS,|r,h|row.put(r,h)))])
                }
                "f3_hit_policy" | "f3_dispenser_binding" | "f3_weapon_services" => {
                    let index = integer(input.get("weapon"))? as usize;
                    let source = class.weapons.get(index).and_then(|&i|defs.weapons.get(i)).ok_or("weapon source out of range")?;
                    let levels = units::weapon_levels(&source.record).map_err(|e|e.to_string())?;
                    let index = input.get("level").map(|v|integer(Some(v))).transpose()?.unwrap_or(0) as usize;
                    let level = levels.get(index).ok_or("charge level out of range")?;
                    match export {
                        "f3_hit_policy" => {
                            let def = level.get("ordnance").ok_or("weapon has no ordnance")?;
                            let row = units::hit_policy(DefId(integer(input.get("definition"))?),def);
                            Ok(vec![("f3_hit_policy",vec![],typed(F3HitPolicy::WORDS,|r,h|row.put(r,h)))])
                        }
                        "f3_dispenser_binding" => {
                            let row = units::dispenser_binding(key(input)?,DefId(integer(input.get("definition"))?),index as u32,level,|s|strings.intern_str(s));
                            Ok(vec![("f3_dispenser_binding",vec![],typed(F3DispenserBinding::WORDS,|r,h|row.put(r,h)))])
                        }
                        _ => {
                            let bindings = input["levels"].as_array().ok_or("level bindings required")?;
                            if bindings.len()!=levels.len() {return Err("one binding for each charge level required".into())}
                            let mut row = Vec::new();
                            for (level,binding) in levels.iter().zip(bindings) {
                                let service = units::weapon_service(&source.record,level,optional_id(binding.get("ordnance"))?.map(DefId),
                                    optional_id(binding.get("item"))?.map(DefId),|s|strings.intern_str(s),binding["hasLightning"].as_bool().unwrap_or(false));
                                row.extend(typed(F3WeaponService::WORDS,|r,h|service.put(r,h)).row);
                            }
                            Ok(vec![("f3_weapon_services",vec![integer(input.get("definition"))? as f64,levels.len() as f64],Call {row,..Default::default()})])
                        }
                    }
                }
                _ => Err(format!("unsupported unit export: {export}")),
            }
        }
    }
}


// The call log supplies owner ids and query answers, while these builders
// still read every authored transform, policy and drop from native content.
// Import rows name the class just constructed; generated Unit decoding keeps
// this observer independent of the store layout's changing field offsets.
fn recorded_words(value: &Value) -> Result<Vec<f64>,String> {
    value.as_array().ok_or("recorded words required")?.iter().map(|v|number(Some(v))).collect()
}
fn base64(text: &str) -> Result<Vec<u8>,String> {
    let mut out=Vec::new(); let mut bits=0u32;let mut count=0;
    for byte in text.bytes() {
        if byte==b'=' {break}
        let n=match byte {b'A'..=b'Z'=>byte-b'A',b'a'..=b'z'=>byte-b'a'+26,b'0'..=b'9'=>byte-b'0'+52,b'+'=>62,b'/'=>63,
            b'\r'|b'\n'|b' '=>continue,_=>return Err("invalid recorded base64".into())};
        bits=(bits<<6)|n as u32;count+=6;if count>=8 {count-=8;out.push((bits>>count) as u8);}
    }
    Ok(out)
}
struct Import {start:usize, bytes:Vec<u8>, count:usize, stride:usize}
impl Import {
    fn patch(&mut self,value:&Value)->Result<(),String> {
        for write in value["writes"].as_array().into_iter().flatten() {
            let at=write["offset"].as_u64().ok_or("write offset missing")? as usize;
            let bytes=base64(write["bytes"].as_str().ok_or("write bytes missing")?)?;
            let left=at.max(self.start);let right=(at+bytes.len()).min(self.start+self.bytes.len());
            if left<right {self.bytes[left-self.start..right-self.start].copy_from_slice(&bytes[left-at..right-at]);}
        }
        Ok(())
    }
    fn words(&self)->Vec<f64> {self.bytes.chunks_exact(8).map(|b|f64::from_le_bytes(b.try_into().expect("eight bytes"))).collect()}
}
fn weapon_refs(words:&[f64])->Result<Vec<u32>,String> {
    fn visit(words:&[f64],at:&mut usize,out:&mut Vec<u32>)->Result<(),String> {
        let tag=*words.get(*at).ok_or("short class value")? as u32;*at+=1;
        match tag {
            0..=3|9=>{},4|5=>{*at+=1;},8=>{out.push(*words.get(*at).ok_or("short weapon ref")? as u32);*at+=1;},
            6|7=>{let n=*words.get(*at).ok_or("short class list")? as usize;*at+=1;for _ in 0..n {if tag==7 {*at+=1;}visit(words,at,out)?;}},
            _=>return Err("unknown class value tag".into()),
        }
        if *at>words.len() {return Err("short class field".into())}Ok(())
    }
    let mut out=Vec::new();visit(words,&mut 0,&mut out)?;Ok(out)
}
// Native owners may intern strings without a host sim_str_intern call.
// Keep recorded ids directly instead of renumbering the observed host prefix.
#[derive(Default)]
struct RecordedStrings { names:std::collections::HashMap<String,u32>, ids:std::collections::HashMap<u32,String> }
impl RecordedStrings {
    fn bind(&mut self,id:u32,text:&str)->Result<(),String> {
        if self.names.get(text).is_some_and(|known|*known!=id) || self.ids.get(&id).is_some_and(|known|known!=text) {
            return Err(format!("inconsistent recorded string binding for {text}"))
        }
        self.names.insert(text.to_owned(),id);self.ids.insert(id,text.to_owned());Ok(())
    }
    fn intern_str(&mut self,text:&str)->StrId {StrId(self.names.get(text).copied().unwrap_or(u32::MAX))}
}
fn recon_source<'a>(id:u32,weapons:&'a std::collections::HashMap<u32,ContentValue>,guns:&std::collections::HashMap<u32,(u32,usize)>)->Option<&'a ContentValue> {
    weapons.get(&id).or_else(|| {let (parent,index)=guns.get(&id)?;weapons.get(parent)?.get("droid")?.get("weapons")?.at(*index)})
}
// Resolve only string handles from the recorded value stream. Tags, property
// order, lengths and every numeric value are still produced from content.
fn bind_value_strings(source:&ContentValue,words:&[f64],strings:&mut RecordedStrings)->Result<(),String> {
    fn visit(source:&ContentValue,words:&[f64],at:&mut usize,depth:usize,strings:&mut RecordedStrings)->Result<(),String> {
        let tag=*words.get(*at).ok_or("short recon value")? as u32;*at+=1;
        match source {
            ContentValue::String(text) if tag==5=>{strings.bind(*words.get(*at).ok_or("short recon string")? as u32,text)?;*at+=1;},
            ContentValue::Number(_) if tag==4=>{*at+=1;},ContentValue::Null if tag==1=>{},
            ContentValue::Bool(v) if tag==if *v{3}else{2}=>{},
            ContentValue::Array(values) if tag==6&&depth<12=>{
                if words.get(*at).copied()!=Some(values.len() as f64){return Err("recon array length differs".into())}*at+=1;
                for value in values.iter(){visit(value,words,at,depth+1,strings)?;}
            }
            ContentValue::Object(object) if tag==7&&depth<12=>{
                if words.get(*at).copied()!=Some(object.entries().count() as f64){return Err("recon property count differs".into())}*at+=1;
                for (key,value) in object.entries(){strings.bind(*words.get(*at).ok_or("short recon key")? as u32,key)?;*at+=1;visit(value,words,at,depth+1,strings)?;}
            }
            ContentValue::Array(_)|ContentValue::Object(_) if tag==9&&depth>=12=>{},
            _=>return Err("recon content type differs".into()),
        }Ok(())
    }
    let mut at=0;visit(source,words,&mut at,0,strings)?;if at!=words.len(){return Err("recon value length differs".into())}Ok(())
}
// Earlier F1 rows can identify an item whose first use precedes the first
// soldier import. Match authored fields, including nested explosion payloads;
// never use the F3 item row to select a source definition.
fn fields_match(source:&ContentValue,fields:&[&str],words:&[f64],strings:&RecordedStrings,
    explosions:&std::collections::HashMap<u32,Vec<f64>>,depth:usize)->bool {
    if depth>32 || words.len()!=fields.len()*2 {return false}
    fields.iter().zip(words.chunks_exact(2)).all(|(field,pair)|match source.get(field) {
        None=>pair[0]==0.0,
        Some(ContentValue::Null)=>pair[0]==1.0,
        Some(ContentValue::Bool(value))=>pair[0]==if *value{3.0}else{2.0},
        Some(ContentValue::Number(value))=>pair[0]==4.0&&pair[1].to_bits()==value.to_bits(),
        Some(ContentValue::String(value))=>pair[0]==5.0&&strings.ids.get(&(pair[1] as u32)).is_some_and(|known|known==value.as_ref()),
        Some(value)=>pair[0]==6.0&&explosions.get(&(pair[1] as u32)).is_some_and(|row|fields_match(value,
            bfe_sim::world::admit::rows::ordnance::EXPLOSION_FIELDS,row,strings,explosions,depth+1)),
    })
}
fn expected_item(content:&Content,source:&ContentValue,binding:&F3ItemClass)->Call {
    let model=source.get("GeometryName").filter(|v|v.truthy()).and_then(ContentValue::as_str).and_then(|n|content.weapon_models.get(n));
    let (radius,center)=bfe_content::derived::item_radius(model);
    let model=ModelAnswer {mesh:model.is_some(),radius,center:center.map(|p|[p.x,p.y,p.z])};
    let mut item=units::ItemGroups::default().prepare(source,binding.def,Some(binding.pool),model);
    // Group id and pool capacity are the earlier item's assigned namespace
    // and mission limit. Every model, gain and lifetime is native content.
    item.base=binding.base;
    typed(F3ItemClass::WORDS,|r,h|item.put(r,h))
}
fn same_words(a:&Call,b:&Call)->bool {
    a.row.len()==b.row.len()&&a.heap.len()==b.heap.len()
        &&a.row.iter().chain(&a.heap).zip(b.row.iter().chain(&b.heap)).all(|(a,b)|a.to_bits()==b.to_bits())
}
fn check(content:&Content,defs:&Defs,body_inputs:Option<&str>)->Result<(),String> {
    use std::collections::BTreeMap;
    let mut records=Vec::new();let mut complete=false;
    for line in std::io::stdin().lock().lines() {
        let line=line.map_err(|e|e.to_string())?;if line.trim().is_empty(){continue}
        let value:Value=serde_json::from_str(&line).map_err(|e|e.to_string())?;
        if value["type"]=="end" {complete=true;}
        if value["type"]=="call" {records.push(value);}
    }
    if !complete {return Err("recording has no completion marker".into())}
    if records.is_empty() {return Err("recording contains no export calls".into())}
    // Direct-bank inputs can be recovered by a validated replay of the
    // original WASM trace. Key them by call sequence, never by row values.
    if let Some(path)=body_inputs {
        let text=std::fs::read_to_string(path).map_err(|e|e.to_string())?;
        let sidecar:Value=serde_json::from_str(&text).map_err(|e|e.to_string())?;
        if sidecar["verified"]!=true {return Err("body-name replay was not verified".into())}
        let mut inputs=BTreeMap::new();
        for battle in sidecar["battles"].as_array().ok_or("body-name battles missing")? {
            let world=battle["battle"].as_u64().ok_or("body-name battle missing")?;
            for call in battle["calls"].as_array().ok_or("body-name calls missing")? {
                let seq=call["sequence"].as_u64().ok_or("body-name sequence missing")?;
                if inputs.insert((world,seq),call["words"].clone()).is_some(){return Err("duplicate body-name input".into())}
            }
        }
        for event in &mut records {if event["export"]=="f3_body_names" {
            let key=(event["battle"].as_u64().ok_or("call battle missing")?,event["sequence"].as_u64().ok_or("call sequence missing")?);
            event["args"]["words"]=inputs.remove(&key).ok_or("body-name replay input missing")?;
        }}
        if !inputs.is_empty(){return Err("unused body-name replay input".into())}
    }
    let mut battles:BTreeMap<u64,Vec<Value>>=BTreeMap::new();
    for event in records {battles.entry(event["battle"].as_u64().unwrap_or(0)).or_default().push(event);}
    for (battle,records) in battles {check_battle(content,defs,records,battle)?;}
    Ok(())
}
// The twice-in-one-process case restarts admission identities for each world.
// A new checker state also prevents one battle's imports naming another's rows.
fn check_battle(content:&Content,defs:&Defs,mut records:Vec<Value>,battle:u64)->Result<(),String> {
    use std::collections::{BTreeMap,HashMap};
    records.sort_by_key(|v|v["sequence"].as_u64().unwrap_or(u64::MAX));
    let mut strings=RecordedStrings::default();
    for event in &records {if let Some(v)=event.get("intern").filter(|v|!v.is_null()) {strings.bind(integer(v.get("id"))?,v["text"].as_str().ok_or("intern text missing")?)?;}}
    let mut unit_import:Option<Import>=None;let mut hull_import:Option<Import>=None;
    let mut call_room:Option<Import>=None;let mut placed_mapped=0;
    let mut current_hull=None;let mut hulls=HashMap::new();let mut hull_index=0usize;
    let mut body_kernels=HashMap::new();let mut collision_names=Vec::new();
    let mut registered_hull=None;let mut pending_hull_body=None;
    let mut current:Option<usize>=None;let mut class_weapons=HashMap::new();let mut weapons=HashMap::new();
    let mut ordnance_sources=HashMap::new();let mut item_sources=HashMap::new();let mut deferred=Vec::new();
    let mut item_fields=HashMap::new();let mut explosion_fields=HashMap::new();
    let mut ai_item_names=HashMap::new();
    let mut f1_explosions=std::collections::BTreeSet::new();
    let mut initial_blasts=std::collections::VecDeque::new();
    let mut initial_impacts=false;let mut latest_explosion=None;let mut latest_combat=None;
    let mut mission_teams:Option<AiMissionTeams>=None;
    let mut buildings=props::Buildings::default();let mut building_sources=Vec::new();
    if let Some(data)=&content.battle.props {
        if let Some(instances)=data.get("instances").and_then(ContentValue::as_array) {
            for desc in instances {
                let base=desc.get("base").and_then(ContentValue::as_str).unwrap_or("");
                let model=desc.get("model").and_then(ContentValue::as_str).and_then(|n|data.get("models").and_then(|v|v.get(n)));
                if model.is_some() && ["trap","powerupstation","armedbuilding","commandpost"].contains(&base) {building_sources.push(desc);}
            }
        }
    }
    let mut recon_classes=HashMap::new();let mut recon_guns=HashMap::new();let mut recon_strikes=HashMap::new();
    for event in &records {if event["export"]=="f3_recon_weapon" {
        let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;
        let binding=F3ReconWeapon::take(&r,&h);recon_classes.insert(binding.class,binding.weapon.0);
    }}
    for event in &records {if event["export"]=="f3_recon_class" {
        let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;
        let binding=F3ReconClass::take(&r,&h);
        if let Some(parent)=recon_classes.get(&binding.id) {for (index,gun) in binding.loadout.iter().enumerate(){recon_guns.insert(gun.0,(*parent,index));}}
    }}
    for event in &records {if event["export"]=="f3_recon_level" {
        let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;
        let binding=F3ReconLevel::take(&r,&h);if let Some(strike)=binding.strike {recon_strikes.insert(strike,(binding.weapon.0,binding.level));}
    }}
    for event in &records {
        let name=event["export"].as_str().unwrap_or("");
        if name=="f3_weapon_services" {
            let a=recorded_words(&event["arguments"])?;let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;
            for (index,part) in r.chunks_exact(F3WeaponService::WORDS).enumerate(){let service=F3WeaponService::take(part,&h);if let Some(id)=service.ordnance {ordnance_sources.insert(id.0,(a[0] as u32,index as usize,false));}}
        }else if name=="f3_recon_level" {
            let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;let level=F3ReconLevel::take(&r,&h);
            if let Some(id)=level.ordnance{ordnance_sources.insert(id.0,(level.weapon.0,level.level as usize,false));}
        }else if name=="f3_recon_strike" {
            let r=recorded_words(&event["args"]["words"])?;let h=recorded_words(&event["args"]["heap"])?;let strike=F3ReconStrike::take(&r,&h);
            if let Some((weapon,level))=recon_strikes.get(&strike.id){ordnance_sources.insert(strike.ordnance.0,(*weapon,*level as usize,true));}
        }
    }
    let mut checked=BTreeMap::new();let mut skipped=BTreeMap::new();let mut differences=Vec::new();let mut missile=0;
    for event in records {
        if let Some(import)=unit_import.as_mut(){import.patch(&event)?;}
        if let Some(import)=hull_import.as_mut(){import.patch(&event)?;}
        if let Some(room)=call_room.as_mut(){room.patch(&event)?;}
        let name=event["export"].as_str().ok_or("recorded export missing")?;
        let arguments=recorded_words(&event["arguments"])?;
        let row=recorded_words(&event["args"]["words"])?;let heap=recorded_words(&event["args"]["heap"])?;
        // Some exports have no recorder argument annotation. The preceding
        // call-room allocation and host writes still carry their exact input.
        if name=="sim_call_room" && placed_mapped<content.placed_items.len() {
            let start=number(event["returned"].get("scalar"))? as usize+BlockHeader::WORDS*8;
            let mut words=recorded_words(&event["returned"]["block"]["words"])?;
            words.extend(recorded_words(&event["returned"]["block"]["heap"])?);
            call_room=Some(Import {start,bytes:words.iter().flat_map(|word|word.to_le_bytes()).collect(),count:arguments[0] as usize,stride:1});
        }
        if name=="sim_import_room" && arguments.first().copied()==Some(KIND_UNIT as f64) {
            let start=number(event["returned"].get("scalar"))? as usize+BlockHeader::WORDS*8;
            let mut words=recorded_words(&event["returned"]["block"]["words"])?;
            words.extend(recorded_words(&event["returned"]["block"]["heap"])?);
            unit_import=Some(Import {start,bytes:words.iter().flat_map(|n|n.to_le_bytes()).collect(),count:arguments[1] as usize,stride:arguments[2] as usize});
        }
        if name=="sim_import_room" && arguments.first().copied()==Some(KIND_VEHICLE as f64) {
            let start=number(event["returned"].get("scalar"))? as usize+BlockHeader::WORDS*8;
            let mut words=recorded_words(&event["returned"]["block"]["words"])?;
            words.extend(recorded_words(&event["returned"]["block"]["heap"])?);
            hull_import=Some(Import {start,bytes:words.iter().flat_map(|n|n.to_le_bytes()).collect(),count:arguments[1] as usize,stride:arguments[2] as usize});
        }
        if name=="sim_import" && arguments.first().copied()==Some(KIND_VEHICLE as f64) {
            let import=hull_import.take().ok_or("hull import has no prepared block")?;
            let words=import.words();let heap_start=import.count*import.stride;
            if words.len()<heap_start||import.stride<Vehicle::IMPORT_STRIDE{return Err("short hull import".into())}
            for record in words[..heap_start].chunks_exact(import.stride) {
                let mut hull=Vehicle::default();
                if import.stride==Vehicle::FULL_STRIDE {hull.take_full(record,&words[heap_start..]);}
                else {hull.take_import(record,&words[heap_start..]);}
                current_hull=Some(hull);
            }
        }
        if name=="f2_register" {
            let hull=current_hull.as_ref().ok_or("registered hull has no imported facts")?.clone();
            let prepared=arguments.get(2).copied().unwrap_or(0.0)!=0.0;
            let index=if prepared{0}else{let index=hull_index;hull_index+=1;index};
            registered_hull=Some(arguments[0] as u32);
            hulls.insert(arguments[0] as u32,(hull,prepared,index));
        }
        if name=="f2_bodies" {
            let bodies=F2Bodies::take(&row,&heap).bodies;
            for body in &bodies {body_kernels.insert((body.body.slot,body.body.gen),body.kernel);}
            collision_names=units::body_names(bodies.into_iter().map(|body|(body.body,body.kernel)));
        }
        if name=="f2_prepare_body" {
            let body=&hulls.get(&registered_hull.ok_or("private body has no registered hull")?).ok_or("private hull facts missing")?.0.body;
            body_kernels.insert((body.slot,body.gen),arguments[0] as u32);
        }
        if name=="f3_hull" {
            let binding=F3HullSetup::take(&row,&heap);
            pending_hull_body=hulls.get(&binding.hull.slot).map(|(hull,_,_)|hull.body);
        }
        if name=="sim_import" && arguments.first().copied()==Some(KIND_UNIT as f64) {
            let import=unit_import.take().ok_or("unit import has no prepared block")?;
            let words=import.words();let heap_start=import.count*import.stride;
            if words.len()<heap_start || import.stride<Unit::IMPORT_STRIDE {return Err("short unit import".into())}
            for record in words[..heap_start].chunks_exact(import.stride) {
                let mut unit=Unit::default();
                if import.stride==Unit::FULL_STRIDE {unit.take_full(record,&words[heap_start..]);}
                else {unit.take_import(record,&words[heap_start..]);}
                let name=strings.ids.get(&unit.class_name.0).ok_or_else(||format!("missing class string binding {}",unit.class_name.0))?;
                current=defs.classes.iter().position(|c|defs.names.text(c.name)==name.as_str());
                if let Some(class)=current {
                    if let Some(ids)=class_weapons.get(&unit.class_def.0) {
                        let ids:&Vec<u32>=ids;
                        for (id,source) in ids.iter().zip(&defs.classes[class].weapons) {weapons.insert(*id,defs.weapons[*source].record.clone());}
                    }
                }
            }
        }
        if name=="cmb_class" {class_weapons.insert(arguments[0] as u32,weapon_refs(&row)?);}
        if name=="f2_firing_adopt" {
            let birth=FfBirth::take(&row,&heap);
            if let Some((hull,_,_))=hulls.get(&birth.hull.slot) {
                if let Some(name)=strings.ids.get(&hull.name.0) {
                    if let Some(class)=defs.vehicles.iter().find(|c|defs.names.text(c.name)==name) {
                        for (seat,source) in birth.seats.iter().zip(&class.seats){for (weapon,source) in seat.weapons.iter().zip(&source.weapons){
                            if let Some(def)=source.get("def"){weapons.insert(weapon.state.def.0,def.clone());}
                            if let Some(id)=weapon.ordnance {ordnance_sources.insert(id.0,(weapon.state.def.0,weapon.state.level as usize,false));}
                        }}
                    }
                }
            }
        }
        if name=="f2_turret_adopt" {
            let birth=F2TurretBirth::take(&row,&heap);
            let source=building_sources.iter().filter(|v|v.get("base").and_then(ContentValue::as_str)==Some("armedbuilding")).nth(birth.turret.state.index as usize);
            if let (Some(weapon),Some(source))=(birth.weapon,source.and_then(|v|v.get("weapon"))) {
                weapons.insert(weapon.def.0,source.clone());if let Some(id)=birth.ordnance {ordnance_sources.insert(id.0,(weapon.def.0,weapon.level as usize,false));}
            }
        }
        if name=="items_def" {item_fields.insert(arguments[0] as u32,row.clone());}
        if name=="ord_explosion" {
            let id=arguments[0] as u32;explosion_fields.insert(id,row.clone());
            f1_explosions.insert(id);latest_explosion=Some(id);
        }
        if name=="cmb_explosion" {latest_combat=Some(arguments[0] as u32);}
        if name=="f3_init" {
            initial_impacts=true;initial_blasts=f1_explosions.iter().copied().collect();
        }
        if name=="f3_impact_rules" {initial_impacts=false;}
        if name=="ai_mission_teams" && arguments.first()==Some(&1.0) {
            let returned=&event["returned"]["block"];
            let r=recorded_words(&returned["words"])?;let h=recorded_words(&returned["heap"])?;
            mission_teams=Some(AiMissionTeams::take(&r,&h));
        }
        if name=="ai_world_content" {
            for item in AiWorldContent::take(&row,&heap).items {
                if let Some(name)=item.class_name.and_then(|id|strings.ids.get(&id.0)) {ai_item_names.insert(item.def.0,name.clone());}
            }
        }
        if name=="items_spawn" {
            let borrowed;
            let (input,indirect)=if row.len()>=SpawnArgs::WORDS {(&row[..],&heap[..])}
            else {
                let room=call_room.as_ref().filter(|room|room.count==SpawnArgs::WORDS).ok_or("placed item spawn has no preceding argument loan")?;
                borrowed=room.words();
                if borrowed.len()<SpawnArgs::WORDS {return Err("placed item spawn loan is short".into())}
                borrowed.split_at(SpawnArgs::WORDS)
            };
            let binding=SpawnArgs::take(input,indirect);
            if let Some(source)=content.placed_items.iter().find(|p|[p.position.x,p.position.y,p.position.z]==binding.position){item_sources.insert(binding.def.0,source.item.clone());}
            placed_mapped+=1;if placed_mapped>=content.placed_items.len() {call_room=None;}
        }
        if name=="f3_hit_policy"||name=="f3_item_class" {deferred.push(event.clone());continue}
        let expected:Option<Call>=match name {
            "f3_init"=>{
                let water=content.battle.world.get("water").filter(|v|v.truthy()).map(|v|bfe_content::js::to_number(v.get("height"))+0.0);
                Some(units::Init::new(arguments[0] as u32,water,arguments[2] as u32).call())
            }
            "f3_missile_classes"=>{
                let rows=units::missile_classes(|text|strings.intern_str(text));
                let item=rows.get(missile).ok_or("too many missile vocabulary calls")?;missile+=1;
                let mut out=typed(F3MissileClass::WORDS,|r,h|item.put(r,h));out.args=vec![1.0];Some(out)
            }
            "f3_body_names"=>{
                let hull=pending_hull_body.take();
                // An empty world has a complete zero-count input even though
                // its row is empty; only nonzero unannotated loans are missing.
                if row.is_empty()&&arguments.first()!=Some(&0.0){None}else{
                    // Private hull refs/kernels came from registration and
                    // f2_prepare_body. The complete list keeps F2 world order.
                    let bodies=if let Some(body)=hull {
                        let kernel=*body_kernels.get(&(body.slot,body.gen)).ok_or("named hull has no earlier kernel binding")?;
                        units::body_names([(body,kernel)])
                    }else{collision_names.clone()};
                    Some(units::Entry::BodyNames(bodies).encode().map_err(|e|e.to_string())?.into_call().1)
                }
            }
            "f3_blast_binding"=>{
                // Both handles were assigned by earlier owners. Initial F1
                // definitions are scanned in id order; subsequent definitions
                // notify F3 immediately after their own ord_explosion export.
                let explosion=if initial_impacts {initial_blasts.pop_front()} else {latest_explosion.take()};
                match (explosion,latest_combat.take()) {
                    (Some(explosion),Some(combat))=>Some(typed(F3BlastBinding::WORDS,|r,h|units::blast_binding(explosion,combat).put(r,h))),
                    _=>None,
                }
            }
            "f3_recon_owners"=>Some(Call {args:vec![recon::owner_capacity(1) as f64],..Default::default()}),
            "f3_relations"=>{
                // missionTeams returns the roster table before Units.setTeams.
                // Registry's diagonal/neutral rules precede that table lookup.
                let count=mission_teams.as_ref().map_or(0,|table|table.teams.len());
                let rows=units::relations(count,|a,b| {
                    if a==b {if a==0.0 {0.0}else{1.0}} else if a==0.0||b==0.0 {0.0}
                    else {mission_teams.as_ref().and_then(|table|table.teams.get(a as usize))
                        .and_then(|team|team.relations.get(b as usize)).copied().unwrap_or(-1.0)}
                }).map_err(|e|e.to_string())?;
                Some(units::Entry::Relations(rows).encode().map_err(|e|e.to_string())?.into_call().1)
            }
            "f3_registry_prepare"=>Some(units::registry_prepare()),
            // After attachF3 the factory's newly inserted members are bodies;
            // hull/objective targets already occupied the initial registry.
            "f3_registry_add"=>Some(units::registry_add(KIND_UNIT).map_err(|e|e.to_string())?),
            "f3_impact_rules"=>Some(typed(F3ImpactRules::WORDS,|r,h|units::impact_rules(None).put(r,h))),
            "f3_body_skeleton"=>{
                let class=current.ok_or("skeleton has no observed unit class")?;
                let binding=F3BodySkeleton::take(&row,&heap);
                let skeleton=units::class_skeleton(content,&defs.classes[class],binding.key,|s|strings.intern_str(s)).map_err(|e|e.to_string())?;
                Some(typed(F3BodySkeleton::WORDS,|r,h|skeleton.put(r,h)))
            }
            "units_droideka_class"=>{
                let class=current.ok_or("droideka has no observed unit class")?;let bindings=F3DroidekaClass::take(&row,&heap);
                let item=units::droideka_class(&defs.classes[class].record,|name|units::DROIDEKA_CLIPS.iter().position(|n|*n==Some(name))
                    .and_then(|i|if bindings.clips[i]>=0.0 {Some(bindings.clips[i] as u32)}else{None}));
                Some(typed(F3DroidekaClass::WORDS,|r,h|item.put(r,h)))
            }
            "f3_weapon_services"=>{
                if let Some(base)=weapons.get(&(arguments[0] as u32)) {
                    let levels=units::weapon_levels(base).map_err(|e|e.to_string())?;
                    if row.len()!=levels.len()*F3WeaponService::WORDS {return Err("service level count differs".into())}
                    let mut out=Call {args:arguments.clone(),..Default::default()};
                    for (level,source) in levels.iter().zip(row.chunks_exact(F3WeaponService::WORDS)) {
                        let binding=F3WeaponService::take(source,&heap);
                        if let (Some(id),Some(source))=(binding.item,level.get("item")){item_sources.insert(id.0,source.clone());}
                        let bank=base.get("AnimationBank").filter(|v|v.truthy()).and_then(ContentValue::as_str).unwrap_or("rifle");
                        strings.bind(binding.bank.0,bank)?;
                        let lightning=level.get("ordnance").and_then(|v|v.get("LightningEffect")).and_then(ContentValue::as_str).unwrap_or("");
                        strings.bind(binding.lightning.0,lightning)?;
                        let has=units::has_lightning(&content.battle,lightning);
                        let service=units::weapon_service(base,level,binding.ordnance,binding.item,|s|strings.intern_str(s),has);
                        out.row.extend(typed(F3WeaponService::WORDS,|r,h|service.put(r,h)).row);
                    }Some(out)
                }else{None}
            }
            "f3_unit_items"=>{
                let class=current.ok_or("unit items have no observed class")?;let class=&defs.classes[class];let binding=F3UnitItems::take(&row,&heap);
                if class.drops.len()!=binding.drops.len(){return Err("drop count differs".into())}
                for (source,recorded) in class.drops.iter().zip(&binding.drops) {
                    if source.item.is_some()!=recorded.def.is_some(){return Err("side drop resolution differs".into())}
                    if let (Some(source),Some(id))=(source.item,recorded.def){item_sources.insert(id.0,defs.items[source].record.clone());}
                }
                let mut at=0;let item=units::unit_items(&class.record,binding.class,binding.explosion,|_|{let id=binding.drops[at].def;at+=1;Ok(id)},|s|strings.intern_str(s)).map_err(|e|e.to_string())?;
                Some(typed(F3UnitItems::WORDS,|r,h|item.put(r,h)))
            }
            "f3_recon_weapon"=>{
                let binding=F3ReconWeapon::take(&row,&heap);
                if let Some(source)=weapons.get(&binding.weapon.0) {
                    let item=F3ReconWeapon {weapon:binding.weapon,class:binding.class,immediate:source.get("SwitchImmediately").is_some_and(ContentValue::truthy)};
                    Some(typed(F3ReconWeapon::WORDS,|r,h|item.put(r,h)))
                }else{None}
            }
            "f3_recon_class"=>{
                let binding=F3ReconClass::take(&row,&heap);
                if let Some(source)=recon_classes.get(&binding.id).and_then(|id|weapons.get(id)).and_then(|v|v.get("droid")) {
                    bind_value_strings(source,&binding.value,&mut strings)?;
                    let value=recon::value_words(Some(source),&mut |s|strings.intern_str(s));
                    let count=source.get("weapons").and_then(ContentValue::as_array).map_or(0,|v|v.len());
                    if count!=binding.loadout.len(){return Err("recon loadout count differs".into())}
                    let item=F3ReconClass {id:binding.id,value,loadout:binding.loadout,explosion:binding.explosion,muzzle:recon::muzzle(&content.battle,source)};
                    Some(typed(F3ReconClass::WORDS,|r,h|item.put(r,h)))
                }else{None}
            }
            "f3_recon_level"=>{
                let binding=F3ReconLevel::take(&row,&heap);
                if let Some(source)=recon_source(binding.weapon.0,&weapons,&recon_guns) {
                    let levels=units::weapon_levels(source).map_err(|e|e.to_string())?;
                    let level=levels.get(binding.level as usize).ok_or("recon level missing")?;
                    bind_value_strings(level,&binding.value,&mut strings)?;
                    let item=F3ReconLevel {weapon:binding.weapon,level:binding.level,value:recon::value_words(Some(level),&mut |s|strings.intern_str(s)),ordnance:binding.ordnance,strike:binding.strike};
                    Some(typed(F3ReconLevel::WORDS,|r,h|item.put(r,h)))
                }else{None}
            }
            "f3_recon_strike"=>{
                let binding=F3ReconStrike::take(&row,&heap);
                if let Some((gun,index))=recon_strikes.get(&binding.id) {
                    if let Some(source)=recon_source(*gun,&weapons,&recon_guns) {
                        let levels=units::weapon_levels(source).map_err(|e|e.to_string())?;
                        let level=levels.get(*index as usize).ok_or("recon strike level missing")?;
                        bind_value_strings(level,&binding.value,&mut strings)?;
                        let item=F3ReconStrike {id:binding.id,ordnance:binding.ordnance,value:recon::value_words(Some(level),&mut |s|strings.intern_str(s))};
                        Some(typed(F3ReconStrike::WORDS,|r,h|item.put(r,h)))
                    }else{None}
                }else{None}
            }
            "f3_dispenser_binding"=>{
                let binding=F3DispenserBinding::take(&row,&heap);
                if let Some(base)=weapons.get(&binding.weapon.0) {
                    let levels=units::weapon_levels(base).map_err(|e|e.to_string())?;
                    let source=levels.get(binding.level as usize).ok_or("dispenser charge level missing")?;
                    let item=units::dispenser_binding(binding.key,binding.weapon,binding.level,source,|s|strings.intern_str(s));
                    Some(typed(F3DispenserBinding::WORDS,|r,h|item.put(r,h)))
                }else{None}
            }
            "f3_hull"=>{
                let binding=F3HullSetup::take(&row,&heap);
                if let Some((hull,prepared,index))=hulls.get(&binding.hull.slot) {
                    let name=strings.ids.get(&hull.name.0).ok_or("missing hull class name")?;
                    let class=defs.vehicles.iter().find(|c|defs.names.text(c.name)==name).ok_or("hull class not found")?;
                    let template=class.template.and_then(|i|content.vehicle_templates.get(i)).ok_or("hull template missing")?;
                    let sphere=template.rig.get("sphere").filter(|v|v.truthy()).map(|v|std::array::from_fn(|i|bfe_content::js::to_number(v.at(i))));
                    let health=class.record.get("settings").and_then(|v|v.get("HealthType")).filter(|v|v.truthy()).and_then(ContentValue::as_str).unwrap_or("vehicle").to_lowercase();
                    let health=["person","animal","droid","vehicle","building"].iter().position(|h|*h==health).unwrap_or(0) as u32;
                    let key=if *prepared {String::new()} else {format!("v{index}")};
                    let identity=units::HullIdentity {hull:binding.hull,member:binding.member,category:binding.category,kind:binding.kind,key:strings.intern_str(&key),health};
                    let item=units::hull(identity,hull.size,hull.center,sphere,binding.explosion,binding.final_explosion);
                    Some(typed(F3HullSetup::WORDS,|r,h|item.put(r,h)))
                }else{None}
            }
            "f3_building"=>{
                let binding=F3BuildingSetup::take(&row,&heap);let source=building_sources.get(binding.index as usize).ok_or("building index differs")?;
                let trap=binding.trap.as_ref().map(|t|props::Trap {pad_sphere:binding.trap_sphere.clone().unwrap_or_default(),point:t.settings.point,effect_at:t.settings.effect_at,
                    build_frames:t.settings.build_frames,trigger_frames:t.settings.trigger_frames});
                let bodies:[Option<Ref<Body>>;3]=binding.bodies.as_slice().try_into().map_err(|_|"building body count differs")?;
                let facts=props::Binding {index:binding.index,target:binding.target,member:binding.member,bodies,generation:binding.state.generation,
                    team:binding.state.team,target_team:binding.state.target_team,category:binding.category,kind:binding.kind,damaged:binding.damaged,crew:binding.live.crew,trap};
                let models=content.battle.props.as_ref().and_then(|p|p.get("models")).ok_or("building models missing")?;
                let item=buildings.prepare(source,models,facts,binding.explosion,|s|strings.intern_str(s)).map_err(|e|e.to_string())?;
                Some(typed(F3BuildingSetup::WORDS,|r,h|item.put(r,h)))
            }
            _=>None,
        };
        if let Some(expected)=expected {
            *checked.entry(name.to_owned()).or_insert(0usize)+=1;
            for (field,actual,reference) in [("arguments",expected.args.as_slice(),arguments.as_slice()),("row",expected.row.as_slice(),row.as_slice()),("heap",expected.heap.as_slice(),heap.as_slice())] {
                let word=(0..actual.len().max(reference.len())).find(|&i|actual.get(i).map(|n|n.to_bits())!=reference.get(i).map(|n|n.to_bits()));
                if let Some(word)=word {if differences.len()<12 {differences.push(json!({"export":name,"sequence":event["sequence"],"field":field,"word":word,
                    "actual":actual.get(word).map(|n|format!("{:016x}",n.to_bits())),"reference":reference.get(word).map(|n|format!("{:016x}",n.to_bits()))}));}}
            }
        }else if name.starts_with("f3_")||name=="units_droideka_class" {*skipped.entry(name.to_owned()).or_insert(0usize)+=1;}
    }
    // F1 ids are linked by the earlier fleet/weapon/drop bindings above.
    // These policies can precede the first unit, so compare them after that
    // structural source map is complete, without taking their values from F3.
    for event in deferred {
        let name=event["export"].as_str().ok_or("deferred export missing")?;
        let row=recorded_words(&event["args"]["words"])?;let heap=recorded_words(&event["args"]["heap"])?;
        let expected=if name=="f3_hit_policy" {
            let binding=F3HitPolicy::take(&row,&heap);
            if let Some((weapon,level,strike))=ordnance_sources.get(&binding.ordnance.0) {
                if let Some(source)=recon_source(*weapon,&weapons,&recon_guns) {
                    let levels=units::weapon_levels(source).map_err(|e|e.to_string())?;
                    levels.get(*level).and_then(|v|v.get(if *strike{"strike"}else{"ordnance"})).map(|source|typed(F3HitPolicy::WORDS,|r,h|units::hit_policy(binding.ordnance,source).put(r,h)))
                }else{None}
            }else{None}
        }else{
            let binding=F3ItemClass::take(&row,&heap);
            if let Some(source)=item_sources.get(&binding.def.0) {Some(expected_item(content,source,&binding))}
            else if let Some(fields)=item_fields.get(&binding.def.0) {
                let candidates=defs.items.iter().filter(|item|ai_item_names.get(&binding.def.0)
                    .is_none_or(|name|item.class.is_some_and(|class|defs.names.text(class)==name)))
                    .filter(|item|fields_match(&item.record,
                    bfe_sim::world::admit::rows::items::ITEM_FIELDS,fields,&strings,&explosion_fields,0))
                    .map(|item|expected_item(content,&item.record,&binding)).collect::<Vec<_>>();
                // Ambiguous F1 identities are usable only when every candidate
                // independently produces the same F3 payload bit for bit.
                candidates.first().filter(|first|candidates.iter().all(|other|same_words(first,other))).cloned()
            }else{None}
        };
        if let Some(expected)=expected {
            *checked.entry(name.to_owned()).or_insert(0usize)+=1;
            for (field,actual,reference) in [("row",expected.row.as_slice(),row.as_slice()),("heap",expected.heap.as_slice(),heap.as_slice())] {
                if let Some(word)=(0..actual.len().max(reference.len())).find(|&i|actual.get(i).map(|n|n.to_bits())!=reference.get(i).map(|n|n.to_bits())) {
                    if differences.len()<12 {differences.push(json!({"export":name,"sequence":event["sequence"],"field":field,"word":word,
                        "actual":actual.get(word).map(|n|format!("{:016x}",n.to_bits())),"reference":reference.get(word).map(|n|format!("{:016x}",n.to_bits()))}));}
                }
            }
        }else{*skipped.entry(name.to_owned()).or_insert(0usize)+=1;}
    }
    println!("{}",json!({"battle":battle,"checked":checked,"skipped":skipped,"differences":differences}));
    if differences.is_empty(){Ok(())}else{Err("unit admission rows differ".into())}
}

fn run() -> Result<(),String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or("content directory required")?;
    let flag = |name: &str,default: &str| args.windows(2).find(|p|p[0]==name).map(|p|p[1].clone()).unwrap_or_else(||default.into());
    let options = Options {map:flag("--map","nab2"),side:flag("--side","rep"),enhanced:args.iter().any(|s|s=="--enhanced"),
        heroes:args.iter().any(|s|s=="--heroes"),seed:flag("--seed","1").parse::<u32>().map_err(|e|e.to_string())?,unit:None,name:None};
    let files = DirFiles::new(root);
    let battle = Battle::load(&Loader::new(&files),&options.map,&options.query())?;
    let content = Content::build(battle)?; let defs = Defs::build(&content)?;
    if args.iter().any(|arg|arg=="--check") {
        let body_inputs=args.windows(2).find(|pair|pair[0]=="--body-names").map(|pair|pair[1].as_str());
        return check(&content,&defs,body_inputs)
    }
    let mut strings = Strings::new(); let mut groups = units::ItemGroups::default(); let mut buildings = props::Buildings::default(); let mut sequence = 0;
    for line in std::io::stdin().lock().lines() {
        let line = line.map_err(|e|e.to_string())?; if line.trim().is_empty() {continue}
        let input: Value = serde_json::from_str(&line).map_err(|e|e.to_string())?;
        if let Some(prefix) = input["strings"].as_array() {
            strings = Strings::new();
            for text in prefix {strings.intern_str(text.as_str().ok_or("string prefix entry is not text")?);}
        }
        let start = strings.list.len();
        for (export,arguments,call) in request(&input,&content,&defs,&mut strings,&mut groups,&mut buildings)? {
            println!("{}",json!({"type":"call","sequence":sequence,"battle":0,"phase":"admission","export":export,
                "arguments":hex(&arguments),"args":{"words":hex(&call.row),"heap":hex(&call.heap)}}));
            sequence += 1;
        }
        for (id,text) in strings.list.iter().enumerate().skip(start) {
            println!("{}",json!({"type":"intern","intern":{"id":id,"text":String::from_utf16_lossy(text)}}));
        }
    }
    Ok(())
}
fn main() { if let Err(error)=run() {eprintln!("{error}");std::process::exit(1);} }
