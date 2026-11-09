//! content-hash.js's gameplay-only fingerprint. Native and browser clients
//! hash their own prepared files; a server's advertised digest never substitutes
//! for missing local content. Text formatting and incidental bone order do not
//! change the agreement that gates spawning.
use bfe_content::{files::Files,mission::{Query,select_mission,utf16_cmp},json,Value};
use sha2::{Sha256,Digest};
use std::{collections::{BTreeMap,BTreeSet},rc::Rc};
fn digest(bytes:&[u8])->String{Sha256::digest(bytes).iter().map(|n|format!("{n:02x}")).collect()}
fn string(text:&str,out:&mut String){out.push_str(&serde_json::to_string(text).expect("UTF-8 JSON string"));}
fn canonical(value:&Value,out:&mut String,depth:usize)->Result<(),String>{
    if depth>256{return Err("gameplay content nesting exceeds bounds".into());}
    match value{
        Value::Null=>out.push_str("null"),Value::Bool(v)=>out.push_str(if *v{"true"}else{"false"}),
        Value::Number(v)=>{if !v.is_finite(){return Err("invalid gameplay JSON number".into());}
            if *v==0.&&v.is_sign_negative(){out.push_str("-0");}else{out.push_str(&jsrt::numfmt::number_to_string(*v));}},
        Value::String(v)=>string(v,out),Value::Array(values)=>{out.push('[');for (i,v)in values.iter().enumerate(){if i>0{out.push(',');}canonical(v,out,depth+1)?;}out.push(']');},
        Value::Object(values)=>{let mut keys=values.keys().collect::<Vec<_>>();keys.sort_by(|a,b|utf16_cmp(a,b));out.push('{');
            for (i,key)in keys.iter().enumerate(){if i>0{out.push(',');}string(key,out);out.push(':');canonical(values.get(key).unwrap(),out,depth+1)?;}out.push('}');},
    }Ok(())
}
pub fn canonical_bytes(bytes:&[u8])->Result<Vec<u8>,String>{let value=json::parse_bytes(bytes).map_err(|e|e.to_string())?;serialize(&value)}
fn serialize(value:&Value)->Result<Vec<u8>,String>{let mut out=String::new();canonical(value,&mut out,0)?;Ok(out.into_bytes())}
fn parsed(bytes:&[u8])->Result<Value,String>{json::parse_bytes(bytes).map_err(|e|e.to_string())}
// Animation tracks name their bones in the fingerprint. Importers may rebase
// the global table while leaving every named track and clip byte unchanged.
fn animation_bytes(bytes:&[u8])->Result<Vec<u8>,String>{
    let value=parsed(bytes)?;let bones=value.get("bones").and_then(Value::as_array).ok_or("invalid animation bones")?;
    let mut names=Vec::with_capacity(bones.len());let mut unique=BTreeSet::new();for bone in bones{let name=bone.as_str().ok_or("invalid animation bone")?;if !unique.insert(name){return Err("duplicate animation bone".into());}names.push(name);}
    let clips=value.get("clips").and_then(Value::as_object).ok_or("invalid animation clips")?;let mut transformed=Vec::new();
    for (name,clip)in clips.entries(){let mut object=clip.as_object().ok_or("invalid animation clip")?.clone();let tracks=clip.get("tracks").and_then(Value::as_array).ok_or("invalid animation tracks")?;
        let mut named=Vec::with_capacity(tracks.len());for track in tracks{let n=track.as_f64().ok_or("invalid animation track")?;if n<0.||n.fract()!=0.||n>=names.len()as f64{return Err("invalid animation track".into());}named.push(Value::str(names[n as usize]));}
        object.set(Rc::from("tracks"),Value::Array(Rc::new(named)));transformed.push((Rc::from(name),Value::Object(Rc::new(object))));}
    names.sort_by(|a,b|utf16_cmp(a,b));let mut object=value.as_object().ok_or("invalid animation table")?.clone();
    object.set(Rc::from("bones"),Value::Array(Rc::new(names.into_iter().map(Value::str).collect())));
    object.set(Rc::from("clips"),Value::Object(Rc::new(bfe_content::value::Object::from_written(transformed))));serialize(&Value::Object(Rc::new(object)))
}
// Aircraft takeoff clips are cosmetic for client compatibility, as in the
// browser fingerprint. Shared walker clips remain part of gameplay agreement.
fn vehicle_bytes(bytes:&[u8])->Result<Vec<u8>,String>{
    let value=parsed(bytes)?;let Some(anims)=value.get("animations").and_then(Value::as_object)else{return serialize(&value);};
    let Some(classes)=value.get("classes").and_then(Value::as_object)else{return serialize(&value);};let mut flyers=BTreeMap::new();
    for def in classes.values(){if let Some(model)=def.get("model").and_then(Value::as_str){let only=flyers.entry(model).or_insert(true);*only&=def.get("base").and_then(Value::as_str)==Some("flyer");}}
    let keep=anims.entries().filter(|(model,_)|!flyers.get(model).copied().unwrap_or(false)).map(|(k,v)|(Rc::from(k),v.clone())).collect();
    let mut object=value.as_object().unwrap().clone();object.set(Rc::from("animations"),Value::Object(Rc::new(bfe_content::value::Object::from_written(keep))));serialize(&Value::Object(Rc::new(object)))
}
fn valid_name(name:&str)->bool{!name.is_empty()&&name.len()<=64&&name.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'_'||b==b'-')}
pub fn manifest(files:&dyn Files,query:&str)->Result<Vec<(String,Option<String>)>,String>{
    let query=Query::parse(query);let map=if query.get("map").is_empty(){"nab2"}else{query.get("map")};let side=if query.get("side").is_empty(){"rep"}else{query.get("side")};
    if !valid_name(map)||!valid_name(side){return Err("invalid content selection".into());}
    let world_path=format!("/maps/{map}/map.json");let raw=files.read(&world_path).ok_or("missing map content")?;let world=select_mission(&parsed(&raw)?,&query);
    let mission=world.get("mission");let mut sides=BTreeSet::new();sides.insert(side.to_string());
    if let Some(names)=mission.and_then(|m|m.get("sides")).and_then(Value::as_array){for name in names{let name=name.as_str().ok_or("invalid mission side")?;if !valid_name(name){return Err("invalid mission side".into());}sides.insert(name.to_string());}}
    let vehicles=world.get("vehicles").and_then(Value::as_str).unwrap_or("vehicles/vehicles.json");let parts=vehicles.split('/').collect::<Vec<_>>();
    if parts.first()!=Some(&"vehicles")||parts.last()!=Some(&"vehicles.json")||!(parts.len()==2||parts.len()==3&&valid_name(parts[1])){return Err("invalid vehicle pack".into());}
    let mut paths=vec!["/common/common.json".into(),"/common/anims.json".into(),"/common/anims.bin".into()];
    for name in ["ai-mission.json","ai-nav.json","ai-nav.bin","collision.bin","collision_index.bin","collision_poly.bin","terrain_collision.bin","terrain_collision_index.bin","props.json","props_anims.bin","props_collision.bin","props_collision_index.bin","props_collision_poly.bin",vehicles]{paths.push(format!("/maps/{map}/{name}"));}
    for side in sides{for name in ["units.json","weapons.json","ai.json"]{paths.push(format!("/sides/{side}/{name}"));}}paths.sort();
    let mission_name=mission.and_then(|m|m.get("name")).and_then(Value::as_str).map(str::to_owned);
    let mut result=vec![(world_path,Some(digest(&canonical_bytes(&raw)?))),("mission".into(),mission_name)];let vehicle_path=format!("/maps/{map}/{vehicles}");
    for path in paths{let hash=if let Some(bytes)=files.read(&path){let bytes=if path=="/common/anims.json"{animation_bytes(&bytes)?}else if path==vehicle_path{vehicle_bytes(&bytes)?}else if path.ends_with(".json"){canonical_bytes(&bytes)?}else{bytes.to_vec()};digest(&bytes)}else{"missing".into()};result.push((path,Some(hash)));}
    result.sort_by(|a,b|utf16_cmp(&a.0,&b.0));Ok(result)
}
pub fn hash(files:&dyn Files,query:&str)->Result<String,String>{let manifest=manifest(files,query)?;let bytes=serde_json::to_vec(&manifest).map_err(|e|e.to_string())?;Ok(format!("bfe-content-sha256-{}",digest(&bytes)))}
