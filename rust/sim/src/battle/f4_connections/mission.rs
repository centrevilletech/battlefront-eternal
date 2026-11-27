//! A scripted battle's connections choose a class and a post. The script's
//! population and spawn rules decide where and when their Characters receive
//! a body: the local player's, and every player's of a server's battle.
use super::{wire_character_in, Entry};
use crate::{
    battle::{f4_core, spawning},
    gen::*,
    store::{Ref, StrId},
    world::owner::World,
};

/// Hands `player`'s deployments to the mission. He must be admitted and not
/// deployed; the body he was admitted with stays his connection's identity
/// until the mission gives him one. He takes the spawn wave of the place he
/// was given, which a bot may already be waiting in.
pub(crate) fn bind(w: &mut World, player: u32) -> Result<Ref<Character>, &'static str> {
    let handle = w
        .input
        .players
        .pool
        .get(player as usize)
        .and_then(|p| p.character.as_ref())
        .ok_or("missing mission player")?;
    let character = wire_character_in(&w.battle, &w.sim.store, handle)
        .ok_or("missing mission connection")?;
    let retained = super::wire_view_in(&w.battle, &w.sim.store, handle)
        .and_then(|view| view.unit)
        .ok_or("missing mission connection body")?;
    if w.sim.store.unit.get(retained).is_none_or(|u| u.alive) {
        return Err("mission spawn selection requires an undeployed player");
    }
    if !w.sim.store.spawning.characters.contains(&character) {
        return Err("the mission has no place for the player");
    }
    let c = w
        .sim
        .store
        .character
        .get_mut(character)
        .ok_or("missing mission character")?;
    let spawn = c.spawn.as_mut().ok_or("missing mission spawn rule")?;
    // Admission keeps a dead body in the connection for input identity.
    // The population counts a Character with a body as deployed.
    if c.unit == Some(retained) {
        c.unit = None;
    }
    spawn.ready = false;
    // The connection roster parks a reserved slot until a player chooses it.
    // The mission owns the personal countdown after an actual death.
    if spawn.timer == f64::INFINITY {
        spawn.timer = 0.;
    }
    let connections = w.battle.connections.as_mut().unwrap();
    connections.rows[character.slot as usize].as_mut().ok_or("missing mission connection")?.mission = Some(player);
    Ok(character)
}

impl World {
    /// Hands the local player's deployments to the mission.
    pub fn bind_mission_deployment(&mut self, player: u32) -> Result<(), &'static str> {
        let character = bind(self, player)?;
        self.battle.connections.as_mut().unwrap().local_mission = Some(character);
        Ok(())
    }

    /// The personal spawn clock of the character that owns local mission
    /// input: whether it has a body and the seconds it still waits.
    pub fn local_mission_spawn_clock(&self) -> Option<(bool, f64)> {
        let character = self.battle.connections.as_ref()?.local_mission?;
        let c = self.sim.store.character.get(character)?;
        Some((c.unit.is_some(), c.spawn.as_ref()?.timer))
    }

    /// Human places that wait for a player to take them: no body, and a clock
    /// that never runs.
    pub fn parked_mission_places(&self) -> usize {
        self.sim
            .store
            .battle
            .characters
            .iter()
            .filter_map(|r| self.sim.store.character.get(*r))
            .filter(|c| c.human && c.unit.is_none() && c.spawn.as_ref().is_some_and(|s| s.timer == f64::INFINITY))
            .count()
    }

    /// Whether a human could take the named class of team 1 or 2 right now:
    /// the team lists it and the members alive in it are under its limit.
    /// None when the mission has no spawn waves or the team has no such class.
    pub fn mission_class_has_room(&self, team: usize, class: &str) -> Option<bool> {
        let rules = self.event_rules()?;
        rules.waves.as_ref()?;
        let store = &self.sim.store;
        let name = store
            .spawning
            .teams
            .get(team)?
            .classes
            .iter()
            .map(|k| k.name)
            .find(|k| self.sim.strings.units(*k).iter().copied().eq(class.encode_utf16()))?;
        Some(rules.has_room(store, team, name))
    }

    /// Spawn state of the human who owns local mission input.
    pub fn local_mission_connection(&self) -> Option<&F4ConnectionView> {
        let connections = self.battle.connections.as_ref()?;
        connections.view(connections.local_mission?)
    }

    pub fn mission_deployment_choice(&self) -> Option<HsCommand> {
        let character = self.battle.connections.as_ref()?.local_mission?;
        let c = self.sim.store.character.get(character)?;
        let view = self.local_mission_connection()?;
        if self.sim.store.battle.over || view.ended || !spawning::awaiting_human(c) {
            return None;
        }
        Some(HsCommand {
            kind: HsCommandKind::Spawn,
            team: c.team,
            class_name: view.class_name,
            post: None,
        })
    }
}

/// A player who changes teams chooses again there.
pub(super) fn withdraw(w: &mut World, character: Ref<Character>) {
    let spawn = w.sim.store.character.get_mut(character).filter(|c| c.unit.is_none()).and_then(|c| c.spawn.as_mut());
    if let Some(spawn) = spawn { spawn.ready = false; }
}

/// A player's choice, refused while he cannot make one and where the mission
/// has no room for another of the class. Until the mission has given him a
/// body he may choose again. Naming no class chooses the one he last was.
pub(super) fn request(
    w: &mut World,
    entry: &mut Entry,
    character: Ref<Character>,
    class: Option<StrId>,
    post: Option<StrId>,
) -> bool {
    let Some(class) = class.or(entry.view.class_name) else {
        return false;
    };
    let team = w.sim.store.character.get(character).map(|c| c.team);
    // A class the 2005 game would not give a human is refused, so the player
    // stays on the screen (no prepared class is: see `Waves::has_room`).
    let full = match (team, w.event_rules().and_then(|rules| rules.waves.as_ref())) {
        (Some(team), Some(waves)) if (0.0..8.0).contains(&team) => {
            !waves.has_room(&w.sim.store, team as usize, class)
        }
        _ => false,
    };
    let store = &mut w.sim.store;
    let Some(c) = store.character.get(character) else {
        return false;
    };
    let roster = store.spawning.teams.iter().find(|t| t.index == c.team);
    if full
        || store.battle.over
        || !spawning::awaiting_human(c)
        || !roster.is_some_and(|t| t.classes.iter().any(|r| r.name == class))
    {
        return false;
    }
    let post = match post {
        None => None,
        Some(name) => {
            let found = store
                .post
                .iter()
                .find(|(_, p)| p.name == name)
                .map(|(r, _)| SpawnPost::Post(r))
                .or_else(|| {
                    store
                        .mobile_post
                        .iter()
                        .find(|(_, p)| p.name == name)
                        .map(|(r, _)| SpawnPost::MobilePost(r))
                });
            let Some(found) = found else { return false };
            Some(Some(found))
        }
    };
    spawning::on_human_ready_named(
        store,
        character,
        spawning::HumanChoice {
            class_name: Some(class),
            post,
            ..Default::default()
        },
        &w.sim.strings,
        |class| f4_core::mission_hero_name(&w.battle, class),
    );
    entry.view.class_name = Some(class);
    true
}

pub(crate) fn spawned(
    w: &mut World,
    character: Ref<Character>,
    unit: Ref<Unit>,
    class: StrId,
    post: StrId,
) {
    let connections = w.battle.connections.as_mut();
    let Some(entry) = connections.and_then(|s| s.rows.get_mut(character.slot as usize)?.as_mut()) else {
        return;
    };
    let Some(player) = entry.mission else {
        return;
    };
    entry.view.unit = Some(unit);
    entry.view.class_name = Some(class);
    entry.view.post = Some(post);
    entry.view.phase = HumanPhase::Alive;
    entry.view.dead_time = 0.;
    entry.view.spawned_once = true;
    if let Some(player) = w.input.players.pool.get_mut(player as usize) {
        player.changed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::{
        f4_core, f4_live, f4_tail,
        rules::{
            events::{EventDriven, EventQueue, EventRules, RuleActions},
            RuleSet,
        },
        script::{state::UnitClassSetup, TeamSetup},
        spawning::{make_teams, mission::Waves},
    };
    use crate::world::owner::World as Root;

    struct NoopRules;
    impl EventRules for NoopRules {
        fn tick(&mut self, _dt: f64, _events: &EventQueue, _actions: &mut dyn RuleActions) {}
    }

    #[test]
    fn a_human_takes_any_listed_class_whatever_the_bots_hold() {
        let mut w = Root::new();
        let positions = w.collision.positions_new(0, 0);
        let tree = w.collision.tree_new(positions, 0, 0);
        let collision = w.collision.world_new(tree, -1, -1);
        w.fleet.collision = Some(crate::vehicles::native::Collision::new(collision as usize, None));
        crate::units::f3::f3_init_in(&mut w, collision, f64::NAN, 0);
        w.sim.calls.result(F4LiveSetup::WORDS, |r, h| F4LiveSetup::default().put(r, h));
        f4_live::f4_live_init_in(&mut w);
        w.sim.calls.result(F4BattleSetup::WORDS, |r, h| F4BattleSetup::default().put(r, h));
        f4_core::f4_battle_init_in(&mut w);
        w.sim.calls.result(F4TailSetup::WORDS, |r, h| F4TailSetup { registry_capacity: 8, ..Default::default() }.put(r, h));
        f4_tail::f4_tail_init_in(&mut w);

        let scout = w.sim.strings.intern_str("scout");
        let guard = w.sim.strings.intern_str("guard");
        w.sim.store.spawning.teams = make_teams(8.0);
        w.sim.store.spawning.teams[1].classes = [scout, guard]
            .map(|name| SpawnClass { name, target: 1.0, hero: false })
            .to_vec();
        let mut setup = TeamSetup::new(1);
        setup.classes = [("scout", Some(1)), ("guard", Some(1))]
            .map(|(name, max_units)| UnitClassSetup { class_name: name.into(), min_units: 1, max_units })
            .to_vec();
        let mut rules = EventDriven::new(NoopRules, 8, 8);
        rules.waves = Some(Waves::new(10.0, 0.5, true, &[(1, setup)].into()));
        f4_tail::install_rules_for_test(&mut w, RuleSet::EventDriven(rules)).unwrap();

        let body_scout = w.sim.store.unit.insert(Unit { alive: true, class_name: scout, ..Default::default() });
        let bot_scout = w.sim.store.character.insert(Character { team: 1.0, unit: Some(body_scout), ..Default::default() });
        let body_guard = w.sim.store.unit.insert(Unit { alive: true, class_name: guard, ..Default::default() });
        let bot_guard = w.sim.store.character.insert(Character { team: 1.0, unit: Some(body_guard), ..Default::default() });
        let human = w.sim.store.character.insert(Character {
            team: 1.0,
            human: true,
            spawn: Some(Spawn { timer: 0.0, ready: false, ..Default::default() }),
            ..Default::default()
        });
        w.sim.store.spawning.teams[1].characters.extend([bot_scout, bot_guard, human]);

        let mut entry = Entry {
            handle: F4Handle::default(),
            ordinal: 1.0,
            view: F4ConnectionView { class_name: Some(scout), ..Default::default() },
            mission: Some(1),
        };
        // A human may take any class his team lists, whatever the bots hold
        // (the 2005 game limits him only in a class with points to unlock, and
        // no prepared class has any): naming no class takes his last one.
        assert_eq!(w.mission_class_has_room(1, "scout"), Some(true));
        assert_eq!(w.mission_class_has_room(1, "guard"), Some(true));
        assert_eq!(w.mission_class_has_room(1, "unknown"), None);
        assert!(request(&mut w, &mut entry, human, None, None), "his last class, at its bots' maximum");
        assert_eq!(entry.view.class_name, Some(scout));
        assert!(w.sim.store.character.get(human).unwrap().spawn.as_ref().unwrap().ready);
        // A class his team does not list is refused.
        let other = w.sim.strings.intern_str("other");
        assert!(!request(&mut w, &mut entry, human, Some(other), None));
        assert_eq!(entry.view.class_name, Some(scout));

        // local_spawn_due reads the bound local_mission connection, not an earlier parked human.
        crate::battle::f4_connections::f4_connections_init_in(&mut w, 8);
        let parked = w.sim.store.character.insert(Character {
            team: 1.0,
            human: true,
            spawn: Some(Spawn { timer: 999.0, ready: false, ..Default::default() }),
            ..Default::default()
        });
        let local_human = w.sim.store.character.insert(Character {
            team: 1.0,
            human: true,
            spawn: Some(Spawn { timer: 2.0, ready: false, ..Default::default() }),
            ..Default::default()
        });
        assert!(parked.slot < local_human.slot);
        w.battle.connections.as_mut().unwrap().local_mission = Some(local_human);
        assert_eq!(w.local_spawn_due(100), Some(220));
        w.sim.store.character.get_mut(local_human).unwrap().spawn.as_mut().unwrap().timer = 0.0;
        assert_eq!(w.local_spawn_due(100), None);
    }
}
