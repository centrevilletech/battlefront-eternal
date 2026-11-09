//! Dumps cold startup/team/camera rows in the admission recorder's format.
//! With --check, consumes uncompressed recorder JSON lines on stdin and
//! compares content-derived rows using the recorder's shared string ids.
//! No game data or call captures are written into the source tree.
use bfe_content::{battle::{Battle, Loader}, files::{DirFiles, Files}, content::Content, defs::Defs};
use bfe_sim::{gen::{AiMissionTeamsInput, F4StartupClass, StrId}, store::Strings,
    world::admit::{Options, rows::{battle as battle_rows, cameras, startup, teams, Cx}}};
use serde_json::{json, Value};
use std::{collections::HashMap, io::{self, BufRead}};

fn bits(words: &[f64]) -> Vec<String> { words.iter().map(|v| format!("{:016x}", v.to_bits())).collect() }
fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 { out.push(if i <= chunk.len() { TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char } else { '=' }); }
    }
    out
}
fn record(export: &str, row: &[f64], heap: &[f64], scratch: &[u8], arguments: &[f64]) -> Value {
    json!({"type":"call", "phase":"admission", "export":export, "arguments":bits(arguments),
        "args":{"words":bits(row),"heap":bits(heap)}, "scratch":base64(scratch)})
}
fn main() {
    if let Err(message) = run() { eprintln!("{message}"); std::process::exit(1); }
}
fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = args.first().ok_or("content directory required")?;
    let mut options = Options { map:"nab2".into(), side:"rep".into(), enhanced:false, seed:1, heroes:false, unit:None };
    let mut check = false; let mut facts_file = None; let mut at = 1;
    while at < args.len() {
        let next = || args.get(at + 1).cloned().ok_or_else(|| format!("{} needs a value", args[at]));
        match args[at].as_str() {
            "--map" => { options.map = next()?; at += 1; }, "--side" => { options.side = next()?; at += 1; },
            "--seed" => { options.seed = next()?.parse().map_err(|_| "invalid seed")?; at += 1; },
            "--unit" => { options.unit = Some(next()?); at += 1; }, "--heroes" => options.heroes = true,
            "--ai" => { options.enhanced = match next()?.as_str() { "enhanced" => true, "classic" => false, _ => return Err("AI must be classic or enhanced".into()) }; at += 1; },
            "--facts" => { facts_file = Some(next()?); at += 1; },
            "--check" => check = true, other => return Err(format!("unknown argument {other}")),
        }
        at += 1;
    }
    let files = DirFiles::new(root);
    let battle = Battle::load(&Loader::new(&files), &options.map, &options.query())?;
    let facts = facts_file.map(|p| std::fs::read(p).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))).transpose()?;
    if check {
        let content = Content::build(battle)?; let defs = Defs::build(&content)?;
        return compare(&content, &defs, &files, &options, facts.as_ref());
    }
    let mut strings = Strings::new();
    let startup = startup::input(&battle, &mut |s| strings.intern_str(s)).map_err(|e| e.to_string())?;
    let mut row = vec![0.0; F4StartupClass::WORDS]; let mut heap = Vec::new(); startup.put(&mut row, &mut heap);
    println!("{}", record("f4_startup_class", &row, &heap, &[], &[]));
    let camera = cameras::aim(&battle);
    println!("{}", record("camera_init", &[], &[], &camera.init, &[]));
    println!("{}", record("camera_add_definition", &[], &[], &camera.definition, &[]));
    for input in [teams::names(&battle, &mut |s| strings.intern_str(s)), teams::after_ai(&battle, true, &mut |s| strings.intern_str(s)).map_err(|e| e.to_string())?] {
        let mut row = vec![0.0; AiMissionTeamsInput::WORDS]; let mut heap = Vec::new(); input.input.put(&mut row, &mut heap);
        println!("{}", record("ai_mission_teams", &row, &heap, &[], &[input.roster as f64]));
    }
    if let Some(facts) = &facts {
        let bound = bound(&battle, &options, facts, &mut |s| strings.intern_str(s))?;
        for (_, name, call) in bound.calls() { println!("{}", record(name, &call.row, &call.heap, &call.scratch, &call.args)); }
    }
    println!("{}", json!({"type":"strings", "strings":strings.list.iter().map(|s| String::from_utf16_lossy(s)).collect::<Vec<_>>()}));
    Ok(())
}

/// Strings already interned by unrelated owners retain their recorded ids.
/// Comparing these rows checks their values and generated heap layout; it
/// does not claim to validate the entire world's interning order.
fn compare(content: &Content, defs: &Defs, files: &dyn Files, options: &Options, facts: Option<&Value>) -> Result<(), String> {
    let battle = &content.battle;
    let mut strings: HashMap<String, StrId> = HashMap::new();
    let mut cx = Cx::new(content, defs, files, options, Strings::new());
    let (mut rows, mut differences, mut first) = (0, 0, None);
    let mut exports = std::collections::BTreeMap::<String,usize>::new();
    let mut words = 0;
    let mut active_battle = Value::Null;
    let mut retained_scratch = json!("");
    let (mut team_calls, mut bound_index) = (0, 0);
    let mut initial_calls: Option<bfe_sim::world::admit::rows::Calls> = None;
    let mut characters = CharacterQueries::default();
    let mut predecessors = Predecessors::default();
    let (mut checkpoint, mut ended) = (false, false);
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let recorded: Value = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        if recorded["type"] == "checkpoint" { checkpoint = true; }
        if recorded["type"] == "end" { ended = true; }
        if recorded["type"] != "call" || recorded["phase"] != "admission" { continue; }
        if recorded["battle"] != active_battle {
            if !active_battle.is_null() && (team_calls != 2 || characters.pending != 0) { return Err("previous battle omitted owned calls".into()); }
            if initial_calls.as_ref().is_some_and(|calls| calls.len() != bound_index) { return Err("recording omitted native battle calls".into()); }
            strings.clear(); retained_scratch = json!(""); cx.returned.clear(); predecessors = Predecessors::default(); team_calls = 0; bound_index = 0; initial_calls = None; characters = CharacterQueries::default(); active_battle = recorded["battle"].clone();
        }
        if let (Some(id), Some(text)) = (recorded["intern"]["id"].as_u64(), recorded["intern"]["text"].as_str()) {
            strings.insert(text.into(), StrId(id as u32));
        }
        predecessors.observe(&recorded)?;
        if initial_calls.is_some() { characters.observe(&recorded)?; }
        let name = recorded["export"].as_str().unwrap_or("");
        // Scratch addresses and camera handles are outputs of earlier calls,
        // supplied to the builder before checking the current export's args.
        let scalar = || -> Result<f64, String> {
            let text = recorded["returned"]["scalar"].as_str().ok_or("missing scalar return")?;
            u64::from_str_radix(text, 16).map(f64::from_bits).map_err(|e| e.to_string())
        };
        if name == "sim_scratch" { cx.returned.push(("sim_scratch", scalar()?)); }
        let mut missing = None;
        let mut intern = |text: &str| -> StrId {
            strings.get(text).copied().unwrap_or_else(|| { missing = Some(text.to_string()); StrId(u32::MAX) })
        };
        let expected = match name {
            "camera_init" => {
                let call = cameras::build_init(&mut cx).map_err(|e| e.to_string())?.remove(0).2;
                cx.returned.push(("camera_init", scalar()?));
                Some(record(name, &call.row, &call.heap, &call.scratch, &call.args))
            }
            "camera_add_definition" => {
                let call = cameras::build_definition(&mut cx).map_err(|e| e.to_string())?.remove(0).2;
                Some(record(name, &call.row, &call.heap, &call.scratch, &call.args))
            }
            "f4_startup_class" => {
                let input = startup::input(battle, &mut intern).map_err(|e| e.to_string())?;
                let mut row = vec![0.0; F4StartupClass::WORDS]; let mut heap = Vec::new(); input.put(&mut row, &mut heap);
                Some(record(name, &row, &heap, &[], &[]))
            }
            "ai_mission_teams" => {
                let input = if team_calls == 0 { teams::names(battle, &mut intern) }
                    else { teams::after_ai(battle, true, &mut intern).map_err(|e| e.to_string())? };
                team_calls += 1;
                let mut row = vec![0.0; AiMissionTeamsInput::WORDS]; let mut heap = Vec::new(); input.input.put(&mut row, &mut heap);
                Some(record(name, &row, &heap, &[], &[input.roster as f64]))
            }
            _ if ["f4_live_init", "f4_battle_init", "f4_battle_capacity", "f4_tail_init", "f4_tail_capacity", "f4_connections_init", "f4_roster_init"].contains(&name) => {
                if initial_calls.is_none() {
                    let bound = match facts {
                        Some(facts) => bound(battle, options, facts, &mut intern)?,
                        None => {
                            if !battle.query.get("spawn").is_empty() { return Err("recorded spawn cursor needs an explicit predecessor input".into()); }
                            let resolved = predecessors.resolved()?;
                            battle_rows::bound(battle, options, &resolved, &predecessors.factions, predecessors.posts.len(), predecessors.mobile.len(), &mut intern).map_err(|e|e.to_string())?
                        }
                    };
                    characters.seed(&predecessors.facts.characters);
                    initial_calls = Some(bound.calls());
                }
                let calls = initial_calls.as_ref().ok_or("missing initial calls")?;
                let call = if let Some((_, expected, call)) = calls.get(bound_index) {
                    if *expected != name { return Err(format!("battle call {bound_index}: expected {expected}, recorded {name}; first difference {first:?}")); }
                    bound_index += 1;
                    call.clone()
                } else if name == "f4_battle_capacity" {
                    if !characters.take() { return Err("capacity query needs a preceding Character birth output; annotate returned events with the verified replayer".into()); }
                    battle_rows::character_capacity()
                } else { return Err(format!("unexpected {name} at sequence {}; first difference {first:?}", recorded["sequence"])); };
                Some(record(name, &call.row, &call.heap, &call.scratch, &call.args))
            }
            _ => {
                // P0 retains the previous scratch upload until StringTable
                // consumes it. Later exports can carry those same bytes
                // without writing or reading scratch themselves. This is
                // an earlier host transport input, not an owned payload.
                retained_scratch = if name == "sim_str_intern" { json!("") } else { recorded["scratch"].clone() };
                None
            },
        };
        let Some(mut expected) = expected else { continue };
        if !name.starts_with("camera_") { expected["scratch"] = retained_scratch.clone(); }
        else { retained_scratch = expected["scratch"].clone(); }
        if let Some(text) = missing { return Err(format!("{name}: missing recorded intern for {text:?}")); }
        let fields = ["args", "arguments", "scratch"];
        for field in fields {
            if expected[field] != recorded[field] {
                differences += 1;
                if first.is_none() {
                    let detail = if field == "scratch" {
                        let a=expected[field].as_str().unwrap_or("");let b=recorded[field].as_str().unwrap_or("");
                        json!({"expectedLength":a.len(),"recordedLength":b.len(),"firstBase64Difference":a.bytes().zip(b.bytes()).position(|(a,b)|a!=b)})
                    } else { json!({"expected":expected[field],"recorded":recorded[field]}) };
                    first = Some(json!({"sequence":recorded["sequence"],"export":name,"field":field,"detail":detail}));
                }
            }
        }
        rows += 1;
        *exports.entry(name.to_owned()).or_default() += 1;
        words += expected["args"]["words"].as_array().map_or(0,Vec::len) + expected["args"]["heap"].as_array().map_or(0,Vec::len) + expected["arguments"].as_array().map_or(0,Vec::len);
    }
    println!("{}", json!({"type":"comparison", "rows":rows, "words":words, "exports":exports, "runtimeFactsProvided":true, "differences":differences, "first":first}));
    if rows == 0 { return Err("no owned admission calls found".into()); }
    if initial_calls.as_ref().is_none_or(|calls| calls.len() != bound_index) { return Err("recording omitted native battle calls".into()); }
    if !checkpoint || !ended { return Err("recording has no completed admission checkpoint/end".into()); }
    if characters.pending != 0 { return Err("recording omitted new-character capacity queries".into()); }
    if team_calls != 2 { return Err("recording omitted a mission team call".into()); }
    if differences != 0 { return Err(format!("{differences} admission differences")); }
    Ok(())
}

/// --facts supplies the preceding native owners' identities and counts.
/// Float fields may use hexadecimal f64 bits, so a negative zero survives
/// this diagnostic boundary exactly as it does in the recorded call log.
fn bound(battle: &Battle, options: &Options, value: &Value, intern: &mut impl FnMut(&str) -> StrId) -> Result<battle_rows::BoundRows, String> {
    let count = |key: &str| -> Result<u32, String> {
        value[key].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| format!("runtime facts need u32 {key}"))
    };
    fn reference<T>(value: &Value) -> Result<Option<bfe_sim::gen::Ref<T>>, String> {
        if value.is_null() { return Ok(None); }
        let part = |i| value[i].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or("reference needs slot and generation");
        Ok(Some(bfe_sim::gen::Ref::new(part(0)?, part(1)?)))
    }
    fn number(value: &Value) -> Result<f64, String> {
        match value.as_str() {
            Some(bits) => u64::from_str_radix(bits, 16).map(f64::from_bits).map_err(|e| e.to_string()),
            None => value.as_f64().ok_or_else(|| "runtime fact needs number or hexadecimal bits".into()),
        }
    }
    let characters = value["characters"].as_array().ok_or("runtime facts need characters")?.iter()
        .map(|v| reference(v)?.ok_or_else(|| "character reference cannot be null".into())).collect::<Result<_, String>>()?;
    let facts = battle_rows::Facts {
        local: reference(&value["local"])? , character: reference(&value["character"])? , characters,
        next_id: count("nextId")?, built: count("built")?, construction_limit: if value["constructionLimit"].is_null() { None } else { Some(count("constructionLimit")?) },
        fleet: count("fleet")?, prepared: count("prepared")?, point_capacity: count("pointCapacity")?,
        spawn_index: number(&value["spawnIndex"])? , random: count("random")?, gameplay: count("gameplay")?,
    };
    let factions = value["factions"].as_array().ok_or("runtime facts need factions")?.iter().map(number).collect::<Result<Vec<_>, _>>()?;
    battle_rows::bound(battle, options, &facts, &factions, count("posts")? as usize, count("mobilePosts")? as usize, intern).map_err(|e| e.to_string())
}

/// Predecessor inputs only: this tracker never decodes any f4_* target row.
/// The registry, fleet and AI observations are the facts a live sequence
/// supplies directly after those owners have run.
#[derive(Default)]
pub struct Predecessors {
    pub facts: battle_rows::Facts,
    pub posts: Vec<bfe_sim::gen::Ref<bfe_sim::gen::Post>>,
    pub mobile: Vec<bfe_sim::gen::Ref<bfe_sim::gen::MobilePost>>,
    pub factions: Vec<f64>,
    fleet: std::collections::HashSet<(u32,u32)>,
    prepared: std::collections::HashSet<(u32,u32)>,
    combat_streams: HashMap<u32,u32>,
    next_id: Option<u32>,
    random: Option<u32>,
    roster_seen: bool,
}
fn decode(words: &Value) -> Result<Vec<f64>, String> {
    words.as_array().ok_or("recorded words must be an array")?.iter().map(|v| {
        u64::from_str_radix(v.as_str().ok_or("recorded word must be hexadecimal")?,16).map(f64::from_bits).map_err(|e| e.to_string())
    }).collect()
}
impl Predecessors {
    pub fn observe(&mut self, event: &Value) -> Result<(), String> {
        use bfe_sim::gen::*;
        let name = event["export"].as_str().unwrap_or("");
        // Decode only dependencies. Large geometry uploads remain streamed
        // past the checker and cannot become accidental facts from a target.
        if !["f2_register","f3_registry_add","f3_factory_reserve","f3_factory_resident","f3_command","ai_world_begin","ai_world_roster","ai_mission_teams","sim_import","sim_export"].contains(&name) { return Ok(()); }
        let args = decode(&event["arguments"])?;
        let row = decode(&event["args"]["words"])?;
        let heap = decode(&event["args"]["heap"])?;
        let returned = || -> Result<f64,String> {
            u64::from_str_radix(event["returned"]["scalar"].as_str().ok_or("predecessor needs a scalar return")?,16).map(f64::from_bits).map_err(|e|e.to_string())
        };
        match name {
            "f2_register" if args.len() == 3 => {
                let key=(word_u32(args[0]),word_u32(args[1]));
                if args[2] != 0.0 { self.prepared.insert(key); } else { self.fleet.insert(key); }
            }
            "f3_registry_add" if args.first().copied() == Some(KIND_UNIT as f64) => {
                self.facts.built = self.facts.built.checked_add(1).ok_or("too many registry births")?;
                self.next_id = Some(word_u32(returned()?).checked_add(1).ok_or("registry id overflow")?);
            }
            "f3_factory_reserve" if !args.is_empty() => {
                self.facts.construction_limit=Some(self.facts.construction_limit.unwrap_or(0).max(word_u32(args[0])));
            }
            "f3_factory_resident" if row.len() >= F3ColdStage::WORDS => {
                let stage=F3ColdStage::take(&row,&heap);
                self.facts.local=Some(Ref::new(stage.adoption.key.slot,stage.adoption.key.generation));
            }
            "f3_command" if row.len() >= F3Change::WORDS => {
                let change=F3Change::take(&row,&heap);
                if change.action == F3Action::RandomBinding && change.index == 0 {
                    self.combat_streams.insert(change.key.slot,word_u32(change.value));
                }
            }
            "ai_world_begin" if row.len() >= AiWorldSetup::WORDS => {
                let setup=AiWorldSetup::take(&row,&heap); self.random=Some(setup.settings.stream_a);
                self.posts=setup.navigation.posts.into_iter().filter_map(|post| match post.post { SpawnPost::Post(r) => Some(r), SpawnPost::MobilePost(_) => None }).collect();
            }
            "ai_world_roster" if row.len() >= AiRosterSetup::WORDS => {
                let setup=AiRosterSetup::take(&row,&heap);
                self.facts.point_capacity=battle_rows::point_capacity(0,setup.pads.iter().map(|pad|Some(&pad.pad))).map_err(|e|e.to_string())?;
                self.roster_seen=true;
            }
            "sim_import" if args.first().copied() == Some(KIND_MOBILE_POSTS as f64) && row.len() >= MobilePosts::FULL_STRIDE => {
                let mut posts=MobilePosts::default();posts.take_full(&row,&heap);self.mobile=posts.posts;
            }
            "sim_import" if args.first().copied() == Some(KIND_UNITS as f64) && row.len() >= Units::FULL_STRIDE => {
                let mut units=Units::default();units.take_full(&row,&heap);self.next_id=Some(word_u32(units.next_id));
            }
            "sim_export" if !event["returned"]["block"].is_null() => {
                let words=decode(&event["returned"]["block"]["words"])?;
                let heap=decode(&event["returned"]["block"]["heap"])?;
                if args.first().copied() == Some(KIND_CHARACTER as f64) {
                    let mut refs=Vec::new();
                    for row in words.chunks_exact(Character::EXPORT_STRIDE) {
                        let birth=AiCharacterBirth::take(row,&heap);
                        let id=Ref::new(birth.slot,birth.generation);let mut character=Character::default(); character.take_full(row,&heap);
                        if character.human { self.facts.character=Some(id); }
                        refs.push(id);
                    }
                    self.facts.characters=refs;
                }
                if args.first().copied() == Some(KIND_SPAWNING as f64) && words.len() >= Spawning::EXPORT_STRIDE {
                    let mut spawning=Spawning::default();spawning.take_full(&words,&heap);
                    self.facts.characters=spawning.characters;
                }
            }
            "ai_mission_teams" if !event["returned"]["block"].is_null() => {
                let words=decode(&event["returned"]["block"]["words"])?;
                let heap=decode(&event["returned"]["block"]["heap"])?;
                if words.len() >= AiMissionTeams::WORDS { self.factions=AiMissionTeams::take(&words,&heap).teams.into_iter().map(|t|t.faction).collect(); }
            }
            _=>{}
        }
        Ok(())
    }
    pub fn resolved(&self) -> Result<battle_rows::Facts,String> {
        let mut facts=self.facts.clone();
        let local=facts.local.ok_or("no preceding resident viewpoint")?;
        facts.next_id=self.next_id.ok_or("no preceding registry id")?;
        facts.random=self.random.ok_or("no preceding AI stream")?;
        facts.gameplay=*self.combat_streams.get(&local.slot).ok_or("no preceding viewpoint combat stream; recording needs direct f3_command rows")?;
        facts.fleet=self.fleet.len().try_into().map_err(|_|"too many fleet refs")?;
        facts.prepared=self.prepared.len().try_into().map_err(|_|"too many prepared refs")?;
        if !self.roster_seen || facts.characters.is_empty() || facts.character.is_none() || self.factions.is_empty() { return Err("incomplete preceding AI roster outputs".into()); }
        Ok(facts)
    }
}

/// A roster command can expose a character absent from F4Battle's initial
/// spawning snapshot. ensureCharacter binds that facade exactly once.
#[derive(Default)]
pub struct CharacterQueries {
    known: std::collections::HashSet<(u32,u32)>,
    pending: usize,
}
impl CharacterQueries {
    pub fn seed(&mut self, refs: &[bfe_sim::gen::Ref<bfe_sim::gen::Character>]) {
        self.known.extend(refs.iter().map(|r|(r.slot,r.gen)));
    }
    pub fn observe(&mut self, event: &Value) -> Result<(),String> {
        use bfe_sim::gen::*;
        if let Some(births)=event["returned"]["characterBirths"].as_array() {
            for birth in births {
                let row=decode(birth)?;
                if row.len() < F4Handle::WORDS { return Err("short Character birth".into()); }
                self.bind(F4Handle::take(&row,&[]));
            }
        }
        if event["export"] != "f4_roster_command" || event["returned"]["block"].is_null() { return Ok(()); }
        let row=decode(&event["returned"]["block"]["words"])?;
        let heap=decode(&event["returned"]["block"]["heap"])?;
        if row.len() < F4RosterResult::WORDS { return Err("short roster result".into()); }
        if let Some(handle)=F4RosterResult::take(&row,&heap).handle {
            self.bind(handle);
        }
        Ok(())
    }
    fn bind(&mut self, handle: bfe_sim::gen::F4Handle) {
        if handle.kind == bfe_sim::gen::F4ObjectKind::Character && self.known.insert((handle.slot,handle.generation)) { self.pending+=1; }
    }
    pub fn take(&mut self) -> bool {
        if self.pending == 0 { false } else { self.pending-=1; true }
    }
}
