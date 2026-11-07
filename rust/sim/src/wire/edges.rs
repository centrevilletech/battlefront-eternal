//! Reliable edge receipt, execution translation and outcome retention.
//! Follows network-entity-edges.js and network-entity-edge-host.js.
use crate::gen::{WeCall, WeEdge, WeOutcome, WePending, WeResult};
use crate::store::{StrId, Strings};
use jsrt::{js_max, same_value_zero};

fn safe_integer(n: f64) -> bool { n.is_finite() && n.fract() == 0.0 && n.abs() <= 9007199254740991.0 }
fn named(strings: &Strings, id: StrId, name: &str) -> bool { strings.units(id).iter().copied().eq(name.encode_utf16()) }

#[derive(Default)]
pub struct Receiver {
    pub epoch: f64,
    pub generation: f64,
    pub capacity: f64,
    pub window: f64,
    pub max_age: f64,
    pub max_future: f64,
    pub queue: Vec<WeEdge>,
    pub seen: Vec<f64>,
    pub highest: f64,
}
impl Receiver {
    pub fn new(capacity: f64, window: f64, max_age: f64, max_future: f64) -> Self {
        Self { capacity, window, max_age, max_future, ..Self::default() }
    }
    pub fn reset(&mut self, epoch: f64, generation: f64) {
        self.epoch = epoch; self.generation = generation; self.queue.clear(); self.seen.clear(); self.highest = 0.0;
    }
    pub fn receive(&mut self, edges: &[WeEdge], tick: f64) -> (Vec<f64>, Vec<(f64, &'static str)>) {
        let mut receipts = Vec::new(); let mut rejected = Vec::new();
        for e in edges {
            let reason = if e.epoch != self.epoch || e.generation != self.generation { Some("life") }
                else if !safe_integer(e.id) || e.id < 1.0 || !safe_integer(e.tick) { Some("invalid") }
                else if self.seen.iter().any(|id| same_value_zero(*id, e.id)) { receipts.push(e.id); continue; }
                else if e.id <= self.highest - self.window { Some("outside-window") }
                else if e.tick < tick - self.max_age { Some("expired") }
                else if e.tick > tick + self.max_future { Some("future") }
                else if self.queue.len() as f64 >= self.capacity { Some("capacity") }
                else { None };
            if let Some(reason) = reason { rejected.push((e.id, reason)); continue; }
            if let Some(at) = self.queue.iter().position(|v| same_value_zero(v.id, e.id)) { self.queue[at] = e.clone(); }
            else { self.queue.push(e.clone()); }
            if !self.seen.iter().any(|id| same_value_zero(*id, e.id)) { self.seen.push(e.id); }
            self.highest = js_max(self.highest, e.id);
            self.seen.retain(|id| !(*id <= self.highest - self.window));
            receipts.push(e.id);
        }
        (receipts, rejected)
    }
    pub fn take(&mut self, tick: f64) -> Vec<WeEdge> {
        let mut out = Vec::new();
        self.queue.retain(|edge| { if edge.tick <= tick { out.push(edge.clone()); false } else { true } });
        jsrt::sort::js_sort(&mut out, &mut |a, b| { let d = a.tick - b.tick; if jsrt::num_truthy(d) { d } else { a.id - b.id } });
        out
    }
}

pub struct Host {
    pub epoch: f64,
    pub generation: f64,
    pub tick: f64,
    pub capacity: f64,
    pub receiver: Receiver,
    pub pending: Vec<WePending>,
    pub results: Vec<WeOutcome>,
    pub received: Vec<f64>,
    pub dropped_edges: f64,
    pub dropped_results: f64,
    pub action_serial: f64,
    pub fire_serial: f64,
}
impl Default for Host {
    fn default() -> Self {
        Self { epoch: 0., generation: 0., tick: 0., capacity: 256., receiver: Receiver::new(32., 256., 30., 120.),
            pending: Vec::new(), results: Vec::new(), received: Vec::new(), dropped_edges: 0., dropped_results: 0., action_serial: 0., fire_serial: 0. }
    }
}
impl Host {
    pub fn reset(&mut self, epoch: f64, generation: f64, actions: f64, fire: f64) {
        self.epoch = epoch; self.generation = generation; self.receiver.reset(epoch, generation);
        self.pending.clear(); self.results.clear(); self.received.clear(); self.tick = 0.;
        self.action_serial = actions; self.fire_serial = fire;
    }
    pub fn result(&mut self, id: f64, status: StrId, tick: f64) {
        let at = self.results.iter().position(|r| same_value_zero(r.id, id));
        if let Some(at) = at { self.results[at] = WeOutcome { id, status, tick }; }
        else {
            if self.results.len() as f64 >= self.capacity {
                if !self.results.is_empty() { self.results.remove(0); }
                self.dropped_results += 1.;
            }
            self.results.push(WeOutcome { id, status, tick });
        }
    }
    pub fn receive(&mut self, epoch: f64, generation: f64, edges: &[WeEdge], tick: f64, strings: &mut Strings) {
        self.tick = tick;
        if epoch != self.epoch || generation != self.generation { return; }
        if (self.pending.len() + self.receiver.queue.len() + edges.len()) as f64 > self.capacity {
            self.dropped_edges += edges.len() as f64; return;
        }
        let (receipts, rejected) = self.receiver.receive(edges, tick);
        for id in receipts { if !self.received.iter().any(|v| same_value_zero(*v, id)) { self.received.push(id); } }
        if self.received.len() > 32 { self.received.drain(..self.received.len() - 32); }
        for (id, reason) in rejected {
            let status = strings.intern_str(if reason == "expired" { "expired" } else { "rejected" });
            self.result(id, status, self.tick);
        }
    }
    pub fn begin(&mut self, tick: f64) -> Vec<WeEdge> { self.tick = tick; self.receiver.take(tick) }
    pub fn prepare(&mut self, edge: &WeEdge, tick: f64, actions: f64, fire: f64, strings: &mut Strings) -> Option<(bool, f64)> {
        if edge.tick < tick - 30. {
            let status = strings.intern_str("expired"); self.result(edge.id, status, self.tick); return None;
        }
        if named(strings, edge.kind, "fire") {
            self.fire_serial = js_max(self.fire_serial, fire) + 1.;
            Some((true, self.fire_serial))
        } else {
            self.action_serial = js_max(self.action_serial, actions) + 1.;
            if ["action", "spawn", "team", "suicide", "cancel"].iter().any(|kind| named(strings, edge.kind, kind)) {
                Some((false, self.action_serial))
            } else {
                let status = strings.intern_str("rejected"); self.result(edge.id, status, self.tick); None
            }
        }
    }
    pub fn finish(&mut self, fire: bool, id: f64, external_id: f64, accepted: bool, strings: &mut Strings) {
        if accepted {
            let pending = WePending { fire, id, external_id };
            if let Some(at) = self.pending.iter().position(|p| p.fire == fire && same_value_zero(p.id, id)) { self.pending[at] = pending; }
            else { self.pending.push(pending); }
        } else { let status = strings.intern_str("rejected"); self.result(external_id, status, self.tick); }
    }
    pub fn consumed(&mut self, fire: bool, outcomes: &[WeOutcome]) {
        for outcome in outcomes {
            if let Some(at) = self.pending.iter().position(|p| p.fire == fire && same_value_zero(p.id, outcome.id)) {
                let pending = self.pending.remove(at); self.result(pending.external_id, outcome.status, self.tick);
            }
        }
    }
    pub fn acknowledge(&mut self, epoch: f64, generation: f64, ids: &[f64]) {
        if epoch != self.epoch || generation != self.generation { return; }
        self.results.retain(|r| !ids.iter().any(|id| same_value_zero(*id, r.id)));
        self.received.retain(|r| !ids.iter().any(|id| same_value_zero(*id, *r)));
    }
    pub fn run(&mut self, c: WeCall, strings: &mut Strings) -> WeResult {
        self.capacity = c.capacity;
        let mut out = WeResult::default();
        match jsrt::to_uint32(c.op) {
            0 => self.reset(c.epoch, c.generation, c.action_received, c.fire_received),
            1 => self.receive(c.epoch, c.generation, &c.edges, c.tick, strings),
            2 => out.edges = self.begin(c.tick),
            3 => if let Some(edge) = &c.edge {
                if let Some((fire, id)) = self.prepare(edge, c.tick, c.action_received, c.fire_received, strings) { out.accepted = true; out.fire = fire; out.id = id; }
            },
            4 => self.finish(c.fire, c.id, c.external_id, c.accepted, strings),
            5 => self.tick = c.tick,
            6 => self.consumed(c.fire, &c.outcomes),
            7 => self.acknowledge(c.epoch, c.generation, &c.ids),
            8 => { out.received = self.received.clone(); out.results = self.results.iter().take(32).cloned().collect(); },
            9 => self.result(c.id, c.status, c.result_tick),
            10 => {
                out.received = self.received.clone(); out.results = self.results.clone(); out.pending = self.pending.clone();
                out.edges = self.receiver.queue.clone(); out.seen = self.receiver.seen.clone(); out.highest = self.receiver.highest;
            },
            _ => {},
        }
        out.epoch = self.epoch; out.generation = self.generation; out.tick = self.tick;
        out.action_serial = self.action_serial; out.fire_serial = self.fire_serial;
        out.dropped_edges = self.dropped_edges; out.dropped_results = self.dropped_results;
        out
    }
}
pub fn wire_edges_words() -> u32 { WeCall::WORDS as u32 }

pub(crate) fn wire_edges_create_in(w: &mut crate::world::owner::World) -> usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
 owner.edges.insert(Host::default()).unwrap_or(0)

}
/// # Safety
/// `host` belongs to this instance and has not been released.
pub(crate) fn wire_edges_drop_in(w: &mut crate::world::owner::World, host: usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.edges.get(host as usize).is_none() {return;}
 if host != 0 { owner.edges.remove(host as usize); }

}
/// # Safety
/// `host` remains live for this call. No import or callback runs here.
pub(crate) fn wire_edges_run_in(w: &mut crate::world::owner::World, host: usize) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.edges.get(host as usize).is_none() {return None;}

    let input = { let (row, heap) = w.sim.calls.args(); WeCall::take(row, heap) };
    let out = owner.edges.get_mut(host as usize).unwrap().run(input, &mut w.sim.strings);
    let s = &mut w.sim; s.calls.result(WeResult::WORDS, |row, heap| out.put(row, heap)); Some(&s.calls.block.words[..])

}

#[cfg(test)]
mod tests {
    use super::*;
    fn edge(strings: &mut Strings, id: f64, tick: f64) -> WeEdge {
        WeEdge { epoch: 1., generation: 3., id, tick, kind: strings.intern_str("action"),
            action: Some(strings.intern_str("jump")), ..WeEdge::default() }
    }
    #[test]
    fn receipt_is_not_execution_and_duplicate_age_does_not_change_receipts() {
        let mut strings = Strings::default(); let mut receiver = Receiver::new(2., 256., 30., 120.);
        receiver.reset(1., 3.);
        let edges = vec![edge(&mut strings, 9., 10.), edge(&mut strings, 8., 10.)];
        assert_eq!(receiver.receive(&edges, 10.), (vec![9., 8.], vec![]));
        assert_eq!(receiver.take(10.).iter().map(|e| e.id).collect::<Vec<_>>(), [8., 9.]);
        assert_eq!(receiver.receive(&edges, 100.), (vec![9., 8.], vec![]));
        assert!(receiver.take(100.).is_empty());
        let late = edge(&mut strings, 7., 1.);
        assert_eq!(receiver.receive(&[late], 100.).1, [(7., "expired")]);
        let future = edge(&mut strings, 10., 221.);
        assert_eq!(receiver.receive(&[future], 100.).1, [(10., "future")]);
    }
    #[test]
    fn translated_outcomes_preserve_insertion_eviction_and_reset_counters() {
        let mut strings = Strings::default(); let mut host = Host::default();
        host.reset(1., 3., 10., 5.); host.capacity = 2.;
        host.receive(1., 3., &[edge(&mut strings, 9., 10.), edge(&mut strings, 8., 10.)], 10., &mut strings);
        for e in host.begin(11.) {
            let (fire, id) = host.prepare(&e, 11., 10., 5., &mut strings).unwrap();
            host.finish(fire, id, e.id, true, &mut strings);
        }
        assert_eq!(host.pending.iter().map(|p| (p.id, p.external_id)).collect::<Vec<_>>(), [(11., 8.), (12., 9.)]);
        assert!(host.results.is_empty());
        let consumed = strings.intern_str("consumed");
        host.consumed(false, &[WeOutcome { id: 12., status: consumed, tick: 99. }]);
        assert_eq!(host.results[0].id, 9.); assert_eq!(host.results[0].tick, 11.);
        host.result(20., consumed, 12.); host.result(9., consumed, 13.); host.result(21., consumed, 14.);
        assert_eq!(host.results.iter().map(|r| r.id).collect::<Vec<_>>(), [20., 21.]);
        assert_eq!(host.dropped_results, 1.);
        host.acknowledge(1., 4., &[20.]); assert_eq!(host.results.len(), 2);
        host.acknowledge(1., 3., &[20.]); assert_eq!(host.results.len(), 1);
        host.reset(1., 4., 2., 3.); assert_eq!(host.dropped_results, 1.); assert!(host.pending.is_empty());
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_edges_words() -> u32 { super::wire_edges_words() }
    #[no_mangle]
    pub extern "C" fn wire_edges_create() -> *mut Host { wire_edges_create_in(crate::abi::facade::world()) as *mut Host }
    #[no_mangle]
    pub extern "C" fn wire_edges_drop(host: *mut Host) { wire_edges_drop_in(crate::abi::facade::world(), host as usize) }
    #[no_mangle]
    pub extern "C" fn wire_edges_run(host: *mut Host) -> *const f64 { wire_edges_run_in(crate::abi::facade::world(), host as usize).map_or(std::ptr::null(), |b| b.as_ptr()) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{wire_edges_create, wire_edges_drop, wire_edges_run};
