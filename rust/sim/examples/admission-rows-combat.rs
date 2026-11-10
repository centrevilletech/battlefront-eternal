//! Dump cold combat inputs as the admission recorder's JSON lines.
//!
//! admission-rows-combat CONTENT_DIR [--map M] [--side S] [--seed N]
//!     [--unit U] [--heroes] [--enhanced] [--requests]
//!
//! The default emits initial item pools and viewpoint combat. --requests
//! reads construction seams from stdin in their actual owner order:
//! {"op":"weapon|class|attach|ordnance|explosion|combat_explosion|item",
//!  "path":["sides","rep","units","class","weapons",0],"id":1,"player":false}.
//! Paths resolve in the loaded Battle, preserving shared object identities.
//! `intern` requests seed strings from earlier owners; `policies` can take
//! an explicit weapons array of ids those earlier owners already assigned.
//! --check reads a complete P0 recording and checks content-derived payloads.
//! Equal payload identities and supplied weapon commands are reported separately.
use bfe_content::battle::{Battle, Loader};
use bfe_content::content::Content;
use bfe_content::defs::Defs;
use bfe_content::files::DirFiles;
use bfe_content::value::{Object, Value};
use bfe_sim::gen::F3WeaponField;
use bfe_sim::store::{DefId, Strings};
use bfe_sim::world::admit::rows::{combat, items, weapon_policies, Call, Calls, Cx};
use bfe_sim::world::admit::{AdmitError, Options, Stage};
use serde_json::json;
use std::io::BufRead;
use std::rc::Rc;

fn hex(words:&[f64])->Vec<String> {words.iter().map(|v|format!("{:016x}",v.to_bits())).collect()}
fn print(calls:Calls,sequence:&mut usize) {
    for (_,name,call) in calls {
        println!("{}",json!({"type":"call","sequence":*sequence,"battle":0,"phase":"admission","export":name,
            "arguments":hex(&call.args),"args":{"words":hex(&call.row),"heap":hex(&call.heap)},"scratch":"","intern":null,
            "returned":null,"strings":call.strings,"writes":[]}));
        *sequence+=1;
    }
}

// Share original content objects with the request path root. Parsing a
// request must never replace a definition with an equal but distinct object.
fn root(battle:&Battle)->Value {
    let sides=battle.sides.iter().map(|side| {
        let fields=Object::from_written(vec![(Rc::from("units"),side.units.clone()),(Rc::from("items"),side.items.clone())]);
        (Rc::from(side.name.as_str()),Value::Object(Rc::new(fields)))
    }).collect();
    Value::Object(Rc::new(Object::from_written(vec![
        (Rc::from("sides"),Value::Object(Rc::new(Object::from_written(sides)))),
        (Rc::from("props"),battle.props.clone().unwrap_or(Value::Null)),
        (Rc::from("vehicles"),battle.vehicle_def.clone().unwrap_or(Value::Null)),
        (Rc::from("soldier"),battle.soldier_def.clone()),(Rc::from("world"),battle.world.clone()),
        (Rc::from("common"),battle.common.clone()),(Rc::from("effects"),battle.effects.clone()),
    ])))
}
fn definition<'a>(root:&'a Value,request:&serde_json::Value)->Result<&'a Value,String> {
    let mut value=root;
    for part in request["path"].as_array().ok_or("request path required")? {
        value=if let Some(index)=part.as_u64() {value.at(usize::try_from(index).map_err(|_|"path index too large")?)}
            else if let Some(key)=part.as_str() {value.get(key)} else {None}.ok_or("definition path does not exist")?;
    }
    Ok(value)
}
fn id(request:&serde_json::Value,key:&str)->Result<u32,String> {
    request[key].as_u64().and_then(|id|u32::try_from(id).ok()).ok_or_else(||format!("request {key} required"))
}
fn main() {if let Err(error)=run() {eprintln!("{error}");std::process::exit(1);}}
fn run()->Result<(),String> {
    let args:Vec<_>=std::env::args().skip(1).collect();
    let root_dir=args.first().filter(|a|!a.starts_with("--")).ok_or("content directory required")?;
    let option=|name:&str,fallback:&str|->Result<String,String> {
        match args.iter().position(|v|v==name) {None=>Ok(fallback.into()),Some(i)=>args.get(i+1).cloned().ok_or_else(||format!("missing {name} value"))}
    };
    let options=Options {map:option("--map","nab2")?,side:option("--side","rep")?,enhanced:args.iter().any(|v|v=="--enhanced"),
        seed:option("--seed","1")?.parse().map_err(|_|"invalid seed")?,heroes:args.iter().any(|v|v=="--heroes"),
        unit:if args.iter().any(|v|v=="--unit") {Some(option("--unit","")?)} else {None},name:None};
    let files=DirFiles::new(root_dir);
    let battle=Battle::load(&Loader::new(&files),&options.map,&options.query())?;
    let content=Content::build(battle)?;
    let defs=Defs::build(&content)?;
    let battle=&content.battle;
    let root=root(battle);
    let mut cx=Cx::new(&content,&defs,&files,&options,Strings::new());
    if args.iter().any(|v|v=="--check") {return check(&content,&files);}
    let mut sequence=0;
    if !args.iter().any(|v|v=="--requests") {
        print(items::build(&mut cx).map_err(|e|e.to_string())?,&mut sequence);
        // Units.add assigns the first viewpoint slot before any fleet targets.
        cx.ids.registry.push(("viewpoint".into(),1.0));
        print(combat::build(&mut cx).map_err(|e|e.to_string())?,&mut sequence);
        return Ok(());
    }
    for line in std::io::stdin().lock().lines() {
        let line=line.map_err(|e|e.to_string())?;
        if line.trim().is_empty() {continue;}
        let request:serde_json::Value=serde_json::from_str(&line).map_err(|e|e.to_string())?;
        let op=request["op"].as_str().ok_or("request op required")?;
        let result:Result<Calls,AdmitError>=match op {
            "intern"=> {
                let text=request["text"].as_str().ok_or("request text required")?;
                let actual=cx.intern(&mut Call::default(),text).0;
                if let Some(expected)=request["id"].as_u64() {
                    if actual as u64!=expected {return Err(format!("intern id {actual} differs from {expected}"));}
                }
                Ok(Vec::new())
            }
            "pools"=>items::build(&mut cx),
            "weapon"=>combat::prepare_weapon(&mut cx,definition(&root,&request)?,Stage::Combat).map(|(_,calls)|calls),
            "vehicle_weapon"=>combat::prepare_vehicle_weapon(&mut cx,definition(&root,&request)?,Stage::Fleet).map(|(_,calls)|calls),
            "attach"=>combat::attach_unit(&mut cx,id(&request,"id")?,definition(&root,&request)?,request["player"].as_bool().unwrap_or(false),Stage::Combat),
            "class"|"ordnance"|"explosion"|"combat_explosion"|"item"=> {
                let def=definition(&root,&request)?;
                combat::with_bindings(&mut cx,Stage::Combat,|bindings,strings| {
                    let calls=match op {
                        "class"=> {bindings.combat.class(def,strings)?;std::mem::take(&mut bindings.combat.calls)},
                        "combat_explosion"=> {bindings.combat.explosion(def,strings)?;std::mem::take(&mut bindings.combat.calls)},
                        "ordnance"=> {bindings.ordnance.definition(def,strings)?;std::mem::take(&mut bindings.ordnance.calls)},
                        "explosion"=> {bindings.ordnance.explosion(def,strings)?;std::mem::take(&mut bindings.ordnance.calls)},
                        "item"=> {bindings.items.definition(def,&mut bindings.ordnance,strings)?;std::mem::take(&mut bindings.items.calls)},
                        _=>unreachable!(),
                    };
                    Ok(((),calls))
                }).map(|(_,calls)|calls)
            }
            "weapon_set"=> {
                let field=u8::try_from(id(&request,"field")?).ok().and_then(F3WeaponField::from_u8).ok_or("invalid weapon field")?;
                let value=if let Some(bits)=request["valueBits"].as_str() {f64::from_bits(u64::from_str_radix(bits,16).map_err(|e|e.to_string())?)}
                    else {request["value"].as_f64().ok_or("request value or valueBits required")?};
                let export=combat::weapon_set(id(&request,"id")?,id(&request,"index")?,field,value);
                Ok(vec![(Stage::Combat,export.name,export.call)])
            }
            "policies"=> {
                let weapons=if let Some(ids)=request["weapons"].as_array() {
                    ids.iter().map(|v|v.as_u64().and_then(|v|u32::try_from(v).ok()).map(DefId).ok_or("invalid weapon id".to_string())).collect::<Result<Vec<_>,_>>()?
                } else {weapon_policies::weapon_ids(&cx).map_err(|e|e.to_string())?};
                Ok(vec![(Stage::Policies,"ai_weapon_policies",weapon_policies::call(&weapon_policies::source(battle,&files,&weapons)))])
            }
            _=>return Err(format!("unknown construction op: {op}")),
        };
        print(result.map_err(|e|e.to_string())?,&mut sequence);
    }
    Ok(())
}

// Oracle mode treats preceding ids and string interns as inputs, and derives
// every definition payload from Content. Equal payloads can have several
// source identities; this checker reports payload coverage, not identity proof.
fn check(content:&Content,files:&DirFiles)->Result<(),String> {
    use std::collections::BTreeMap;
    let mut events=Vec::new();let mut complete=false;
    for line in std::io::stdin().lock().lines() {
        let line=line.map_err(|e|e.to_string())?;if line.trim().is_empty(){continue}
        let event:serde_json::Value=serde_json::from_str(&line).map_err(|e|e.to_string())?;
        if event["type"]=="end" {complete=true;}
        if event["type"]=="call" && (!event["intern"].is_null() || matches!(event["export"].as_str(),Some("cmb_weapon"|"cmb_class"|"cmb_new"|"cmb_weapon_set"|"cmb_explosion"|"ord_def"|"ord_explosion"|"items_def"|"items_pools"|"ai_weapon_policies"))) {events.push(event);}
    }
    if !complete||events.is_empty(){return Err("a complete admission recording is required".into())}
    let mut battles=BTreeMap::<u64,Vec<serde_json::Value>>::new();
    for event in events {battles.entry(event["battle"].as_u64().ok_or("call battle is absent")?).or_default().push(event);}
    for (battle,events) in battles {println!("{}",json!({"battle":battle,"result":check_battle(content,files,events)?}));}
    Ok(())
}

fn check_battle(content:&Content,files:&DirFiles,mut events:Vec<serde_json::Value>)->Result<serde_json::Value,String> {
    use std::collections::{BTreeMap,HashMap,HashSet};
    use bfe_sim::world::admit::rows::ordnance::{OrdnanceRows,ORDNANCE_FIELDS,EXPLOSION_FIELDS};
    fn words(v:&serde_json::Value)->Result<Vec<f64>,String> {
        v.as_array().ok_or("missing recorded words")?.iter().map(|v|v.as_str().ok_or_else(||"word is not hex".to_string()).and_then(|s|u64::from_str_radix(s,16).map(f64::from_bits).map_err(|e|e.to_string()))).collect()
    }
    fn same(a:&[f64],b:&[f64])->bool {a.len()==b.len()&&a.iter().zip(b).all(|(a,b)|a.to_bits()==b.to_bits())}
    fn visit(v:&Value,out:&mut Vec<Value>,seen:&mut HashSet<usize>) {
        if let Some(id)=bfe_content::defs::identity(v) {
            if !seen.insert(id){return}
            if v.as_object().map(|o|o.entries().any(|(k,_)|k=="class"||k=="weapons"||items::ITEM_FIELDS.contains(&k)||ORDNANCE_FIELDS.contains(&k)||EXPLOSION_FIELDS.contains(&k))).unwrap_or(false){out.push(v.clone());}
        }
        match v {Value::Object(o)=>for (_,v) in o.entries(){visit(v,out,seen)},Value::Array(a)=>for v in a.iter(){visit(v,out,seen)},_=>{}}
    }
    events.sort_by_key(|e|e["sequence"].as_u64().unwrap_or(u64::MAX));
    let mut interns=BTreeMap::new();
    for e in &events {if let Some(i)=e.get("intern").filter(|i|!i.is_null()) {
        let id=i["id"].as_u64().ok_or("intern id missing")?;
        let text=i["text"].as_str().ok_or("intern text missing")?;
        if let Some(old)=interns.insert(id,text.to_string()){if old!=text{return Err("conflicting recorded string".into())}}
    }}
    let mut strings=Strings::new();
    // Body construction interns internally too. Those occupied slots are
    // earlier-owner inputs even when no JS sim_intern call reveals the text.
    // Keep them opaque; any owned field requiring an unknown text will fail
    // the content match rather than substituting a guessed string identity.
    for id in 0..=interns.keys().copied().max().unwrap_or(0) {
        let text=interns.get(&id).cloned().unwrap_or_else(||format!("\0opaque string {id}"));
        if strings.intern_str(&text).0 as u64!=id {return Err(format!("recorded string id conflicts at {id}"))}
    }
    let mut candidates=Vec::new();let mut seen=HashSet::new();
    // Retain the synthetic root wrappers as well as the Battle children.
    // Their visited addresses must not be reused by a later derived record.
    let source_root=root(&content.battle);
    visit(&source_root,&mut candidates,&mut seen);
    visit(&content.battle.common,&mut candidates,&mut seen);visit(&content.battle.effects,&mut candidates,&mut seen);
    // ClassDef/Weapon levels retain shallow children. Guided tank variants
    // are constructed before policy publication, once for each hull reader.
    for def in candidates.clone() {let derived=combat::vehicle_missile_definition(&def).map_err(|e|e.to_string())?;if !derived.same(&def) {visit(&derived,&mut candidates,&mut seen);}}
    let mut encoded=Vec::with_capacity(candidates.len());
    for v in &candidates {encoded.push(combat::value_words(Some(v),&mut strings.clone(),None));}
    let mut explosions:HashMap<u32,Vec<f64>>=HashMap::new();
    let mut weapons:HashMap<u32,Vec<f64>>=HashMap::new();
    let mut classes:HashMap<u32,Vec<Value>>=HashMap::new();
    let mut checked=BTreeMap::new();let mut supplied=0usize;
    for e in &events {
        let name=e["export"].as_str().ok_or("missing export")?;
        if !matches!(name,"cmb_weapon"|"cmb_class"|"cmb_new"|"cmb_weapon_set"|"cmb_explosion"|"ord_def"|"ord_explosion"|"items_def"|"items_pools"|"ai_weapon_policies"){continue}
        let args=words(&e["arguments"])?;let row=words(&e["args"]["words"])?;let heap=words(&e["args"]["heap"])?;
        let id=args.first().copied().unwrap_or(0.0) as u32;
        let mut expected=None;
        match name {
            "cmb_weapon"|"cmb_explosion"=> {
                if let Some(payload)=encoded.iter().find(|v|same(v,&row)){expected=Some(Call {args:vec![id as f64],row:payload.clone(),..Call::default()});}
                if expected.is_none() {
                    let best=encoded.iter().enumerate().min_by_key(|(_,v)|v.len().abs_diff(row.len())+v.iter().zip(&row).filter(|(a,b)|a.to_bits()!=b.to_bits()).count());
                    if let Some((index,got))=best {
                        let differences:Vec<_>=got.iter().zip(&row).enumerate().filter(|(_, (a,b))|a.to_bits()!=b.to_bits()).take(8).map(|(i,(a,b))|json!({"word":i,"actual":format!("{:016x}",a.to_bits()),"expected":format!("{:016x}",b.to_bits())})).collect();
                        return Err(format!("{} {name} id={id}: no content payload matches; closest class {:?}, lengths {}/{}, differences {differences:?}",e["sequence"],candidates[index].get("class").and_then(Value::as_str),got.len(),row.len()));
                    }
                }
                if name=="cmb_weapon" {weapons.insert(id,row.clone());}
            }
            "ord_explosion"|"ord_def"|"items_def"=> {
                let fields=match name {"ord_explosion"=>EXPLOSION_FIELDS,"ord_def"=>ORDNANCE_FIELDS,_=>items::ITEM_FIELDS};
                for def in &candidates {
                    let mut rounds=OrdnanceRows::default();
                    let Ok(mut actual)=rounds.fields(def,fields,&mut strings.clone()) else {continue};
                    if actual.len()!=row.len(){continue}
                    let mut linked=true;
                    for (actual,want) in actual.chunks_exact_mut(2).zip(row.chunks_exact(2)) {
                        if actual[0]==6.0 {
                            let nested=&rounds.explosions.values[actual[1] as usize];
                            let nested=OrdnanceRows::default().fields(nested,EXPLOSION_FIELDS,&mut strings.clone()).map_err(|e|e.to_string())?;
                            if want[0]!=6.0||!explosions.get(&(want[1] as u32)).map(|v|same(v,&nested)).unwrap_or(false){linked=false;break}
                            actual[1]=want[1];
                        }
                    }
                    if linked&&same(&actual,&row){expected=Some(Call {args:vec![id as f64],row:actual,..Call::default()});break}
                }
                if name=="ord_explosion" {explosions.insert(id,row.clone());}
            }
            "cmb_class"=> {
                let mut matched=Vec::new();
                for def in &candidates {
                    if def.as_object().is_none(){continue}
                    let empty=Value::Array(Rc::new(Vec::new()));
                    let loadout=def.get("weapons").filter(|v|v.truthy()).unwrap_or(&empty);
                    let Some(list)=loadout.as_array() else {continue};
                    let refs=weapon_refs(&row)?;
                    if refs.len()!=list.len(){continue}
                    if !list.iter().zip(&refs).all(|(v,id)|weapons.get(id).map(|w|same(w,&combat::value_words(Some(v),&mut strings.clone(),None))).unwrap_or(false)){continue}
                    // Receipts for earlier weapon calls supply these handles.
                    let mut table=combat::Definitions::default();
                    for index in 0..=weapons.keys().copied().max().unwrap_or(0) {
                        let value=refs.iter().position(|id|*id==index).map(|i|list[i].clone()).unwrap_or_else(||Value::Object(Rc::new(Object::default())));
                        if table.id(&value,Stage::Combat).map_err(|e|e.to_string())?.0!=index {break}
                    }
                    let mut actual=vec![7.0,3.0];let mut local=strings.clone();
                    for (key,value) in [("AddHealth",def.get("AddHealth")),("base",def.get("base")),("weapons",Some(loadout))] {actual.push(local.intern_str(key).0 as f64);actual.extend(combat::value_words(value,&mut local,Some(&table)));}
                    if same(&actual,&row){expected=Some(Call {args:vec![id as f64],row:actual,..Call::default()});matched.push(def.clone());}
                }
                classes.insert(id,matched);
            }
            "cmb_new"=> {
                if args.len()!=6{return Err("cmb_new argument count".into())}
                if let Some(defs)=classes.get(&(args[1] as u32)) {for def in defs {
                    let call=combat::new_combat(id,DefId(args[1] as u32),def,args[2]!=0.0,&mut strings.clone()).map_err(|e|e.to_string())?.call;
                    if same(&call.args,&args){expected=Some(call);break}
                }}
            }
            "items_pools"=>expected=Some(items::pools(content.battle.world.get("mission"),&mut OrdnanceRows::default(),&mut strings.clone()).map_err(|e|e.to_string())?.call),
            "ai_weapon_policies"=> {
                // IDs are the preceding owner's registered table, not a
                // content catalog order. JSON bytes always come from Files.
                let source=bfe_sim::gen::AiWeaponPolicySource::take(&row,&heap);
                expected=Some(weapon_policies::call(&weapon_policies::source(&content.battle,files,&source.weapons)));
            }
            "cmb_weapon_set"=> {
                if args.len()!=4 {return Err("cmb_weapon_set argument count".into())}
                let field=F3WeaponField::from_u8(args[2] as u8).ok_or("invalid weapon field")?;
                expected=Some(combat::weapon_set(id,args[1] as u32,field,args[3]).call);supplied+=1;
            },
            _=>unreachable!(),
        }
        let actual=expected.ok_or_else(||format!("{} {name} id={id}: no loaded content matches {} words",e["sequence"],row.len()))?;
        // Match P0 --inputs-only: these exports never consume scratch.
        // Its visible bytes can still be an earlier body's upload loan.
        if !same(&actual.args,&args)||!same(&actual.row,&row)||!same(&actual.heap,&heap) {return Err(format!("{} {name}: payload differs",e["sequence"]))}
        *checked.entry(name.to_string()).or_insert(0usize)+=1;
    }
    if checked.is_empty(){return Err("recording contains no combat admission calls".into())}
    Ok(json!({"checked":checked,"supplied_weapon_commands":supplied,"definition_identity":"equal payload candidates; reference ids supplied by earlier recorded calls"}))
}

// Read only the reference receipts of a class's tagged argument stream.
// The decoder does not derive source content from the observed payload.
fn weapon_refs(words:&[f64])->Result<Vec<u32>,String> {
    fn visit(words:&[f64],at:&mut usize,out:&mut Vec<u32>)->Result<(),String> {
        let tag=*words.get(*at).ok_or("short class words")? as u32;*at+=1;
        match tag {
            0..=3|9=>{},4|5=>*at+=1,
            8=>{out.push(*words.get(*at).ok_or("short weapon receipt")? as u32);*at+=1;},
            6|7=>{let n=*words.get(*at).ok_or("short container")? as usize;*at+=1;for _ in 0..n {if tag==7 {*at+=1;}visit(words,at,out)?;}},
            _=>return Err("invalid class word tag".into()),
        }
        if *at>words.len(){return Err("short class field".into())}Ok(())
    }
    let mut out=Vec::new();let mut at=0;visit(words,&mut at,&mut out)?;
    if at!=words.len(){return Err("trailing class words".into())}Ok(out)
}
