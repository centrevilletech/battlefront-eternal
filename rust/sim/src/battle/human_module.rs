//! Live HumanSpawn commands. A command owns its state outside Sim while
//! world callbacks run. Mutating callbacks receive that state and its
//! events first, and return what nested commands left behind.
use super::human::{self, Authority, Command, Event, State, World};
use crate::abi::sim;
use crate::gen::*;
use crate::store::StrId;

impl From<&HumanSpawn> for HsOwned {
    fn from(s: &HumanSpawn) -> Self {
        Self { auto: s.auto.clone(), class_name: s.class_name, post: s.post,
            phase: s.phase, can_switch_teams: s.can_switch_teams,
            spawned_once: s.spawned_once, dead_time: s.dead_time,
            pending: s.pending.clone(), ended: s.ended }
    }
}

impl From<HsOwned> for HumanSpawn {
    fn from(s: HsOwned) -> Self {
        Self { auto: s.auto, class_name: s.class_name, post: s.post,
            phase: s.phase, can_switch_teams: s.can_switch_teams,
            spawned_once: s.spawned_once, dead_time: s.dead_time,
            pending: s.pending, ended: s.ended }
    }
}

impl From<State> for HsState {
    fn from(s: State) -> Self {
        Self { phase: s.phase, team: s.team, class_name: s.class_name,
            class_index: s.class_index, post: s.post,
            can_switch_teams: s.can_switch_teams, over: s.over }
    }
}

impl From<Event> for HsEvent {
    fn from(event: Event) -> Self {
        let mut out = Self::default();
        match event {
            Event::AwaitingSpawn(s) => { out.kind = HsEventKind::AwaitingSpawn; out.state = Some(s.into()); }
            Event::Spawned { team, class_name, post } => {
                out.kind = HsEventKind::Spawned; out.team = team; out.class_name = Some(class_name); out.post = post;
            }
            Event::TeamChanged { team, previous } => {
                out.kind = HsEventKind::TeamChanged; out.team = team; out.previous = previous;
            }
            Event::PlayerDied { team, class_name } => {
                out.kind = HsEventKind::PlayerDied; out.team = team; out.class_name = Some(class_name);
            }
        }
        out
    }
}

struct JsWorld;

impl JsWorld {
    fn query(&mut self, op: HsQueryOp, team: f64, class_name: Option<StrId>) -> HsQueryResult {
        import_hs_query(&HsQueryArgs { op, team, class_name })
    }

    fn action(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, op: HsActionOp,
        team: f64, class_name: Option<StrId>, post: Option<StrId>) -> bool {
        let args = HsActionArgs { op, state: HsOwned::from(&*s),
            events: human::flush(events).into_iter().map(Into::into).collect(), team, class_name, post };
        let result = import_hs_action(&args);
        *s = result.state.into();
        result.accepted
    }
}

impl World for JsWorld {
    fn team(&mut self) -> f64 { self.query(HsQueryOp::Team, 0.0, None).number }
    fn dead(&mut self) -> bool { self.query(HsQueryOp::Dead, 0.0, None).flag }
    fn over(&mut self) -> bool { self.query(HsQueryOp::Over, 0.0, None).flag }
    fn roster(&mut self, team: f64) -> Vec<StrId> { self.query(HsQueryOp::Roster, team, None).roster }
    fn classic(&mut self) -> bool { self.query(HsQueryOp::Classic, 0.0, None).flag }
    fn character_spawn(&mut self) -> Option<(bool, f64)> {
        let r = self.query(HsQueryOp::CharacterSpawn, 0.0, None);
        r.present.then_some((r.has_unit, r.timer))
    }
    fn can_spawn(&mut self, class_name: StrId) -> bool { self.query(HsQueryOp::CanSpawn, 0.0, Some(class_name)).flag }
    fn set_team(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, team: f64) {
        self.action(s, events, HsActionOp::SetTeam, team, None, None);
    }
    fn spawn(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) {
        self.action(s, events, HsActionOp::Spawn, 0.0, Some(choice.class_name), choice.post);
    }
    fn ready(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) -> bool {
        self.action(s, events, HsActionOp::Ready, 0.0, Some(choice.class_name), choice.post)
    }
    fn kill(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>) {
        self.action(s, events, HsActionOp::Kill, 0.0, None, None);
    }
    fn cancel(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>) {
        self.action(s, events, HsActionOp::Cancel, 0.0, None, None);
    }
    fn rejected(&mut self, s: &mut HumanSpawn, events: &mut Vec<Event>, class_name: StrId, team: f64) {
        self.action(s, events, HsActionOp::Rejected, team, Some(class_name), None);
    }
}

#[no_mangle]
pub extern "C" fn human_spawn_words(which: u32) -> u32 {
    (match which { 0 => HsInput::WORDS, 1 => HsResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn human_spawn_run() -> *const f64 {
    // Read everything before the first import. Reentry may replace the
    // call block, the store's singleton, and grow the string table.
    let (mut s, mut input) = {
        let sim = sim();
        let (row, heap) = sim.calls.args();
        let mut input = HsInput::take(row, heap);
        let truthy = |id: Option<StrId>| id.filter(|&id| !sim.strings.units(id).is_empty());
        input.post = truthy(input.post);
        if let Some(a) = &mut input.authority {
            a.class_name = truthy(a.class_name);
            a.post = truthy(a.post);
        }
        (std::mem::take(&mut sim.store.human_spawn), input)
    };
    let commands: Vec<_> = input.commands.into_iter().map(|c| match c.kind {
        HsCommandKind::Team => Command::Team(c.team),
        HsCommandKind::Spawn => Command::Spawn(HumanPick { class_name: c.class_name.expect("a spawn class"), post: c.post }),
        HsCommandKind::Suicide => Command::Suicide,
        HsCommandKind::Cancel => Command::Cancel,
    }).collect();
    let mut events = Vec::new();
    let mut world = JsWorld;
    let mut view = None;
    match input.op {
        HsOperation::Update => human::update(&mut s, &mut events, input.dt, &commands, &mut world),
        HsOperation::Apply => for c in &commands { human::apply(&mut s, &mut events, c, &mut world); },
        HsOperation::Phase => human::update_phase(&mut s, &mut events, &mut world),
        HsOperation::Realize => human::realize(&mut s, &mut events, &mut world),
        HsOperation::Died => human::died(&mut s, &mut events, &mut world),
        HsOperation::Spawned => human::spawned_normalized(&mut s, &mut events, input.class_name.expect("a spawned class"), input.post, &mut world),
        HsOperation::TeamChanged => human::team_changed(&mut s, &mut events, input.team, input.previous, &mut world),
        HsOperation::State => view = Some(human::state(&s, &mut world).into()),
        HsOperation::Authority => {
            let a = input.authority.take().expect("an authoritative state");
            let a = Authority { phase: a.phase, class_name: a.class_name, post: a.post,
                spawned_once: a.spawned_once, ended: a.ended, dead_time: a.dead_time, pending: a.pending, auto: a.auto };
            events = human::apply_authoritative_normalized(&mut s, a, &mut events, &mut world);
        }
    }
    let result = HsResult { events: events.into_iter().map(Into::into).collect(), view };
    let sim = sim();
    sim.store.human_spawn = s;
    sim.calls.result(HsResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}
