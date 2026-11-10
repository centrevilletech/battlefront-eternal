//! The first two fingerprints: `worldState(game)` (src/runtime/world-state.js)
//! and the deep `digest()` of tools/world-hash.mjs, as the JSON texts whose
//! `JSON.stringify` bytes the Node tool hashes. The JavaScript objects those
//! functions read are the store's kinds (the Unit, Vehicle, Post, Battle,
//! Character, HumanSpawn and round rows are the native rows the JavaScript
//! mirrors), so the values come from one export of each kind per tick,
//! decoded with the generated `take_full` readers; a JavaScript unit's `id`
//! is its store slot (the F3 bindings index by either), and the registry
//! lists hull targets after the player and before the soldiers.
//!
//! Not yet produced here (the lane report lists them): the random state,
//! the player's arsenal, the turret targets' rows, the event ring.
use bfe_sim::abi::context::Context;
use bfe_sim::gen::{
    self, Air, Battle, F3WeaponAction, BlockHeader as B, Bolt, Character, CommandPosts, FlightState, Grenade,
    HumanPhase, HumanSpawn, Ordnance, Post, Posture, Round, SeatLease, Unit, UnitBody, Vehicle,
    Vehicles,
};
use bfe_sim::store::StrId;

use super::host::Host;
use super::json::Json;
use bfe_sim::host::journal::{Event, Journal, Life, Presentation, Presented, SQUAD_LABELS};

/// One kind's export: every row decoded, by store slot.
struct Rows<T> {
    slots: Vec<(u32, T)>,
}
impl<T: Default> Rows<T> {
    /// `Context::export(kind, 1)`: the full rows (mode 1 lists no retired
    /// rows and drains nothing, unlike the mirror's mode 0).
    fn read(
        context: &Context,
        kind: u32,
        take: fn(&mut T, &[f64], &[f64]),
    ) -> Result<Self, String> {
        let words = context
            .export(kind, 1)
            .map_err(|e| format!("export {kind}: {}", e.message()))?;
        let rows = words[B::ROWS] as usize;
        let stride = words[B::STRIDE] as usize;
        let heap_at = words[B::HEAP] as usize;
        let heap = &words[heap_at..heap_at + words[B::HEAP_WORDS] as usize];
        let mut slots = Vec::with_capacity(rows);
        for i in 0..rows {
            let row = &words[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
            let mut value = T::default();
            take(&mut value, row, heap);
            slots.push((row[0] as u32, value));
        }
        Ok(Self { slots })
    }
    fn get(&self, slot: u32) -> Option<&T> {
        self.slots.iter().find(|(s, _)| *s == slot).map(|(_, v)| v)
    }
}

/// The store as the two observers read it after a tick.
pub struct Snapshot {
    units: Rows<Unit>,
    vehicles: Rows<Vehicle>,
    fleet: Rows<Vehicles>,
    leases: Rows<SeatLease>,
    posts: Rows<Post>,
    command_posts: Rows<CommandPosts>,
    battle: Rows<Battle>,
    characters: Rows<Character>,
    human: Rows<HumanSpawn>,
    grenades: Rows<Grenade>,
    bolts: Rows<Bolt>,
    rounds: Rows<Round>,
    ordnance: Rows<Ordnance>,
}

impl Snapshot {
    pub fn read(host: &Host) -> Result<Self, String> {
        let c = &host.context;
        Ok(Self {
            units: Rows::read(c, u32::from(gen::KIND_UNIT), Unit::take_full)?,
            vehicles: Rows::read(c, u32::from(gen::KIND_VEHICLE), Vehicle::take_full)?,
            fleet: Rows::read(c, u32::from(gen::KIND_VEHICLES), Vehicles::take_full)?,
            leases: Rows::read(c, u32::from(gen::KIND_SEAT_LEASE), SeatLease::take_full)?,
            posts: Rows::read(c, u32::from(gen::KIND_POST), Post::take_full)?,
            command_posts: Rows::read(
                c,
                u32::from(gen::KIND_COMMAND_POSTS),
                CommandPosts::take_full,
            )?,
            battle: Rows::read(c, u32::from(gen::KIND_BATTLE), Battle::take_full)?,
            characters: Rows::read(c, u32::from(gen::KIND_CHARACTER), Character::take_full)?,
            human: Rows::read(c, u32::from(gen::KIND_HUMAN_SPAWN), HumanSpawn::take_full)?,
            grenades: Rows::read(c, u32::from(gen::KIND_GRENADE), Grenade::take_full)?,
            bolts: Rows::read(c, u32::from(gen::KIND_BOLT), Bolt::take_full)?,
            rounds: Rows::read(c, u32::from(gen::KIND_ROUND), Round::take_full)?,
            ordnance: Rows::read(c, u32::from(gen::KIND_ORDNANCE), Ordnance::take_full)?,
        })
    }
}

/// What `u.soldier` reads the same way for either body: a droideka has no
/// posture, air state or fuel (the JavaScript reads `undefined` there).
struct Body {
    position: [f64; 3],
    velocity: [f64; 3],
    aim_yaw: f64,
    aim_pitch: f64,
    posture: Option<Posture>,
    air: Option<Air>,
    fuel: Option<f64>,
}
fn body(unit: &Unit) -> Body {
    match &unit.soldier {
        UnitBody::Soldier(s) => Body {
            position: s.position,
            velocity: s.velocity,
            aim_yaw: s.aim_yaw,
            aim_pitch: s.aim_pitch,
            posture: Some(s.posture),
            air: s.air,
            fuel: Some(s.fuel),
        },
        // DroidekaBody keeps a soldier's shape where it has no state of its
        // own: standing, never airborne, no fuel.
        UnitBody::Droideka(d) => Body {
            position: d.position,
            velocity: d.velocity,
            aim_yaw: d.aim_yaw,
            aim_pitch: d.aim_pitch,
            posture: Some(Posture::Stand),
            air: None,
            fuel: Some(0.0),
        },
    }
}

/// A store string as JSON (`null` for an absent one).
fn string(json: &mut Json, context: &Context, id: Option<StrId>) {
    match id.and_then(|id| context.string(id).ok()) {
        Some(units) => json.string_utf16(&units),
        None => json.null(),
    }
}

/// `JSON.stringify(worldState(game))` for the tick just stepped.
pub fn state_json(
    json: &mut Json,
    host: &Host,
    snapshot: &Snapshot,
    tick: f64,
) -> Result<(), String> {
    let context = &host.context;
    json.clear();
    json.begin_object();
    json.key("tick");
    json.number(tick);
    // g.randomState(): the Classic AI generator's seed, the Enhanced one's,
    // and the named streams drawn so far in first-draw order. The entries
    // are the F3 random bank's; the two AI entries were bound by name, the
    // named ones were registered by `unitsSim.random.configure(gameplay,
    // grenade-spin, gameplay, seat-weapons)` in that order.
    json.key("random");
    json.begin_object();
    {
        use bfe_sim::abi::context_step::bank;
        use bfe_sim::gen::F3RandomState;
        let mut words = Vec::new();
        let states: Vec<F3RandomState> = if context
            .read_bank(bank::F3_RANDOM_STATES, &mut words)
            .is_ok()
        {
            words
                .chunks_exact(F3RandomState::WORDS)
                .map(|row| F3RandomState::take(row, &[]))
                .collect()
        } else {
            Vec::new()
        };
        let bindings = context.ai_random_bindings().unwrap_or_default();
        json.key("ai");
        match bindings.first().and_then(|id| states.get(*id as usize)) {
            Some(s) => json.number(f64::from(s.state)),
            None => json.null(),
        }
        json.key("game");
        match bindings.get(1).and_then(|id| states.get(*id as usize)) {
            Some(s) => json.number(f64::from(s.state)),
            None => json.null(),
        }
        json.key("streams");
        json.begin_object();
        for (name, state) in host.random_bindings.present(&states) { // matrix-b
            json.key(name);
            json.number(f64::from(state.state));
        }
        json.end_object();
    }
    json.end_object();
    // units: every registry unit with a soldier body, [id, generation, team,
    // alive, x, y, z, health, aimYaw, aimPitch].
    json.key("units");
    json.begin_array();
    for (slot, u) in &snapshot.units.slots {
        let s = body(u);
        json.begin_array();
        json.number(f64::from(*slot));
        json.number(u.generation);
        json.number(u.team);
        json.boolean(u.alive);
        for v in s.position {
            json.number(v);
        }
        json.number(u.combat.health);
        json.number(s.aim_yaw);
        json.number(s.aim_pitch);
        json.end_array();
    }
    json.end_array();
    // posts: g.posts.posts in order, [team, progress, neutralize, toward, contested].
    json.key("posts");
    json.begin_array();
    if let Some((_, posts)) = snapshot.command_posts.slots.first() {
        for post in &posts.posts {
            if let Some(p) = snapshot.posts.get(post.slot) {
                json.begin_array();
                json.number(p.team);
                json.number(p.progress);
                json.number(p.neutralize);
                json.number(p.toward);
                json.boolean(p.contested);
                json.end_array();
            }
        }
    }
    json.end_array();
    let battle = snapshot.battle.slots.first().map(|(_, b)| b);
    // tickets: battle.reinforcements, an object keyed by team number.
    json.key("tickets");
    json.begin_object();
    if let Some(b) = battle {
        for (team, count) in b.reinforcements.iter() {
            json.key(&jsrt::numfmt::number_to_string(team));
            json.number(*count);
        }
    }
    json.end_object();
    // scores: battle.characters in roster order.
    json.key("scores");
    json.begin_array();
    if let Some(b) = battle {
        for character in &b.characters {
            if let Some(c) = snapshot.characters.get(character.slot) {
                json.begin_array();
                json.number(c.team);
                string(json, context, c.name);
                json.number(c.kills);
                json.number(c.team_kills);
                json.number(c.deaths);
                json.number(c.captures);
                json.end_array();
            }
        }
    }
    json.end_array();
    json.key("gameOver");
    json.boolean(battle.is_some_and(|b| b.game_over));
    // weapons: g.combat.arsenal.weapons, the viewpoint's (the first unit the
    // registry added), [clip, ammo, heat]; the facade has no `ammo`.
    json.key("weapons");
    json.begin_array();
    if let Some((_, viewpoint)) = snapshot.units.slots.first() {
        for w in &viewpoint.combat.weapons {
            json.begin_array();
            json.number(w.clip);
            json.null();
            json.number(w.heat);
            json.end_array();
        }
    }
    json.end_array();
    json.key("vehicles");
    json.begin_array();
    if let Some((_, fleet)) = snapshot.fleet.slots.first() {
        for vehicle in &fleet.vehicles {
            if let Some(v) = snapshot.vehicles.get(vehicle.slot) {
                json.begin_array();
                string(json, context, Some(v.name));
                json.boolean(v.alive);
                json.number(v.health);
                for x in v.position {
                    json.number(x);
                }
                for x in v.velocity {
                    json.number(x);
                }
                json.number(v.yaw);
                // Vehicle.pitch is the driver's seat's pitch.
                json.number(v.seats.get(v.seat_index as usize).map_or(0.0, |s| s.pitch));
                for x in v.quaternion {
                    json.number(x);
                }
                match v.flight_state {
                    Some(state) => json.string(FlightState::NAMES[state as usize]),
                    None => json.null(),
                }
                json.number(v.airspeed);
                match v.power {
                    Some(power) => json.number(power),
                    None => json.null(),
                }
                json.number(v.dying);
                json.begin_array();
                for seat in &v.seats {
                    json.begin_array();
                    let lease = seat.lease.and_then(|l| snapshot.leases.get(l.slot));
                    match lease.and_then(|l| l.unit) {
                        Some(unit) => json.number(f64::from(unit.slot)),
                        None => json.null(),
                    }
                    match lease {
                        Some(l) => json.number(l.generation),
                        None => json.null(),
                    }
                    json.number(seat.aim_yaw);
                    json.number(seat.pitch);
                    json.end_array();
                }
                json.end_array();
                json.end_array();
            }
        }
    }
    json.end_array();
    json.key("grenades");
    json.begin_array();
    if let Some((_, ordnance)) = snapshot.ordnance.slots.first() {
        for grenade in &ordnance.grenades {
            if let Some(g) = snapshot.grenades.get(grenade.slot) {
                json.begin_array();
                for x in g.position {
                    json.number(x);
                }
                for x in g.velocity {
                    json.number(x);
                }
                json.null();
                match g.gone {
                    Some(gone) => json.boolean(gone),
                    None => json.null(),
                }
                json.end_array();
            }
        }
    }
    json.end_array();
    json.key("humanState");
    json.begin_object();
    if let Some((_, h)) = snapshot.human.slots.first() {
        json.key("phase");
        json.string(HumanPhase::NAMES[h.phase as u8 as usize]);
        json.key("team");
        let team = snapshot
            .characters
            .slots
            .iter()
            .find(|(_, c)| c.human)
            .map_or(0.0, |(_, c)| c.team);
        json.number(team);
        json.key("className");
        string(json, context, Some(h.class_name));
        // HumanSpawn.state(): the class's index in the team's roster, 0
        // when the roster lacks it.
        json.key("classIndex");
        let roster = if team >= 0.0 { context.human_roster(team as usize).unwrap_or_default() } else { Vec::new() };
        json.number(roster.iter().position(|c| *c == h.class_name).map_or(0.0, |i| i as f64));
        json.key("post");
        string(json, context, h.post);
        json.key("canSwitchTeams");
        json.boolean(h.can_switch_teams);
        json.key("over");
        json.boolean(battle.is_some_and(|b| b.over));
        json.key("spectator");
        json.boolean(false);
    }
    json.end_object();
    json.key("events");
    host.context
        .world_host_journal_with_recon(|journal, definitions| events_json(json, context, journal, definitions)) // matrix-b
        .map_err(|e| e.message().to_string())?;
    json.end_object();
    Ok(())
}

/// `g.simEvents.ring.filter(Boolean).map(eventValue)`: each record as its
/// emitter wrote it, `{seq, time, kind, ...fields}` in the emitter's key
/// order (`Event`'s comments), a unit reference as `{"id","generation"}`
/// and a store string as its text (`null` when the store has none).
fn events_json(json: &mut Json, context: &Context, journal: &Journal, definitions: &bfe_sim::host::recon::Definitions) { // matrix-b
    let life = |json: &mut Json, life: Option<Life>| match life {
        Some(Life { id, generation }) => {
            json.begin_object();
            json.key("id");
            json.number(f64::from(id));
            json.key("generation");
            json.number(generation);
            json.end_object();
        }
        None => json.null(),
    };
    let string =
        |json: &mut Json, id: Option<StrId>| match id.and_then(|id| context.string(id).ok()) {
            Some(units) => json.string_utf16(&units),
            None => json.null(),
        };
    json.begin_array();
    for record in journal.records() {
        json.begin_object();
        json.key("seq");
        json.number(record.seq as f64);
        json.key("time");
        json.number(record.time);
        json.key("kind");
        json.string(record.event.kind());
        match record.event {
            Event::Recon(r) => super::recon::event_fields(json, definitions, r), // matrix-b
            Event::Spawn {
                target,
                team,
                position,
                class_name,
                post,
            } => {
                json.key("target");
                life(json, Some(target));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("className");
                string(json, Some(class_name));
                json.key("post");
                string(json, post);
            }
            Event::Vo {
                source,
                team,
                position,
                human,
                event,
                property,
                command,
                context: ctx,
                positional,
                notice,
            } => {
                json.key("source");
                life(json, Some(source));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("human");
                json.boolean(human);
                json.key("event");
                string(json, Some(event));
                json.key("property");
                string(json, Some(property));
                if let Some(command) = command {
                    json.key("command");
                    json.number(command);
                }
                if let Some(ctx) = ctx {
                    json.key("context");
                    json.number(ctx);
                }
                if let Some(positional) = positional {
                    json.key("positional");
                    json.boolean(positional);
                }
                if let Some(notice) = notice {
                    json.key("notice");
                    json.boolean(notice);
                }
            }
            Event::Shot {
                source,
                team,
                position,
                direction,
                weapon,
                repair,
                melee,
                swing,
            } => {
                json.key("source");
                life(json, Some(source));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("direction");
                json.vec3(&direction);
                json.key("weapon");
                string(json, weapon);
                json.key("repair");
                json.boolean(repair);
                json.key("melee");
                json.boolean(melee);
                json.key("swing");
                json.number(swing);
            }
            Event::MountedShot {
                source,
                mount,
                seat,
                team,
                position,
                direction,
                weapon,
                repair,
                melee,
                swing,
            } => {
                json.key("source");
                life(json, source);
                json.key("mount");
                life(json, Some(mount));
                json.key("seat");
                json.number(seat);
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("direction");
                json.vec3(&direction);
                json.key("weapon");
                string(json, weapon);
                json.key("repair");
                json.boolean(repair);
                json.key("melee");
                json.boolean(melee);
                json.key("swing");
                json.number(swing);
            }
            Event::Explosion {
                position,
                team,
                source,
                radius,
            } => {
                json.key("position");
                json.vec3(&position);
                json.key("team");
                json.number(team);
                json.key("source");
                life(json, source);
                json.key("radius");
                json.number(radius);
            }
            Event::Damage { target, team, position, amount, typed, generic, known, point, source, health, attacker, attacker_team } => {
                json.key("target");
                life(json, Some(target));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("amount");
                json.number(amount);
                json.key("typed");
                json.number(typed);
                json.key("generic");
                json.number(generic);
                json.key("known");
                json.boolean(known);
                json.key("point");
                json.vec3_or_null(point.as_ref());
                json.key("source");
                json.vec3_or_null(source.as_ref());
                json.key("health");
                json.number(health);
                json.key("attacker");
                life(json, attacker);
                json.key("attackerTeam");
                json.number(attacker_team);
            }
            Event::Presentation(presentation) => {
                json.key("event");
                presentation_json(json, &string, presentation);
            }
            Event::Grenade { position, team, source, def } => {
                json.key("position");
                json.vec3(&position);
                json.key("team");
                json.number(team);
                json.key("source");
                life(json, source);
                json.key("def");
                string(json, def);
            }
            Event::Death { target, team, position, attacker, attacker_team } => {
                json.key("target");
                life(json, Some(target));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("attacker");
                life(json, attacker);
                json.key("attackerTeam");
                json.number(attacker_team);
            }
            Event::HullDamage { target, team, position, amount, attacker, attacker_team } => {
                json.key("target");
                life(json, Some(target));
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("amount");
                json.number(amount);
                json.key("typed");
                json.number(1.0);
                json.key("generic");
                json.number(1.0);
                json.key("known");
                json.boolean(false);
                json.key("point");
                json.null();
                json.key("health");
                json.number(amount);
                json.key("attacker");
                life(json, attacker);
                json.key("attackerTeam");
                json.number(attacker_team);
            }
            Event::PropDamage { target, team, position, amount, typed, generic, point, health, attacker, attacker_team } => {
                json.key("target");
                match target {
                    Some((id, generation)) => {
                        json.begin_object();
                        json.key("id");
                        json.number(f64::from(id));
                        json.key("generation");
                        match generation {
                            Some(g) => json.number(g),
                            None => json.null(),
                        }
                        json.end_object();
                    }
                    None => json.null(),
                }
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("amount");
                json.number(amount);
                json.key("typed");
                json.number(typed);
                json.key("generic");
                json.number(generic);
                json.key("known");
                json.boolean(point.is_some());
                json.key("point");
                json.vec3_or_null(point.as_ref());
                json.key("health");
                json.number(health);
                json.key("attacker");
                life(json, attacker);
                json.key("attackerTeam");
                json.number(attacker_team);
            }
            Event::Seat {
                source,
                team,
                position,
                target,
                seat,
                ..
            } => {
                json.key("source");
                life(json, source);
                json.key("team");
                json.number(team);
                json.key("position");
                json.vec3(&position);
                json.key("target");
                life(json, target);
                json.key("seat");
                json.number(seat);
            }
        }
        json.end_object();
    }
    json.end_array();
}

/// A `presentation` record's event object in its key order; an undefined
/// action or posture is left out, as JSON.stringify leaves it.
fn presentation_json(json: &mut Json, string: &dyn Fn(&mut Json, Option<StrId>), presentation: Presentation) {
    let owner = |json: &mut Json, owner: Life| {
        json.key("ownerId");
        json.number(f64::from(owner.id));
        json.key("ownerGeneration");
        json.number(owner.generation);
    };
    let action = |json: &mut Json, action: Option<F3WeaponAction>| {
        if let Some(action) = action {
            json.key("action");
            json.string(F3WeaponAction::NAMES[action as usize]);
        }
    };
    json.begin_object();
    match presentation {
        Presentation::WeaponAction { action: a, bank, posture, prepare, owner: o } => {
            json.key("kind");
            json.string("weaponAction");
            action(json, a);
            json.key("bank");
            string(json, Some(bank));
            if let Some(posture) = posture {
                json.key("posture");
                json.string(Posture::NAMES[posture as usize]);
            }
            json.key("prepare");
            json.boolean(prepare);
            owner(json, o);
        }
        Presentation::EndWeaponAction { action: a, owner: o } => {
            json.key("kind");
            json.string("endWeaponAction");
            action(json, a);
            owner(json, o);
        }
        Presentation::Effect { name, position, owner: o } => {
            json.key("kind");
            json.string("effect");
            json.key("name");
            string(json, Some(name));
            json.key("position");
            json.vec3(&position);
            if let Some(o) = o {
                owner(json, o);
            }
        }
        Presentation::Presented { kind, owner: o, amount, source } => {
            json.key("kind");
            json.string(kind.name());
            owner(json, o);
            json.key("amount");
            json.number(amount);
            json.key("source");
            json.vec3_or_null(source.as_ref());
            json.key("hitKind");
            match kind {
                Presented::ShieldHitExplosion => json.string("explosion"),
                Presented::ShieldHit | Presented::BlowApart | Presented::DeathBlast => json.null(),
            }
        }
        Presentation::Lightning { name, from, to, charge, channel, owner: o } => {
            json.key("kind");
            json.string("lightning");
            json.key("name");
            string(json, Some(name));
            json.key("from");
            json.vec3(&from);
            json.key("to");
            json.vec3(&to);
            json.key("charge");
            json.number(charge);
            if let Some(channel) = channel {
                json.key("channel");
                json.number(channel);
            }
            owner(json, o);
        }
        Presentation::Squad { label, owner: o } => {
            json.key("kind");
            json.string("squad");
            owner(json, o);
            json.key("label");
            json.string(SQUAD_LABELS.get(label as usize).copied().unwrap_or(""));
        }
    }
    json.end_object();
}

/// `JSON.stringify(digest())`: `{units, hulls, rounds: [bolts, grenades, rounds]}`.
pub fn digest_json(json: &mut Json, host: &Host, snapshot: &Snapshot) {
    let context = &host.context;
    json.clear();
    json.begin_object();
    json.key("units");
    json.begin_array();
    // The registry's hull and building targets sit between the viewpoint
    // (id 1) and the soldiers; a target row is [id, generation, alive,
    // undefined, team, position, undefined x4] (JSON writes undefined in an
    // array as null). A VehicleTarget's team is Vehicle.controllingTeam:
    // the first seat with a lease, else the hull's own.
    let mut targets: Vec<(f64, f64, bool, f64, [f64; 3])> = snapshot
        .vehicles
        .slots
        .iter()
        .map(|(_, v)| {
            let team = v
                .seats
                .iter()
                .find_map(|seat| {
                    seat.lease
                        .and_then(|l| snapshot.leases.get(l.slot))
                        .map(|l| l.team)
                })
                .unwrap_or(v.team.unwrap_or(0.0));
            (v.target_id, v.target_generation, v.alive, team, v.position)
        })
        .collect();
    for p in context.prop_targets().unwrap_or_default() {
        targets.push((p.id, p.life, p.alive, p.team, p.position));
    }
    targets.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut target = targets.into_iter().peekable();
    for (slot, u) in &snapshot.units.slots {
        while let Some(t) = target.peek() {
            if t.0 >= f64::from(*slot) {
                break;
            }
            let (id, generation, alive, team, position) = target.next().unwrap();
            json.begin_array();
            json.number(id);
            json.number(generation);
            json.boolean(alive);
            json.null();
            json.number(team);
            json.vec3(&position);
            for _ in 0..4 {
                json.null();
            }
            json.end_array();
        }
        let s = body(u);
        let i = &u.intent;
        json.begin_array();
        json.number(f64::from(*slot));
        json.number(u.generation);
        json.boolean(u.alive);
        json.boolean(u.removed);
        json.number(u.team);
        json.vec3(&s.position);
        json.number(u.combat.health);
        json.begin_array();
        json.vec3(&s.velocity);
        match s.posture {
            Some(posture) => json.string(Posture::NAMES[posture as usize]),
            None => json.null(),
        }
        match s.air {
            Some(air) => json.string(Air::NAMES[air as usize]),
            None => json.null(),
        }
        json.number(s.aim_yaw);
        json.number(s.aim_pitch);
        match s.fuel {
            Some(fuel) => json.number(fuel),
            None => json.null(),
        }
        json.end_array();
        json.begin_array();
        json.number(i.forward);
        json.number(i.right);
        json.number(i.turn);
        json.number(i.pitch);
        json.number(i.buttons[0]);
        json.number(i.buttons[1]);
        json.vec3_or_null(i.aim.as_ref());
        json.end_array();
        match u.seat.and_then(|l| snapshot.leases.get(l.slot)) {
            Some(lease) => {
                json.begin_array();
                string(
                    json,
                    context,
                    lease
                        .vehicle
                        .and_then(|v| snapshot.vehicles.get(v.slot))
                        .map(|v| v.name),
                );
                json.number(lease.index);
                json.end_array();
            }
            None => json.null(),
        }
        json.end_array();
    }
    json.end_array();
    json.key("hulls");
    json.begin_array();
    let poses = context.fleet_seat_poses().unwrap_or_default();
    if let Some((_, fleet)) = snapshot.fleet.slots.first() {
        for vehicle in &fleet.vehicles {
            if let Some(v) = snapshot.vehicles.get(vehicle.slot) {
                json.begin_array();
                string(json, context, Some(v.name));
                json.boolean(v.alive);
                json.number(v.health);
                json.vec3(&v.position);
                json.vec3(&v.velocity);
                json.number(v.yaw);
                json.number(v.tilt);
                json.number(v.roll);
                json.number(v.tilt_rate);
                json.number(v.roll_rate);
                json.number(v.yaw_rate);
                json.number(v.support);
                match v.sleeping {
                    Some(sleeping) => json.boolean(sleeping),
                    None => json.null(),
                }
                json.number(v.forward_input);
                json.number(v.right_input);
                // v.seats: [aimYaw, pitch, xyz(target), yawRate, weapons[[barrel,
                // aimers[[yaw, pitch, onTarget, inLimits, xyz(facing)]]]]].
                json.begin_array();
                let posed = poses
                    .iter()
                    .find(|(slot, _)| *slot == vehicle.slot)
                    .map(|(_, seats)| seats);
                for (i, seat) in v.seats.iter().enumerate() {
                    json.begin_array();
                    json.number(seat.aim_yaw);
                    json.number(seat.pitch);
                    match posed.and_then(|seats| seats.get(i)) {
                        Some((yaw_rate, target, weapons)) => {
                            json.vec3_or_null(target.as_ref());
                            json.number(*yaw_rate);
                            json.begin_array();
                            for (barrel, aimers) in weapons {
                                json.begin_array();
                                json.number(*barrel);
                                json.begin_array();
                                for (yaw, pitch, on_target, in_limits, facing) in aimers {
                                    json.begin_array();
                                    json.number(*yaw);
                                    json.number(*pitch);
                                    json.boolean(*on_target);
                                    match in_limits {
                                        Some(limit) => json.boolean(*limit),
                                        None => json.null(),
                                    }
                                    json.vec3(facing);
                                    json.end_array();
                                }
                                json.end_array();
                                json.end_array();
                            }
                            json.end_array();
                        }
                        None => {
                            json.null();
                            json.number(0.0);
                            json.begin_array();
                            json.end_array();
                        }
                    }
                    json.end_array();
                }
                json.end_array();
                json.end_array();
            }
        }
    }
    json.end_array();
    json.key("rounds");
    json.begin_array();
    if let Some((_, ordnance)) = snapshot.ordnance.slots.first() {
        json.begin_array();
        for bolt in &ordnance.bolts {
            if let Some(b) = snapshot.bolts.get(bolt.slot) {
                json.begin_array();
                json.vec3(&b.position);
                json.null();
                json.number(b.life);
                json.end_array();
            }
        }
        json.end_array();
        json.begin_array();
        for grenade in &ordnance.grenades {
            if let Some(g) = snapshot.grenades.get(grenade.slot) {
                json.begin_array();
                json.vec3(&g.position);
                json.vec3(&g.velocity);
                json.null();
                json.end_array();
            }
        }
        json.end_array();
        json.begin_array();
        for round in &ordnance.rounds {
            if let Some(r) = snapshot.rounds.get(round.slot) {
                json.begin_array();
                json.vec3(&r.position);
                json.vec3(&r.velocity);
                json.number(r.life);
                json.end_array();
            }
        }
        json.end_array();
    } else {
        for _ in 0..3 {
            json.begin_array();
            json.end_array();
        }
    }
    json.end_array();
    json.end_object();
}

/// A diagnostic print of banks and rows the observer does not render yet
/// (`WORLD_HASH_NATIVE_PROBE=1`), for comparison with the Node tool's values.
pub fn probe(host: &Host) {
    use bfe_sim::abi::context_step::bank;
    use bfe_sim::gen::{F3RandomGlobals, F3RandomState};
    let c = &host.context;
    let mut words = Vec::new();
    if c.read_bank(bank::F3_RANDOM_STATES, &mut words).is_ok() {
        for (i, row) in words.chunks_exact(F3RandomState::WORDS).enumerate() {
            let s = F3RandomState::take(row, &[]);
            eprintln!(
                "probe random[{i}] state {} present {} sequence {}",
                s.state, s.present, s.sequence
            );
        }
    }
    if c.read_bank(bank::F3_RANDOM_GLOBALS, &mut words).is_ok()
        && words.len() >= F3RandomGlobals::WORDS
    {
        let g = F3RandomGlobals::take(&words, &[]);
        eprintln!(
            "probe globals gameplay {} spin {} drop {} seat {}",
            g.gameplay, g.spin, g.drop, g.seat
        );
    }
    if let Ok(snapshot) = Snapshot::read(host) {
        for (slot, h) in &snapshot.human.slots {
            eprintln!(
                "probe human[{slot}] phase {:?} class {:?} spawned_once {} ended {}",
                h.phase, h.class_name, h.spawned_once, h.ended
            );
        }
        if let Some(unit) = host.player_unit.and_then(|s| snapshot.units.get(s)) {
            for w in &unit.combat.weapons {
                eprintln!("probe player weapon def {:?} level {} clip {} reserve {} heat {} bottomless {}", w.def, w.level, w.clip, w.reserve, w.heat, w.bottomless);
            }
        }
        if let Some((_, viewpoint)) = snapshot.units.slots.first() {
            for w in &viewpoint.combat.weapons {
                eprintln!("probe viewpoint weapon def {:?} level {} clip {} reserve {} heat {} bottomless {}", w.def, w.level, w.clip, w.reserve, w.heat, w.bottomless);
            }
        }
        for (slot, v) in &snapshot.vehicles.slots {
            eprintln!(
                "probe vehicle[{slot}] target {} gen {} team {:?} alive {}",
                v.target_id, v.target_generation, v.team, v.alive
            );
        }
    }
}
