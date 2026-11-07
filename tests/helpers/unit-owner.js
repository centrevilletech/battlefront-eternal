// Shared native unit ownership for isolated fleet fixtures. Construction is
// cold, as in createGame; a fixture may still supply presentation observers.
import {RandomStreams} from '../../src/ai/random.js';
import * as THREE from 'three';
import {Unit} from '../../src/units.js';
import {Soldier} from './body.js';
import {Droideka} from './droideka.js';
import {Units} from '../../src/registry.js';
import {Building,Turret} from '../../src/props.js';
import {Vehicle} from '../../src/vehicles.js';
import {VehicleTarget,TurretTarget,SeatUnit} from '../../src/seats.js';
import {ObjectiveTarget} from '../../src/objectives.js';
import {Items} from '../../src/items.js';
import {attachItemsSim} from '../../src/sim/wasm/items-sim.js';
import {attachF3Combat} from '../../src/sim/wasm/f3-combat.js';
import {attachF3} from '../../src/sim/wasm/f3-sim.js';

export function unitOwner(system,bridge) {
  const units=system.units || new Units();
  if (!system.units) system.attachUnits(units);
  const ordnance=system.ordnance,collision=system.collision;
  ordnance.units=units;
  const items=new Items({scene:new THREE.Scene(),collision,model:()=>null,explode(){}});
  attachItemsSim(items,ordnance.sim);
  const instance={exports:bridge.x,holder:{bridge}};
  const combatSim=attachF3Combat(instance,{Unit,Soldier,Droideka});
  const live=system.fixtureLive || {buildings:[],turrets:[],animated:[]};
  live.buildings ||= live.turrets || [];
  const factory={context:{units,ordnance,items,vehicles:system,live,
    deathNotice:(unit,event)=>{
      unit.combat.deaths?.push(event.attacker);
      unit.combat.onDeath?.();unit.animator?.die?.();
    },
    blastNotice:(def,event)=>ordnance.onExplode(def,event.position,event.owner,{wet:event.wet})},sides:{},built:[]};
  const phase=attachF3(instance,{Unit,Soldier,Droideka,Building,Turret,Vehicle,VehicleTarget,TurretTarget,SeatUnit,ObjectiveTarget},
    {factory,combatSim,collision,water:system.water,map:{killY:-300,deathRegions:[],scripted:[]},presentEffect(){}});
  const streams=new RandomStreams(7);
  ordnance.random=streams.draw('gameplay');ordnance.spinRandom=streams.draw('grenade-spin');
  system.seatRandom=streams.draw('seat-weapons');
  phase.random.configure(ordnance.random,ordnance.spinRandom,streams.draw('item-drop'),system.seatRandom);
  phase.construction.reserve(units.nextId+128);
  function adopt(unit) {
    if (!unit || unit.f3) return unit;
    if(unit.removed===undefined)unit.removed=!unit.alive;
    const old=unit.combat,def={class:'fixture',base:'soldier',MaxHealth:old?.maxHealth||300,weapons:[],...unit.classDef};
    const soldier=unit.soldier || new Soldier(collision,def);
    const actor=unit.actor || {root:new THREE.Object3D(),bones:[]};actor.bones ||= [];
    const animator=unit.animator || {clips:new Map(),reset(){}};animator.clips ||= new Map();
    const intent=unit.intent;
    const defaults=new Unit({side:'fixture',className:def.class,def,soldier,actor,animator,presentation:unit.presentation||{reset(){}}});
    for (const key of Object.keys(defaults)) if (!(key in unit)) unit[key]=defaults[key];
    unit.side ||= 'fixture';unit.className ||= def.class;
    unit.presentation ||= defaults.presentation;
    unit.presentation.position ||= new THREE.Vector3();
    unit.classDef=def;unit.actor=actor;unit.animator=animator;unit.soldier=soldier;
    if (!(unit instanceof Unit)) Object.setPrototypeOf(unit,Unit.prototype);
    if (units.byId(unit.id)!==unit) {
      add(unit);
    }

    unit.combat=combatSim.make({soldier,def,actor,animator,collision,ordnance,items,owner:unit});
    unit.combat.attach();
    if(old?.deaths)unit.combat.deaths=old.deaths;
    if (old?.health!==undefined) unit.combat.health=old.health;
    unit.combat.team=unit.team;
    if (old?.onDamaged) unit.combat.onDamaged=old.onDamaged;
    if (intent) unit.intent=intent;
    phase.adopt(unit,true);
    factory.built.push(unit);
    return unit;
  }
  const add=units.add;
  units.add=unit=>{
    add(unit);
    if (unit.soldier) adopt(unit);
    return unit;
  };
  const prepareOwner=phase.owners.prepare.bind(phase.owners);
  phase.owners.prepare=owner=>{
    if(owner&&typeof owner==='object'&&!Object.isFrozen(owner)&&('generation' in owner)&&!owner.f3)adopt(owner);
    prepareOwner(owner);
  };
  const board=system.board;
  system.board=(unit,...args)=>{adopt(unit);return board(unit,...args);};
  const attachTurrets=system.f2.seats.attachTurrets.bind(system.f2.seats);
  system.f2.seats.attachTurrets=crews=>{
    crews.attachUnits(units);
    for(const turret of crews.turrets)if(!phase.props.buildings.includes(turret)) {
      const index=phase.props.buildings.length;
      phase.props.buildings.push(turret);phase.props.prepare(index);
    }
    attachTurrets(crews);
    const man=crews.man;
    crews.man=(unit,...args)=>{adopt(unit);return man(unit,...args);};
  };
  system.update=dt=>phase.phase(dt);
  system.fixtureOwner={phase,adopt};
  return system.fixtureOwner;
}
