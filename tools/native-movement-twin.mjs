import assert from 'node:assert/strict';
import * as THREE from 'three';
import { loadSim } from '../src/sim/module.js';
import { RayKernel } from '../src/ray-kernel.js';
import { MovementKernel } from '../src/movement-kernel.js';
import { CollisionWorld } from '../src/collision.js';
import { Soldier } from '../src/soldier.js';
import { MovementTrace } from './native-movement-records.mjs';
const args=process.argv.slice(2), file=args[args.indexOf('--out')+1];
assert(args.includes('--out'));
const trace=new MovementTrace(file,{source:process.env.BFE_COMMIT ?? null,kind:'synthetic'});
function geometry(g) {return {p:g.attributes.position.array,i:Uint32Array.from(g.index.array),polygons:Uint32Array.from({length:g.index.count/3},(_,i)=>i>>1)};}
async function scene(cx) {
  const rays=new RayKernel((await loadSim()).exports), floor=geometry(new THREE.BoxGeometry(80,1,80).translate(0,cx*4-0.5,0));
  const ground=geometry(new THREE.PlaneGeometry(140,140,24,24).rotateX(-Math.PI/2).translate(0,-2+cx*4,0));
  const world=new CollisionWorld(floor.p,floor.i,floor.polygons,ground.p,ground.i,rays), kernel=new MovementKernel(world);
  world.movement=kernel;
  const bodies=[];
  for(let n=0;n<12;n++) {
    const box=geometry(new THREE.BoxGeometry(3,0.3+n*0.1,3).translate(n%4*4-6,cx*4+0.3,Math.floor(n/4)*5-5));
    bodies.push(world.addBody(box.p,box.i,box.polygons,{n}));
  }
  const box=geometry(new THREE.BoxGeometry(4,1.5,4)), shape=world.createBodyShape(box.p,box.i,box.polygons);
  const moving=world.addMovingBody(shape,new THREE.Matrix4().makeTranslation(0,cx*4+0.75,-8),{});bodies.push(moving);
  trace.attach(kernel,cx,true);
  const movers=Array.from({length:8},(_,n)=>{
    const s=new Soldier(world,{JetType:n%2?'hover':'jump',JetFuelRechargeRate:.2});
    s.spawn(n*2-7,cx*4+1,1,n*.2);s.getupTime=.9;s.diveTime=.6;s.maxFlyHeight=15+cx*4;
    return s;
  });
  return {cx,world,kernel,bodies,moving,movers};
}
const scenes=[await scene(0),await scene(1)];let growths=0,removals=0;
for(let tick=0;tick<420;tick++) for(const s of scenes) {
  if(tick%7===0) s.world.moveBody(s.moving,new THREE.Matrix4().makeRotationY(tick*.013).setPosition(Math.sin(tick*.03)*6,s.cx*4+.75,-8));
  s.bodies.forEach((b,i)=>{b.enabled=(tick+i)%23!==0;});
  if(tick===90) {s.world.removeBody(s.bodies[3]);removals++;}
  if(tick===130) {s.world.bodies.reverse();s.world.bodiesVersion++;}
  if(tick===200) {
    const old=s.kernel.view(),pointers=[s.kernel.x.movement_io_ptr(),s.kernel.x.movement_flags_ptr()];
    s.kernel.x.memory.grow(16);assert.equal(old.byteLength,0);assert.deepEqual([s.kernel.x.movement_io_ptr(),s.kernel.x.movement_flags_ptr()],pointers);growths++;
    trace.emit({op:'grow',cx:s.cx});
    for(let n=0;n<80;n++) s.kernel.add(new Soldier(s.world,{}));
    const p=s.movers[0].position, dense=geometry(new THREE.PlaneGeometry(4,4,48,48).rotateX(-Math.PI/2).translate(p.x,s.cx*4+.01,p.z));
    dense.polygons=Uint32Array.from({length:dense.i.length/3},(_,i)=>i);
    s.bodies.push(s.world.addBody(dense.p,dense.i,dense.polygons,{}));
  }
  for(let i=0;i<s.movers.length;i++) {
    const soldier=s.movers[i];
    if(tick===240) {soldier.air='thrown';soldier.grounded=false;soldier.velocity.set(4,9,-3);}
    if(tick===300) {soldier.air='jet';soldier.grounded=false;soldier.fuel=.9;}
    soldier.passThrough=tick%5===0?s.moving:null;
    const input={forward:i%3===0?0:.7,right:i%3===0?1:(i%2?-.3:.3),turn:.12*(i%2?1:-1),pitch:.03,jump:tick%65===0,crouch:tick===50||tick===120,prone:tick===150||tick===180};
    trace.tick(s.kernel,soldier,input,1/60,()=>s.kernel.tick(soldier,input,1/60));
  }
}
const summary=trace.finish({growths,removals});assert(summary.maxGather>256);console.log(JSON.stringify(summary));
