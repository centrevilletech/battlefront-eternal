import { PhaseCounters, MirrorWriteCheck } from './sim/phase-contract.js';
import { F3UnitMirrorMirror, F3SoldierMotionMirror, F3DroidekaMotionMirror, WaterContactMirror, syncF3UnitMirror } from './sim/gen/units.js';

export { PhaseCounters as F3Counters };
export class F3MirrorCheck extends MirrorWriteCheck {
  constructor() { super('F3'); }
}

const UNIT_FIELDS = ['team', 'alive', 'dead', 'deadTime', 'removed', 'targetable', 'tickCount', 'character', 'controller', 'seat'];
const BODY_VECTORS = ['position', 'previous', 'settled', 'velocity', 'avoidance'];
const SOLDIER_FIELDS = Object.keys(new F3SoldierMotionMirror());
const DROIDEKA_FIELDS = Object.keys(new F3DroidekaMotionMirror());

class Field {
  constructor(object, key, label) { this.object = object; this.key = key; this.label = label; }
  toString() { return this.label; }
}
function readField(_object, field) { return field.object[field.key]; }

// UnitFactory.build's ordinary pooled Unit and its body vectors are the mirror.
// The generated view borrows those vectors instead of replacing them on sync.
export class F3UnitBinding {
  constructor(unit, key, debug = false) {
    this.unit = unit;
    const view = this.view = new F3UnitMirrorMirror();
    view.key.slot = key.slot; view.key.generation = key.generation; view.key.life = key.life;
    const body = unit.soldier;
    for (let i = 0; i < BODY_VECTORS.length; i++) {
      const name = BODY_VECTORS[i];
      view[name] = body[name];
    }
    view.soldier = unit.walker ? null : body;
    view.droideka = unit.walker ? body : null;
    view.waterContact = unit.waterContact;
    // A water clock first appears during a live body phase. Reserve its
    // record now so an absent interval never allocates a generated mirror.
    this.waterContact = unit.waterContact ?? new WaterContactMirror();
    this.detector = debug ? new F3MirrorCheck() : null;
    this.fields = [];
    if (debug) {
      this.field(unit, 'soldier', 'soldier');
      this.field(unit, 'generation', 'generation');
      this.field(unit, 'held', 'held');
      for (let i = 0; i < UNIT_FIELDS.length; i++) this.field(unit, UNIT_FIELDS[i], UNIT_FIELDS[i]);
      for (let i = 0; i < BODY_VECTORS.length; i++) this.vector(body, BODY_VECTORS[i]);
      const fields = unit.walker ? DROIDEKA_FIELDS : SOLDIER_FIELDS;
      for (let i = 0; i < fields.length; i++) {
        const name = fields[i];
        if (name === 'normal') this.vector(body, name);
        else if (name === 'shield') {
          this.field(body, name, 'soldier.shield');
          if (body.shield) { this.field(body.shield, 'value', 'soldier.shield.value'); this.field(body.shield, 'on', 'soldier.shield.on'); }
        } else if (name !== 'lastGun') this.field(body, name, `soldier.${name}`);
      }
      if (unit.supply) this.field(unit.supply, 'position', 'supply.position');
      if (unit.itemTarget) {
        this.field(unit.itemTarget, 'position', 'itemTarget.position');
        this.field(unit.itemTarget, 'velocity', 'itemTarget.velocity');
      }
    }
  }

  // PlayerBodies.adopt retains the Unit while its prepared body changes.
  // Debug stamps still watch that same public object after structural birth.
  retarget(unit) {
    const old = this.unit;
    this.unit = unit;
    this.retargetFields(old,unit);
  }

  // Structural class realization retains debug watches on the replacement
  // public Unit while its existing vectors keep their identities.
  retargetFields(prior,next) {
    if (!this.detector) return;
    for (let i=0;i<this.fields.length;i++) {
      const field=this.fields[i][0];
      if (field.object===prior) field.object=next;
    }
  }

  bindCombat(combat) {
    if (!this.detector) return;
    const state = combat.state;
    for (const name of ['health','dead','deathTime','lastAttacker','repairTarget','repairLock','blowApart','zoomStep','mounted','gunUnderwater']) this.field(state, name, `combat.${name}`);
    for (const name of ['origin','target','aimDir','shotDir','left','up','pivot']) this.vector(state, name, 'combat');
    for (const name of ['rate','hold','offset']) this.field(state.kick, name, `combat.kick.${name}`);
    for (let i = 0; i < 2; i++) this.field(state.arsenal.selected, i, `combat.selected.${i}`);
    const weapons = state.arsenal.weapons;
    for (let i = 0; i < weapons.length; i++) {
      const weapon = weapons[i];
      for (const name of ['level','admits','deployed','firing','timer','bottomless','perShot','clip','reserve','heat','overheated','floor','kick',
        'spreadScale','sniperScale','yawSpreadOverride','wait','reloadLeft','throwTime','released','hidden','charging','held','charge','limit','pattern',
        'salvosLeft','salvoWait','salvoSeed','salvoCount','lockTargetId','lockGeneration','lockElapsed','lockComplete']) this.field(weapon, name, `weapon.${i}.${name}`);
    }
  }

  // AnimatedProp's retained hold is a published mirror too. Watch its
  // ordinary fields and matrix storage without installing property traps.
  bindHeld(held, frame) {
    if (!this.detector) return;
    for (const name of ['prop','grab','since','frame','time']) this.field(held,name,`held.${name}`);
    this.field(frame,'elements','held.frame.elements');
    for (let i=0;i<16;i++) this.field(frame.elements,i,`held.frame.${i}`);
  }

  field(object, key, label) { this.fields.push([new Field(object, key, label), object[key]]); }
  vector(body, key, prefix = 'soldier') {
    const v = body[key];
    this.field(body, key, `${prefix}.${key}`);
    this.field(v, 'x', `${prefix}.${key}.x`); this.field(v, 'y', `${prefix}.${key}.y`); this.field(v, 'z', `${prefix}.${key}.z`);
  }

  // Generated F3UnitMirror rows contain no class data, shape caches or rigs.
  sync(bridge, floats, at, heap) {
    this.view.waterContact = this.waterContact;
    syncF3UnitMirror(bridge, floats, at, heap, this.view);
    this.apply();
  }

  apply() {
    const unit = this.unit, view = this.view;
    unit.generation = view.key.life;
    // Unit's public scalar fields have stable storage. Named assignments
    // avoid generic property copies boxing every numeric mirror value.
    unit.team = view.team;
    unit.alive = view.alive;
    unit.dead = view.dead;
    unit.deadTime = view.deadTime;
    unit.removed = view.removed;
    unit.targetable = view.targetable;
    unit.tickCount = view.tickCount;
    unit.character = view.character;
    unit.controller = view.controller;
    unit.seat = view.seat;
    unit.waterContact = view.waterContact;
    if (unit.combat.remotes) unit.combat.remotes.authorityActive = view.remoteActive === true;
    if (view.held === null || view.held === undefined) unit.held = view.held;
    if (unit.actor) unit.actor.root.visible = view.visible;
    this.published();
  }

  published() {
    if (!this.detector) return;
    for (let i = 0; i < this.fields.length; i++) {
      const pair = this.fields[i], field = pair[0];
      pair[1] = field.object[field.key];
    }
    this.detector.published(this.unit, this.fields);
  }
  check() { if (this.detector) this.detector.check(this.unit, readField); }
}
