//! Cold admission for a resolved 2005 mission on its authority.
use super::{sequence, AdmitError, Admitted, Options, Rows, Stage};
use crate::battle::rules::events::Reinforcements;
use crate::battle::rules::ModeParameters;
use crate::battle::script::{
    ScriptChain, ScriptChunk, ScriptRules, ScriptRulesConfig, ScriptTelemetryHandle,
    DEFAULT_SCRIPT_COMMAND_LIMIT, DEFAULT_SCRIPT_TIMER_LIMIT,
};
use crate::world::owner::World;
use bfe_content::battle::{Battle, Loader};
use bfe_content::files::Files;
use bfe_family::GameFamily;
use bfe_content::value::{Object, Value as ContentValue};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

/// Cold resources retained by a native host for this selection.
pub struct MissionResources {
    pub selection: MissionSelection,
    pub preferences: MissionPreferences,
    pub query: String,
    /// Movement ceiling composed by the authority before bodies are admitted, in metres.
    pub max_fly_height: f64,
    /// Identity of the exact bytes read to construct and replay this developer world.
    pub admission_digest: String,
    /// The ordered mission object used to build the authority and its local scene.
    pub mission_json: String,
    pub building_sources: Vec<usize>,
    pub definitions: bfe_content::defs::Defs,
    pub registry: bfe_content::Value,
}

struct AdmissionFiles<'a> {
    files: &'a dyn Files,
    reads: RefCell<BTreeMap<String, Option<[u8; 32]>>>,
}

impl Files for AdmissionFiles<'_> {
    fn read(&self, path: &str) -> Option<Rc<[u8]>> {
        let path = path.trim_start_matches('/');
        let bytes = self.files.read(path);
        self.reads
            .borrow_mut()
            .entry(path.to_owned())
            .or_insert_with(|| bytes.as_ref().map(|b| Sha256::digest(b).into()));
        bytes
    }
}

impl AdmissionFiles<'_> {
    fn admission_digest(&self, selection: &MissionSelection) -> String {
        let mut hash = Sha256::new();
        hash.update(b"BFE prepared mission v1");
        for id in [
            selection.family.as_str(),
            &selection.map,
            &selection.era,
            &selection.mode,
        ] {
            hash.update((id.len() as u32).to_le_bytes());
            hash.update(id);
        }
        for (path, body) in self.reads.borrow().iter() {
            hash.update(bfe_wide::Wide::from_u32(u32::try_from(path.len()).unwrap_or(u32::MAX)).to_le_bytes());
            hash.update(path);
            hash.update([u8::from(body.is_some())]);
            if let Some(bytes) = body {
                hash.update(bytes);
            }
        }
        format!("bfe-admission-sha256-{:x}", hash.finalize())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionSelection {
    pub family: GameFamily,
    pub map: String,
    pub era: String,
    pub mode: String,
    pub seed: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MissionPreferences {
    pub assault_target: Option<u32>,
    pub bots_per_team: Option<u32>,
    /// Start a local developer session with its selected body already deployed.
    pub direct_spawn: bool,
}

/// The journal retains the resolved request and its native construction rows.
/// Replaying admission reads the same fingerprinted files and starts a fresh VM.
pub struct MissionJournal {
    /// An explicitly requested authored starting node in a direct local battle.
    local_spawn_path: Option<String>,
    pub selection: MissionSelection,
    pub preferences: MissionPreferences,
    pub options: Options,
    pub rows: Rows,
    pub resources: MissionResources,
}

impl MissionJournal {
    /// Recreate the selected authority, accepting only its original content and construction rows.
    pub fn replay(&self, files: &dyn Files) -> Result<(Box<World>, Admitted), AdmitError> {
        let mut world = World::new();
        world.units.spawn_path.clone_from(&self.local_spawn_path);
        let (admitted, journal, _) = admit_developer_mission(
            &mut world, files, &self.options, &self.selection, self.preferences,
        )?;
        if journal.resources.admission_digest != self.resources.admission_digest {
            return Err(error("mission replay content differs from its admission journal"));
        }
        compare_rows(&self.rows, &journal.rows)?;
        Ok((world, admitted))
    }
}

fn compare_rows(expected: &Rows, actual: &Rows) -> Result<(), AdmitError> {
    use super::rows::{units::call, Call};
    use crate::gen::{F3ColdStage, WorldTickSetup};

    // Native rows carry signed zeros and NaN payloads, including absent water levels.
    fn words_match(a: &[f64], b: &[f64]) -> bool {
        a.iter().map(|n| n.to_bits()).eq(b.iter().map(|n| n.to_bits()))
    }
    fn calls_match(a: &Call, b: &Call) -> bool {
        words_match(&a.args, &b.args) && words_match(&a.row, &b.row)
            && words_match(&a.heap, &b.heap) && a.scratch == b.scratch && a.strings == b.strings
    }
    let setup = |s: &WorldTickSetup| call(WorldTickSetup::WORDS, |r, h| s.put(r, h));
    if !calls_match(&setup(&expected.setup), &setup(&actual.setup)) {
        return Err(error("mission replay world setup differs"));
    }
    let cold = |s: &F3ColdStage| call(F3ColdStage::WORDS, |r, h| s.put(r, h));
    if expected.cold_stages.len() != actual.cold_stages.len()
        || !expected.cold_stages.iter().zip(&actual.cold_stages)
            .all(|(a, b)| calls_match(&cold(a), &cold(b))) {
        return Err(error("mission replay cold bodies differ"));
    }
    if expected.calls.len() != actual.calls.len() {
        return Err(error("mission replay stage count differs"));
    }
    for ((stage, calls), (other, replay)) in expected.calls.iter().zip(&actual.calls) {
        if stage != other || calls.len() != replay.len() {
            return Err(error(format!("mission replay stage differs at {stage:?}")));
        }
        for ((s, name, a), (t, other_name, b)) in calls.iter().zip(replay) {
            if s != t || name != other_name || !calls_match(a, b) {
                return Err(error(format!("mission replay call differs at {s:?}/{name}")));
            }
        }
    }
    Ok(())
}

fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content {
        stage: Stage::Content,
        message: message.into(),
    }
}

fn basename(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

impl MissionSelection {
    fn menu_title(&self, common: &ContentValue) -> String {
        let strings = common.get("strings");
        let label = |key: String| strings?.get(&bfe_content::zero_hash::fnv1a_hex(&key))?.as_str().filter(|s| !s.is_empty());
        let map = label(format!("mapname.name.{}", self.map));
        let mode_key = if self.mode == "eli" { "elimination" } else { &self.mode };
        let mode = label(format!("modename.name.{mode_key}"));
        format!("{} ({})", map.unwrap_or(&self.map), mode.unwrap_or(mode_key))
    }

    fn validate(&self, options: &Options) -> Result<(), AdmitError> {
        if self.family != GameFamily::Swbf2
            || self.family != options.family
            || self.mode != options.mode
            || options.practice_target.is_some()
            || self.map != options.map
            || self.seed != options.seed
            || !basename(&self.map)
            || !matches!(
                (self.era.as_str(), options.side.as_str()),
                ("cw", "rep" | "cis") | ("gcw", "all" | "imp")
            )
            || !matches!(self.mode.as_str(), "eli" | "ass")
        {
            return Err(error("invalid scripted mission selection"));
        }
        Ok(())
    }

    pub(crate) fn read_chain(
        &self,
        files: &dyn Files,
        map: &Value,
    ) -> Result<(ScriptChain, Vec<String>), AdmitError> {
        let path = format!("maps/{}/scripts/{}-{}.json", self.map, self.era, self.mode);
        let bytes = files
            .read(&path)
            .ok_or_else(|| error(format!("missing mission manifest {path}")))?;
        if bytes.len() > 64 * 1024 {
            return Err(error("mission manifest too large"));
        }
        let manifest: Value = serde_json::from_slice(&bytes).map_err(|e| error(e.to_string()))?;
        if manifest["version"] != 1
            || manifest["family"] != self.family.as_str()
            || manifest["map"] != self.map
            || manifest["era"] != self.era
            || manifest["mode"] != self.mode
        {
            return Err(error("mission manifest does not match selection"));
        }
        let entry = manifest["entry"]
            .as_str()
            .filter(|s| basename(s))
            .ok_or_else(|| error("invalid mission entry"))?;
        let trace = manifest["chunks"]
            .as_array()
            .ok_or_else(|| error("invalid mission chunk list"))?;
        if trace.first().and_then(Value::as_str) != Some(entry) || trace.len() > 256 {
            return Err(error("invalid mission chunk order or count"));
        }
        let rows = manifest["scripts"]
            .as_array()
            .filter(|rows| rows.len() <= 512)
            .ok_or_else(|| error("invalid script records"))?;
        if !rows.first().is_some_and(|row| row["id"] == entry && row.get("error").is_none()) {
            return Err(error("prepared script records must start with the mission entry"));
        }
        let mission = &map["missions"][entry];
        if mission["scriptManifest"] != path || mission["scripts"] != manifest["scripts"] {
            return Err(error("mission script references do not match"));
        }
        let mut chunks: Vec<ScriptChunk> = Vec::new();
        let mut by_name = BTreeMap::new();
        let mut bodies: BTreeMap<&str, Rc<[u8]>> = BTreeMap::new();
        let mut order = Vec::new();
        let mut total = 0usize;
        for row in rows {
            let name = row["id"]
                .as_str()
                .filter(|s| basename(s))
                .ok_or_else(|| error("invalid script identifier"))?;
            if !row["name"].as_str().is_some_and(|s| s.eq_ignore_ascii_case(name))
                || !row["archive"].as_str().is_some_and(|s| !s.is_empty())
            {
                return Err(error("invalid prepared script name or archive"));
            }
            if let Some(failure) = row.get("error") {
                if !failure.as_str().is_some_and(|s| !s.is_empty())
                    || ["file", "length", "sha256"].iter().any(|key| row.get(*key) != Some(&Value::Null))
                {
                    return Err(error("invalid unresolved script request"));
                }
                continue;
            }
            let hash = row["sha256"]
                .as_str()
                .filter(|s| {
                    s.len() == 64
                        && s.bytes()
                            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                })
                .ok_or_else(|| error("invalid script hash"))?;
            let file = format!("scripts/{hash}.luac");
            if row["file"] != file
                || trace.get(order.len()).and_then(Value::as_str) != Some(name)
            {
                return Err(error("invalid script record order or path"));
            }
            order.push(name.to_string());
            let length = row["length"]
                .as_u64()
                .ok_or_else(|| error("missing script length"))?;
            if let Some(previous) = by_name.insert(name, (length, hash)) {
                if previous != (length, hash) {
                    return Err(error("conflicting repeated script reference"));
                }
                continue;
            }
            let bytes = if let Some(body) = bodies.get(hash) {
                Rc::clone(body)
            } else {
                let body = files
                    .read(&file)
                    .ok_or_else(|| error(format!("missing script {name}")))?;
                total = total.saturating_add(body.len());
                if total > 16 * 1024 * 1024 || format!("{:x}", Sha256::digest(&body)) != hash {
                    return Err(error("mission script size or digest mismatch"));
                }
                bfe_script::load_chunk(&body).map_err(|e| error(format!("{name}: {e}")))?;
                bodies.insert(hash, body.clone());
                body
            };
            if usize::try_from(length).ok() != Some(bytes.len()) {
                return Err(error("mission script length mismatch"));
            }
            chunks.push(ScriptChunk {
                name: name.to_owned(),
                bytes,
            });
        }
        if order.len() != trace.len() {
            return Err(error("incomplete script chain"));
        }
        Ok((ScriptChain::new(entry, chunks), order))
    }
}

fn team_rows(
    rules: &ScriptRules,
    battle: &Battle,
) -> Result<ContentValue, AdmitError> {
    let host = rules.host();
    let roster = battle.roster_report()
        .ok_or_else(|| error("missing prepared roster"))?;
    let mut teams = Object::new();
    for (&id, team) in &host.setup.teams {
        if !(1..=7).contains(&id) {
            return Err(error("invalid mission team"));
        }
        let declared = team.unit_count.unwrap_or(0);
        let mut population = declared.min(host.config.effective_assault_num_bots());
        if population > 64
            || team.classes.is_empty()
            || team.classes.len() > crate::battle::spawning::CLASS_CAPACITY
        {
            return Err(error("unsupported mission roster size"));
        }
        let mut quotas = Vec::new();
        let mut limits = Vec::new();
        for class in &team.classes {
            if !roster.classes.iter().any(|entry| entry.class == class.class_name) {
                return Err(error(format!("missing mission class {}", class.class_name)));
            }
            let max = class.max_units.unwrap_or(population as i32);
            if !basename(&class.class_name)
                || class.min_units < 0
                || max < 0
                || class.max_units.is_some_and(|n| n < class.min_units)
            {
                return Err(error("invalid class limits"));
            }
            quotas.push((class.min_units as u32).min(population));
            limits.push((max as u32).min(population));
        }
        population = population.min(limits.iter().sum());
        // When the bot preference shrinks the roster, share the available
        // places between the declared minimums before filling flexible slots.
        let minimums = quotas.clone();
        quotas.fill(0);
        let mut left = population;
        while left > 0 && quotas.iter().zip(&minimums).any(|(n, min)| n < min) {
            for (n, min) in quotas.iter_mut().zip(&minimums) {
                if left > 0 && *n < *min {
                    *n += 1;
                    left -= 1;
                }
            }
        }
        while left > 0 {
            for (quota, max) in quotas.iter_mut().zip(&limits) {
                if left > 0 && *quota < *max {
                    *quota += 1;
                    left -= 1;
                }
            }
        }
        let slots = team.classes.iter().zip(quotas).map(|(class, count)| {
            ContentValue::Array(Rc::new(vec![
                ContentValue::str(&class.class_name),
                ContentValue::Number(count.into()),
            ]))
        }).collect();
        let tickets = team.initial_reinforcements.map(|t| match t {
            Reinforcements::Finite(n) => f64::from(n),
            Reinforcements::Unlimited => -1.0,
        });
        let row = Object::from_written(vec![
            ("name".into(), team.name.as_deref().map_or(ContentValue::Null, ContentValue::str)),
            ("classes".into(), ContentValue::Array(Rc::new(slots))),
            ("units".into(), ContentValue::Number(population.into())),
            ("reinforcements".into(), tickets.map_or(ContentValue::Null, ContentValue::Number)),
        ]);
        teams.set(id.to_string().into(), ContentValue::Object(Rc::new(row)));
    }
    Ok(ContentValue::Object(Rc::new(teams)))
}

/// Developer authority entry. The file provider is rooted at the selected family's directory.
pub fn admit_developer_mission(
    world: &mut World,
    files: &dyn Files,
    options: &Options,
    selection: &MissionSelection,
    preferences: MissionPreferences,
) -> Result<(Admitted, MissionJournal, ScriptTelemetryHandle), AdmitError> {
    selection.validate(options)?;
    let mut rows = Rows::default();
    let (admitted, telemetry, resources) = prepare(
        world,
        files,
        options,
        selection,
        preferences,
        false,
        |w, content, files, opt| sequence::run_into(w, &content, files, opt, &mut rows),
    )?;
    Ok((
        admitted,
        MissionJournal {
            local_spawn_path: world.units.spawn_path.clone(),
            selection: selection.clone(),
            preferences,
            options: options.clone(),
            rows,
            resources,
        },
        telemetry,
    ))
}

pub(super) fn prepare<T>(
    world: &mut World,
    files: &dyn Files,
    options: &Options,
    selection: &MissionSelection,
    preferences: MissionPreferences,
    network: bool,
    construct: impl FnOnce(
        &mut World,
        bfe_content::content::Content,
        &dyn Files,
        &Options,
    ) -> Result<T, AdmitError>,
) -> Result<(T, ScriptTelemetryHandle, MissionResources), AdmitError> {
    selection.validate(options)?;
    if !world.can_admit_mission() {
        return Err(error("mission requires a fresh authority"));
    }
    if preferences
        .assault_target
        .is_some_and(|v| v == 0 || v > crate::battle::script::adapter::MAX_EXACT_LUA_HANDLE)
        || preferences.bots_per_team.is_some_and(|v| v > 64)
    {
        return Err(error("invalid mission preferences"));
    }
    let tracked = AdmissionFiles {
        files,
        reads: RefCell::new(BTreeMap::new()),
    };
    let map_path = format!("maps/{}/map.json", selection.map);
    let raw = tracked
        .read(&map_path)
        .ok_or_else(|| error("missing mission world"))?;
    let map: Value = serde_json::from_slice(&raw).map_err(|e| error(e.to_string()))?;
    if map["family"] != "swbf2" || map["name"] != selection.map {
        return Err(error("mission entry is absent from the selected world"));
    }
    let (chain, order) = selection.read_chain(&tracked, &map)?;
    let mut fresh = World::new();
    if !network && preferences.direct_spawn {
        fresh.units.spawn_path.clone_from(&world.units.spawn_path);
    }
    fresh.battle.goal_handles = Some(world.goal_handles());
    let mut rules = ScriptRules::with_goal_handles(
        chain,
        ScriptRulesConfig {
            network,
            seed: selection.seed,
            assault_num_bots: preferences.bots_per_team,
            assault_score_limit: preferences.assault_target,
            world_filename: selection.map.clone(),
            ..Default::default()
        },
        fresh.goal_handles(),
    )
    .map_err(|e| error(e.to_string()))?;
    if let Err(e) = rules.run_load() {
        if rules.setup_failure().is_some() {
            let common = files.read("common/common.json").and_then(|bytes| bfe_content::json::parse_bytes(&bytes).ok());
            let title = selection.menu_title(common.as_ref().unwrap_or(&ContentValue::Null));
            return Err(AdmitError::MissionSetup(ScriptRules::startup_message(Some(&title))));
        }
        return Err(error(e.to_string()));
    }
    if rules.host().setup.loaded_scripts != order {
        return Err(error("executed mission setup differs from prepared chain"));
    }
    let own = if matches!(options.side.as_str(), "cis" | "imp") {
        2
    } else {
        1
    };
    let setup = &rules.host().setup;
    let wave = if network {
        15.0
    } else {
        setup.spawn_wave_delay.unwrap_or(10.0)
    };
    let stagger = setup.spawn_slot_delay.unwrap_or(0.25);
    if wave <= 0.0 || stagger <= 0.0 {
        return Err(error("invalid spawn period"));
    }
    let declared = &setup.teams.get(&own)
        .ok_or_else(|| error("selected team has no classes"))?.classes;
    let selected = match &options.unit {
        Some(unit) if declared.iter().any(|c| &c.class_name == unit) => unit.as_str(),
        Some(_) => return Err(error("selected class is outside the mission roster")),
        None => &declared.first().ok_or_else(|| error("selected team has no classes"))?.class_name,
    };
    if !basename(selected) {
        return Err(error("invalid mission class"));
    }
    let side = selected.split('_').next().unwrap();
    let ai = if options.enhanced { "enhanced" } else { "classic" };
    let mut query = format!(
        "?map={}&side={side}&era={}&ai={ai}&seed={}&unit={selected}&family=swbf2&mode={}&mission={}",
        selection.map, selection.era, selection.seed, selection.mode, order[0]
    );
    if !network && !preferences.direct_spawn {
        query.push_str("&spawnscreen=1");
    }
    let mut battle = Battle::load_for_family(
        &Loader::new(&tracked), selection.family, &selection.map, &query,
    ).map_err(error)?;
    let teams = team_rows(&rules, &battle)?;
    let mut opt = options.clone();
    opt.unit = Some(selected.to_owned());
    opt.side = selected.split('_').next().unwrap().to_owned();
    opt.heroes = false;

    let composed = battle.world.get("mission").and_then(ContentValue::as_object)
        .ok_or_else(|| error("missing composed mission"))?;
    let mut mission = Object::from_written(composed.entries()
        .filter(|(key, _)| !matches!(*key, "synthesized" | "standIn"))
        .map(|(key, value)| (key.into(), value.clone())).collect());
    mission.set("teams".into(), teams);
    mission.set("era".into(), ContentValue::str(&selection.era));
    for (key, height) in [
        ("maxFlyHeight", setup.max_fly_height),
        ("maxPlayerFlyHeight", setup.max_player_fly_height),
        ("minFlyHeight", setup.min_fly_height),
    ] {
        if let Some(height) = height {
            mission.set(key.into(), ContentValue::Number(height));
        }
    }
    let delays = vec![ContentValue::Number(wave), ContentValue::Number(stagger)];
    mission.set("spawnDelay".into(), ContentValue::Array(Rc::new(delays)));
    let attacker = setup.attacking_team.map_or(ContentValue::Null, |team| ContentValue::Number(team.into()));
    mission.set("attacker".into(), attacker);
    let max_fly_height = mission.get("maxFlyHeight").and_then(ContentValue::as_f64).unwrap_or(200.0);
    let resolved = ContentValue::Object(Rc::new(mission));
    battle.policy = bfe_content::mission::spawn_policy(&battle.query, Some(&resolved));
    if battle.unit != battle.policy.class_name {
        return Err(error("selected mission soldier could not be loaded"));
    }
    let mut world_data = battle
        .world
        .as_object()
        .cloned()
        .ok_or_else(|| error("invalid composed mission world"))?;
    let mission_json = bfe_content::js::json_stringify(Some(&resolved));
    world_data.set("mission".into(), resolved);
    battle.world = bfe_content::value::Value::Object(world_data.into());
    rules.menu_title = Some(selection.menu_title(&battle.common));
    let content = bfe_content::content::Content::build(battle).map_err(error)?;
    let mut resources = MissionResources {
        selection: selection.clone(),
        preferences,
        query,
        max_fly_height,
        admission_digest: String::new(),
        mission_json,
        building_sources: super::rows::props::live_sources(&content.battle),
        definitions: bfe_content::defs::Defs::build(&content).map_err(error)?,
        registry: bfe_content::registry::registry_content(&content),
    };
    let params = ModeParameters {
        command_limit: DEFAULT_SCRIPT_COMMAND_LIMIT,
        timer_limit: DEFAULT_SCRIPT_TIMER_LIMIT,
        score_limit: Some(rules.host().config.effective_assault_score_limit(None)),
        time_limit: None,
    };
    let declared_regions = rules.host().setup.landing_regions.clone();
    let declared_extents = rules.host().setup.world_extents;
    let declared_notify = rules.host().setup.ai_vehicle_notify_radius;
    let telemetry = rules.attach_telemetry();
    fresh
        .prepare_script_rules(&selection.mode, params, rules)
        .map_err(|e| error(format!("{e:?}")))?;
    let result = construct(&mut fresh, content, &tracked, &opt)?;
    if fresh.space().is_some() {
        for name in &declared_regions { fresh.space_add_landing_region(name).ok_or_else(|| error(format!("unknown landing region {name}")))?; }
        if let Some(n) = declared_extents { fresh.space_set_world_extents(n); }
        if let Some(r) = declared_notify { fresh.space_set_pilot_notify_radius(r); }
    }
    resources.admission_digest = tracked.admission_digest(selection);
    if fresh.mission_state().is_none() {
        return Err(error("mission rules were not installed"));
    }
    std::mem::swap(world, &mut fresh);
    Ok((result, telemetry, resources))
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use bfe_content::files::MemFiles;

    #[test]
    #[ignore = "requires prepared 2005 missions"]
    fn prepared_rosters_follow_script_declarations() {
        use crate::world::admit::Bots;
        use bfe_content::files::DirFiles;
        let files = DirFiles::from_resolved_root(std::env::var_os("BFE_SWBF2_PREPARED").unwrap(), bfe_family::GameFamily::Swbf2);
        for (map, era, mode, side, own) in [
            ("tat2", "gcw", "eli", "all", 1), ("tat2", "gcw", "eli", "imp", 2),
            ("spa6", "cw", "ass", "rep", 1), ("spa6", "cw", "ass", "cis", 2),
        ] {
            let selection = MissionSelection {
                family: GameFamily::Swbf2, map: map.into(), era: era.into(), mode: mode.into(), seed: 59,
            };
            let options = Options {
                family: selection.family, mode: selection.mode.clone(), practice_target: None,
                map: map.into(), side: side.into(), seed: selection.seed, unit: None, heroes: false,
                enhanced: false, name: None, bots: Bots::read("classic", "medium", "full"),
            };
            let raw = files.read(&format!("maps/{map}/map.json")).unwrap();
            let source = bfe_content::json::parse_bytes(&raw).unwrap();
            let mut world = World::new();
            let mut rows = Rows::default();
            let mut flight_limits = [None; 3];
            let (_, _, resources) = prepare(&mut world, &files, &options, &selection,
                MissionPreferences { bots_per_team: Some(3), ..Default::default() }, false,
                |w, content, files, opt| {
                    let battle = &content.battle;
                    let mission = battle.world.get("mission").unwrap();
                    flight_limits = ["maxFlyHeight", "maxPlayerFlyHeight", "minFlyHeight"]
                        .map(|key| mission.get(key).and_then(ContentValue::as_f64));
                    let entry = battle.query.get("mission");
                    let prepared = source.get("missions").unwrap().get(entry).unwrap();
                    let packages = prepared.get("sides").unwrap().as_object().unwrap();
                    let sides: Vec<_> = mission.get("sides").unwrap().as_array().unwrap()
                        .iter().map(|s| s.as_str().unwrap()).collect();
                    assert_eq!(sides, packages.keys().collect::<Vec<_>>());
                    for key in ["layers", "sound"] {
                        assert_eq!(mission.get(key).is_some(), prepared.get(key).is_some());
                    }
                    sequence::run_into(w, &content, files, opt, &mut rows)
                }).unwrap();
            let query = bfe_content::mission::Query::parse(&resources.query);
            let setup = &world.mission_state().unwrap().0.host().setup;
            assert_eq!(flight_limits, [setup.max_fly_height, setup.max_player_fly_height, setup.min_fly_height]);
            let mut soldiers = world.sim.store.unit.iter().filter_map(|(_, unit)| match &unit.soldier {
                crate::gen::UnitBody::Soldier(body) => Some(body.max_fly_height),
                _ => None,
            }).peekable();
            assert!(soldiers.peek().is_some());
            assert!(soldiers.all(|height| Some(height) == setup.max_fly_height));
            for (&team, declaration) in &setup.teams {
                let expected: Vec<_> = declaration.classes.iter()
                    .map(|class| class.class_name.as_str()).collect();
                let slots: Vec<_> = world.sim.store.spawning.teams[team as usize].classes.iter()
                    .map(|slot| String::from_utf16(world.sim.strings.units(slot.name)).unwrap()).collect();
                assert_eq!(slots, expected);
                if team == own { assert_eq!(query.get("unit"), expected[0]); }
            }
        }
    }

    #[test]
    fn journal_checks_native_float_bits_and_call_payloads() {
        use super::super::rows::Call;
        let payload = f64::from_bits(0x7ff8_0000_0000_0037);
        let mut expected = Rows::default();
        expected.setup.water_height = payload;
        let call = Call { args: vec![-0.0], heap: vec![payload], scratch: vec![3],
            strings: vec![vec![48]], ..Default::default() };
        expected.calls.push((Stage::Units, vec![(Stage::Units, "f3_init", call)]));
        let mut replay = expected.clone();
        assert!(compare_rows(&expected, &replay).is_ok());
        replay.calls[0].1[0].2.args[0] = 0.0;
        assert!(compare_rows(&expected, &replay).is_err());
        replay.calls[0].1[0].2.args[0] = -0.0;
        replay.calls[0].1[0].2.heap[0] = f64::NAN;
        assert!(compare_rows(&expected, &replay).is_err());
        replay.calls[0].1[0].2.heap[0] = payload;
        replay.calls[0].1[0].2.strings[0].clear();
        assert!(compare_rows(&expected, &replay).is_err());
    }

    #[test]
    fn selected_manifest_shares_verified_bodies_and_rejects_mismatches() {
        let bytes = include_bytes!("../../../tests/lua/12_beacon.luac");
        let hash = format!("{:x}", Sha256::digest(bytes));
        let body_path = format!("scripts/{hash}.luac");
        let path = "maps/valley/scripts/cw-eli.json";
        let selection = MissionSelection {
            family: GameFamily::Swbf2,
            map: "valley".into(),
            era: "cw".into(),
            mode: "eli".into(),
            seed: 3,
        };
        let row = |id: &str| json!({"name":id,"id":id,"archive":"trials.lvl","file":body_path,"length":bytes.len(),"sha256":hash});
        let absent = json!({"name":"Spool","id":"spool","archive":"trials.lvl","error":"absent",
            "file":null,"length":null,"sha256":null});
        let rows = json!([row("bell"), absent, row("spare"), row("bell")]);
        let manifest = json!({"version":1,"family":"swbf2","map":"valley","mode":"eli","era":"cw","entry":"bell",
            "scripts":rows,"chunks":["bell","spare","bell"]});
        let map = json!({"missions":{"bell":{"scriptManifest":path,"scripts":rows}}});
        let mut files = MemFiles::default();
        files.insert(&body_path, bytes.to_vec());
        files.insert(path, serde_json::to_vec(&manifest).unwrap());
        let (chain, order) = selection.read_chain(&files, &map).unwrap();
        assert_eq!(order, ["bell", "spare", "bell"]);
        assert_eq!(chain.chunks.len(), 2);
        assert!(Rc::ptr_eq(&chain.chunks[0].bytes, &chain.chunks[1].bytes));
        for (index, key, bad) in [(0, "archive", Value::Null), (1, "length", json!(0)), (1, "name", json!("Ribbon"))] {
            let mut changed = manifest.clone();
            changed["scripts"][index][key] = bad;
            let mut changed_map = map.clone();
            changed_map["missions"]["bell"]["scripts"] = changed["scripts"].clone();
            files.insert(path, serde_json::to_vec(&changed).unwrap());
            assert!(selection.read_chain(&files, &changed_map).is_err());
        }
        for (key, value) in [
            ("family", json!("swbf1")),
            ("era", json!("gcw")),
            ("map", json!("other")),
            ("mode", json!("ass")),
            ("chunks", json!(["bell", "bell", "spare"])),
        ] {
            let mut changed = manifest.clone();
            changed[key] = value;
            files.insert(path, serde_json::to_vec(&changed).unwrap());
            assert!(selection.read_chain(&files, &map).is_err());
        }
        files.insert(path, serde_json::to_vec(&manifest).unwrap());
        files.insert(&body_path, vec![0u8; bytes.len()]);
        assert!(selection.read_chain(&files, &map).is_err());
    }
}
