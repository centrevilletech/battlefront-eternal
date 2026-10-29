import test from 'node:test';
import assert from 'node:assert/strict';
import { Object3D } from 'three';
import { Arsenal } from '../src/weapons.js';
import { captureUnitState, applyUnitState, decodeUnitState } from '../src/runtime/entity-unit-state.js';
import { presentationRecord, decodePresentation, presentEntityEvent } from '../src/runtime/entity-presentation-events.js';
import { CombatPresentation } from '../src/runtime/network-entity-combat.js';
import { encodeEntityPacket, decodeEntityPacket } from '../src/runtime/network-entity-codec.js';

const registry = { weaponIds: new Map([['rifle', 1], ['grenade', 2]]), weapons: new Map([[1, 'rifle'], [2, 'grenade']]) };
function unit() {
  return { id: 3, generation: 2, hero: false, targetable: true, deadTime: 0,
    soldier: { stateTime: .25 }, actor: { root: new Object3D() },
    combat: { arsenal: new Arsenal([
      { class: 'rifle', channel: 0, base: 'cannon', RoundsPerClip: 30, ammo: 3 },
      { class: 'grenade', channel: 1, base: 'grenade', RoundsPerClip: 1, ammo: 4 },
    ]), updateHeld() {} } };
}
const wire = record => decodeEntityPacket(encodeEntityPacket({ epoch: 1, packetId: 1, tick: record.tick, records: [record] })).records[0];

test('unit presentation carries secondary throws, reload, overheat, repair and destroyed body state', () => {
  const authority = unit(), shown = unit();
  Object.assign(authority.combat.arsenal.current(0), { heat: .9, overheated: true, reloadLeft: 1.5 });
  Object.assign(authority.combat.arsenal.current(1), { throwTime: .3, hidden: true, released: true });
  authority.combat.repairLock = 'building:2'; authority.combat.blowApart = true;
  authority.actor.root.visible = false; authority.deadTime = 2;
  const game = { units: { byId: () => shown }, playerUnit: {}, battle: { characters: [] } };
  assert.equal(applyUnitState(game, wire(captureUnitState(authority, registry, 10)), registry), true);
  assert.equal(shown.combat.arsenal.current(0).reloading, true);
  assert.equal(shown.combat.arsenal.current(0).overheated, true);
  assert.equal(shown.combat.arsenal.current(1).busy, true);
  assert.equal(shown.combat.arsenal.current(1).hidden, true);
  assert.equal(shown.combat.repairLock, 'building:2');
  assert.equal(shown.combat.blowApart, true);
  assert.equal(shown.actor.root.visible, false);
  assert.equal(shown.deadTime, 2);
  shown.generation++;
  authority.actor.root.visible = true;
  assert.equal(applyUnitState(game, captureUnitState(authority, registry, 11), registry), false);
  assert.equal(shown.actor.root.visible, false, 'old-life records cannot mutate a new body');
});

test('unit presentation preserves exact own weapon replay timers', () => {
  const authority = unit(), shown = unit(), weapon = shown.combat.arsenal.current(0);
  weapon.wait = .123456789; weapon.reloadLeft = 1.23456789;
  authority.combat.arsenal.current(0).wait = .8;
  authority.combat.arsenal.current(0).reloadLeft = 2;
  applyUnitState({ units: { byId: () => shown }, playerUnit: shown }, wire(captureUnitState(authority, registry, 10)), registry);
  assert.equal(weapon.wait, .123456789);
  assert.equal(weapon.reloadLeft, 1.23456789);
});

test('sparse presentation resets zero timers and absent locks without changing active Float32 state',()=>{
  const authority=unit(),shown=unit(),source=authority.combat.arsenal.current(0),target=shown.combat.arsenal.current(0);
  Object.assign(source,{heat:.3,wait:.1,lockTargetId:19,lockGeneration:7,lockComplete:true});
  const active=captureUnitState(authority,registry,1);
  applyUnitState({units:{byId:()=>shown}},wire(active),registry);
  assert.equal(target.heat,Math.fround(.3));assert.equal(target.lockTargetId,19);assert.equal(target.lockGeneration,7);
  Object.assign(source,{heat:0,wait:0,lockTargetId:-1,lockGeneration:-1,lockComplete:false});
  const idle=captureUnitState(authority,registry,2);
  assert.equal(idle.payload.length,31,'two idle weapons use five bytes each beside21-byte header');
  assert.equal(active.payload.length-idle.payload.length,14);
  applyUnitState({units:{byId:()=>shown}},wire(idle),registry);
  assert.equal(target.heat,0);assert.equal(target.wait,0);assert.equal(target.lockTargetId,-1);assert.equal(target.lockGeneration,-1);assert.equal(target.lockComplete,false);
  assert.throws(()=>decodeUnitState({...idle,payload:idle.payload.slice(0,-1)},registry));
  assert.throws(()=>decodeUnitState({...idle,payload:new Uint8Array([...idle.payload,0])},registry),/layout/);
});

test('remote weapon presentation cannot overwrite any private own replay field',()=>{
  const authority=unit(),shown=unit(),weapon=shown.combat.arsenal.current(1);
  Object.assign(weapon,{heat:.123456789,throwTime:.987654321,charge:.314159,held:2,hidden:true,lockTargetId:99,lockGeneration:4});
  const before={...weapon};
  applyUnitState({units:{byId:()=>shown},playerUnit:shown},wire(captureUnitState(authority,registry,1)),registry);
  for(const name of ['heat','throwTime','charge','held','hidden','lockTargetId','lockGeneration'])assert.equal(weapon[name],before[name],name);
});

test('scripted droid death retains the saturated debris effect through the entity wire', () => {
  const record = wire(presentationRecord({ id: 10, generation: 1, tick: 20,
    event: { kind: 'blowApart', ownerId: 3, ownerGeneration: 2, amount: -Infinity } }));
  const event = decodePresentation(record), shown = unit(), calls = [];
  assert.equal(event.amount, -50);
  assert.equal(presentEntityEvent({ units: { byId: () => shown }, presentEvent: (...args) => calls.push(args) }, event, registry), true);
  assert.equal(calls.length, 1); assert.equal(calls[0][2], -50);
  shown.generation++;
  assert.equal(presentEntityEvent({ units: { byId: () => shown } }, event, registry), false);
});

test('an animation waits for its unit and is presented once despite retransmission', () => {
  const record = presentationRecord({ id: 11, generation: 1, tick: 20,
    event: { kind: 'weaponAction', ownerId: 3, ownerGeneration: 2, action: 'reload', bank: 'rifle', posture: 'stand' } });
  let shown = null, played = 0;
  const game = { units: { byId: () => shown } };
  const presentation = new CombatPresentation({ ordnance: { clearProjectilePresentation() {} },
    definition: () => null, onPresentation: event => presentEntityEvent(game, event, registry) });
  presentation.receive(record, { epoch: 1, tick: 20 });
  presentation.receive(record, { epoch: 1, tick: 20 });
  shown = unit(); shown.animator = { weaponAction() { played++; } };
  presentation.present(21); presentation.present(22);
  assert.equal(played, 1);
});
