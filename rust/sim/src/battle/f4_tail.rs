//! Battlefield and battle rules enter once after items. Canonical Store rows
//! remain native through capture, deaths, queues, AI hooks and human choices.
use bfe_wide::Wide;
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use super::{
    battlefield::Battlefield, human::Command, human_posts::Choices, human_world,
    native_actions::Actions, native_lifecycle::Native, owned_world::Owner,
    rules::RuleSet,
};
use crate::{ gen::*, store::StrId, units::f3};

pub(super) struct State {
    owner: Owner,
    capacity: super::rules::events::BattleCapacity,
    human: super::native_human::Driver,
    shell: Option<Store>,
    commands: Vec<Command>,
    input: Vec<f64>,
    random: u32,
    gameplay: u32,
    effects: [StrId; 3],
}
fn state(battle: &mut BattleOwner) -> &mut State {
    {
        battle.tail
            .as_mut()
            .expect("native battle tail initialized")
    }
}
#[cfg(test)]
pub(crate) fn human_choices(battle: &BattleOwner) -> &Choices {
    &battle.tail.as_ref().expect("native battle tail initialized").human.human.choices
}
pub(crate) fn reset(battle: &mut BattleOwner) {
    // This reset cannot reach the bots: the dropped rules' goal bots wait for
    // the next tail or rules installation to hand their bots back.
    if let Some(bots) = take_goal_bots(battle) {
        battle.parked_goal_bots.push(*bots);
    }
    battle.bot_phase = if battle.parked_goal_bots.is_empty() {
        crate::ai::owned::entry::native_phase_in::<false>
    } else { retire_before_phase };
    battle.soldier_phase = f3::released_step_in;
    battle.tail = None;
    battle.rule_events = None;
    battle.death_tickets = super::rules::DeathTickets::Conquest;
}

/// The goal bots of the installed rules, when they have a goal table.
pub(crate) fn goal_bots(battle: &mut BattleOwner) -> Option<&mut crate::ai::goals::GoalController> {
    match &mut battle.tail.as_mut()?.owner.tail.rules {
        RuleSet::Conquest => None,
        RuleSet::EventDriven(mode) => mode.goal_bots(),
    }
}

pub(crate) fn soldier_inputs(battle: &mut BattleOwner) -> Option<&mut crate::ai::goals::SoldierInputs> {
    let RuleSet::EventDriven(mode) = &mut battle.tail.as_mut()?.owner.tail.rules else { return None };
    mode.soldier_inputs()
}

/// Take the installed rules' goal bots out, keeping every goal handle their
/// table issued out of later installations.
fn take_goal_bots(battle: &mut BattleOwner) -> Option<Box<crate::ai::goals::GoalController>> {
    let tail = battle.tail.as_mut()?;
    let RuleSet::EventDriven(mode) = &mut tail.owner.tail.rules else { return None };
    mode.take_goal_bots()
}

/// Hand back the bots of goal bots whose rules are gone. False while the
/// bots cannot be reached; those goal bots then wait for the next try.
fn retire_parked_in(w: &mut Root) -> bool {
    while let Some(mut bots) = w.battle.parked_goal_bots.pop() {
        if !crate::ai::goals::api::retire_in(w, &mut bots) {
            w.battle.parked_goal_bots.push(bots);
            return false;
        }
    }
    true
}

/// A reset can happen inside a physical callback while the AI is on loan.
fn retire_before_phase(w: &mut Root) -> u32 {
    if !retire_parked_in(w) { return 0; }
    if let Some(ai) = w.ai.battle_mut() { ai.runtime.core.goal_publications(false); }
    w.battle.bot_phase = crate::ai::owned::entry::native_phase_in::<false>;
    (w.battle.bot_phase)(w)
}

/// Install `rules` as an admitted selection would.
#[cfg(test)]
pub(crate) fn install_rules_for_test(w: &mut Root, mut rules: RuleSet) -> Option<()> { try_install(w, &mut rules) }

#[cfg(test)]
pub(crate) fn event_rules_mut_for_test(w: &mut Root) -> Option<&mut super::rules::events::EventDriven> {
    match &mut w.battle.tail.as_mut()?.owner.tail.rules {
        RuleSet::EventDriven(mode) => Some(mode),
        RuleSet::Conquest => None,
    }
}

pub(crate) fn select_rules(w: &mut Root, family: bfe_family::GameFamily, mode: &str,
    parameters: super::rules::ModeParameters, objects: Vec<super::rules::objects::NamedObject>) -> super::rules::objects::RuleResult<()> {
    if w.battle.tail.is_none() { return Err(super::rules::objects::RuleError::NotAvailable); }
    let objects = super::rules::objects::MissionObjects::admitted(w, objects);
    use super::rules::objects::RuleError;
    if w.battle.prepared_mission.is_some() {
        if !w.battle.rule_request.as_ref().is_some_and(|r| r.family == family && r.mode == mode && r.parameters == parameters) {
            return Err(RuleError::InvalidValue);
        }
        let mut staged = w.battle.prepared_mission.take().unwrap();
        if let RuleSet::EventDriven(mode) = staged.as_mut() {
            mode.admit_objects(objects, parameters.score_limit);
        }
        if try_install(w, &mut staged).is_none() {
            w.battle.prepared_mission = Some(staged);
            return Err(RuleError::LimitReached);
        }
        w.battle.rule_request = None;
        return Ok(());
    }
    let mut rules = RuleSet::select(family, mode, parameters, w.battle.rule_factory.as_deref(), objects)?;
    if matches!(&rules, RuleSet::EventDriven(r) if r.mission().is_some()) { return Err(RuleError::InvalidValue); }
    try_install(w, &mut rules).ok_or(RuleError::LimitReached)
}

impl Root {
    pub(crate) fn observe_local_mission_transitions(&mut self) {
        if let Some(tail) = &mut self.battle.tail {
            if let RuleSet::EventDriven(mode) = &mut tail.owner.tail.rules {
                mode.observe_transitions();
            }
        }
    }
}

/// Whether a battle's rules leave room for one more alive of a class of the
/// human roster: always in a battle without a mission.
pub(crate) fn wire_class_room_in(battle: &BattleOwner, store: &Store, team: usize, class: StrId) -> bool {
    event_rules_in(battle).is_none_or(|rules| rules.has_room(store, team, class))
}
pub(crate) fn event_rules_in(battle: &BattleOwner) -> Option<&super::rules::events::EventDriven> {
    match &battle.tail.as_ref()?.owner.tail.rules {
        RuleSet::EventDriven(mode) => Some(mode),
        RuleSet::Conquest => None,
    }
}

impl Root {
    pub fn event_rules(&self) -> Option<&super::rules::events::EventDriven> { event_rules_in(&self.battle) }

    /// Version-local checksum of the quiescent mission authority and its native inputs.
    /// Physics and combat publications have their own replay digest.
    pub fn mission_digest(&self) -> Option<Wide> {
        let rules = self.event_rules()?;
        let mut h = super::script::state::MissionDigest::new();
        h.field(&rules.mission()?.0.authoritative_digest());
        rules.digest_into(&mut h);
        self.battle.rule_events.as_ref()?.digest_into(&mut h);
        crate::units::f3_hulls::digest_mission(&self.units,&mut h);
        crate::units::f3_props::digest_mission(self, &mut h);
        let s = &self.sim.store;
        h.field(&(&s.spawning.wave,&s.spawning.wave_countdown,&s.battle.reinforcements,s.battle.over,s.battle.loser));
        h.field(&s.spawning.teams);
        for row in rules.named_objects().iter() {
            h.field(row);
            let life=super::native_actions::current(self,s,&row.object);
            h.field(&life);
            if let Ok(object)=life {
                h.field(&super::native_actions::object_state(self,s,&object));
                h.field(&super::native_actions::shields(self,s,&object));
            }
        }
        for r in &s.spawning.characters {
            if let Some(c)=s.character.get(*r) {
                h.field(&(r,c.unit,c.team,c.human,c.hero,c.kills,c.class_name,&c.spawn));
                if let Some(body) = c.unit.and_then(|unit| s.unit.get(unit)) {
                    h.field(&(body.character, body.class_name, body.alive, body.combat.dead,
                        body.combat.health, body.combat.max_health));
                }
            }
        }
        Some(h.finish())
    }

    pub fn mission_state(&self) -> Option<(&super::script::ScriptRules, &[super::rules::awards::PersonalScore])> {
        self.event_rules()?.mission()
    }

    pub fn mission_view(&self) -> Option<super::script::MissionView<'_>> {
        let rules = self.event_rules()?;
        Some(super::script::MissionView { host: rules.mission()?.0.host(), rules,
            battle: &self.sim.store.battle, human_spawn: &self.sim.store.human_spawn })
    }

}

#[cfg(test)]
fn set_rules(w: &mut Root, mut rules: RuleSet) -> Option<()> { try_install(w, &mut rules) }

fn try_install(w: &mut Root, rules: &mut RuleSet) -> Option<()> {
    let capacity = w.battle.tail.as_ref()?.capacity;
    // Everything that can fail comes first: a failed installation leaves the
    // installed rules and their goal bots as they are.
    let producer = match &mut *rules {
        RuleSet::Conquest => None,
        RuleSet::EventDriven(mode) => {
            if w.ai.battle_on_loan() || !mode.accepts_goal_handles(&w.goal_handles()) { return None; }
            let producer = mode.prepare(capacity)?;
            let epoch = w.battle.goal_installations.checked_add(1)?;
            if let Some(bots) = mode.goal_bots() {
                let (characters, regions) = crate::ai::goals::api::admission_in(w)?;
                bots.admit(characters, regions, epoch)?;
            } else {
                let characters = w.ai.battle().map_or(0, |ai| ai.character_capacity())
                    .max(w.sim.store.character.slots() as usize);
                mode.prepare_unassigned_inputs(characters, epoch)?;
            }
            let targets = w.ai.battle().map_or(0, |ai| ai.state.identity_capacity());
            mode.soldier_inputs()?.reserve_targets(targets)?;
            Some(producer)
        }
    };
    // The replaced rules' goal bots hand their bots back before the new rules
    // take over; while that cannot happen, the installation is refused.
    if !retire_parked_in(w) {
        return None;
    }
    if let Some(mut bots) = take_goal_bots(&mut w.battle) {
        if !crate::ai::goals::api::retire_in(w, &mut bots) {
            if let Some(RuleSet::EventDriven(mode)) = w.battle.tail.as_mut().map(|t| &mut t.owner.tail.rules) {
                mode.restore_goal_bots(bots);
            }
            return None;
        }
    }
    if matches!(&*rules, RuleSet::EventDriven(_)) {
        w.battle.goal_installations += 1;
        w.sim.store.battle.reinforcements.reserve(super::spawning::TEAM_COUNT);
    }
    if producer.as_ref().is_some_and(|p| p.mission_events()) { crate::units::f3_hulls::prepare_mission(&mut w.units); }
    let battle = &mut w.battle;
    let tail = battle.tail.as_mut()?;
    battle.rule_events = producer;
    battle.death_tickets = match rules {
        RuleSet::Conquest => super::rules::DeathTickets::Conquest,
        RuleSet::EventDriven(_) => super::rules::DeathTickets::Handler,
    };
    if let Some(ai) = w.ai.battle_mut() {
        ai.runtime.core.goal_publications(matches!(&*rules, RuleSet::EventDriven(mode) if mode.has_goal_bots()));
    }
    battle.soldier_phase = if battle.melee_driver.is_some() && matches!(&*rules, RuleSet::EventDriven(_)) {
        f3::goal_step_in
    } else { f3::released_step_in };
    battle.bot_phase = match &mut *rules {
        RuleSet::EventDriven(_) => crate::ai::owned::entry::native_phase_in::<true>,
        _ => crate::ai::owned::entry::native_phase_in::<false>,
    };
    tail.owner.tail.rules = std::mem::replace(rules, RuleSet::Conquest);
    Some(())
}

/// captureWorldSummaries reads the same prepared human roster without
/// running a spawn choice, advancing a queue or consuming a random draw.
pub(crate) fn wire_roster_in(battle: &BattleOwner, team: usize) -> &[StrId] {
    battle.tail.as_ref().expect("prepared battle tail").human.human.prepared.rosters.get(team).map_or(&[], Vec::as_slice)
}
/// The same roster, or none before the battle's tail is prepared.
pub(crate) fn human_roster_in(battle: &BattleOwner, team: usize) -> &[StrId] {
    if battle.tail.is_none() {
        return &[];
    }
    wire_roster_in(battle, team)
}
// The pre-AI boundary also replaces CommandPosts.all's former JS refresh.
// State is lent out inside a tail; its World refreshes before its own hooks.
pub(crate) fn refresh_mobile_views_in(w: &mut Root){
    let Some(state)=({w.battle.tail.as_mut()}) else{return};
    super::fleet_posts::refresh_posts(&mut w.sim.store,&mut state.owner.prepared.pads);
    super::f4_core::mark_mobile(&mut w.battle);
}
/// Cold native roster construction follows tail content preparation. Bind
/// its new human before any lifecycle or human-spawn phase can observe it.
pub(crate) fn initial_human_in(battle: &mut BattleOwner, character:Option<crate::store::Ref<Character>>) {
    let state=state(battle);state.owner.prepared.local_character=character;
    state.human.human.prepared.character=character;
}
use super::stats::WorldText as Text;
pub(crate) struct Random(pub u32);
impl<W: super::spawning::RandomWorld + ?Sized> super::spawning::ContextDraws<W> for Random {
    fn next_in(&mut self, world: &mut W) -> f64 { world.random_next(self.0) }
}
pub fn f4_tail_init_in(w: &mut Root) -> Option<&[f64]> {
    assert!(
        f3::active_in(&w.units) && super::f4_core::active_in(&mut w.battle),
        "canonical unit and battle owners required"
    );
    // A new tail replaces the installed rules: their goal bots, and any whose
    // rules a reset dropped, hand their bots back first.
    if let Some(bots) = take_goal_bots(&mut w.battle) {
        w.battle.parked_goal_bots.push(*bots);
    }
    w.battle.soldier_phase = f3::released_step_in;
    w.battle.bot_phase = if retire_parked_in(w) {
        if let Some(ai) = w.ai.battle_mut() { ai.runtime.core.goal_publications(false); }
        crate::ai::owned::entry::native_phase_in::<false>
    } else {
        retire_before_phase
    };
    let (row, heap) = w.sim.calls.args();
    let cfg = F4TailSetup::take(row, heap);
    let edges: Vec<_> = cfg.boundary.polygons.into_iter().map(|p| p.edges).collect();
    let mut owner = Owner::new(
        &mut w.sim.store,
        Battlefield {
            boundary: crate::ai::nav::boundary::Boundary::from_edges(&edges),
        },
        cfg.local,
        cfg.classic,
        cfg.registry_capacity as usize,
        cfg.victim_capacity as usize,
    );
    owner.prepared.enhanced = cfg.enhanced;
    owner.prepared.local_character = cfg.character;
    owner.tail.reserve_spawn_points(cfg.point_capacity as usize);
    let posts = w.sim.store.command_posts.posts.len() + w.sim.store.mobile_posts.posts.len();
    let choices = Choices::new(
        posts,
        cfg.victim_capacity as usize,
        cfg.point_capacity as usize,
    );
    let human = human_world::Human::new(
        human_world::Prepared {
            character: cfg.character,
            classic: cfg.classic,
            rosters: cfg.rosters.into_iter().map(|r| r.classes).collect(),
            classes: cfg.classes,
        },
        choices,
        cfg.command_capacity as usize,
        posts,
        cfg.point_capacity as usize,
    );
    let effects = [
        w.sim.strings.intern_str("explosion"),
        w.sim.strings.intern_str("med_explosion"),
        w.sim.strings.intern_str("small_explosion"),
    ];
    // The historical initialization call includes the default Conquest selection.
    w.battle.rule_events = None;
    w.battle.death_tickets = super::rules::DeathTickets::Conquest;
    super::f4_core::configure_tail(&mut w.battle, cfg.classic, cfg.local);
    {
        w.battle.tail = Some(State {
            owner,
            capacity: super::rules::events::BattleCapacity {
                registry: cfg.registry_capacity as usize,
                victims: cfg.victim_capacity as usize,
                buildings: super::f4::live_ref(&w.battle).buildings.len() + super::f4::live_ref(&w.battle).traps.len(),
                posts,
            },
            human: super::native_human::Driver {
                human,
                classic: cfg.classic,
                random: cfg.random,
                gameplay: cfg.gameplay,
                effects,
                spawn_index: cfg.spawn_index,
                fallback_path: cfg.fallback_path,
            },
            shell: Some(Store::default()),
            commands: Vec::with_capacity(cfg.command_capacity as usize),
            input: vec![0.0; cfg.command_capacity as usize * HsCommand::WORDS],
            random: cfg.random,
            gameplay: cfg.gameplay,
            effects,
        });
    }
    super::f4_live::descriptor_words(w)
}
#[cfg(test)]
/// HumanSpawn's commands share the tail owner's retained bank. Native hosts
/// publish a count and no address; Chromium keeps its existing typed view.
pub fn f4_tail_write_commands_in(w: &mut Root, commands: &[HsCommand]) -> bool {
    let Some(tail) = w.battle.tail.as_mut() else { return false };
    if commands.len() > tail.commands.capacity() || commands.len() > tail.input.len() / HsCommand::WORDS { return false; }
    // Check both owners before changing the command bank or its header.
    if w.battle.io.is_none() { return false; }
    for (row, command) in tail.input.chunks_exact_mut(HsCommand::WORDS).zip(commands) {
        command.put(row, &mut Vec::new());
    }
    let address = 0;
    let input = super::f4_live::tail_input_bank_mut_in(w).expect("checked tail input");
    input[F4TailInput::HUMAN_COMMANDS] = address as f64;
    input[F4TailInput::HUMAN_COUNT] = commands.len() as f64;
    true
}
pub fn f4_tail_commands_in(w: &mut Root) -> Option<&mut [f64]> {
    w.battle.tail.as_mut().map(|tail| &mut tail.input[..])
}
pub fn f4_tail_unit_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let c = F4UnitClass::take(row, heap);
    let p = &mut state(&mut w.battle).owner.prepared;
    p.registry.class(c.def, c.captures);
    p.observations.class(F4StatsClass {
        def: c.def,
        label: c.label,
    });
}
pub fn f4_tail_weapon_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let weapon = F4WeaponClass::take(row, heap);
    state(&mut w.battle).owner.prepared.observations.weapon(F4StatsWeapon {
        def: weapon.def,
        primary: weapon.primary,
        key: weapon.key,
        labels: weapon.labels,
    });
}
pub fn f4_tail_hull_class_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let h = F4HullClass::take(row, heap);
    state(&mut w.battle).owner.prepared.observations.hull(F4StatsHull {
        hull: h.hull,
        label: h.label,
    });
    // A landed transport is offered as the post named after its pad,
    // transport-<pad>, the name the entity registry and the spawn screen use
    // (flight/spawn.js transportPosts), not after the hull's class.
    let slot = w.sim.store.vehicle.get(h.hull).expect("prepared human hull").slot_index;
    let name = w.sim.strings.intern_str(&format!("transport-{}", jsrt::numfmt::number_to_string(slot)));
    state(&mut w.battle).human.human.choices.hull(h.hull, name);
}
pub fn f4_tail_post_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let p = F4PostSetup::take(row, heap);
    state(&mut w.battle)
        .human
        .human
        .choices
        .raw_post(p.post, p.raw_path);
}
pub fn f4_tail_pad_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let p = F4PadSetup::take(row, heap);
    let prepared = &mut state(&mut w.battle).owner.prepared;
    prepared.pads.adopt(p.index as usize, p.pad);
    for &r in &w.sim.store.mobile_posts.posts {
        let post = w.sim.store.mobile_post.get_mut(r).unwrap();
        if post.index == p.index as f64 {
            prepared.pads.reserve_post(p.index as usize, post);
        }
    }
}
pub fn f4_tail_hero_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let h = F4HeroSetup::take(row, heap);
    let names = &mut state(&mut w.battle).owner.prepared.hero_names;
    names.resize(names.len().max(h.class_name.0 as usize + 1), None);
    names[h.class_name.0 as usize] = h.name;
    super::f4_core::prepare_hero(&mut w.battle, h.class_name,h.name);
}
pub fn f4_tail_capture_team_in(w: &mut Root) {
    let (row, heap) = w.sim.calls.args();
    let c = F4CaptureTeam::take(row, heap);
    let table = &mut state(&mut w.battle).owner.prepared.captures;
    table.resize(table.len().max(c.team as usize + 1), None);
    table[c.team as usize] = Some(c.value);
}
pub fn f4_tail_ready_in(w: &mut Root) -> Option<&[f64]> {
    super::f4_core::prepare_tail_publications(w);
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
    f3::publish_changed_in(w);
    super::f4_live::descriptor_words(w)
}
pub fn f4_tail_in(w: &mut Root) {
    let input = super::f4_live::tail_input(&mut w.battle);
    if w.battle.rule_events.as_ref().is_some_and(|q| q.mission_events()) {
        super::rules::hero_health::advance(w,input.dt);
    }
    let mut state = {
        w.battle.tail
            .take()
            .expect("battle tail cannot reenter")
    };
    state.decode_commands(&input, &w.sim.strings);
    let outer = f3::external_in(w);
    f3::with_services_in(w, |w, services| {
        let mut services = services.expect("native battle body services");
        let mut shell = state.shell.take().unwrap();
        std::mem::swap(&mut shell, &mut w.sim.store);
        let mut lifecycle = Native {
            local: state.owner.local,
            // A server can reuse the dormant local Character's roster slot
            // for a network player. That player belongs to his connection's
            // body, even when the retained Character reference is identical.
            character: if input.server { None } else { state.owner.prepared.local_character },
        };
        let mut actions = Actions { w,
            services:services.reborrow(),
            lifecycle: &mut lifecycle,
            human: &mut state.human,
            local: state.owner.local,
            effects: state.effects,
        };
        state.owner.step(
            &input,
            &mut shell,
            &Text,
            &state.commands,
            &mut actions,
            &mut Random(state.random),
            &mut Random(state.gameplay),
        );
        super::f4_core::mark_tail_in(&mut w.battle, &shell);
        std::mem::swap(&mut shell, &mut w.sim.store);
        state.shell = Some(shell);
    });
    {
        w.battle.tail = Some(state);
    }
    // Let damage handlers see a depleted shield before its next recharge step.
    if w.space.is_some() || w.battle.rule_events.as_ref().is_some_and(|q| q.mission_events()) {
        crate::units::f3_props::advance_mission_shields(&mut w.units, input.dt);
    }
    outer.finish(w, true);
    super::f4_live::count_tail(w, input.human_count as usize);
}

/// Roster commands run outside the phase and borrow its prepared post/class
/// readers. Mutating services still install the canonical Store at AI hooks.
pub(crate) fn spawning<T>(w: &mut Root,
    s: &mut Store,
    mut services: crate::units::f3::WorldServices<'_>,
    run: impl FnOnce(
        &mut Store,
        &mut (dyn SpawnContext + '_),
        &mut dyn for<'w> super::spawning::ContextDraws<dyn SpawnContext + 'w>,
    ) -> T,
) -> T {
    let mut state = w.battle.tail.take().expect("idle battle tail");
    let local = state.owner.local;
    let mut lifecycle = Native {
        local,
        character: state.owner.prepared.local_character,
    };
    let mut actions = Actions { w,
        services:services.reborrow(),
        lifecycle: &mut lifecycle,
        human: &mut state.human,
        local,
        effects: state.effects,
    };
    let mut world = super::owned_world::World {
        prepared: &mut state.owner.prepared,
        services: &mut actions,
        local,
        classic: state.owner.classic,
    };
    let result = run(s, &mut world, &mut Random(state.random));
    w.battle.tail = Some(state);
    result
}

/// Game.onRespawn is a stateful combat callback, before the live/tail slots.
/// It cannot be delayed with visual body notices to the end of a phase.
pub(crate) fn owns_player_in(w: &mut Root, unit:crate::store::Ref<Unit>)->bool {
    super::f4_core::owns_lifecycle_in(&mut w.battle) && state(&mut w.battle).owner.local==Some(unit)
}
pub(crate) fn auto_respawn_in(w: &mut Root, services:f3::WorldServices<'_>) {
    let mut state = w.battle.tail.take().expect("idle battle tail");
    let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let mut shell = state.shell.take().expect("retained tail shell");
    std::mem::swap(&mut shell, &mut w.sim.store);
    state.human.auto_respawn(w,&mut shell,&mut state.owner.prepared,services,&mut lifecycle,state.owner.local);
    std::mem::swap(&mut shell, &mut w.sim.store);
    state.shell = Some(shell);
    w.battle.tail = Some(state);
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);
}

/// MultiplayerWorld.spawn uses the same prepared post facts as local human
/// choices. Network spawn points use the connection ordinal's fixed uniform;
/// choosing a fallback post uses one half, never a gameplay RNG draw.
pub(crate) fn connection_spawn(w: &mut Root, character:crate::store::Ref<Character>, previous:Option<crate::store::Ref<Unit>>,
    class:StrId, post:Option<StrId>,ordinal:f64,mut services:f3::WorldServices<'_>)->Option<(crate::store::Ref<Unit>,StrId)> {
    let team=w.sim.store.character.get(character)?.team;
    let mut state=w.battle.tail.take().expect("idle battle tail");
    let result=(|| {
    if !state.human.human.prepared.rosters.get(team as usize).is_some_and(|r|r.contains(&class)) {return None;}
    let mut lifecycle=Native{local:None,character:None};let mut human=super::native_human::NoHuman;
    let mut actions=Actions{w,services:services.reborrow(),lifecycle:&mut lifecycle,human:&mut human,local:None,effects:state.effects};
    let mut world=super::owned_world::World{prepared:&mut state.owner.prepared,services:&mut actions,local:None,classic:state.owner.classic};
    let choices=&mut state.human.human.choices;
    let mut shell=state.shell.take().expect("retained tail shell");
    std::mem::swap(&mut shell,&mut world.services.w.sim.store);
    let choice=(|| {
    // Landed transports use the original ground-normal admission test.
    choices.begin_with(&mut shell,&mut world,|world,at,body| {
        let w = &mut *world.services.w;
        let world_id = f3::collision_world_in(&w.units);
        crate::units::native::ordnance_cast_in(w, world_id,[at.x,at.y,at.z],[0.,-1.,0.],12.,Some(body),true,false)
            .map(|hit|(three::Vector3::new(hit.point[0],hit.point[1],hit.point[2]),three::Vector3::new(hit.normal[0],hit.normal[1],hit.normal[2])))
    });
    let index=choices.choose(team,post,|a,b|f3::relation_in(&world.services.w.units,a,b),||0.5)?;
    let point=choices.point_with(&shell,index,team,&mut world,|world,a,b|f3::relation_in(&world.services.w.units,a,b),||(ordinal*0.381966)%1.0)?;
    let name=choices.name(index);
    Some((point, name))
    })();
    std::mem::swap(&mut shell,&mut world.services.w.sim.store);
    state.shell=Some(shell);
    let (point,name)=choice?;
    let w=&mut *world.services.w;
    let was_alive=previous.and_then(|r|w.sim.store.unit.get(r)).is_some_and(|u|u.alive);
    let unit=super::f4_factory::player(w,&F4PlayerRequest{character:Some(character),body:previous,class_name:class,
        post:Some(name),position:[point[0],point[1],point[2]],yaw:point[3],local:false},world.services.services.reborrow())?;
    let c=w.sim.store.character.get_mut(character).unwrap();c.unit=Some(unit);c.class_name=Some(class);
    if let Some(spawn)=&mut c.spawn {spawn.ready=false;spawn.timer=f64::INFINITY;}
    if !was_alive {if let Some(team)=w.sim.store.spawning.teams.iter_mut().find(|row|row.index==team) {team.living+=1.0;}}
    super::f4_core::mark_character_in(&mut w.battle, character);
    let u=w.sim.store.unit.get_mut(unit).unwrap();u.combat.player=true;
    let p=u.position();
    F4SpawnNotice{key:F3UnitKey::of(unit,u),unit,character:Some(character),class_name:class,
        post:Some(name),team:u.team,position:[p.x,p.y,p.z],local:false}.emit(&mut w.sim.events);
    Some((unit,name))
    })();
    w.battle.tail=Some(state);
    result
}

/// StubAI's body request is an explicit command outside the owned AI span.
/// Its native lifecycle shares waves' credit, body and spawn-notice ordering.
pub fn f4_unit_spawn_in(w: &mut Root)->i32 {
    use super::native_actions::Lifecycle;
    let (row,heap)=w.sim.calls.args();let input=F4UnitSpawn::take(row,heap);
    let Some(character)=super::f4_core::resolve(&mut w.battle, &w.sim.store,&input.target) else{return -1};
    if input.post.life!=1. {return -1;}
    let post=match input.post.kind {
        F4ObjectKind::Post=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !w.sim.store.post.contains(r){return -1};SpawnPost::Post(r)},
        F4ObjectKind::MobilePost=>{let r=crate::store::Ref::new(input.post.slot,input.post.generation);if !w.sim.store.mobile_post.contains(r){return -1};SpawnPost::MobilePost(r)},
        _=>return -1,
    };
    super::f4_live::count_external(&mut w.battle, F4UnitSpawn::WORDS);
    let outer=f3::external_in(w);
    let state=state(&mut w.battle);let mut lifecycle=Native{local:state.owner.local,character:state.owner.prepared.local_character};
    let unit=f3::with_services_in(w, |w, services|lifecycle.spawn(w,character,Some(input.class_name),post,input.position,Some(input.yaw),services.expect("native spawn body services")));
    super::f4_core::mark_tail_in(&mut w.battle, &w.sim.store);outer.finish(w, true);
    unit.map_or(-1,|r|r.slot as i32)
}

impl State {
    fn decode_commands(&mut self, input: &F4TailInput, strings: &crate::store::Strings) {
        // Clamp at the owner as well as the JS writer. A stale or oversized
        // count must never index the adjacent phase input or grow the command list.
        let count = (input.human_count as usize).min(self.commands.capacity()).min(self.input.len() / HsCommand::WORDS);
        self.commands.clear();
        for i in 0..count {
            let c = HsCommand::take(&self.input[i * HsCommand::WORDS..], &[]);
            let command = match c.kind {
                HsCommandKind::Team => Command::team(c.team),
                HsCommandKind::Spawn => c
                    .class_name
                    .and_then(|class| Command::spawn(class, c.post, strings)),
                HsCommandKind::Suicide => Some(Command::Suicide),
                HsCommandKind::Cancel => Some(Command::Cancel),
            };
            if let Some(command) = command {
                self.commands.push(command);
            }
        }
    }
}


pub(crate) trait SpawnContext: super::world::SpawningWorld + super::stats::TextWorld + super::spawning::RandomWorld {}
impl<T:super::world::SpawningWorld + super::stats::TextWorld + super::spawning::RandomWorld> SpawnContext for T {}

#[cfg(test)]
mod native_commands {
    use super::*;
    #[test]
    fn tail_commands_are_owned_bounded_and_addressless() {
        let mut w = Root::new();
        assert!(!f4_tail_write_commands_in(&mut w, &[]));
        f3::f3_init_in(&mut w, 0, f64::NAN, 0);
        w.sim.calls.result(F4BattleSetup::WORDS, |row, heap| F4BattleSetup::default().put(row, heap));
        super::super::f4_core::f4_battle_init_in(&mut w);
        w.sim.calls.result(F4LiveSetup::WORDS, |row, heap| F4LiveSetup::default().put(row, heap));
        super::super::f4_live::f4_live_init_in(&mut w);
        w.sim.calls.result(F4TailSetup::WORDS, |row, heap| F4TailSetup { command_capacity: 2, ..Default::default() }.put(row, heap));
        f4_tail_init_in(&mut w);
        let commands = [HsCommand { kind: HsCommandKind::Suicide, ..Default::default() }, HsCommand { kind: HsCommandKind::Cancel, ..Default::default() }];
        assert!(f4_tail_write_commands_in(&mut w, &commands));
        let input = super::super::f4_live::tail_input(&mut w.battle);
        assert_eq!((input.human_count, input.human_commands), (2, 0));
        let saved = f4_tail_commands_in(&mut w).unwrap().to_vec();
        assert!(!f4_tail_write_commands_in(&mut w, &[commands[0].clone(), commands[1].clone(), commands[0].clone()]));
        assert_eq!(f4_tail_commands_in(&mut w).unwrap(), saved);
        state(&mut w.battle).decode_commands(&input, &w.sim.strings);
        assert_eq!(state(&mut w.battle).commands.len(), 2);
        assert!(f4_tail_write_commands_in(&mut w, &[]));
        assert_eq!(super::super::f4_live::tail_input(&mut w.battle).human_count, 0);
    }
}

#[cfg(test)]
pub(super) mod rule_tests;
