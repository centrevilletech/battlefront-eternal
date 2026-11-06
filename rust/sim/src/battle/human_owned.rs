//! Human spawn decisions on the canonical Store. Read-only services inspect
//! native bodies and prepared rosters; mutating services see the current human
//! frame and can synchronously complete team, spawn and death callbacks.
use super::human::{self, Command, Event};
use crate::gen::{HumanPick, HumanSpawn, Store};
use crate::store::StrId;

pub trait Services {
    fn team(&mut self, s: &Store) -> f64;
    fn dead(&mut self, s: &Store) -> bool;
    fn over(&mut self, s: &Store) -> bool;
    fn roster(&mut self, s: &Store, team: f64) -> &[StrId];
    fn classic(&mut self, s: &Store) -> bool;
    fn character_spawn(&mut self, s: &Store) -> Option<(bool, f64)>;
    fn can_spawn(&mut self, s: &Store, class_name: StrId) -> bool;
    fn set_team(&mut self, s: &mut Store, events: &mut Vec<Event>, team: f64);
    fn spawn(&mut self, s: &mut Store, events: &mut Vec<Event>, choice: &HumanPick);
    fn ready(&mut self, s: &mut Store, events: &mut Vec<Event>, choice: &HumanPick) -> bool;
    fn kill(&mut self, s: &mut Store, events: &mut Vec<Event>);
    fn cancel(&mut self, s: &mut Store, events: &mut Vec<Event>);
    fn rejected(&mut self, s: &mut Store, events: &mut Vec<Event>, class_name: StrId, team: f64);
}
struct Context<'a, S: Services + ?Sized> { s: &'a mut Store, services: &'a mut S }
impl<S: Services + ?Sized> Context<'_, S> {
    fn action<R>(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>,
        run: impl FnOnce(&mut S, &mut Store, &mut Vec<Event>) -> R) -> R {
        // Only the small frame moves. A service sees the actual canonical
        // state, and its nested lifecycle writes become the resumed frame.
        std::mem::swap(frame, &mut self.s.human_spawn);
        let result = run(self.services, self.s, events);
        std::mem::swap(frame, &mut self.s.human_spawn);
        result
    }
}
// Read-only facts come from units, Characters and prepared class tables.
// The small HumanSpawn frame is local while the pure state machine runs;
// every state-changing service receives it in Store before it can reenter.
impl<S: Services + ?Sized> human::World for Context<'_, S> {
    fn team(&mut self) -> f64 { self.services.team(self.s) }
    fn dead(&mut self) -> bool { self.services.dead(self.s) }
    fn over(&mut self) -> bool { self.services.over(self.s) }
    fn roster(&mut self, team: f64) -> &[StrId] { self.services.roster(self.s, team) }
    fn classic(&mut self) -> bool { self.services.classic(self.s) }
    fn character_spawn(&mut self) -> Option<(bool, f64)> { self.services.character_spawn(self.s) }
    fn can_spawn(&mut self, name: StrId) -> bool { self.services.can_spawn(self.s, name) }
    fn set_team(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>, team: f64) {
        self.action(frame, events, |w, s, e| w.set_team(s, e, team));
    }
    fn spawn(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) {
        self.action(frame, events, |w, s, e| w.spawn(s, e, choice));
    }
    fn ready(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>, choice: &HumanPick) -> bool {
        self.action(frame, events, |w, s, e| w.ready(s, e, choice))
    }
    fn kill(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>) {
        self.action(frame, events, |w, s, e| w.kill(s, e));
    }
    fn cancel(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>) {
        self.action(frame, events, |w, s, e| w.cancel(s, e));
    }
    fn rejected(&mut self, frame: &mut HumanSpawn, events: &mut Vec<Event>, name: StrId, team: f64) {
        self.action(frame, events, |w, s, e| w.rejected(s, e, name, team));
    }
}

fn with_frame<S: Services + ?Sized, R>(s: &mut Store, events: &mut Vec<Event>, services: &mut S,
    run: impl FnOnce(&mut HumanSpawn, &mut Vec<Event>, &mut dyn human::World) -> R) -> R {
    let mut frame = std::mem::take(&mut s.human_spawn);
    let result = run(&mut frame, events, &mut Context { s, services });
    s.human_spawn = frame;
    result
}
/// Unit and roster lifecycle services use the same canonical frame even when
/// a spawn command synchronously causes another team, spawn or death notice.
pub fn died<S: Services + ?Sized>(s: &mut Store, events: &mut Vec<Event>, services: &mut S) {
    with_frame(s, events, services, |f, e, w| human::died(f, e, w));
}
pub fn spawned<S: Services + ?Sized>(s: &mut Store, events: &mut Vec<Event>, class_name: StrId, post: Option<StrId>, services: &mut S) {
    with_frame(s, events, services, |f, e, w| human::spawned_normalized(f, e, class_name, post, w));
}
pub fn team_changed<S: Services + ?Sized>(s: &mut Store, events: &mut Vec<Event>, team: f64, previous: f64, services: &mut S) {
    with_frame(s, events, services, |f, e, w| human::team_changed(f, e, team, previous, w));
}

pub struct Human { events: Vec<Event>, max_commands: usize }
impl Human {
    pub fn new(max_commands: usize) -> Self {
        Self { events: Vec::with_capacity(max_commands * 3 + 8), max_commands }
    }
    /// The authoritative tail calls this after rules and AI, and never for
    /// a dedicated server's dormant local player. No entire Store is copied.
    pub fn step<S: Services + ?Sized>(&mut self, s: &mut Store, dt: f64, commands: &[Command], services: &mut S) {
        // Input has a fixed admitted batch. Notices instead retain every
        // value across a long load/pause, as HumanSpawn.queue did: Vec can
        // grow on an exceptional backlog and its storage is reused afterward.
        let commands = &commands[..commands.len().min(self.max_commands)];
        with_frame(s, &mut self.events, services, |f, e, w| human::update(f, e, dt, commands, w));
    }
    pub fn spawned<S:Services + ?Sized>(&mut self,s:&mut Store,class_name:StrId,post:Option<StrId>,services:&mut S) {
        spawned(s,&mut self.events,class_name,post,services);
    }
    pub fn events(&self) -> &[Event] { &self.events }
    /// The ordered journal consumes notices after the stateful span. Clearing
    /// retains the event buffer; the next phase does not allocate it again.
    pub fn published(&mut self) { self.events.clear(); }
    // Emit immutable payloads before clearing the scratch queue. A later
    // Character reuse cannot change the team or class in an earlier notice.
    pub fn journal(&mut self, journal: &mut crate::store::calls::Events) {
        for &event in &self.events { crate::gen::HsEvent::from(event).emit(journal); }
        self.events.clear();
    }
}
