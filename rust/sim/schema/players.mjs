// A dedicated match's connected players (src/runtime/multiplayer-world.js):
// what a connection says of itself on admission, the input it admits after
// its mailbox, and what the host reads back after a tick. Their queues,
// their tick and their camera facts live in rust/sim/src/players.
export default {
  enums: { PlayerLane: ['actions', 'fire'] },
  records: {
    // addPlayer's record of a connection, once its Character and (for a
    // player) its connection body exist. A spectator has no Character handle.
    PlayerSetup: { $layout: true, mailbox: 'u32', character: 'rec:F4Handle|null', spectator: 'bool', reliableFire: 'bool',
      separateActions: 'bool', teamSelect: 'bool', autoRespawn: 'bool', ordinal: 'f64', team: 'f64', reconKey: 'u32|null', at: 'f64' },
    // A reconnect replaces the mailbox and the reliable lanes (addPlayer's
    // existing branch); the body and roster place are kept.
    PlayerLink: { $layout: true, mailbox: 'u32', reliableFire: 'bool', separateActions: 'bool', at: 'f64' },
    // A reliable trigger edge (world.fire) and a reliable action message
    // (world.actions), with the host's shape checks already made.
    PlayerFire: { $layout: true, edge: 'rec:IfEdge', epochOne: 'bool', pressedValid: 'bool', tick: 'f64', at: 'f64' },
    PlayerActions: { $layout: true, entry: 'rec:IaEntry', sameEpoch: 'bool', payloadValid: 'bool', humanValid: 'bool',
      human: 'list:rec:IaHuman', tick: 'f64', at: 'f64' },
    // The reliable lanes as the host reads them (edge receipts, the
    // diagnostics' counters and the tests' button state).
    PlayerLanes: { $layout: true, actionsReceived: 'f64', actionsAck: 'f64', actionsDiscarded: 'f64', fireReceived: 'f64',
      fireButtons: 'f64x2', stale: 'bool', outcomes: 'list:rec:IaOutcome' },
    // A player whose connection changed during a tick: his body, class,
    // team and spawn screen, or his retirement at the end of his grace.
    PlayerView: { $layout: true, handle: 'u32', deleted: 'bool', unit: 'ref:unit|null', className: 'str|null', team: 'f64',
      phase: 'enum:HumanPhase', post: 'str|null', spawnedOnce: 'bool', deadTime: 'f64', ended: 'bool', disconnected: 'bool',
      // The intent his controls (or his stop) wrote to his body this tick, for
      // the host's own copy of the body: nothing in the host reads it back.
      intent: 'rec:Intent|null', stopped: 'bool' },
    PlayerViews: { $layout: true, players: 'list:rec:PlayerView' },
    // The presses a player's body fired this tick, a button each (Unit.networkShots),
    // read by the host before its shot records are delivered.
    PlayerShots: { $layout: true, handle: 'u32', fire0: 'f64|null', fire1: 'f64|null' },
    PlayerShotList: { $layout: true, players: 'list:rec:PlayerShots' },
    // world.actions' answer: refused, accepted, or a protocol fault the host
    // reports as the source threw it.
    PlayerAdmission: { $layout: true, accepted: 'bool', error: 'str|null' },
    // A squad command a player gave: the HUD notice the page shows under
    // his body (the presentation record of kind squad).
    PlayerSquad: { unit: 'u32', life: 'f64', label: 'u32' },
  },
  events: { playerSquad: 'PlayerSquad' },
};
