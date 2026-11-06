import { setAnimKernel } from '../../src/anim.js';
import { loadAnimKernel } from '../../src/anim-kernel.js';
import * as THREE from 'three';
import { loadSim } from '../../src/sim/module.js';
import { loadRayKernel } from '../../src/ray-kernel.js';
import { CollisionWorld } from '../../src/collision.js';
import { Soldier } from '../../src/soldier.js';
import { Droideka } from '../../src/droideka.js';
import { Unit } from '../../src/units.js';
import { Units } from '../../src/registry.js';
import { ObjectiveTarget } from '../../src/objectives.js';
import { Building, Turret } from '../../src/props.js';
import { attachF3Combat } from '../../src/f3-combat.js';
import { LightPool, Ordnance } from '../../src/ordnance.js';
import { Items } from '../../src/items.js';
import { attachItemsSim } from '../../src/items-sim.js';
import { attachOrdnanceSim } from '../../src/ordnance-sim.js';
import { attachF3 } from '../../src/f3-sim.js';
import { RandomStreams } from '../../src/ai/random.js';
import { Vehicle } from '../../src/vehicles.js';
import { VehicleTarget, TurretTarget, SeatUnit } from '../../src/seats.js';

// A real owned Unit/combat phase over a flat native collision tree. Only
// presentation resources are stubs; movement, registry and damage are live.
export async function f3World(definitions, {
  debug = true, water = null, terrain = false, prepareCollision = null, shieldHit = null, headshotCredit = null, deathCredit = null, deathNotice = null,
  bodyNotice = null, blastNotice = null, prepareUnit = null, afterAttach = null, prepareLive = null, prepareFleet = null,
  hullDestroyedCredit = null, passiveClasses = [], itemPools = {}, itemClasses = {},
} = {}) {
  const instance = await loadSim();
  setAnimKernel(await loadAnimKernel(instance));
  const rays = await loadRayKernel(instance);
  const floor = new Float32Array([-100,0,-100,-100,0,100,100,0,100,100,0,-100]);
  const indices = new Uint32Array([0,1,2,0,2,3]);
  const collision = terrain
    ? new CollisionWorld(new Float32Array(),new Uint32Array(),new Uint32Array(),floor,indices,rays)
    : new CollisionWorld(floor,indices,new Uint32Array([0,1]),undefined,undefined,rays);
  prepareCollision?.(collision);
  const units = new Units();
  const combatSim = attachF3Combat(instance, { Unit, Soldier, Droideka });
  const scene = new THREE.Scene();
  const ordnance = new Ordnance({ scene,collision,lights:new LightPool(scene) });
  const streams = new RandomStreams(7);
  ordnance.random = streams.draw('gameplay');
  ordnance.spinRandom = streams.draw('grenade-spin');
  ordnance.units = units;
  ordnance.water = water;
  attachOrdnanceSim(ordnance, combatSim.bridge);
  const items = new Items({scene:new THREE.Scene(),collision,model:()=>null,explode(){},water,pools:itemPools});
  items.random = streams.draw('item-drop');
  attachItemsSim(items,ordnance.sim);
  const factory = {
    built: [],
    sides: {rep:{items:itemClasses}},
    context: { units, ordnance, items, headshotCredit, deathCredit, deathNotice, bodyNotice, blastNotice, hullDestroyedCredit, shieldHit },
    acquire(name) { return this.context.unitPhase.acquire(name); },
    release(unit) { return this.context.unitPhase.release(unit); },
    build(name, passive = false) {
      const prepared = typeof passive === 'object' && passive.prepared;
      const def = definitions[name];
      const actor = { root:new THREE.Object3D(), bones:[] };
      const soldier = def.base === 'walkerdroid' ? new Droideka(collision,def,new Map(),actor) : new Soldier(collision, def);
      const animator = soldier.animator ?? { reset(){}, clips:new Map() };
      const presentation = { reset(){}, position:new THREE.Vector3() };
      const unit = new Unit({ side:'rep', className:name, def, actor, soldier, animator, presentation, walker:def.base === 'walkerdroid' });
      unit.combat = combatSim.make({ soldier, def, actor, animator, collision, ordnance, items, owner:unit });
      unit.combat.items = items;
      // UnitFactory's pickup object keeps body aliases. Native pickup tests
      // select these ordinary objects, then read geometry from the store.
      unit.itemTarget = {position:soldier.position,velocity:soldier.velocity};
      // A fixture can construct a complete corpse/life before the sole body
      // adoption, rather than writing an already owned simulation mirror.
      prepareUnit?.(unit);
      if (prepared) unit.id = units.nextId;
      else units.add(unit);
      unit.combat.attach();
      afterAttach?.(unit);
      if (this.context.unitPhase) this.context.unitPhase.adopt(unit);
      if (!passive && !prepared) this.built.push(unit);
      return unit;
    },
  };
  factory.context.vehicles = prepareFleet ? await prepareFleet({instance,collision,units,ordnance}) : null;
  const passive = [];
  for (const name of passiveClasses) passive.push(factory.build(name,true));
  const live = prepareLive ? prepareLive(collision, units, factory.context) : null;
  factory.context.live = live;
  const separation = {global:true,camera:new THREE.Vector3()};
  const phase = attachF3(instance, { Unit, Soldier, Droideka, ObjectiveTarget, Building, Turret, Vehicle, VehicleTarget, TurretTarget, SeatUnit }, {
    factory, combatSim, collision, water,
    separation,
    map:{ killY:-100, deathRegions:[], scripted:[] },
    presentEffect(){}, debug,
  });
  for (const unit of passive) phase.adopt(unit,true);
  phase.random.configure(ordnance.random,ordnance.spinRandom,streams.draw('item-drop'),streams.draw('seat-weapons'));
  phase.construction.reserve(units.nextId + 128);
  for (const name of Object.keys(definitions)) phase.construction.prepare(name,4);
  function spawn(name, x, z, networkHuman = false) {
    const unit = factory.acquire(name);
    unit.controller = {};
    unit.networkHuman = networkHuman;
    unit.human = networkHuman;
    unit.spawn(x, 0, z, 0);
    return unit;
  }
  return { phase, factory, units, ordnance, items, spawn, live, passive, separation };
}
