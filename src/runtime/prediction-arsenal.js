import { Arsenal } from '../weapons.js';
import { prepareMelee } from '../heroes.js';

const random = () => .5;
const EMPTY_CONTEXT = Object.freeze({});

// PredictionArsenal.begin formerly captured/decoded network weapon bytes to
// copy an already validated authority snapshot. Copy that same field set
// directly. RNG cursors, class constants and callbacks deliberately stay on
// the private view, just as they did with captureArsenalState's checkpoint.
// The network/simulation boundary already validates these live values. This
// view neither parses packets nor needs a second wire-validation round trip.
export function copyPredictionWeapon(from, to) {
  const level = from.levels.indexOf(from.def);
  if (level < 0 || level > 255 || !to.levels[level]) throw Error('weapon state definition mismatch');
  to.level = level; to.def = to.levels[level];
  to.timer = from.timer; to.clip = from.clip; to.reserve = from.reserve;
  to.heat = from.heat; to.kick = from.kick; to.spreadScale = from.spreadScale;
  to.sniperScale = from.sniperScale; to.yawSpreadOverride = from.yawSpreadOverride;
  to.wait = from.wait; to.reloadLeft = from.reloadLeft; to.throwTime = from.throwTime;
  to.held = from.held; to.charge = from.charge; to.pattern = from.pattern;
  to.salvosLeft = from.salvosLeft; to.salvoWait = from.salvoWait;
  to.lockTargetId = from.lockTargetId; to.lockGeneration = from.lockGeneration;
  to.lockElapsed = from.lockElapsed; to.limit = from.limit;
  to.deployed = from.deployed; to.firing = from.firing; to.overheated = from.overheated;
  to.released = from.released; to.hidden = from.hidden;
  to.charging = from.charging; to.lockComplete = from.lockComplete;
}

// A drawn Weapon still owns animation/effect interfaces. Its mutable state is
// a private view of the Rust session, never the authoritative Weapon itself.
export class PredictionArsenal {
  constructor() {
    this.source = null; this.epoch = 0; this.identity = 0; this.generation = 0;
    this.presentedThrough = -1; this.saved = null;
    this.snapshot = { arsenal: null, zoomStep: 0 };
    this.meleeStates = [];
  }
  restore() {
    if (!this.saved) return;
    this.combat.arsenal = this.saved.arsenal;
    this.combat.zoomStep = this.saved.zoomStep;
    this.saved = null;
  }
  dispose() { this.restore(); }
  begin(unit, context = EMPTY_CONTEXT) {
    // A repeated begin restores the previous borrow before saving authority.
    // Disposal may call restore again; the retained snapshot is attached once.
    this.restore();
    const epoch = context.epoch === undefined ? 1 : context.epoch, tick = context.tick;
    const combat = unit.combat, authority = combat.arsenal;
    // Keep the old begin() lifetime rules without formatting a string every
    // frame. A class/arsenal replacement invalidates even the same unit life.
    if (this.epoch !== epoch || this.identity !== unit.id || this.generation !== unit.generation || this.source !== authority) {
      this.epoch = epoch; this.identity = unit.id; this.generation = unit.generation;
      this.source = authority; this.presentedThrough = tick;
      this.arsenal = prepareMelee(new Arsenal(authority.weapons.map(w => w.levels[0]), { random }));
      this.meleeStates.length = 0;
      for (let i = 0; i < this.arsenal.weapons.length; i++) {
        if (this.arsenal.weapons[i].melee) this.meleeStates[i] = { time: 0, index: 0, active: false };
      }
    }
    this.combat = combat;
    this.snapshot.arsenal = authority; this.snapshot.zoomStep = combat.zoomStep;
    this.saved = this.snapshot;
    // Refresh all weapons before publishing the view. Keep its selected array
    // and melee scratch records so no wire buffer, decoded row or rebind map
    // is allocated for a warm frame. Selection still uses the authority slots.
    for (let i = 0; i < authority.weapons.length; i++) {
      const from = authority.weapons[i], to = this.arsenal.weapons[i];
      copyPredictionWeapon(from, to);
      if (from.melee) {
        const state = this.meleeStates[i];
        state.time = from.melee.time; state.index = from.melee.index;
        // MeleeSwing.captureState used the swing, not its cached active flag.
        state.active = Boolean(from.melee.swing); to.melee.restoreState(state);
      }
    }
    // Keep the private selection array's identity for drawn weapon readers.
    // Only its two slot values are part of the former arsenal checkpoint.
    this.arsenal.selected[0] = authority.selected[0];
    this.arsenal.selected[1] = authority.selected[1];
    combat.arsenal = this.arsenal;
    this.presentedThrough = Math.max(this.presentedThrough, tick);
  }
}
