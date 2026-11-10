//! UnitFactory body rows in admission-call form. With --requests, stdin
//! supplies the enclosing world's identities and cookie for each body;
//! without it, dump every prepared class with an independent test identity.
//! --check-units instead advances the native world on recorded frame input
//! and compares preparePhase's count before each step (no recorded count is
//! ever passed as the native step's units input).
use bfe_content::{battle::{Battle, Loader}, content::Content, defs::Defs, files::DirFiles};
use bfe_sim::{gen::*, store::{DefId, Strings}, world::admit::{bodies::{self, Bindings}, Options}};
use serde_json::{json, Value};
use std::io::BufRead;

fn options(args: &[String]) -> Result<Options, String> {
    let opt = |name: &str, fallback: &str| args.iter().position(|s| s == name)
        .and_then(|i| args.get(i + 1)).cloned().unwrap_or_else(|| fallback.into());
    Ok(Options { map: opt("--map", "nab2"), side: opt("--side", "rep"), enhanced: opt("--ai", "classic") == "enhanced",
        seed: opt("--seed", "1").parse::<u32>().map_err(|e| e.to_string())?, heroes: args.iter().any(|a| a == "--heroes"),
        unit: args.iter().position(|a| a == "--unit").and_then(|i| args.get(i + 1)).cloned(), name: None })
}
fn bits(words: &[f64]) -> Vec<String> { words.iter().map(|w| format!("{:016x}", w.to_bits())).collect() }
fn call(name: &str, arguments: &[f64], words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) {
    let mut row = vec![0.; words]; let mut heap = Vec::new(); put(&mut row, &mut heap);
    println!("{}", json!({"type":"call","export":name,"arguments":bits(arguments),"args":{"words":bits(&row),"heap":bits(&heap)},"scratch":"","intern":null}));
}
fn main() {
    if let Err(error) = run() { eprintln!("{error}"); std::process::exit(1); }
}
fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let root = args.first().filter(|s| !s.starts_with("--")).ok_or("content directory required")?;
    let options = options(&args)?;
    let files = DirFiles::new(root);
    if args.iter().any(|a| a == "--check-units") { return check_units(&files, &options); }
    let content = Content::build(Battle::load(&Loader::new(&files), &options.map, &options.query())?)?;
    let defs = Defs::build(&content)?;
    if args.iter().any(|a| a == "--compare-imports") { return compare_imports(&content, &defs, &options); }
    let ceiling = content.battle.world.get("mission").map_or(200., |m| match m.get("maxFlyHeight") {
        None | Some(bfe_content::Value::Null) => 200., value => bfe_content::js::to_number(value),
    });
    let requests = if args.iter().any(|a| a == "--requests") {
        std::io::stdin().lock().lines().map(|line| serde_json::from_str::<Value>(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?
    } else {
        defs.classes.iter().filter(|c| c.body.is_some()).map(|c| json!({"class":defs.names.text(c.name)})).collect()
    };
    for request in requests {
        let name = request["class"].as_str().ok_or("request class")?;
        let class = defs.classes.iter().find(|c| defs.names.text(c.name) == name).ok_or_else(|| format!("unknown class {name}"))?;
        let mut strings = Strings::new();
        // A comparison supplies the real admission namespace. Re-interning
        // its ordered strings reproduces every numeric identity in body rows.
        for text in request["strings"].as_array().into_iter().flatten() {
            let units = text.as_array().ok_or("UTF-16 string")?.iter().map(|u| u.as_u64().and_then(|n| u16::try_from(n).ok()).ok_or("UTF-16 word")).collect::<Result<Vec<_>, _>>()?;
            strings.intern(&units);
        }
        let id = |field: &str, fallback: u32| request[field].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(fallback);
        let side = strings.intern_str(defs.names.text(class.side));
        let class_name = strings.intern_str(name);
        let scale = strings.intern_str(health_scale(&class.record));
        let binding = Bindings { key: F3UnitKey { slot: id("slot", 1), generation: id("generation", 0), life: request["life"].as_f64().unwrap_or(0.) },
            side, class_name, class: DefId(id("definition", 1)), health_scale: scale,
            shield: class.record.get("weapons").and_then(bfe_content::Value::as_array).filter(|ws| ws.iter().any(|w| w.get("base").and_then(bfe_content::Value::as_str) == Some("shield"))).map(|_| DefId(id("shield", 1))) };
        let body = bodies::build(&content, class, binding, ceiling, request["passive"].as_bool().unwrap_or(false), |name| strings.intern_str(name)).map_err(|e| e.to_string())?;
        println!("{}", json!({"type":"body","class":name,"strings":strings.list}));
        call("f3_body_skeleton", &[], F3BodySkeleton::WORDS, |row, heap| body.skeleton.put(row, heap));
        let stage = body.stage(id("cookie", 0));
        if let Some(muzzles) = body.muzzles {
            let arguments=[body.adoption.key.slot as f64,id("walkerDefinition",0) as f64];
            call("units_droideka_attach",&arguments,0,|_,_|{});
            call("units_muzzle_attach", &arguments, muzzles.words.len(), |row, _| row.copy_from_slice(&muzzles.words));
        }
        call("f3_adopt", &[], F3Adoption::WORDS, |row, heap| body.adoption.put(row, heap));
        call(if stage.adoption.passive {"f3_factory_resident"} else {"f3_factory_stage"}, &[], F3ColdStage::WORDS, |row, heap| stage.put(row, heap));
    }
    Ok(())
}
fn health_scale(def: &bfe_content::Value) -> &'static str {
    match def.get("HealthType").and_then(bfe_content::Value::as_str).unwrap_or("person").to_lowercase().as_str() {
        "animal" => "AnimalScale", "droid" => "DroidScale", "vehicle" => "VehicleScale", "building" => "BuildingScale", "shield" => "ShieldScale", _ => "PersonScale",
    }
}

#[cfg(all(not(target_arch = "wasm32"), panic = "unwind"))]
fn check_units(files: &DirFiles, options: &Options) -> Result<(), String> {
    use bfe_sim::abi::context::Context;
    let mut context: Option<Context> = None;
    let mut battle = None;
    let (mut rows, mut differences, mut unit_differences) = (0u64, 0u64, 0u64);
    let mut first = Value::Null;
    let mut clock=WorldClockState::default();
    for line in std::io::stdin().lock().lines() {
        let frame: Value = serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        if frame["battle"].as_u64() != battle || context.is_none() {
            if let Some(old) = context.take() { old.dispose().map_err(|e| e.to_string())?; }
            let world = Context::new();
            world.world_admit(files, options).map_err(|e| e.to_string())?;
            context = Some(world); battle = frame["battle"].as_u64();
            clock=WorldClockState {hz:60.,dt:1./60.,max_steps:8.,budget:0.035,max_debt:8.,..Default::default()};
        }
        let world = context.as_ref().ok_or("native world")?;
        world.world_replay_prepare(&frame["frame"]).map_err(|e| e.to_string())?;
        let mut outputs = Vec::new();
        for encoded in frame["worldRows"].as_array().ok_or("worldRows")? {
            let words = encoded.as_array().ok_or("world row")?.iter().map(|w| {
                u64::from_str_radix(w.as_str().ok_or("word bits")?, 16).map(f64::from_bits).map_err(|e| e.to_string())
            }).collect::<Result<Vec<_>, String>>()?;
            if words.len() != WorldTickInput::WORDS { return Err("wrong world row width".into()); }
            let computed = world.world_units().map_err(|e| e.to_string())?;
            if (computed as f64).to_bits() != words[WorldTickInput::UNITS].to_bits() {unit_differences+=1;}
            // WorldTick stamps the completed native clock and registry count.
            // Only wall time and the host's mouse setting are external facts.
            let input=WorldTickInput {dt:clock.dt,tick:clock.tick+1.,clock:clock.clone(),units:computed,
                ai:true,live:true,camera:0,offline_camera:false,squads:0,
                at:words[WorldTickInput::AT],mouse_scale:words[WorldTickInput::MOUSE_SCALE]};
            let mut expected=vec![0.;WorldTickInput::WORDS];input.put(&mut expected,&mut Vec::new());
            if let Some(word)=expected.iter().zip(&words).position(|(a,b)|a.to_bits()!=b.to_bits()) {
                differences+=1;
                if first.is_null() {first=json!({"row":rows,"battle":battle,"word":word,
                    "computed":format!("{:016x}",expected[word].to_bits()),"recorded":format!("{:016x}",words[word].to_bits())});}
            }
            let output = world.world_step(input).map_err(|e| e.to_string())?;
            if !output.accepted { return Err(format!("native step refused row {rows}")); }
            clock=output.clock.clone();outputs.push(output); rows += 1;
        }
        world.world_replay_publish(&frame["frame"], &outputs).map_err(|e| e.to_string())?;
    }
    println!("{}", json!({"rows":rows,"differences":differences,"first":first,"unitsComputed":true,"unitDifferences":unit_differences}));
    Ok(())
}
#[cfg(not(all(not(target_arch = "wasm32"), panic = "unwind")))]
fn check_units(_: &DirFiles, _: &Options) -> Result<(), String> { Err("native world checks require unwinding".into()) }

// The call recorder keeps Unit imports as host byte writes. Its Node reader
// reconstructs the import block; generated take/put routines decode the row
// here so the comparison never hand-copies a body's field offsets.
fn words(values: &Value) -> Result<Vec<f64>,String> {
    values.as_array().ok_or("hex words")?.iter().map(|w| u64::from_str_radix(w.as_str().ok_or("word bits")?,16)
        .map(f64::from_bits).map_err(|e|e.to_string())).collect()
}
fn bytes(value: &Value) -> Result<Vec<u8>,String> {
    let text=value.as_str().ok_or("hex bytes")?;
    if text.len()%2!=0 || !text.is_ascii() {return Err("hex byte length".into());}
    (0..text.len()).step_by(2).map(|i|u8::from_str_radix(&text[i..i+2],16).map_err(|e|e.to_string())).collect()
}
fn recorded_strings(values: &Value) -> Result<Strings,String> {
    let mut strings=Strings::new();
    for (index,text) in values.as_array().ok_or("strings")?.iter().enumerate() {
        let units=if text.is_null() {format!("\0reserved string {index}").encode_utf16().collect()}
            else {text.as_array().ok_or("UTF-16 text")?.iter().map(|u|u.as_u64().and_then(|n|u16::try_from(n).ok()).ok_or("UTF-16 word")).collect::<Result<Vec<_>,_>>()?};
        if strings.intern(&units).0 as usize!=index {return Err("recorded string ids are not unique".into());}
    }
    Ok(strings)
}

// Values supplied by preceding owners are qualified references and mutable
// weapon/building facts. Geometry, seat/rider policy and lock rows are rebuilt
// from content; no recorded skeleton or settings seed those computations.
fn holder_rows(content: &Content, defs: &Defs, options: &Options, request: &Value, checked: &mut std::collections::BTreeMap<String,u64>) -> Result<(Vec<bodies::Holder>,u64,Vec<Value>),String> {
    let strings=recorded_strings(&request["strings"])?;
    let mut vehicles=std::collections::BTreeMap::new();
    for value in request["vehicles"].as_array().ok_or("vehicle imports")? {
        let row=words(&value["row"])?;let heap=words(&value["heap"])?;
        if row.len()!=Vehicle::FULL_STRIDE {return Err("vehicle import width".into());}
        let mut vehicle=Vehicle::default();vehicle.take_full(&row,&heap);
        vehicles.insert((row[0] as u32,row[1] as u32),String::from_utf16_lossy(strings.units(vehicle.name)));
    }
    let mut holders=Vec::new(); let mut differences=Vec::new();let mut facts=0;
    for value in request["hulls"].as_array().ok_or("hull facts")? {
        let row=words(&value["row"])?;let heap=words(&value["heap"])?;
        let recorded=SeatHullFacts::take(&row,&heap);
        let name=vehicles.get(&(recorded.vehicle.slot,recorded.vehicle.gen)).ok_or("hull has no preceding vehicle import")?;
        let class=defs.vehicles.iter().find(|c|defs.names.text(c.name)==name).ok_or("hull has no content class")?;
        let expected=bodies::hull_facts(recorded.vehicle,recorded.index as usize,value["prepared"].as_bool().ok_or("prepared")?,class);
        let mut out=vec![0.;SeatHullFacts::WORDS];let mut indirect=Vec::new();expected.put(&mut out,&mut indirect);
        if bits(&row)!=bits(&out)||bits(&heap)!=bits(&indirect) {differences.push(json!({"export":"f2_seats_adopt","class":name,"word":out.iter().zip(&row).position(|(a,b)|a.to_bits()!=b.to_bits())}));}
        let template=class.template.and_then(|i|content.vehicle_templates.get(i)).ok_or("hull template")?;
        let rig=bfe_pose::rig::hull::HullRig::new(&bfe_pose::rig::content::hull_desc(&class.record,template));
        holders.push(bodies::hull_holder(recorded.vehicle,&rig));facts+=1;*checked.entry("f2_seats_adopt".into()).or_default()+=1;
    }
    let props=content.battle.props.as_ref();
    let models=props.and_then(|p|p.get("models"));
    let turrets=defs.props.iter().filter(|p|p.record.get("base").and_then(bfe_content::Value::as_str)==Some("armedbuilding")
        && p.record.get("model").and_then(bfe_content::Value::as_str).and_then(|n|models.and_then(|m|m.get(n))).is_some()).collect::<Vec<_>>();
    for value in request["turrets"].as_array().ok_or("turret facts")? {
        let row=words(&value["row"])?;let heap=words(&value["heap"])?;
        let recorded=F2TurretBirth::take(&row,&heap);
        let index=recorded.turret.state.index as usize;
        let prop=turrets.get(index).ok_or("turret has no content instance")?;
        let model=prop.record.get("model").and_then(bfe_content::Value::as_str).and_then(|n|models.and_then(|m|m.get(n))).ok_or("turret model")?;
        let geometry=bodies::turret_geometry(&prop.record,model,index).map_err(|e|e.to_string())?;
        holders.push(bodies::Holder::Turret {turret:recorded.turret.turret,rider:geometry.rider.clone()});
        let binding=bodies::TurretBinding {turret:recorded.turret.turret,life:recorded.life,alive:recorded.turret.alive,
            body:recorded.turret.body,center:recorded.turret.center,radius:recorded.turret.radius,ordnance:recorded.ordnance,collide:recorded.collide};
        let expected=bodies::turret_birth(geometry,binding,recorded.weapon.clone(),prop.record.get("weapon")).map_err(|e|e.to_string())?;
        let mut out=vec![0.;F2TurretBirth::WORDS];let mut indirect=Vec::new();expected.put(&mut out,&mut indirect);
        if bits(&row)!=bits(&out)||bits(&heap)!=bits(&indirect) {differences.push(json!({"export":"f2_turret_adopt","index":index,"word":out.iter().zip(&row).position(|(a,b)|a.to_bits()!=b.to_bits()),"heapWord":indirect.iter().zip(&heap).position(|(a,b)|a.to_bits()!=b.to_bits())}));}
        facts+=1;*checked.entry("f2_turret_adopt".into()).or_default()+=1;
    }
    for call in request["factory"].as_array().ok_or("factory inputs")? {
        let export=call["export"].as_str().ok_or("factory export")?;
        let mut out=Vec::new();let mut indirect=Vec::new();let mut arguments=Vec::new();
        match export {
            "f3_factory_reserve"=>{
                let mut registry=Units::default();
                registry.take_import(&words(&call["registryUnit"]["row"])?,&words(&call["registryUnit"]["heap"])?);
                // Registry returns are earlier-owner assignments, not slots
                // derived from a unit class's position in authored content.
                let adds=words(&call["registryAdds"])?;
                let next=adds.last().map_or(registry.next_id,|id|id+1.);
                arguments.push(next+512.);
            },
            "f3_factory_player_bind"=>{
                let source=F3PlayerBinding::take(&words(&call["row"])?,&words(&call["heap"])?);
                let tail=F4TailSetup::take(&words(&call["tail"]["row"])?,&words(&call["tail"]["heap"])?);
                let local=call["local"].as_bool().ok_or("player binding local")?;
                let character=if local {tail.character.ok_or("local character")?} else {
                    let roster=F4RosterResult::take(&words(&call["roster"]["words"])?,&words(&call["roster"]["heap"])?);
                    let handle=roster.handle.filter(|h|h.kind==F4ObjectKind::Character).ok_or("roster character")?;
                    bfe_sim::store::Ref::new(handle.slot,handle.generation)
                };
                // Spread/recon ids and a remote's seed are bindings from the
                // preceding stream/recon owners. Character/body/local and the
                // gameplay id come from the earlier roster/tail facts.
                let expected=F3PlayerBinding {character,body:if local {tail.local}else{None},local,
                    combat_random:tail.gameplay,weapon_random:source.weapon_random,recon_key:source.recon_key,
                    recon_seed:if local {options.seed}else{source.recon_seed}};
                out.resize(F3PlayerBinding::WORDS,0.);expected.put(&mut out,&mut indirect);
            },
            _=>return Err(format!("unknown factory input {export}")),
        }
        if bits(&arguments)!=bits(&words(&call["arguments"])?) || bits(&out)!=bits(&words(&call["row"])?) || bits(&indirect)!=bits(&words(&call["heap"])?) {
            differences.push(json!({"export":export,"field":"factory input","word":out.iter().zip(words(&call["row"])?).position(|(a,b)|a.to_bits()!=b.to_bits())}));
        }
        facts+=1;*checked.entry(export.into()).or_default()+=1;
    }
    // References come from the preceding fleet admission. Retain each holder
    // once even when a class emitted its immutable facts again on respawn.
    let mut seen=std::collections::BTreeSet::new();holders.retain(|holder| match holder {
        bodies::Holder::Hull {hull,..}=>seen.insert((0,hull.slot,hull.gen)),
        bodies::Holder::Turret {turret,..}=>seen.insert((1,turret.slot,turret.gen)),
    });
    Ok((holders,facts,differences))
}

fn compare_imports(content: &Content, defs: &Defs, options: &Options) -> Result<(), String> {
    let mut rows = 0u64; let mut facts = 0u64; let mut differences = 0u64; let mut first = Value::Null;
    let mut classes = std::collections::BTreeSet::new();
    let mut checked=std::collections::BTreeMap::new();
    let mut holders=Vec::new();let mut clips=Vec::new();let mut cookie=0u32;let mut walker_classes=Vec::new();
    let decode=words;
    for line in std::io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if request["kind"].as_str()==Some("holders") {
            let (prepared,count,diffs)=holder_rows(content,defs,options,&request,&mut checked)?;holders=prepared;cookie=0;
            clips=request["clips"].as_array().ok_or("clip uploads")?.clone();walker_classes.clear();
            for binding in request["walkerClasses"].as_array().ok_or("walker classes")? {
                let row=decode(&binding["unit"]["row"])?;let heap=decode(&binding["unit"]["heap"])?;let mut unit=Unit::default();
                if row.len()==Unit::FULL_STRIDE {unit.take_full(&row,&heap);}else{unit.take_import(&row,&heap);}
                let id=decode(&json!([binding["id"]]))?[0];
                if !id.is_finite() || id<0. || id.fract()!=0. || id>u32::MAX as f64 {return Err("walker class id".into());}
                walker_classes.push((unit.class_def,id as u32));
            }facts+=count;differences+=diffs.len() as u64;
            if first.is_null() {if let Some(diff)=diffs.first(){first=diff.clone();}}
            continue;
        }
        let row = decode(&request["unit"]["row"])?; let heap = decode(&request["unit"]["heap"])?;
        let mut imported = Unit::default();
        if row.len() == Unit::IMPORT_STRIDE { imported.take_import(&row,&heap); }
        else if row.len() == Unit::FULL_STRIDE { imported.take_full(&row,&heap); }
        else { return Err(format!("unexpected unit import width {}",row.len())); }
        let factory=!imported.human && imported.generation==0. && imported.removed && imported.tick_count==0.;
        let constructor=request["checks"].as_array().is_some_and(|checks|checks.iter().any(|c|c["export"]=="f3_body_skeleton"));
        if !constructor {continue;}
        let mut strings=recorded_strings(&request["strings"])?;
        let name = String::from_utf16_lossy(strings.units(imported.class_name));
        let side = String::from_utf16_lossy(strings.units(imported.side));
        let class = defs.class(&side,&name).ok_or_else(||format!("unknown imported class {name}"))?;
        let key = F3UnitKey {slot:row[0] as u32,generation:row[1] as u32,life:imported.generation};
        let shield = if let UnitBody::Droideka(d) = &imported.soldier {d.shield.as_ref().map(|s|s.def)} else {None};
        let binding = Bindings {key:key.clone(),side:imported.side,class_name:imported.class_name,class:imported.class_def,
            health_scale: strings.intern_str(bodies::health_scale(&class.record)),shield};
        let ceiling = content.battle.world.get("mission").map_or(200., |mission| match mission.get("maxFlyHeight") {
            None | Some(bfe_content::Value::Null) => 200., value => bfe_content::js::to_number(value),
        });
        let body = bodies::build(content,class,binding,ceiling,imported.human && imported.network_human!=Some(true),|text|strings.intern_str(text)).map_err(|e|e.to_string())?;
        let mut expected = vec![0.;Unit::IMPORT_STRIDE]; let mut expected_heap = Vec::new();
        body.unit.put_import(&mut expected,&mut expected_heap);
        // The slab writes the common two-word entity key. This comparison
        // derives the body's fields, while the enclosing owner supplies its id.
        expected[..2].copy_from_slice(&row[..2]);
        let (imported_words, imported_heap) = if row.len()==Unit::IMPORT_STRIDE {(row.clone(),heap.clone())} else {
            let mut value=vec![0.;Unit::IMPORT_STRIDE]; let mut indirect=Vec::new(); imported.put_import(&mut value,&mut indirect);value[..2].copy_from_slice(&row[..2]);(value,indirect)
        };
        let diff = expected.iter().zip(&imported_words).position(|(a,b)|a.to_bits()!=b.to_bits())
            .map(|word|json!({"field":"unit.row","word":word,"expected":format!("{:016x}",expected[word].to_bits()),"recorded":format!("{:016x}",imported_words[word].to_bits())}))
            .or_else(||(bits(&expected_heap)!=bits(&imported_heap)).then(||json!({"field":"unit.heap","expectedWords":expected_heap.len(),"recordedWords":imported_heap.len()})));
        if let Some(diff)=diff.filter(|_| factory) {differences+=1;if first.is_null(){first=json!({"row":rows,"class":name,"difference":diff});}}
        classes.insert(name.clone()); if factory {rows+=1;}
        for check in request["checks"].as_array().into_iter().flatten() {
            let export=check["export"].as_str().ok_or("check export")?;
            let mut words=Vec::new();let mut indirect=Vec::new();let mut arguments=Vec::new();
            match export {
                "f3_body_skeleton"=>{words.resize(F3BodySkeleton::WORDS,0.);body.skeleton.put(&mut words,&mut indirect);},
                "f3_adopt"=>{words.resize(F3Adoption::WORDS,0.);body.adoption.put(&mut words,&mut indirect);},
                "f3_factory_stage" | "f3_factory_resident"=>{let stage=body.stage(cookie);cookie=cookie.checked_add(1).ok_or("cookie overflow")?;words.resize(F3ColdStage::WORDS,0.);stage.put(&mut words,&mut indirect);},
                "f2_rider_birth"=>{
                    let recorded=F2RiderBirth::take(&decode(&check["row"])?,&decode(&check["heap"])?);
                    let first_clip=recorded.clips.first().ok_or("empty rider holder")?;
                    let holder=holders.iter().find(|holder|match holder {
                        bodies::Holder::Hull {hull,..}=>first_clip.hull==Some(*hull),
                        bodies::Holder::Turret {turret,..}=>first_clip.turret==Some(*turret),
                    }).ok_or("rider holder has no content facts")?;
                    let bank=class.walker_clips.and_then(|i|content.walker_clips.get(i)).map(|(_,c)|c)
                        .or_else(||class.clips.and_then(|i|content.body_clips.get(i)).map(|(_,c)|c.as_ref())).ok_or("rider bank")?;
                    // Resolve policy first. Numeric clip ids are bindings from
                    // the preceding clip_new exports; verify their immutable
                    // uploads against the content before using those ids.
                    let mut selected=Vec::new();
                    // A later external boarding reuses this constructor's immutable
                    // rig at the F3 lifecycle's current life, not life zero.
                    let mut expected=bodies::rider_birth(key.reference(),check["life"].as_f64().ok_or("rider has no preceding F3 life")?,body.unit.root_scale,bank,holder,|clip| {
                        selected.push(clip);selected.len() as u32-1
                    });
                    for (index,binding) in expected.clips.iter_mut().enumerate() {
                        if binding.clip<0 {continue;}
                        let clip=selected[binding.clip as usize];
                        let id=recorded.clips.get(index).ok_or("rider seat count")?.clip;
                        let upload=clips.iter().find(|v|v["id"].as_i64()==Some(id as i64)).ok_or("rider clip has no preceding upload")?;
                        let arguments=decode(&upload["arguments"])?;
                        let expected_args=[clip.frames_f64(),clip.tracks.len() as f64,clip.fps,u8::from(clip.root_motion) as f64];
                        let data=clip.data().iter().flat_map(|f|f.to_le_bytes()).collect::<Vec<_>>();
                        let tracks=clip.tracks.iter().flat_map(|i|(*i as i32).to_le_bytes()).collect::<Vec<_>>();
                        let uploaded_data=bytes(&upload["data"])?;
                        let uploaded_tracks=bytes(&upload["tracks"])?;
                        binding.clip=if bits(&arguments)==bits(&expected_args)&&data==uploaded_data&&tracks==uploaded_tracks {id}else{-2};
                    }
                    words.resize(F2RiderBirth::WORDS,0.);expected.put(&mut words,&mut indirect);
                },
                "units_droideka_attach" | "units_muzzle_attach"=>{
                    let class=walker_classes.iter().find(|(class,_)|*class==imported.class_def).ok_or("walker has no preceding class binding")?.1;
                    arguments.extend([key.slot as f64,class as f64]);
                    if export=="units_muzzle_attach" {words=body.muzzles.as_ref().ok_or("muzzles missing")?.words.clone();}
                },
                _=>return Err(format!("unknown construction check {export}")),
            }
            let recorded=decode(&check["row"])?;let recorded_heap=decode(&check["heap"])?;
            if bits(&arguments)!=bits(&decode(&check["arguments"])?)||bits(&words)!=bits(&recorded)||bits(&indirect)!=bits(&recorded_heap) {
                differences+=1;
                if first.is_null(){first=json!({"row":rows.saturating_sub(1),"class":name,"export":export,
                    "word":words.iter().zip(&recorded).position(|(a,b)|a.to_bits()!=b.to_bits()),"expectedWords":words.len(),"recordedWords":recorded.len()});}
            }
            facts+=1;*checked.entry(export.to_string()).or_default()+=1;
        }
    }
    println!("{}",json!({"rows":rows,"facts":facts,"differences":differences,"classes":classes.len(),"first":first,"checked":checked}));
    Ok(())
}
