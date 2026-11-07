//! Network roster commands mutate canonical Characters and spawn queues.
//! Only controller attachment/planning remains foreign. Connection names and
//! new slots are admitted outside the retained per-tick tail.
use crate::world::owner::World as Root;
use super::owner::BattleOwner;
use super::{
    f4_core, f4_tail,
    names::BotNames,
    native_lifecycle,
    roster::{self, Players, PlayersWorld, RosterWorld},
    spawning,
};
use crate::{
    gen::*,
    store::{block::Block, Ref, StrId},
    units::f3::{self, WorldServices},
};

pub(super) struct State {
    classic: bool,
    players: Players,
    names: Vec<BotNames>,
    shell: Option<Store>,
    result: F4RosterResult,
    output: Block,
}
fn state(battle: &mut BattleOwner) -> &mut State {
    {
        battle.roster
            .as_mut()
            .expect("native roster initialized")
    }
}
pub(crate) fn reset(battle: &mut BattleOwner) {
    {
        battle.roster = None;
    }
}
use super::stats::WorldText as Text;
fn text(w: &mut Root, id: StrId) -> String {
    String::from_utf16_lossy(w.sim.strings.units(id))
}
fn naming(w: &mut Root, n: F4RosterNames) -> BotNames {
    if n.faction == super::names::LOCALS {
        return BotNames::Locals(
            n.local
                .map(|id| text(w, id))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "LOCAL".into()),
        );
    }
    let seed = if (1.0..=4.0).contains(&n.faction) {
        n.faction
    } else {
        0.0
    };
    BotNames::Side {
        random: n
            .state
            .map(jsrt::rng::ClassicRandom::from_state)
            .unwrap_or_else(|| jsrt::rng::ClassicRandom::new(seed)),
        first: n.first.into_iter().take(64).map(|id| text(w, id)).collect(),
        last: n.last.into_iter().take(64).map(|id| text(w, id)).collect(),
    }
}
fn player_name(w: &mut Root, name: &[u16]) -> StrId {
    w.sim.strings.intern(roster::player_name(name))
}
struct World<'w, 'a> { w: &'w mut Root,
    services: WorldServices<'a>,
    names: &'a mut [BotNames],
    born: Option<Ref<Character>>,
}
impl World<'_, '_> {
    fn attach(&mut self, s: &mut Store, r: Ref<Character>, op: F4AIHookOp) {
        super::f4_live::with_store_in(self.w, s, |w| {
            let character_handle=f4_core::character_handle_in(&mut w.battle, r);
            native_lifecycle::hook(w, F4AIHook {
                op,
                character_handle,
                ..Default::default()
            },self.services.reborrow())
        });
    }
    fn recycle(&mut self, s: &mut Store, r: Ref<Character>) {
        f4_core::recycle(&mut self.w.battle, s, r);
    }
    fn reassign(&mut self, s: &Store, r: Ref<Character>) {
        if self.born == Some(r) {
            self.born = None;
            f4_core::mark_character_in(&mut self.w.battle, r);
        } else {
            f4_core::reassign(&mut self.w.battle, s, r);
        }
        if let Some(ai)=self.services.ai.as_deref_mut() {
            if let Some(handle)=f4_core::character_handle_in(&mut self.w.battle, r) {ai.character_reassigned(r,&handle);}
        }
    }
}
impl RosterWorld for World<'_, '_> {
    fn recycle_record(&mut self, s: &mut Store, r: Ref<Character>) {
        self.recycle(s, r);
    }
    fn unit_alive(&mut self, s: &Store, r: Ref<Unit>) -> bool {
        s.unit.get(r).is_some_and(|u| u.alive)
    }
    fn reassigned(&mut self, s: &Store, r: Ref<Character>) {
        self.reassign(s, r);
    }
    fn ai_detach(&mut self, s: &mut Store, r: Ref<Character>) {
        self.attach(s, r, F4AIHookOp::Detach);
    }
    fn ai_attach(&mut self, s: &mut Store, r: Ref<Character>) {
        self.attach(s, r, F4AIHookOp::Attach);
    }
    fn retire_unit(&mut self, s: &mut Store, _: Ref<Character>, r: Ref<Unit>) {
        super::f4_live::with_store_in(self.w, s, |w| {
            crate::vehicles::module::f2::live_seats::leave_voluntary_in(w, r, self.services.reborrow());
            let u = w.sim.store.unit.get_mut(r).expect("retired roster body");
            u.human = false;
            u.network_human = Some(false);
            u.combat.player = false;
            f3::remove_native_in(w, r, self.services.reborrow());
        });
    }
    fn bot_name(&mut self, _: &mut Store, team: usize) -> StrId {
        let name = self.names[team].next();
        self.w.sim.strings.intern_str(&name)
    }
    fn player_name(&mut self, name: &[u16]) -> StrId {
        player_name(self.w, name)
    }
    fn insert(&mut self, s: &mut Store, team: f64, name: StrId) -> Option<Ref<Character>> {
        let r = spawning::insert(s, team, false, -1.0, name, &mut self.w.sim.strings)?;
        f4_core::adopt_in(self.w, s, r);
        Some(r)
    }
    fn activate(&mut self, s: &mut Store, r: Ref<Character>) {
        f4_tail::spawning(self.w, s, self.services.reborrow(), |s, world, random| {
            spawning::activate(s, r, &Text, world, random)
        });
    }
    fn pad_quotas(&mut self, s: &mut Store, team: usize) {
        f4_tail::spawning(self.w, s, self.services.reborrow(), |s, world, random| {
            spawning::pad_quotas(&mut s.spawning, team, &mut spawning::BoundDraws(random,world))
        });
    }
}
impl PlayersWorld for World<'_, '_> {
    fn recycle_record(&mut self, s: &mut Store, r: Ref<Character>) {
        self.recycle(s, r);
    }
    fn reassigned(&mut self, s: &Store, r: Ref<Character>) {
        self.reassign(s, r);
    }
    fn player_name(&mut self, name: &[u16]) -> StrId {
        player_name(self.w, name)
    }
    fn new_human(&mut self, _: &mut Players, s: &mut Store, team: f64) -> Ref<Character> {
        let c = super::rules::new_character(
            s.battle.characters.len() as f64,
            team,
            true,
            &mut self.w.sim.strings,
        );
        let r = s.character.insert(c);
        s.battle.characters.push(r);
        f4_core::adopt_in(self.w, s, r);
        self.born = Some(r);
        r
    }
    fn remove_body(&mut self, _: &mut Players, s: &mut Store, _: Ref<Character>, r: Ref<Unit>) {
        super::f4_live::with_store_in(self.w, s, |w| f3::remove_native_in(w, r, self.services.reborrow()));
    }
}
pub fn f4_roster_init_in(w: &mut Root) {
    assert!(
        f4_core::owns_lifecycle_in(&mut w.battle),
        "canonical battle lifecycle required"
    );
    let (row, heap) = w.sim.calls.args();
    let cfg = F4RosterSetup::take(row, heap);
    // Native initial Spawning.begin already advanced each private naming
    // generator. Transfer that authority instead of reseeding from facades.
    let native_names=if cfg.classic {
        w.ai.battle_mut().and_then(|battle|battle.initial_naming.take()).map(|mut initial| {
            (0..spawning::TEAM_COUNT).map(|team| {
                initial.teams.get_mut(team).and_then(Option::take).unwrap_or_else(||
                    BotNames::new(initial.lists.as_ref(),w.sim.store.spawning.teams[team].faction))
            }).collect()
        })
    }else{None};
    let names = native_names.unwrap_or_else(||cfg.names.into_iter().map(|names| naming(w, names)).collect());
    let mut owner = State {
        classic: cfg.classic,
        players: Players::default(),
        names,
        shell: Some(Store::default()),
        result: F4RosterResult {
            counts: Vec::with_capacity(2),
            ..Default::default()
        },
        output: Block::default(),
    };
    owner
        .players
        .reserved
        .reserve((roster::MAX_TEAM_SIZE * 2.0) as usize);
    owner
        .output
        .words
        .reserve(crate::store::block::HEADER + F4RosterResult::WORDS + 2 * RoCount::WORDS);
    owner.output.heap.reserve(2 * RoCount::WORDS);
    let outer = f3::external_in(w);
    // MultiplayerWorld relinquishes the dormant viewpoint before constructing
    // its roster. Retiring the Character must not return that passive body.
    if let Some(local)=cfg.local {w.sim.store.character.get_mut(local).expect("local roster Character").unit=None;}
    f3::with_services_in(w, |w, services| {
        let mut services=services.expect("native roster body services");
        let mut shell = owner.shell.take().unwrap();
        std::mem::swap(&mut shell, &mut w.sim.store);
        let mut world = World { w,
            services: services.reborrow(),
            names: &mut owner.names,
            born: None,
        };
        if cfg.classic {
            roster::create_with(&mut shell, cfg.army_size, cfg.local, &mut world);
        } else {
            owner.players = Players::new(&mut shell, cfg.army_size, cfg.has_ai);
            owner
                .players
                .reserved
                .reserve((roster::MAX_TEAM_SIZE * 2.0) as usize);
        }
        let w = world.w;
        f4_core::roster_slots_in(&mut w.battle, &mut shell);
        f4_core::mark_tail_in(&mut w.battle, &shell);
        std::mem::swap(&mut shell, &mut w.sim.store);
        owner.shell = Some(shell);
    });
    {
        w.battle.roster = Some(owner);
    }
    outer.finish(w, true);
}
pub fn f4_roster_command_in(w: &mut Root) -> Option<&[f64]> {
    let (row, heap) = w.sim.calls.args();
    let input = F4RosterCommand::take(row, heap);
    super::f4_live::count_external(&mut w.battle, F4RosterCommand::WORDS);
    let mut owner = {
        w.battle.roster
            .take()
            .expect("roster command cannot reenter")
    };
    owner.result.handle = None;
    owner.result.flag = false;
    owner.result.full = false;
    owner.result.counts.clear();
    let mut name = [0u16; 32];
    let source = w.sim.strings.units(input.name);
    let length = source.len().min(name.len());
    name[..length].copy_from_slice(&source[..length]);
    let outer = f3::external_in(w);
    f3::with_services_in(w, |w, services| {
        let mut services=services.expect("native roster body services");
        let mut shell = owner.shell.take().unwrap();
        std::mem::swap(&mut shell, &mut w.sim.store);
        let target = input
            .target
            .as_ref()
            .and_then(|h| f4_core::resolve(&mut w.battle, &shell, h));
        let mut world = World { w,
            services: services.reborrow(),
            names: &mut owner.names,
            born: None,
        };
        // Team indices cross the command boundary as numbers. Invalid or
        // oversized choices reject before Classic's dense team/name lookup.
        let valid_team = input.team == 1.0 || input.team == 2.0;
        match input.op {
            RoOperation::Reserve if !valid_team => owner.result.full = true,
            RoOperation::SwitchTeam if !valid_team => {},
            RoOperation::Reserve => {
                let result = if owner.classic {
                    roster::reserve_with(
                        &mut shell,
                        input.team,
                        input.player_id,
                        &name[..length],
                        &mut world,
                    )
                } else {
                    owner.players.reserve(
                        &mut shell,
                        input.team,
                        input.player_id,
                        &name[..length],
                        &mut world,
                    )
                };
                match result {
                    Ok(r) => {
                        shell.character.get_mut(r).unwrap().hero=Some(false);
                        owner.result.handle = f4_core::character_handle_in(&mut world.w.battle, r);
                        owner.result.flag = true;
                    }
                    Err(_) => owner.result.full = true,
                }
            }
            RoOperation::Release => {
                if let Some(r) = target {
                    if owner.classic {
                        roster::release_with(&mut shell, r, &mut world);
                    } else {
                        owner.players.release(&mut shell, r, &mut world);
                    }
                    owner.result.flag = true;
                }
            }
            RoOperation::SwitchTeam => {
                if let Some(r) = target {
                    owner.result.flag = if owner.classic {
                        roster::switch_team_with(&mut shell, r, input.team, &mut world)
                    } else {
                        owner.players.switch_team(&mut shell, r, input.team)
                    };
                }
            }
            RoOperation::Counts => {
                if owner.classic {
                    for (team, humans, total) in roster::counts(&shell) {
                        owner.result.counts.push(RoCount {
                            team,
                            humans,
                            total: Some(total),
                        });
                    }
                } else {
                    for team in [1.0, 2.0] {
                        owner.result.counts.push(RoCount {
                            team,
                            humans: owner.players.humans(&shell, team),
                            total: None,
                        });
                    }
                }
            }
            RoOperation::Create => {},
        }
        let w = world.w;
        if input.op != RoOperation::Counts {
            f4_core::roster_slots_in(&mut w.battle, &mut shell);
        f4_core::mark_tail_in(&mut w.battle, &shell);
        }
        std::mem::swap(&mut shell, &mut w.sim.store);
        owner.shell = Some(shell);
    });
    {
        w.battle.roster = Some(owner);
    }
    outer.finish(w, true);
    let owner = state(&mut w.battle);
    owner.output.begin(1, F4RosterResult::WORDS);
    let (row, heap) = owner.output.row(0);
    owner.result.put(row, heap);
    owner.output.finish();
    Some(&owner.output.words[..])
}



// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;

    #[no_mangle]
    pub extern "C" fn f4_roster_init() { f4_roster_init_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn f4_roster_command() -> *const f64 { f4_roster_command_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
}

impl super::stats::TextWorld for World<'_, '_> { fn text_nonempty(&self,id:StrId)->bool { !self.w.sim.strings.units(id).is_empty() } }

impl super::spawning::RandomWorld for World<'_, '_> { fn random_next(&mut self, stream:u32)->f64 { f3::random_next_in(&mut self.w.units,stream) as f64 } }
