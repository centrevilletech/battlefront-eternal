//! Fleet content rows as admission recorder JSON lines. String ids are local
//! to this dump; each call also carries its UTF-16 strings so comparison can
//! resolve them against the recording's shared table without changing words.
//! Pipe an uncompressed admission recording to --compare to check the pure
//! content rows. Raw logs and game-derived bytes stay on the content machine.
use bfe_content::battle::{Battle, Loader};
use bfe_content::files::DirFiles;
use bfe_sim::world::admit::rows::{fleet, vehicle_pack::{ExportCall, Pack}};
use bfe_sim::world::admit::Options;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufRead;

fn bits(words: &[f64]) -> Vec<String> { words.iter().map(|v| format!("{:016x}", v.to_bits())).collect() }
fn decode(words: &Value) -> Result<Vec<f64>, String> {
    words.as_array().ok_or("missing argument words")?.iter().map(|v| {
        u64::from_str_radix(v.as_str().ok_or("argument is not hex")?,16).map(f64::from_bits).map_err(|e| e.to_string())
    }).collect()
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3)*4);
    for b in bytes.chunks(3) {
        let word = ((b[0] as u32)<<16) | ((b.get(1).copied().unwrap_or(0) as u32)<<8) | b.get(2).copied().unwrap_or(0) as u32;
        for shift in [18,12,6,0] { out.push(ALPHABET[((word>>shift)&63) as usize] as char); }
        if b.len() < 3 { out.pop(); out.push('='); }
        if b.len() < 2 { let at=out.len()-2; out.replace_range(at..at+1,"="); }
    }
    out
}
fn unbase64(text: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::new(); let mut word = 0u32; let mut count = 0;
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace()) {
        if c == b'=' { break; }
        let value = match c { b'A'..=b'Z'=>c-b'A', b'a'..=b'z'=>c-b'a'+26,b'0'..=b'9'=>c-b'0'+52,b'+'=>62,b'/'=>63,_=>return Err("invalid base64 byte".into()) };
        word=(word<<6)|value as u32; count+=6;
        if count>=8 { count-=8; out.push((word>>count) as u8); }
    }
    Ok(out)
}
fn dump(calls: &[ExportCall]) {
    for (sequence,export) in calls.iter().enumerate() {
        let name=export.name; let call=&export.call;
        println!("{}",json!({"type":"call","sequence":sequence,"battle":0,"phase":"admission","export":name,
            "arguments":bits(&export.call.args),"args":{"words":bits(&call.row),"heap":bits(&call.heap)},"scratch":base64(&call.scratch),
            "strings":call.strings.iter().map(|v|String::from_utf16_lossy(v)).collect::<Vec<_>>(),
            "intern":null,"returned":{"scalar":null,"block":null},"writes":[],"parent":null}));
    }
}

// Upstream constructor facts come from the imports and allocation returns
// that preceded fleet admission. None is copied from the F2 row being checked.
// The native sequence supplies the same facts through Cx.ids.fleet/bodies.
#[derive(Default)]
struct Construction {
    vehicles: Vec<Option<bfe_sim::gen::Vehicle>>,
    kernels: Vec<Option<u32>>,
    created_bodies: Vec<u32>,
    caches: Vec<i32>,
    world: Option<i32>,
    set: Option<i32>,
    input: Vec<u8>,
    input_at: Option<usize>,
    input_bytes: usize,
    order: Vec<u8>,
    order_at: Option<usize>,
    body_refs: HashMap<u32,bfe_sim::gen::Ref<bfe_sim::gen::Body>>,
    collider: usize,
    registered: Option<usize>,
    checked: usize,
}
impl Construction {
    fn new(count: usize) -> Self {
        Self { vehicles: vec![None;count], kernels: vec![None;count], ..Default::default() }
    }
    fn observe(&mut self, record: &Value, strings: &HashMap<u32,String>, plans: &[fleet::Hull], data: Option<&bfe_content::Value>,
        differences: &mut Vec<Value>) -> Result<bool,String> {
        for (pointer,loan) in [(self.input_at,&mut self.input),(self.order_at,&mut self.order)] {
        if let Some(at)=pointer {
            // veh_input resizes one retained Vec. Native consumers read it;
            // host memory patches describe the changed words before each call.
            for patch in record["writes"].as_array().into_iter().flatten() {
                let offset=patch["offset"].as_u64().ok_or("input patch offset")? as usize;
                let encoded=patch["bytes"].as_str().ok_or("input patch bytes")?;
                let padding=encoded.as_bytes().iter().rev().take_while(|&&c|c==b'=').count();
                let length=encoded.len()/4*3-padding;
                let begin=offset.max(at); let end=(offset+length).min(at+loan.len());
                if begin<end {
                    let bytes=unbase64(encoded)?;
                    loan[begin-at..end-at].copy_from_slice(&bytes[begin-offset..end-offset]);
                }
            }
        }
        }
        let Some(name)=record["export"].as_str() else { return Ok(false); };
        let returned=record["returned"]["scalar"].as_str().map(|hex|u64::from_str_radix(hex,16).map(f64::from_bits)).transpose().map_err(|e|e.to_string())?;
        match name {
            "body_new" => if let Some(id)=returned {self.created_bodies.push(id as u32)},
            "world_new" => self.world=returned.map(|v|v as i32),
            "set_new" => self.set=returned.map(|v|v as i32),
            "cache_new" => if let Some(id)=returned {self.caches.push(id as i32)},
            "world_order" => {
                let args=decode(&record["arguments"])?;
                self.order.resize(args.get(1).copied().ok_or("world_order count")? as usize*4,0);
                self.order_at=returned.map(|v|v as usize);
            }
            "veh_input" => {
                let args=decode(&record["arguments"])?;
                self.input_bytes=args.first().copied().ok_or("veh_input count")? as usize*8;
                // Fleet::veh_input grows but never shrinks its retained bank.
                // Diffs may omit unchanged bytes beyond a shorter prior loan.
                if self.input.len()<self.input_bytes {self.input.resize(self.input_bytes,0);}
                self.input_at=returned.map(|v|v as usize);
            }
            "sim_import" => {
                let args=decode(&record["arguments"])?;
                if args.first().copied()==Some(bfe_sim::gen::KIND_VEHICLE as f64) {
                    let words=decode(&record["args"]["words"])?; let heap=decode(&record["args"]["heap"])?;
                    let active=plans.iter().take_while(|h|!h.prepared).count();
                    for row in words.chunks_exact(bfe_sim::gen::Vehicle::IMPORT_STRIDE) {
                        let mut vehicle=bfe_sim::gen::Vehicle::default(); vehicle.take_import(row,&heap);
                        let name=strings.get(&vehicle.name.0).ok_or("vehicle name was not interned")?;
                        let index=plans.iter().position(|h|h.pad==vehicle.slot_index as usize && h.name==*name).ok_or("imported vehicle is absent from pad plan")?;
                        if self.kernels[index].is_none() {
                            self.kernels[index]=if index<active {self.created_bodies.len().checked_sub(active).and_then(|start|self.created_bodies.get(start+index).copied())} else {self.created_bodies.last().copied()};
                        }
                        if let Some(kernel)=self.kernels[index] {self.body_refs.insert(kernel,vehicle.body);}
                        self.vehicles[index]=Some(vehicle);
                    }
                }
            }
            "f2_register" => {
                let args=decode(&record["arguments"])?;
                self.registered=args.first().map(|v|*v as usize);
                return Ok(false); // The caller checks slab slot/generation/order.
            }
            "veh_collision" => {
                let blockers=data.and_then(|d|d.get("blockers")).and_then(|b|b.get("indices"))
                    .and_then(bfe_content::Value::as_array).is_some_and(|v|!v.is_empty());
                let expected=fleet::collision(self.world.ok_or("collision world has not been allocated")?,
                    if blockers {self.set.ok_or("blocker set has not been allocated")?} else {-1});
                self.check(record,&expected,differences)?; return Ok(true);
            }
            "f2_bodies" => {
                let mut bodies=Vec::new();
                let mut next=self.body_refs.values().map(|r|r.slot).max().map_or(0,|v|v+1);
                // Non-vehicle bodies first enter the unkeyed RefPool in this
                // world-order walk; vehicles already named theirs in imports.
                for word in self.order.chunks_exact(4) {
                    let kernel=u32::from_le_bytes(word.try_into().map_err(|_|"order word")?);
                    let body=*self.body_refs.entry(kernel).or_insert_with(|| {let body=bfe_sim::gen::Ref::new(next,1);next+=1;body});
                    bodies.push(bfe_sim::gen::F2BodyRef {body,kernel});
                }
                self.check(record,&fleet::bodies(bodies),differences)?; return Ok(true);
            }
            "veh_collider" => {
                let index=self.collider; self.collider+=1;
                let plan=plans.get(index).ok_or("extra collider")?;
                let shape=fleet::collider_shape(data.ok_or("collider without vehicle pack")?,&plan.name).map_err(|e|e.to_string())?;
                let vehicle=self.vehicles[index].as_ref().ok_or("collider's vehicle was not imported")?;
                let cache_count=shape.walls+shape.springs;
                if self.caches.len()!=cache_count {return Err(format!("{} collider {} has {} allocated caches, needs {}",plan.name,index,self.caches.len(),cache_count));}
                let header=bfe_sim::gen::F2ColliderHeader {body:vehicle.body.slot,kernel:self.kernels[index].ok_or("collider has no kernel")?,min:shape.min,max:shape.max};
                let expected=fleet::collider(header,&self.caches[..shape.walls],&self.caches[shape.walls..]).map_err(|e|e.to_string())?;
                self.check(record,&expected,differences)?;
                let input=&self.input[..self.input_bytes];
                if input!=expected.call.scratch {differences.push(json!({"export":name,"kind":"collider input","index":index,
                    "firstByte":input.iter().zip(&expected.call.scratch).position(|(a,b)|a!=b),"actualBytes":input.len(),"expectedBytes":expected.call.scratch.len(),"actual":input.chunks_exact(8).map(|v|format!("{:016x}",u64::from_le_bytes(v.try_into().expect("eight-byte chunk")))).collect::<Vec<_>>(),"expected":expected.call.scratch.chunks_exact(8).map(|v|format!("{:016x}",u64::from_le_bytes(v.try_into().expect("eight-byte chunk")))).collect::<Vec<_>>()}));}
                self.caches.clear(); return Ok(true);
            }
            "f2_prepare_body" => {
                let index=self.registered.ok_or("prepared body preceded registration")?;
                let expected=fleet::prepare_body(self.kernels.get(index).copied().flatten().ok_or("prepared hull has no kernel")?);
                self.check(record,&expected,differences)?; return Ok(true);
            }
            _=>{}
        }
        Ok(false)
    }
    fn check(&mut self, record:&Value, expected:&ExportCall, differences:&mut Vec<Value>)->Result<(),String> {
        let args=decode(&record["arguments"])?;
        if bits(&args)!=bits(&expected.call.args) {differences.push(json!({"export":expected.name,"kind":"bound arguments","expected":bits(&expected.call.args),"actual":bits(&args)}));}
        // Direct vehicle loans are compared separately as raw bytes; generated
        // f2_bodies uses precisely this argument row and indirect heap.
        if expected.name=="f2_bodies" {
            let words=decode(&record["args"]["words"])?; let heap=decode(&record["args"]["heap"])?;
            if bits(&words)!=bits(&expected.call.row)||bits(&heap)!=bits(&expected.call.heap) {
                differences.push(json!({"export":expected.name,"kind":"bound block","expectedWords":bits(&expected.call.row),"actualWords":bits(&words),"expectedHeapWords":expected.call.heap.len(),"actualHeapWords":heap.len(),"firstHeap":heap.iter().zip(&expected.call.heap).position(|(a,b)|a.to_bits()!=b.to_bits())}));
            }
        }
        self.checked+=1; Ok(())
    }
}

// Compare all content-only scalar arguments and string text. Handles created
// by other owners are not oracle inputs to the builder; they are reported as
// unresolved here and checked by resolved() when native construction binds them.
fn compare(battle: &Battle, calls: &[ExportCall]) -> Result<(),String> {
    let mut strings = HashMap::<u32,String>::new();
    let hulls = battle.vehicle_def.as_ref().map(|data|fleet::hulls(data,&battle.world)).transpose().map_err(|e|e.to_string())?.unwrap_or_default();
    let mut construction=Construction::new(hulls.len());
    let mut register_at=0; let mut registrations=0; let mut battles=0;
    let names: Vec<_> = calls.iter().map(|call| call.name).collect();
    let mut at=0; let mut compared=0; let mut differences=Vec::new();
    let pack = Pack::from_battle(battle).map_err(|e|e.to_string())?;
    let mut pack_pointer=None;
    let mut bytes = pack.as_ref().map(|p|vec![0u8;p.bytes.len()]).unwrap_or_default();
    let mut pack_bytes_compared=0;
    let mut unresolved = HashMap::<String,u64>::new();
    for line in std::io::stdin().lock().lines() {
        let line=line.map_err(|e|e.to_string())?;
        if line.is_empty() { continue; }
        let record:Value=serde_json::from_str(&line).map_err(|e|e.to_string())?;
        if record["type"]=="battle" {
            if battles>0 && at!=calls.len() { differences.push(json!({"kind":"missing calls","expected":calls.len(),"observed":at})); }
            construction=Construction::new(hulls.len());
            at=0; register_at=0; strings.clear(); bytes.fill(0); pack_pointer=None; battles+=1;
        }
        if record["phase"]=="spawn" { continue; }
        if let (Some(id),Some(text))=(record["intern"]["id"].as_u64(),record["intern"]["text"].as_str()) { strings.insert(id as u32,text.into()); }
        let Some(name)=record["export"].as_str() else { continue; };
        if construction.observe(&record,&strings,&hulls,battle.vehicle_def.as_ref(),&mut differences)? { continue; }
        if name=="veh_bytes" {
            if let Some(hex)=record["returned"]["scalar"].as_str() { pack_pointer=Some(f64::from_bits(u64::from_str_radix(hex,16).map_err(|e|e.to_string())?) as usize); }
        }
        if name=="veh_pack" {
            if let (Some(pointer),Some(pack))=(pack_pointer,pack.as_ref()) {
                let scratch=record["scratch"].as_str().unwrap_or("");
                if !scratch.is_empty() {
                    // The recorder normalizes direct vehicle uploads onto
                    // veh_pack. Earlier traces retain only memory patches.
                    bytes=unbase64(scratch)?;
                } else {
                for patch in record["writes"].as_array().into_iter().flatten() {
                    let offset=patch["offset"].as_u64().ok_or("write has no offset")? as usize;
                    let data=unbase64(patch["bytes"].as_str().ok_or("write has no bytes")?)?;
                    let first=offset.max(pointer); let last=(offset+data.len()).min(pointer+bytes.len());
                    if first<last { bytes[first-pointer..last-pointer].copy_from_slice(&data[first-offset..last-offset]); }
                }
                }
                if bytes!=pack.bytes { differences.push(json!({"export":name,"kind":"pack bytes","first":bytes.iter().zip(&pack.bytes).position(|(a,b)|a!=b),"bytes":bytes.len()})); }
                pack_bytes_compared=bytes.len();
            }
        }
        if name=="f2_register" {
            let actual=decode(&record["arguments"])?;
            let Some(hull)=hulls.get(register_at) else { differences.push(json!({"export":name,"kind":"extra hull registration"})); continue; };
            // The vehicle kind is an unkeyed RefPool, untouched by other
            // entity kinds. Admission only allocates: consecutive slots, gen1.
            let want=fleet::register(bfe_sim::gen::Ref::new(register_at as u32,1),hull.prepared);
            if bits(&want.call.args)!=bits(&actual) { differences.push(json!({"export":name,"kind":"registration","expected":bits(&want.call.args),"actual":bits(&actual)})); }
            register_at+=1; registrations+=1; continue;
        }
        if !names.contains(&name) {
            if ["f2_register","f2_prepare_body","veh_collision","veh_collider","f2_bodies","f2_firing_supply"].contains(&name) { *unresolved.entry(name.into()).or_default()+=1; }
            continue;
        }
        if at>=calls.len() { differences.push(json!({"export":name,"kind":"extra content call"})); continue; }
        let export=&calls[at]; let expected_name=export.name; let expected=&export.call;
        if expected_name!=name { differences.push(json!({"export":name,"expected":expected_name,"kind":"order"})); at+=1; continue; }
        let actual=decode(&record["arguments"])?;
        let mut want=export.call.args.clone();
        if name=="veh_class" {
            let class=String::from_utf16_lossy(&expected.strings[0]);
            if let Some(index)=hulls.iter().position(|h|h.name==class) {
                if let Some(vehicle)=&construction.vehicles[index] {
                    let definition=battle.vehicle_def.as_ref().and_then(|d|d.get("classes")).and_then(|c|c.get(&class)).ok_or("class definition is missing")?;
                    want[2]=if vehicle.alive {fleet::spawned_turn_rate(definition)} else {fleet::turn_rate(definition)};
                }
            }
        }
        for (i,text) in expected.strings.iter().enumerate() {
            let text=String::from_utf16_lossy(text);
            let found=actual.get(i).and_then(|v|strings.get(&(*v as u32)));
            if found!=Some(&text) { differences.push(json!({"export":name,"word":i,"kind":"string","expected":text.chars().take(160).collect::<String>(),"actual":found.map(|s|s.chars().take(160).collect::<String>())})); }
            if let Some(v)=actual.get(i) { want[i]=*v; }
        }
        if bits(&want)!=bits(&actual) { differences.push(json!({"export":name,"kind":"arguments","expected":bits(&want),"actual":bits(&actual)})); }
        compared+=1; at+=1;
    }
    if register_at!=hulls.len() { differences.push(json!({"kind":"missing hull registrations","expected":hulls.len(),"observed":register_at})); }
    if at!=calls.len() { differences.push(json!({"kind":"missing calls","expected":calls.len(),"observed":at})); }
    println!("{}",json!({"battles":battles,"calls":compared,"registrations":registrations,"boundCalls":construction.checked,"expectedPerBattle":calls.len(),"packBytes":pack_bytes_compared,"differences":differences.len(),"first":differences.first(),"unresolvedHandles":unresolved}));
    if differences.is_empty() { Ok(()) } else { Err("fleet content row comparison failed".into()) }
}
fn main() {
    let result=(|| -> Result<(),String> {
        let args:Vec<String>=std::env::args().skip(1).collect();
        let root=args.first().ok_or("content directory required")?;
        let option=|name:&str|args.windows(2).find(|a|a[0]==name).map(|a|a[1].clone());
        let options=Options { map:option("--map").unwrap_or("nab2".into()), side:option("--side").unwrap_or("rep".into()),
            enhanced:option("--ai").is_some_and(|v|v=="enhanced"),seed:option("--seed").unwrap_or("1".into()).parse::<u32>().map_err(|e|e.to_string())?,
            heroes:args.iter().any(|a|a=="--heroes"),unit:option("--unit") };
        let files=DirFiles::new(root);
        let battle=Battle::load(&Loader::new(&files),&options.map,&options.query())?;
        let calls=fleet::content_calls(&battle,&options).map_err(|e|e.to_string())?;
        if args.iter().any(|a|a=="--compare") { compare(&battle,&calls) } else { dump(&calls); Ok(()) }
    })();
    if let Err(error)=result { eprintln!("{error}"); std::process::exit(1); }
}
