import test from 'node:test';
import assert from 'node:assert/strict';
import {TurretSeats} from '../src/seats.js';
import {F4Live} from '../src/f4-live.js';
import {f3World} from './fixtures/f3-world.js';
import {fleetFixture} from './fixtures/f3-fleet.js';
import {turretAt} from './fixtures/f3-turret.js';

// Destruction crosses all three native owners before returning its command:
// F4 health, F2 crew/lease and F3 body damage. No callback-world stand-ins.
test('native turret destruction kills its seated body in place and retains credit',async()=>{
  const turret=turretAt();let crews;
  const w=await f3World({soldier:{class:'soldier',base:'soldier',MaxHealth:100,weapons:[]}}, {
    prepareFleet:fleetFixture({positions:[]}),
    prepareLive:(collision,units,context)=>{
      const live={buildings:[turret],turrets:[turret],animated:[],traps:[]};
      crews=new TurretSeats({live,collision,ordnance:context.ordnance});context.vehicles.f2.seats.attachTurrets(crews);
      return live;
    },
  });
  new F4Live(w.phase,{sound(){}});
  const unit=w.spawn('soldier',0,0),killer=w.spawn('soldier',20,20);
  killer.team=killer.combat.team=2;
  assert.equal(crews.man(unit,turret),true);
  const position=unit.position.clone(),lease=unit.seat;
  turret.damage(1000,killer);
  assert.equal(unit.alive,false);assert.equal(unit.combat.lastAttacker,killer);
  assert.equal(unit.seat,null);assert.equal(lease.released,true);
  assert.ok(unit.position.distanceTo(position)<1e-5);
  assert.equal(turret.alive,false);assert.equal(turret.lease,null);
});
