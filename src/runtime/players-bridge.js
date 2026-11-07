// The native players' host entries (rust/sim/src/players/module.rs): a
// dedicated match admits its connections and their input through these,
// and reads back after each tick what the connections changed. Nothing of
// a tick is decided here.
import { PlayerSetupMirror, PlayerSetupLayout, putPlayerSetup, PlayerLinkMirror, PlayerLinkLayout, putPlayerLink,
  PlayerFireMirror, PlayerFireLayout, putPlayerFire, PlayerActionsMirror, PlayerActionsLayout, putPlayerActions,
  PlayerLanesMirror, syncPlayerLanes, PlayerViewsMirror, syncPlayerViews, PlayerAdmissionMirror, syncPlayerAdmission,
  PlayerShotListMirror, syncPlayerShotList } from '../sim/gen/players.js';

const SETUP = { words: PlayerSetupLayout.words, put: putPlayerSetup };
const LINK = { words: PlayerLinkLayout.words, put: putPlayerLink };
const FIRE = { words: PlayerFireLayout.words, put: putPlayerFire };
const ACTIONS = { words: PlayerActionsLayout.words, put: putPlayerActions };
const LANES = { Mirror: PlayerLanesMirror, sync: syncPlayerLanes };
const VIEWS = { Mirror: PlayerViewsMirror, sync: syncPlayerViews };
const ADMISSION = { Mirror: PlayerAdmissionMirror, sync: syncPlayerAdmission };
const SHOTS = { Mirror: PlayerShotListMirror, sync: syncPlayerShotList };

export class PlayersBridge {
  constructor(bridge) {
    this.bridge = bridge; this.x = bridge.x;
    this.setup = new PlayerSetupMirror(); this.link = new PlayerLinkMirror();
    this.fireRow = new PlayerFireMirror(); this.actionsRow = new PlayerActionsMirror();
    this.laneRow = new PlayerLanesMirror(); this.views = new PlayerViewsMirror(); this.admission = new PlayerAdmissionMirror(); this.shotList = new PlayerShotListMirror();
    this.x.players_init();
  }
  create(fields) {
    const s = this.setup;
    s.mailbox = fields.mailbox; s.character = fields.character; s.spectator = fields.spectator; s.reliableFire = fields.reliableFire;
    s.separateActions = fields.separateActions; s.teamSelect = fields.teamSelect; s.autoRespawn = fields.autoRespawn;
    s.ordinal = fields.ordinal; s.team = fields.team; s.reconKey = fields.reconKey; s.at = fields.at;
    this.bridge.callArgs(SETUP, s);
    const handle = this.x.players_create();
    if (!handle) throw Error('player admission failed');
    return handle;
  }
  relink(handle, { mailbox, reliableFire, separateActions, at }) {
    const l = this.link; l.mailbox = mailbox; l.reliableFire = reliableFire; l.separateActions = separateActions; l.at = at;
    this.bridge.callArgs(LINK, l);
    if (!this.x.players_link(handle)) throw Error('player reconnect failed');
  }
  drop(handle) { this.x.players_drop(handle); }
  stop(handle) { this.x.players_stop(handle); }
  disconnect(handle, tick) { this.x.players_disconnect(handle, tick); }
  autoRespawn(handle, flag) { this.x.players_auto(handle, flag ? 1 : 0); }
  input(handle, at, lead, accepted) { this.x.players_input(handle, at, lead, accepted ? 1 : 0); }
  // world.fire's edge, with the host's shape checks already made.
  fire(handle, edge, tick, at) {
    const r = this.fireRow;
    r.edge = edge; r.epochOne = edge.epoch === 1; r.pressedValid = typeof edge.pressed === 'boolean'; r.tick = tick; r.at = at;
    this.bridge.callArgs(FIRE, r);
    return Boolean(this.x.players_fire(handle));
  }
  // world.actions' message: the lane refuses, accepts or faults as the
  // source did (a fault threw out of the admission).
  actions(handle, epoch, entry, humans, humanValid, tick, at) {
    const r = this.actionsRow;
    r.entry = entry; r.sameEpoch = epoch === 1; r.payloadValid = true; r.humanValid = humanValid; r.human = humans; r.tick = tick; r.at = at;
    this.bridge.callArgs(ACTIONS, r);
    const ptr = this.x.players_actions(handle);
    if (!ptr) throw Error('invalid reliable actions');
    const out = this.bridge.callResult(ADMISSION, ptr, this.admission);
    if (out.error !== null) throw Error(out.error);
    return out.accepted;
  }
  lanes(handle, lane, drain) {
    const ptr = this.x.players_lanes(handle, lane, drain ? 1 : 0);
    if (!ptr) throw Error('unknown player');
    return this.bridge.callResult(LANES, ptr, this.laneRow);
  }
  enableOutcomes(handle) { this.x.players_outcomes_enable(handle); }
  changed() { return this.bridge.callResult(VIEWS, this.x.players_views(), this.views).players; }
  shots() { return this.bridge.callResult(SHOTS, this.x.players_shots(), this.shotList).players; }
}

// A connection's reliable lane as the host reads it (the wire's edge
// receipts, diagnostics and tests): InputActions' and FireCommands' public
// counters over the native lane.
export class LaneView {
  constructor(players, handle, lane) { this.players = players; this.handle = handle; this.lane = lane; }
  read(drain = false) { return this.players.lanes(this.handle, this.lane, drain); }
  get received() { const l = this.read(); return this.lane === 0 ? l.actionsReceived : l.fireReceived; }
  get ack() { return this.read().actionsAck; }
  get discarded() { return this.read().actionsDiscarded; }
  get buttons() { return Array.from(this.read().fireButtons); }
  enableOutcomes() { this.players.enableOutcomes(this.handle); return this; }
  drainOutcomes() { return this.read(true).outcomes.map(o => ({ id: o.id, status: o.status })); }
}
