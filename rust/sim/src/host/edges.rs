//! Each client's entity edge runtime (`WireEntityEdges`,
//! src/runtime/wire-edges.js over `wire_edges_run`) as `snapshot-host.js`
//! drives it around the step: `advanceEdges()` before it (a reset when the
//! actor's unit life moved since the runtime last saw it, then
//! `advance(worldClock.tick + 1)`), `consumedEdges(tick)` after it (op 5,
//! then each lane's drained outcomes as op 6).
use super::{block_row, Authority, Client};
use crate::abi::context::Context;
use crate::gen::{self, BlockHeader as B, PlayerLanes, WeCall, WeOutcome, WeResult};
use crate::store::StrId;

/// `capacity` of `new EntityEdges(...)`.
const EDGE_CAPACITY: f64 = 256.0;

/// A `WeCall` of operation `op` with every other field empty.
fn call(op: f64) -> WeCall {
    WeCall {
        op,
        epoch: 0.0,
        generation: 0.0,
        tick: 0.0,
        capacity: EDGE_CAPACITY,
        action_received: 0.0,
        fire_received: 0.0,
        edges: Vec::new(),
        ids: Vec::new(),
        edge: None,
        outcomes: Vec::new(),
        fire: false,
        id: 0.0,
        external_id: 0.0,
        accepted: false,
        status: StrId(0),
        result_tick: 0.0,
    }
}

/// `LaneView.read(drain)`: a player's lane counters and, when draining,
/// its outcomes.
fn lanes(context: &Context, player: u32, lane: u32, drain: bool) -> Result<PlayerLanes, String> {
    context
        .players_lanes(player, lane, u32::from(drain), |words| {
            words.map(|w| {
                let (row, heap) = block_row(w);
                PlayerLanes::take(row, heap)
            })
        })
        .map_err(|e| e.message().to_string())?
        .ok_or_else(|| "unknown player".to_string())
}

/// `p.unit.generation` as the JavaScript reads it live: the unit mirror's
/// life count, from the store's unit export (0 without a body).
fn unit_generation(context: &Context, unit: Option<u32>) -> Result<f64, String> {
    let Some(slot) = unit else { return Ok(0.0) };
    let units = context
        .export(u32::from(gen::KIND_UNIT), 0)
        .map_err(|e| e.message().to_string())?;
    let stride = units[B::STRIDE] as usize;
    let rows = units[B::ROWS] as usize;
    for i in 0..rows {
        let row = &units[B::WORDS + i * stride..B::WORDS + (i + 1) * stride];
        if row[0] == f64::from(slot) {
            // Unit::take_export reads `generation` first, at word 2.
            return Ok(row[2]);
        }
    }
    Err(format!("a client's unit (slot {slot}) is not in the store"))
}

impl Authority {
    /// `WireEntityEdges.run(op, args)` for one client; its remembered
    /// generation follows every result.
    fn edges_run(
        &mut self,
        context: &Context,
        client: usize,
        call: &WeCall,
    ) -> Result<WeResult, String> {
        let edges = f64::from(self.clients[client].edges);
        self.args
            .record(&[edges], WeCall::WORDS, |row, heap| call.put(row, heap));
        self.args.run(context, gen::OPERATION_WIRE_EDGES_RUN)?;
        let (row, heap) = self.args.answer();
        let result = WeResult::take(row, heap);
        self.clients[client].generation = result.generation;
        Ok(result)
    }

    /// `snapshot-host.js` `add`: the client's runtime generation from its
    /// `feedback()` (op 8, a read), and his body as his connection's view
    /// names it.
    pub(super) fn add_client(
        &mut self,
        context: &Context,
        player: u32,
        edges: u32,
    ) -> Result<(), String> {
        let unit = context
            .world(|w| {
                let character = w
                    .input
                    .players
                    .pool
                    .get(player as usize)?
                    .character
                    .clone()?;
                crate::battle::f4_connections::wire_view_in(&w.battle, &w.sim.store, &character)?
                    .unit
                    .map(|u| u.slot)
            })
            .map_err(|e| e.message().to_string())?;
        self.clients.retain(|c| c.player != player);
        self.clients.push(Client {
            player,
            edges,
            generation: 0.0,
            unit,
            presses: [None, None],
        });
        let index = self.clients.len() - 1;
        self.edges_run(context, index, &call(8.0))?;
        Ok(())
    }

    /// `host.advanceEdges()` for every client. A taken edge would be
    /// dispatched to the player's fire or actions lane (`EntityEdges.advance`);
    /// that dispatch is not native yet, so a window holding one fails the
    /// tick instead of dropping the client's input.
    pub(super) fn advance_edges(&mut self, context: &Context, tick: f64) -> Result<(), String> {
        for index in 0..self.clients.len() {
            let unit_generation = unit_generation(context, self.clients[index].unit)?;
            if unit_generation != self.clients[index].generation {
                let player = self.clients[index].player;
                let mut reset = call(0.0);
                reset.epoch = 1.0;
                reset.generation = unit_generation;
                reset.action_received = lanes(context, player, 0, false)?.actions_received;
                reset.fire_received = lanes(context, player, 1, false)?.fire_received;
                self.edges_run(context, index, &reset)?;
            }
            let mut advance = call(2.0);
            advance.tick = tick;
            let out = self.edges_run(context, index, &advance)?;
            if !out.edges.is_empty() {
                return Err("entity input edge dispatch is not native yet".into());
            }
        }
        Ok(())
    }

    /// `host.consumedEdges(tick)` for every client: op 5, then each lane's
    /// drained outcomes as op 6 (actions first, then fire), as
    /// `WireEntityEdges.consumed`.
    pub(super) fn consumed_edges(&mut self, context: &Context, tick: f64) -> Result<(), String> {
        for index in 0..self.clients.len() {
            let player = self.clients[index].player;
            let mut consumed = call(5.0);
            consumed.tick = tick;
            self.edges_run(context, index, &consumed)?;
            for lane in 0..2u32 {
                let outcomes = lanes(context, player, lane, true)?.outcomes;
                let mut outcome = call(6.0);
                outcome.fire = lane == 1;
                // `outcome(value)` copies id and status; the tick is undefined.
                outcome.outcomes = outcomes
                    .iter()
                    .map(|o| WeOutcome {
                        id: o.id,
                        status: o.status,
                        tick: f64::NAN,
                    })
                    .collect();
                self.edges_run(context, index, &outcome)?;
            }
        }
        Ok(())
    }
}
