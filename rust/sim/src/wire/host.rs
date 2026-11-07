//! Snapshot publication policy and per-client retained own state. Time is
//! supplied by the host; these methods never read a wall clock or call JS.
use super::{codec, ring::SnapshotRing};
use crate::gen::{WireBaselineChoice, WireField, WireHostOwn};
use jsrt::{js_max, js_min, num_or};

pub fn choose_baselines(tick: f64, acks: &[Vec<f64>], max: f64) -> Vec<f64> {
    let mut holders: Vec<(f64, Vec<usize>)> = Vec::new();
    for (i, ack) in acks.iter().enumerate() {
        for &t in ack {
            if let Some((_, indices)) = holders
                .iter_mut()
                .find(|(key, _)| jsrt::same_value_zero(*key, t))
            {
                indices.push(i);
            } else {
                holders.push((t, vec![i]));
            }
        }
    }
    holders.sort_by(|a, b| {
        (b.0 - a.0)
            .partial_cmp(&0.)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if holders.len() as f64 <= max {
        return holders.iter().map(|x| x.0).collect();
    }
    let mut costs = vec![1024.; acks.len()];
    let mut chosen = Vec::new();
    while (chosen.len() as f64) < max {
        let mut best = -1.;
        let mut best_gain = 0.;
        for (candidate, indices) in &holders {
            let age = tick - candidate;
            let mut gain = 0.;
            for &i in indices {
                if costs[i] > age {
                    gain += costs[i] - age;
                }
            }
            if gain > best_gain {
                best = *candidate;
                best_gain = gain;
            }
        }
        if best < 0. {
            break;
        }
        chosen.push(best);
        for &i in &holders.iter().find(|(t, _)| *t == best).unwrap().1 {
            costs[i] = js_min(costs[i], tick - best);
        }
    }
    chosen.sort_by(|a, b| {
        (b - a)
            .partial_cmp(&0.)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    chosen
}
#[derive(Clone, Default)]
pub struct Own {
    pub payload: Option<Vec<u8>>,
    pub summaries: Vec<WireField>,
}
pub struct Client {
    pub id: u32,
    pub started: bool,
    pub ready: bool,
    pub acked: Vec<f64>,
    pub baseline: f64,
    pub own: SnapshotRing<Own>,
    pub publication: Option<super::publisher::Personal>,
    pub sent: Vec<(f64, f64)>,
    pub sent_bytes: f64,
    pub own_sent_at: f64,
    pub heard_at: f64,
    pub summaries_due: f64,
    pub has_summaries: bool,
    pub metrics: [f64; 10],
}
impl Client {
    fn new(id: u32, at: f64) -> Self {
        Self {
            id,
            started: false,
            ready: false,
            acked: Vec::with_capacity(9),
            baseline: 0.,
            own: SnapshotRing::default(),
            publication: None,
            sent: Vec::with_capacity(2048),
            sent_bytes: 0.,
            own_sent_at: f64::NEG_INFINITY,
            heard_at: at,
            summaries_due: 0.,
            has_summaries: false,
            metrics: [0.; 10],
        }
    }
    pub fn budget_used(&mut self, at: f64) -> f64 {
        let mut drop = 0;
        while drop < self.sent.len() && self.sent[drop].0 <= at - 1000. {
            self.sent_bytes -= self.sent[drop].1;
            drop += 1;
        }
        if drop > 0 {
            self.sent.drain(..drop);
        }
        self.sent_bytes
    }
    pub fn charge(&mut self, at: f64, bytes: f64) {
        charge(&mut self.sent, &mut self.sent_bytes, at, bytes);
    }
}
/// A publication uses one clock sample for all packet charges. Coalescing
/// those charges preserves budgetUsed's window while bounding catch-up work.
pub fn charge(sent: &mut Vec<(f64, f64)>, used: &mut f64, at: f64, bytes: f64) {
    if let Some(last) = sent.last_mut().filter(|last| last.0 == at) { last.1 += bytes; }
    else {
        // An invalid clock can submit arbitrarily many distinct instants
        // inside a second. Merge oldest debt conservatively if it fills the
        // reserved window; packets remain owed rather than growing a frame.
        if sent.len() == sent.capacity() { let old = sent.remove(0); if let Some(next) = sent.first_mut() { next.1 += old.1; } }
        sent.push((at, bytes));
    }
    *used += bytes;
}
pub struct Host {
    pub local: bool,
    pub interval: f64,
    pub budget: f64,
    pub stale_after: f64,
    pub epoch: f64,
    pub captured_tick: f64,
    pub ring: SnapshotRing<()>,
    pub clients: Vec<Client>,
}
impl Host {
    pub fn new(local: bool, budget: f64, stale_after: f64, epoch: f64) -> Self {
        Self {
            local,
            interval: if local { 1. } else { 2. },
            budget: if local { f64::INFINITY } else { budget },
            stale_after,
            epoch,
            captured_tick: -1.,
            ring: SnapshotRing::default(),
            clients: Vec::with_capacity(256),
        }
    }
    pub fn client(&self, id: u32) -> &Client {
        self.clients
            .iter()
            .find(|c| c.id == id)
            .expect("registered snapshot client")
    }
    pub fn client_mut(&mut self, id: u32) -> &mut Client {
        self.clients
            .iter_mut()
            .find(|c| c.id == id)
            .expect("registered snapshot client")
    }
    pub fn add(&mut self, id: u32, at: f64) {
        assert!(!self.clients.iter().any(|c| c.id == id));
        self.clients.push(Client::new(id, at));
    }
    pub fn remove(&mut self, id: u32) {
        self.clients.retain(|c| c.id != id);
    }
    pub fn set_interval(&mut self, ticks: f64) {
        if self.local {
            self.interval = js_max(1., js_min(3., num_or(ticks.floor(), 1.)));
        }
    }
    pub fn due(&self, tick: f64) -> bool {
        if self.captured_tick < 0. {
            tick % self.interval == 0.
        } else {
            tick - self.captured_tick >= self.interval
        }
    }
    pub fn captured(&mut self, tick: f64) -> codec::Result<()> {
        self.ring.set(tick as i64, self.epoch as u32, ())?;
        self.captured_tick = tick;
        Ok(())
    }
    pub fn accept_ack(&mut self, id: u32, tick: f64, epoch: f64) -> bool {
        if epoch != self.epoch
            || !tick.is_finite()
            || tick.fract() != 0.
            || tick.abs() > 9007199254740991.
        {
            return false;
        }
        if !self.ring.has(tick as i64, Some(epoch as u32)) {
            return false;
        }
        let c = self.client_mut(id);
        let held = c.publication.as_ref().map_or_else(|| c.own.has(tick as i64, Some(epoch as u32)), |p| p.ring.has(tick, epoch));
        if tick > c.baseline && held {
            c.acked.insert(0, tick);
            c.acked.truncate(8);
            c.baseline = tick;
            if let Some(p) = &mut c.publication { p.outbox.acknowledge(tick); }
            true
        } else {
            false
        }
    }
    pub fn held(&mut self, id: u32, tick: f64) -> Vec<f64> {
        let ring = &self.ring;
        let epoch = self.epoch;
        let c = self.clients.iter_mut().find(|c| c.id == id).unwrap();
        c.acked.retain(|a| {
            !(tick - a >= 128.
                || !ring.has(*a as i64, Some(epoch as u32))
                || !c.own.has(*a as i64, Some(epoch as u32)))
        });
        c.acked.clone()
    }
    pub fn resync(&mut self, id: u32) {
        let c = self.client_mut(id);
        c.acked.clear();
        c.baseline = 0.;
        if let Some(p) = &mut c.publication { p.outbox.resend(); }
    }
    pub fn eligible(&mut self, id: u32, at: f64, snapshot: bool) -> bool {
        let stale = self.stale_after;
        let c = self.client_mut(id);
        if !snapshot || !c.ready {
            return false;
        }
        if at - c.heard_at > stale {
            c.metrics[9] += 1.;
            return false;
        }
        true
    }
    /// 2: full snapshot, 1: own-only fallback, 0: skip this publication.
    pub fn delivery(&mut self, id: u32, at: f64) -> u32 {
        let budget = self.budget;
        let c = self.client_mut(id);
        if c.budget_used(at) < budget {
            2
        } else {
            c.metrics[7] += 1.;
            if at - c.own_sent_at >= 100. {
                1
            } else {
                0
            }
        }
    }
    pub fn stale(&self, at: f64, seconds: f64) -> Vec<f64> {
        self.clients
            .iter()
            .filter(|c| c.started && at - c.heard_at >= seconds * 1000.)
            .map(|c| c.id as f64)
            .collect()
    }
    pub fn prepare_own(&mut self, id: u32, input: WireHostOwn) -> codec::Result<Vec<Vec<u8>>> {
        let c = self.client_mut(id);
        let base = c
            .own
            .get(input.baseline_tick as i64, Some(input.epoch as u32))
            .cloned();
        let own = Own {
            payload: input
                .correction
                .as_ref()
                .map(|c| c.payload.clone())
                .or_else(|| base.as_ref().and_then(|b| b.payload.clone())),
            summaries: input.summaries,
        };
        c.own
            .set(input.tick as i64, input.epoch as u32, own.clone())?;
        let delta = match &input.correction {
            Some(c) => {
                codec::word_delta(&c.payload, base.as_ref().and_then(|b| b.payload.as_deref()))?
            }
            None => Vec::new(),
        };
        let summaries =
            codec::world_section(&own.summaries, base.as_ref().map_or(&[], |b| &b.summaries))?;
        Ok(vec![delta, summaries])
    }
}

// The former token-budget transport profile remains usable by native hosts.
pub struct EntityBudget {
    pub rate: f64,
    pub burst: f64,
    pub tokens: f64,
    pub at: f64,
    pub offered: f64,
    pub denied: f64,
}
impl EntityBudget {
    pub fn new(rate: f64, burst: f64, at: f64) -> codec::Result<Self> {
        if !rate.is_finite()
            || rate <= 0.
            || !burst.is_finite()
            || burst.fract() != 0.
            || burst < 1100.
            || burst > 9007199254740991.
        {
            return Err("invalid entity budget");
        }
        Ok(Self {
            rate,
            burst,
            tokens: burst,
            at,
            offered: 0.,
            denied: 0.,
        })
    }
    pub fn available(&mut self, now: f64) -> codec::Result<f64> {
        if !now.is_finite() {
            return Err("invalid budget clock");
        }
        self.tokens = js_min(
            self.burst,
            self.tokens + js_max(0., now - self.at) * self.rate / 1000.,
        );
        self.at = js_max(self.at, now);
        Ok(self.tokens.floor())
    }
    pub fn take(&mut self, bytes: f64, now: f64) -> codec::Result<bool> {
        validate_size(bytes)?;
        if self.available(now)? < bytes {
            self.denied += 1.;
            return Ok(false);
        }
        self.tokens -= bytes;
        self.offered += bytes;
        Ok(true)
    }
    pub fn refund(&mut self, bytes: f64, now: f64) -> codec::Result<()> {
        validate_size(bytes)?;
        self.available(now)?;
        self.tokens = js_min(self.burst, self.tokens + bytes);
        self.offered = js_max(0., self.offered - bytes);
        Ok(())
    }
}
fn validate_size(bytes: f64) -> codec::Result<()> {
    if !bytes.is_finite() || bytes.fract() != 0. || !(1. ..=1100.).contains(&bytes) {
        Err("invalid entity message size")
    } else {
        Ok(())
    }
}

use crate::{
    gen::{WireHostList, WireHostOwn as OwnInput},
};
fn list(s: &mut crate::abi::Sim, values: Vec<f64>) -> Option<&[f64]> {
    let out = WireHostList { values };
    s.calls
        .result(WireHostList::WORDS, |row, heap| out.put(row, heap));
    Some(&s.calls.block.words[..])
}
pub(crate) fn wire_host_create_in(w: &mut crate::world::owner::World, local: u32, budget: f64, stale: f64, epoch: f64) -> usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");

    owner.hosts.insert(Host::new(local != 0, budget, stale, epoch)).unwrap_or(0)

}
pub(crate) fn wire_host_drop_in(w: &mut crate::world::owner::World, p: usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return;}

    if p != 0 {
        owner.hosts.remove(p as usize);
    }

}
pub fn wire_host_words(op: u32) -> u32 {
    match op {
        0 => WireBaselineChoice::WORDS as u32,
        1 => OwnInput::WORDS as u32,
        _ => 0,
    }
}

pub(crate) fn wire_baselines_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let a = {
        let (row, heap) = w.sim.calls.args();
        WireBaselineChoice::take(row, heap)
    };
    list(&mut w.sim, choose_baselines(
        a.tick,
        &a.acks.into_iter().map(|x| x.ticks).collect::<Vec<_>>(),
        a.max,
    ))
}
pub(crate) fn wire_host_get_in(w: &mut crate::world::owner::World, p: usize, id: u32, field: u32) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return f64::NAN;}

    let h = owner.hosts.get(p as usize).unwrap();
    if field == 0 {
        return h.interval;
    }
    if field == 1 {
        return h.ring.newest_tick() as f64;
    }
    let c = h.client(id);
    match field {
        2 => c.started as u8 as f64,
        3 => c.ready as u8 as f64,
        4 => c.baseline,
        5 => c.own_sent_at,
        6 => c.heard_at,
        7 => c.summaries_due,
        8 => c.has_summaries as u8 as f64,
        10..=19 => c.metrics[(field - 10) as usize],
        _ => f64::NAN,
    }

}
pub(crate) fn wire_host_set_in(w: &mut crate::world::owner::World, p: usize, id: u32, field: u32, value: f64) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return;}

    let h = owner.hosts.get_mut(p as usize).unwrap();
    if field == 0 {
        h.set_interval(value);
        return;
    }
    let c = h.client_mut(id);
    match field {
        2 => c.started = value != 0.,
        3 => c.ready = value != 0.,
        5 => c.own_sent_at = value,
        6 => c.heard_at = value,
        7 => c.summaries_due = value,
        8 => c.has_summaries = value != 0.,
        10..=19 => c.metrics[(field - 10) as usize] = value,
        _ => {}
    }

}
pub(crate) fn wire_host_op_in(w: &mut crate::world::owner::World, p: usize, op: u32, id: u32, a: f64, b: f64) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return f64::NAN;}

    let h = owner.hosts.get_mut(p as usize).unwrap();
    match op {
        0 => {
            h.add(id, a);
            0.
        }
        1 => {
            h.remove(id);
            0.
        }
        2 => h.due(a) as u8 as f64,
        3 => {
            h.captured(a).expect("safe snapshot tick");
            0.
        }
        4 => h.accept_ack(id, a, b) as u8 as f64,
        5 => h.client_mut(id).budget_used(a),
        6 => {
            h.client_mut(id).charge(a, b);
            0.
        }
        7 => h.eligible(id, a, b != 0.) as u8 as f64,
        8 => h.delivery(id, a) as f64,
        9 => {
            h.resync(id);
            0.
        }
        _ => f64::NAN,
    }

}
pub(crate) fn wire_host_held_in(w: &mut crate::world::owner::World, p: usize, id: u32, tick: f64) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return None;}

    list(&mut w.sim, (owner.hosts.get_mut(p as usize).unwrap()).held(id, tick))

}
pub(crate) fn wire_host_stale_in(w: &mut crate::world::owner::World, p: usize, at: f64, seconds: f64) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return None;}

    list(&mut w.sim, (owner.hosts.get(p as usize).unwrap()).stale(at, seconds))

}
pub(crate) fn wire_host_own_in(w: &mut crate::world::owner::World, p: usize, id: u32) -> Option<&[f64]> {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.hosts.get(p as usize).is_none() {return None;}

    let a = {
        let (row, heap) = w.sim.calls.args();
        OwnInput::take(row, heap)
    };
    super::module::output(&mut w.sim, (owner.hosts.get_mut(p as usize).unwrap()).prepare_own(id, a))

}

pub(crate) fn wire_budget_create_in(w: &mut crate::world::owner::World, rate: f64, burst: f64, at: f64) -> usize {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");

    match EntityBudget::new(rate, burst, at) {
        Ok(value) => owner.budgets.insert(value).unwrap_or(0),
        Err(_) => 0,
    }

}
pub(crate) fn wire_budget_drop_in(w: &mut crate::world::owner::World, p: usize) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.budgets.get(p as usize).is_none() {return;}

    if p != 0 {
        owner.budgets.remove(p as usize);
    }

}
pub(crate) fn wire_budget_get_in(w: &mut crate::world::owner::World, p: usize, field: u32) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.budgets.get(p as usize).is_none() {return f64::NAN;}

    let b = owner.budgets.get(p as usize).unwrap();
    match field {
        0 => b.rate,
        1 => b.burst,
        2 => b.tokens,
        3 => b.at,
        4 => b.offered,
        5 => b.denied,
        _ => f64::NAN,
    }

}
pub(crate) fn wire_budget_set_in(w: &mut crate::world::owner::World, p: usize, field: u32, value: f64) {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.budgets.get(p as usize).is_none() {return;}

    let b = owner.budgets.get_mut(p as usize).unwrap();
    match field {
        0 => b.rate = value,
        1 => b.burst = value,
        2 => b.tokens = value,
        3 => b.at = value,
        4 => b.offered = value,
        5 => b.denied = value,
        _ => {}
    }

}
pub(crate) fn wire_budget_run_in(w: &mut crate::world::owner::World, p: usize,
    op: u32,
    bytes: f64,
    at: f64,) -> f64 {
    let owner = w.wire.as_deref_mut().expect("admitted wire owner");
if owner.budgets.get(p as usize).is_none() {return f64::NAN;}

    if op != 0 && validate_size(bytes).is_err() {
        return -1.;
    }
    if !at.is_finite() {
        return -2.;
    }
    let b = owner.budgets.get_mut(p as usize).unwrap();
    match op {
        0 => b.available(at).unwrap(),
        1 => b.take(bytes, at).unwrap() as u8 as f64,
        2 => {
            b.refund(bytes, at).unwrap();
            0.
        }
        _ => f64::NAN,
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ack_history_requires_both_rings_and_keeps_the_legacy_age_limit() {
        let mut h = Host::new(false, 20000., 5000., 3.);
        h.add(1, 0.);
        h.add(2, 0.);
        for tick in (2..=300).step_by(2) {
            let tick = tick as f64;
            h.captured(tick).unwrap();
            h.prepare_own(
                1,
                WireHostOwn {
                    tick,
                    epoch: 3.,
                    baseline_tick: tick - 2.,
                    correction: None,
                    summaries: vec![],
                },
            )
            .unwrap();
            assert!(!h.accept_ack(2, tick, 3.));
            assert!(!h.accept_ack(1, tick, 2.));
            assert!(h.accept_ack(1, tick, 3.));
            assert!(!h.accept_ack(1, tick, 3.));
        }
        assert_eq!(
            h.held(1, 300.),
            [300., 298., 296., 294., 292., 290., 288., 286.]
        );
        assert_eq!(h.held(1, 426.), [300.]);
        assert!(h.held(1, 428.).is_empty());
        h.resync(1);
        assert_eq!(h.client(1).baseline, 0.);
        assert!(!h.accept_ack(1, 302., 3.));
    }
    #[test]
    fn capture_gaps_budget_window_and_stale_boundaries_are_independent() {
        let mut h = Host::new(false, 20000., 5000., 1.);
        h.add(1, 10.);
        assert!(!h.due(1.));
        assert!(h.due(2.));
        h.captured(5.).unwrap();
        assert!(!h.due(6.));
        assert!(h.due(7.));
        h.client_mut(1).ready = true;
        assert!(h.eligible(1, 5010., true));
        assert!(!h.eligible(1, 5010.1, true));
        h.client_mut(1).charge(1., 20000.);
        assert_eq!(h.delivery(1, 1.), 1);
        h.client_mut(1).own_sent_at = 1.;
        assert_eq!(h.delivery(1, 100.), 0);
        assert_eq!(h.delivery(1, 101.), 1);
        assert_eq!(h.delivery(1, 1001.), 2);
        h.client_mut(1).charge(2000., 10.);
        h.client_mut(1).charge(1990., 20.);
        assert_eq!(h.client_mut(1).budget_used(2995.), 30.);
        assert_eq!(h.client_mut(1).budget_used(3000.), 0.);
        assert_eq!(h.client(1).metrics[7], 3.);
        assert_eq!(h.client(1).metrics[9], 1.);
        h.client_mut(1).started = true;
        assert_eq!(h.stale(5010., 5.), [1.]);
    }
    #[test]
    fn greedily_shared_baselines_count_each_client_and_break_ties_newest() {
        let acks = vec![vec![99., 97.], vec![98., 97.], vec![96.], vec![]];
        assert_eq!(choose_baselines(100., &acks, 1.), [97.]);
        assert_eq!(choose_baselines(100., &acks, 2.), [97., 96.]);
        assert_eq!(choose_baselines(100., &acks, 3.), [99., 97., 96.]);
        assert_eq!(choose_baselines(100., &[vec![99.], vec![98.]], 1.), [99.]);
    }
    #[test]
    fn transport_tokens_refund_only_offered_bytes_and_do_not_rewind_the_clock() {
        let mut b = EntityBudget::new(1000., 2200., 10.).unwrap();
        assert!(b.take(1100., 10.).unwrap());
        assert!(b.take(1100., 10.).unwrap());
        assert!(!b.take(1., 10.).unwrap());
        assert_eq!(b.available(11.5).unwrap(), 1.);
        assert_eq!(b.available(0.).unwrap(), 1.);
        b.refund(1000., 0.).unwrap();
        assert_eq!(b.tokens, 1001.5);
        assert_eq!(b.offered, 1200.);
        assert_eq!(b.denied, 1.);
        assert_eq!(b.take(0., f64::NAN), Err("invalid entity message size"));
        assert_eq!(b.take(1., f64::NAN), Err("invalid budget clock"));
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn wire_host_words(op: u32) -> u32 { super::wire_host_words(op) }
    #[no_mangle]
    pub extern "C" fn wire_host_create(local: u32, budget: f64, stale: f64, epoch: f64) -> *mut Host { wire_host_create_in(crate::abi::facade::world(), local, budget, stale, epoch) as *mut Host }
    #[no_mangle]
    pub extern "C" fn wire_host_drop(p: *mut Host) { wire_host_drop_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    pub extern "C" fn wire_baselines() -> *const f64 { wire_baselines_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn wire_host_get(p: *const Host, id: u32, field: u32) -> f64 { wire_host_get_in(crate::abi::facade::world(), p as usize, id, field) }
    #[no_mangle]
    pub extern "C" fn wire_host_set(p: *mut Host, id: u32, field: u32, value: f64) { wire_host_set_in(crate::abi::facade::world(), p as usize, id, field, value) }
    #[no_mangle]
    pub extern "C" fn wire_host_op(p: *mut Host, op: u32, id: u32, a: f64, b: f64) -> f64 { wire_host_op_in(crate::abi::facade::world(), p as usize, op, id, a, b) }
    #[no_mangle]
    pub extern "C" fn wire_host_held(p: *mut Host, id: u32, tick: f64) -> *const f64 { wire_host_held_in(crate::abi::facade::world(), p as usize, id, tick).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn wire_host_stale(p: *const Host, at: f64, seconds: f64) -> *const f64 { wire_host_stale_in(crate::abi::facade::world(), p as usize, at, seconds).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn wire_host_own(p: *mut Host, id: u32) -> *const f64 { wire_host_own_in(crate::abi::facade::world(), p as usize, id).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn wire_budget_create(rate: f64, burst: f64, at: f64) -> *mut EntityBudget { wire_budget_create_in(crate::abi::facade::world(), rate, burst, at) as *mut EntityBudget }
    #[no_mangle]
    pub extern "C" fn wire_budget_drop(p: *mut EntityBudget) { wire_budget_drop_in(crate::abi::facade::world(), p as usize) }
    #[no_mangle]
    pub extern "C" fn wire_budget_get(p: *const EntityBudget, field: u32) -> f64 { wire_budget_get_in(crate::abi::facade::world(), p as usize, field) }
    #[no_mangle]
    pub extern "C" fn wire_budget_set(p: *mut EntityBudget, field: u32, value: f64) { wire_budget_set_in(crate::abi::facade::world(), p as usize, field, value) }
    #[no_mangle]
    pub extern "C" fn wire_budget_run(p: *mut EntityBudget,
        op: u32,
        bytes: f64,
        at: f64,) -> f64 { wire_budget_run_in(crate::abi::facade::world(), p as usize, op, bytes, at) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{wire_host_create, wire_host_drop, wire_baselines, wire_host_get, wire_host_set, wire_host_op, wire_host_held, wire_host_stale, wire_host_own, wire_budget_create, wire_budget_drop, wire_budget_get, wire_budget_set, wire_budget_run};
