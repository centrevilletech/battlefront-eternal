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

/// EntityEdges.advance's world.fire/world.actions boundary. The argument
/// buffers are retained by Authority; an empty tick allocates no command.
#[allow(clippy::too_many_arguments)]
fn dispatch(
    context: &Context,
    player: u32,
    edge: &gen::WeEdge,
    id: f64,
    fire: bool,
    epoch: f64,
    tick: f64,
    at: f64,
    source: Option<u32>,
    actions: &mut gen::PlayerActions,
) -> Result<bool, String> {
    use gen::{HsCommand, HsCommandKind, IaHuman, IfEdge, PlayerFire};
    let named = |name: StrId, text: &str| {
        context
            .world(|w| {
                w.sim
                    .strings
                    .units(name)
                    .iter()
                    .copied()
                    .eq(text.encode_utf16())
            })
            .map_err(|e| e.message().to_string())
    };
    if fire {
        let secondary = edge
            .button
            .map(|button| named(button, "fireSecondary"))
            .transpose()?
            .unwrap_or(false);
        return context
            .players_fire(
                player,
                &PlayerFire {
                    edge: IfEdge {
                        id,
                        external_id: Some(edge.id),
                        tick: edge.tick,
                        generation: edge.generation,
                        button: f64::from(u8::from(secondary)),
                        pressed: edge.pressed.unwrap_or(false),
                        aim_yaw: edge.aim_yaw,
                        aim_pitch: edge.aim_pitch,
                        view_tick: edge.view_tick,
                    },
                    epoch_one: edge.epoch == epoch,
                    pressed_valid: edge.pressed.is_some(),
                    tick,
                    at,
                },
            )
            .map(|accepted| accepted != 0)
            .map_err(|e| e.message().to_string());
    }
    actions.entry.id = id;
    actions.entry.tick = tick;
    actions.entry.generation = edge.generation;
    actions.entry.actions.clear();
    actions.human.clear();
    actions.same_epoch = edge.epoch == epoch;
    actions.payload_valid = true;
    actions.human_valid = true;
    actions.tick = tick;
    actions.at = at;
    if named(edge.kind, "action")? {
        let Some(action) = edge.action else {
            return Ok(false);
        };
        actions.entry.actions.push(action);
    } else {
        let mut command = HsCommand::default();
        if named(edge.kind, "spawn")? {
            // classId is required; postId zero means the ordinary automatic
            // post choice. Unknown ids reject without becoming a pending edge.
            let names = context
                .world(|w| {
                    let capture = &w.wire.as_deref()?.sources.get(source? as usize)?.capture;
                    let class = capture.edge_name(edge.class_id?, true)?;
                    if w.sim.strings.units(class).len() > 64 {
                        return None;
                    }
                    let post = match edge.post_id.filter(|&id| id != 0.) {
                        Some(id) => {
                            let post = capture.edge_name(id, false)?;
                            if w.sim.strings.units(post).len() > 64 {
                                return None;
                            }
                            Some(post)
                        }
                        None => None,
                    };
                    Some((class, post))
                })
                .map_err(|e| e.message().to_string())?;
            let Some((class, post)) = names else {
                return Ok(false);
            };
            command.kind = HsCommandKind::Spawn;
            command.class_name = Some(class);
            command.post = post;
        } else if named(edge.kind, "team")? {
            command.kind = HsCommandKind::Team;
            command.team = edge.team.unwrap_or(f64::NAN);
        } else if named(edge.kind, "suicide")? {
            command.kind = HsCommandKind::Suicide;
        } else if named(edge.kind, "cancel")? {
            command.kind = HsCommandKind::Cancel;
        } else {
            return Ok(false);
        }
        actions.human.push(IaHuman {
            command,
            valid: true,
        });
    }
    context
        .players_actions(player, actions)
        .map(|answer| answer.is_some_and(|value| value.accepted))
        .map_err(|e| e.message().to_string())
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
        self.clients[client].epoch = result.epoch;
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
            epoch: 0.0,
            generation: 0.0,
            unit,
            presses: [None, None],
        });
        let index = self.clients.len() - 1;
        self.edges_run(context, index, &call(8.0))?;
        Ok(())
    }

    /// EntityEdges.advance translates external ids to monotonic player-lane
    /// ids, delivers the command, then retains its consumption translation.
    pub(super) fn advance_edges(
        &mut self,
        context: &Context,
        tick: f64,
        at: f64,
    ) -> Result<(), String> {
        for index in 0..self.clients.len() {
            let unit_generation = unit_generation(context, self.clients[index].unit)?;
            let player = self.clients[index].player;
            if unit_generation != self.clients[index].generation {
                let mut reset = call(0.0);
                reset.epoch = self.clients[index].epoch;
                reset.generation = unit_generation;
                reset.action_received = lanes(context, player, 0, false)?.actions_received;
                reset.fire_received = lanes(context, player, 1, false)?.fire_received;
                self.edges_run(context, index, &reset)?;
            }
            let mut advance = call(2.0);
            advance.tick = tick;
            let out = self.edges_run(context, index, &advance)?;
            for edge in out.edges {
                // prepare owns expiry, kind validation and serial reservation.
                // SnapshotHost reads the lanes for each edge, after earlier
                // deliveries may have advanced their received counters.
                let mut prepare = call(3.0);
                prepare.tick = tick;
                prepare.action_received = lanes(context, player, 0, false)?.actions_received;
                prepare.fire_received = lanes(context, player, 1, false)?.fire_received;
                prepare.edge = Some(edge.clone());
                let prepared = self.edges_run(context, index, &prepare)?;
                if !prepared.accepted {
                    continue;
                }
                let accepted = dispatch(
                    context,
                    player,
                    &edge,
                    prepared.id,
                    prepared.fire,
                    prepared.epoch,
                    tick,
                    at,
                    self.capture.as_ref().map(|c| c.store_source),
                    &mut self.edge_actions,
                )?;
                let mut finish = call(4.0);
                finish.fire = prepared.fire;
                finish.id = prepared.id;
                finish.external_id = edge.id;
                finish.accepted = accepted;
                self.edges_run(context, index, &finish)?;
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
