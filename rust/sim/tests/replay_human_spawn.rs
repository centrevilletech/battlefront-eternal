//! Real offline battles' spawn state machine, including reentrant world
//! callbacks and the queue the screen receives after the tick.
#![cfg(feature = "replay")]
use bfe_sim::battle::human::{self, Command, Event, State, World};
use bfe_sim::gen::{HumanPhase, HumanSpawn, Replay, KIND_HUMAN_SPAWN};
use bfe_sim::store::{StrId, Strings};
use sim_replay::{check_num, check_value, replay_corpus, Arena, Call, Corpus, Frame, Log, Outcome, V};

fn name(v: V, strings: &mut Strings) -> Option<StrId> { v.str().map(|s| strings.intern_str(s)) }
fn phase(v: V) -> Result<HumanPhase, String> {
    match v.str() {
        Some("unspawned") => Ok(HumanPhase::Unspawned), Some("alive") => Ok(HumanPhase::Alive),
        Some("dying") => Ok(HumanPhase::Dying), Some("awaiting") => Ok(HumanPhase::Awaiting),
        _ => Err(format!("unknown human phase {}", v.show())),
    }
}
fn queue(v: V, strings: &mut Strings) -> Result<Vec<Event>, String> {
    v.items().map(|e| {
        let d = e.at(1);
        let team = d.get("team").num();
        let class_name = name(d.get("className"), strings).unwrap_or_default();
        Ok(match e.at(0).str() {
            Some("spawned") => Event::Spawned { team, class_name, post: name(d.get("post"), strings) },
            Some("teamChanged") => Event::TeamChanged { team, previous: d.get("previous").num() },
            Some("playerDied") => Event::PlayerDied { team, class_name },
            Some("awaitingSpawn") => Event::AwaitingSpawn(State { phase: phase(d.get("phase"))?, team, class_name,
                class_index: d.get("classIndex").num(), post: name(d.get("post"), strings),
                can_switch_teams: d.get("canSwitchTeams").bool(), over: d.get("over").bool() }),
            _ => return Err(format!("unknown human event {}", e.show())),
        })
    }).collect()
}
#[derive(Clone)]
struct Reads { team: f64, dead: bool, over: bool, classic: bool, rosters: Vec<Vec<StrId>>, character: Option<(bool, f64)> }
impl Reads {
    fn read(v: V, strings: &mut Strings) -> Self {
        Self { team: v.get("team").num(), dead: v.get("dead").bool(), over: v.get("over").bool(), classic: v.get("classic").bool(),
            rosters: v.get("rosters").items().map(|r| r.items().map(|n| name(n, strings).unwrap()).collect()).collect(),
            character: (!v.get("character").is_nullish()).then(|| (v.get("character").at(0).bool(), v.get("character").at(1).num())) }
    }
}
struct After { state: HumanSpawn, events: Vec<Event>, reads: Reads }
struct Served<'f, 's> { log: Log<'f>, strings: &'s Strings, reads: Reads, afters: Vec<After>, next: usize, failure: Option<String> }
impl<'f> Served<'f, '_> {
    fn call(&mut self, label: &str) -> Option<(&'f Call, usize)> {
        if self.failure.is_some() { return None; }
        match self.log.call(label) {
            Ok(call) => { let at = self.next; self.next += 1; Some((call, at)) }
            Err(e) => { self.failure = Some(e.0); None }
        }
    }
    fn arg_name(&mut self, call: &Call, i: usize, value: Option<StrId>) {
        let same = match value {
            None => call.arg(i).is_null(),
            Some(s) => call.arg(i).str().is_some_and(|v| v.encode_utf16().eq(self.strings.units(s).iter().copied())),
        };
        if !same { self.failure.get_or_insert_with(|| format!("{} arg {i}: {}", call.name, call.arg(i).show())); }
    }
    fn restore(&mut self, at: usize, s: &mut HumanSpawn, events: &mut Vec<Event>) {
        let after = &self.afters[at];
        *s = after.state.clone();
        *events = after.events.clone();
        self.reads = after.reads.clone();
    }
    fn choice(&mut self, label: &str, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &bfe_sim::gen::HumanPick) -> bool {
        let Some((call, at)) = self.call(label) else { return false };
        self.arg_name(call, 0, Some(choice.class_name));
        self.arg_name(call, 1, choice.post);
        self.restore(at, s, events);
        call.result().truthy()
    }
}
impl World for Served<'_, '_> {
    fn team(&mut self) -> f64 { self.reads.team }
    fn dead(&mut self) -> bool { self.reads.dead }
    fn over(&mut self) -> bool { self.reads.over }
    fn roster(&mut self, team: f64) -> Vec<StrId> { self.reads.rosters.get(team as usize).cloned().unwrap_or_default() }
    fn classic(&mut self) -> bool { self.reads.classic }
    fn character_spawn(&mut self) -> Option<(bool, f64)> { self.reads.character }
    fn can_spawn(&mut self, name: StrId) -> bool {
        let Some((call, _)) = self.call("human.canSpawn") else { return false };
        self.arg_name(call, 0, Some(name));
        call.result().truthy()
    }
    fn set_team(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, team: f64) {
        if let Some((call, at)) = self.call("human.setTeam") {
            if let Err(e) = check_num("human.setTeam", call.arg(0), team) { self.failure.get_or_insert(e.0); }
            self.restore(at, s, events);
        }
    }
    fn spawn(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &bfe_sim::gen::HumanPick) { self.choice("human.spawn", s, events, choice); }
    fn ready(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &bfe_sim::gen::HumanPick) -> bool { self.choice("human.ready", s, events, choice) }
    fn kill(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>) {
        if let Some((_, at)) = self.call("human.kill") { self.restore(at, s, events); }
    }
    fn cancel(&mut self, _: &mut HumanSpawn, _: &mut Vec<Event>) {
        if let Some((_, at)) = self.call("human.cancel") { self.reads = self.afters[at].reads.clone(); }
    }
    fn rejected(&mut self, _: &mut HumanSpawn, _: &mut Vec<Event>, _: StrId, _: f64) {}
}

fn replay(frame: &Frame) -> Result<(), String> {
    let before = frame.before();
    let mut cx = Replay::default();
    cx.read_value(KIND_HUMAN_SPAWN, before.get("human"))?;
    let mut events = queue(before.get("queue"), &mut cx.strings)?;
    let reads = Reads::read(before.get("reads"), &mut cx.strings);
    let mut afters = Vec::new();
    for call in frame.calls() {
        let x = call.extra();
        afters.push(After { state: HumanSpawn::from_value(x.get("human"), &mut cx, "human")?,
            events: queue(x.get("queue"), &mut cx.strings)?, reads: Reads::read(x.get("reads"), &mut cx.strings) });
    }
    let mut commands = Vec::new();
    for c in frame.arg(1).items() {
        let command = match c.get("kind").str() {
            Some("team") => c.get("team").opt_num().and_then(Command::team),
            Some("suicide") => Some(Command::Suicide), Some("cancel") => Some(Command::Cancel),
            Some("spawn") => match name(c.get("className"), &mut cx.strings) {
                Some(class) if c.get("post").is_nullish() || c.get("post").str().is_some() => {
                    let post = name(c.get("post"), &mut cx.strings);
                    Command::spawn(class, post, &cx.strings)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(command) = command { commands.push(command); }
    }
    let mut s = std::mem::take(&mut cx.store.human_spawn);
    let mut world = Served { log: Log::new(frame), strings: &cx.strings, reads, afters, next: 0, failure: None };
    human::update(&mut s, &mut events, frame.arg(0).num(), &commands, &mut world);
    if let Some(failure) = world.failure { return Err(failure); }
    world.log.finish().map_err(|e| e.0)?;
    cx.store.human_spawn = s;
    let mut arena = Arena::default();
    let actual = cx.write_value(KIND_HUMAN_SPAWN, &mut arena);
    check_value("human", frame.after().get("human"), arena.view(&actual)).map_err(|e| e.0)?;
    let expected = queue(frame.after().get("queue"), &mut cx.strings)?;
    if events != expected { return Err(format!("human.queue: expected {expected:?}, got {events:?}")); }
    Ok(())
}

#[test]
fn human_spawn_updates_match_real_battles() {
    let Some(corpus) = Corpus::from_env().expect("BFE_REPLAY_CORPUS") else { return };
    let report = replay_corpus(&corpus, "HumanSpawn.update", |_, frame| match replay(frame) {
        Ok(()) => Outcome::Same, Err(e) => Outcome::Failed(e),
    }).expect("the capture reads");
    println!("{}", report.summary());
    assert!(report.replayed() > 0, "the corpus contains no HumanSpawn.update frames");
    report.assert_clean();
}
