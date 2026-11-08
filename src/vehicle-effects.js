import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import * as THREE from "three";
import {EffectPhase} from './effects-gpu.js';
import {VehicleFxInput as I,VehicleFxFrame as F,VehicleFxQuery as Q,VehicleFxRecord as O,VehicleFxStatus as S,VehicleFxNode as N,VehicleFxInspect as T,VehicleFxAction as A,VisualLimits as L} from './presentation/visual-effects-layout.js';
import { VisualFeet } from "./presentation/feet.js";

// What a hull shows beyond its model, at Classic (docs/vehicles.md,
// "Effects and sounds"), from its class's fx (tools/zero_vehicle.py
// vehicle_fx):
//
// - Thrust: each ThrustEffect burns at its attach point and offset from the
//   moment a flyer starts to take off until it has landed. Its particles
//   are drawn at a scale that rests at ThrustEffectMinScale and, once the
//   hull's speed passes the point ThrustEffectScaleStart of the way from
//   MinSpeed to MaxSpeed, grows in step with it, reaching
//   ThrustEffectMaxScale at MaxSpeed. The original game doesn't stop it there,
//   but its flyers never go much faster; the port's boost does, so here it
//   stops at ThrustEffectMaxScale.
// - Damage: each damage effect burns at its attach point while the hull's
//   health is at most DamageStartPercent and above DamageStopPercent of its
//   MaxHealth, at DamageEffectScale, its particles carrying
//   DamageInheritVelocity of the hull's velocity.
// - Water: a hover's WaterEffect, on the water's surface under it while it
//   is within WATER_HEIGHT of it.
// - Stomp: a walker's StompEffect where a foot comes down (the native foot phase),
//   at the foot level with the walker's root, turned as the foot is.
// - Death: the hull's effects stop and its chunks fly off (Debris), with
//   their trails and each one's ChunkSmokeEffect. A walker with a death
//   animation is dying first (Vehicle.dying, the simulation's): every
//   DeathAnimationExplosionTime one of its DeathAnimationExplosion effects
//   (picked at random) goes off at a node of its model picked at random,
//   its DeathDustEffect rises at DeathDustOffset DeathDustDelay after it
//   was destroyed if it is still dying then (the original game runs the
//   dust's clock only while the animation plays), and its chunks fly when
//   it is gone, with its FinalExplosion (the simulation's blast).
//
// Placed turrets and destructible posts (src/props.js) go through the same
// rules (updateBuildings): their damage effects while built, at the built
// model's nodes, a post's death animation as a walker's (with its
// DeathEffects at their hardpoints as it ends), and their chunks when
// destroyed (tools/extract.py write_props
// gives them a hull's fx; the building reader takes those keys as a hull's).
//
// It only draws: it reads what every host gives a hull (alive, dying,
// health, flight state, velocity and the pose drawn this frame) or a
// building (its state and health) and runs on the page, so a battle
// simulated here, in a worker or on a server looks the same. Missing
// attach points fall back to the hull's root, as the original game's do.

const EMPTY=Object.freeze([]),UP=new THREE.Vector3(0,1,0),STILL=Object.freeze(new THREE.Vector3());
const BANKS=[['header',Float64Array],['order',Uint32Array],['input',Float64Array],['nodes',Float64Array],['output',Float64Array],['status',Float64Array],['inspection',Float64Array]];
const REFUSED=Object.freeze({refused:true});
const FLIGHT_STATES=['Landed','TakingOff','Flying','Landing'];

class BuildingView {
  constructor(building) {
    const rig = building.looks?.built || building.rig || null;
    this.building = building;
    this.def = { fx: building.desc?.fx };
    this.root = rig?.root ?? null;
    this.byName = rig?.byName || {};
    this.bones = rig?.bones || [];
    this.velocity = STILL;
    this.aiTarget = { generation: 0 };

  }

  // The collision it has now (a wreck's, once destroyed): its pieces start
  // inside it.
  get body() { return this.building.bodies?.[this.building.state] ?? null; }
  get alive() { return this.building.alive; }
  get health() { return this.building.health; }
  get maxHealth() { return this.building.maxHealth; }
  // A destructible post's death animation left (src/props.js ObjectivePost).
  get dying() { return this.building.dying ?? 0; }


}


// Resolve names, bind chains and debris resource identities only at admission.
// Every numeric transform and effect decision uses the generated phase banks.
function content(v){
  const fx=v.def?.fx||{},nodes=[v.root],names=[],frameNodes=new Set([0]);
  const nodeId=node=>{node=node||v.root;let id=nodes.indexOf(node);if(id<0){id=nodes.length;nodes.push(node);}return id;};
  const nameId=name=>{let id=names.indexOf(name);if(id<0){id=names.length;names.push(name);}return id;};
  const entry=e=>{const node=nodeId(v.byName?.[e.node]);frameNodes.add(node);return {...e,effect:nameId(e.effect),node};};
  const thrust=(fx.thrust||EMPTY).map(entry),damage=(fx.damage||EMPTY).map(entry);
  const dust=fx.deathDust?{entry:entry(fx.deathDust),delay:fx.deathDust.delay||0}:null;
  const dying=fx.dying?{span:fx.dying.span||0,interval:fx.dying.interval??.4,effects:(fx.dying.effects||EMPTY).map(nameId),final:(fx.dying.final||EMPTY).map(entry)}:null;
  const bones=(v.bones||EMPTY).map(nodeId),bind=[];let body=v.byName?.bone_root,valid=Boolean(body&&v.bones&&v.bindPositions&&v.bindQuaternions);
  if(valid)for(let bone=body;bone!==v.root;bone=bone?.parent){const i=bone?v.bones.indexOf(bone):-1;if(i<0){valid=false;break;}bind.push({position:v.bindPositions[i].toArray(),quaternion:v.bindQuaternions[i].toArray(),scale:bone.scale.toArray()});}
  const bodyIndex=valid?nodeId(body):null;if(bodyIndex!==null)frameNodes.add(bodyIndex);
  const carrier=new THREE.Object3D();sceneProperty(carrier,"matrixAutoUpdate",false);
  const normalBones={...v.byName,'':v.root},fallenBones={...v.byName,'':carrier};
  for(const chunk of fx.chunks||EMPTY){const node=String(chunk.ChunkNodeName||'').toLowerCase();if(!normalBones[node])normalBones[node]=v.root;if(!fallenBones[node])fallenBones[node]=carrier;}
  const water=fx.water?nameId(fx.water):null;
  return {descriptor:{thrust,damage,dust,dying,water,speed:fx.speed||[10,30],nodes:nodes.length,bones,body:bodyIndex,bind},
    nodes,names,frameNodes:Array.from(frameNodes),carrier,normalBones,fallenBones,debrisOptions:{centre:true,floor:0},persistent:thrust.length+damage.length+1,events:2+(dying?.final.length||0)};
}
function writeNode(phase,row,index){const node=row.nodes[index];node.updateWorldMatrix(true,false);const at=(row.nodeStart+index)*N.STRIDE;for(let n=0;n<16;n++)phase.nodes[at+N.MATRIX+n]=node.matrixWorld.elements[n];}
function writeNodes(phase,row){for(let k=0;k<row.frameNodes.length;k++)writeNode(phase,row,row.frameNodes[k]);}
// Cold diagnostic readers use the same native rules as the complete frame.
// They are retained public fixture APIs, never called by the battle phase.
let inspection;
function inspector(){if(!inspection)inspection=new EffectPhase('PageVehicleEffects',BANKS);inspection.refresh();return inspection;}
export function thrustScale(entry,speed,speeds){const p=inspector(),x=p.inspection;x[T.SPEED]=speed;x[T.MIN_SPEED]=speeds[0];x[T.MAX_SPEED]=speeds[1];x[T.MIN_SCALE]=entry.minScale;x[T.MAX_SCALE]=entry.maxScale;x[T.SCALE_START]=entry.scaleStart;p.kernel.inspect();return x[T.SCALE];}
export function damageBurns(entry,health,maxHealth){const p=inspector(),x=p.inspection;x[T.HEALTH]=health;x[T.MAX_HEALTH]=maxHealth;x[T.START]=entry.start;x[T.STOP]=entry.stop;p.kernel.inspect();return Boolean(x[T.BURNS]);}
const carriers=new WeakMap();
export function bodyCarrier(v){
  const p=inspector();let row=carriers.get(v);if(!row){row=content(v);row.id=p.kernel.add(JSON.stringify(row.descriptor));if(row.id>=L.VEHICLES)return null;row.nodeStart=p.kernel.node_start(row.id);carriers.set(v,row);p.buffer=null;p.refresh();}
  writeNodes(p,row);p.kernel.carrier(row.id);const at=row.id*I.STRIDE;if(!p.input[at+I.CARRIER_VALID])return null;
  sceneTransform(row.carrier.matrix.fromArray(p.input,at+I.CARRIER));sceneTransform(row.carrier.matrixWorld.copy(row.carrier.matrix));return row.carrier;
}

export class VehicleEffects {
  constructor({effects,water=null,debris=null,random=Math.random}){
    this.effects=effects;this.water=water;this.debris=debris;this.random=random;this.feet=new VisualFeet();this.views=new WeakMap();
    this.phase=new EffectPhase('PageVehicleEffects',BANKS);this.rows=new WeakMap();this.active=new Array(L.VEHICLES);this.count=0;this.position=new THREE.Vector3();this.velocity=new THREE.Vector3();this.quaternion=new THREE.Quaternion();this.origin=new THREE.Vector3();
  }
  register(v){
    let row=this.rows.get(v);if(row)return row===REFUSED?null:row;
    row=content(v);row.id=this.phase.kernel.add(JSON.stringify(row.descriptor));if(row.id>=L.VEHICLES){this.rows.set(v,REFUSED);return null;}
    row.nodeStart=this.phase.kernel.node_start(row.id);row.outputStart=this.phase.kernel.output_start(row.id);row.handles=new Array(row.persistent).fill(null);
    this.rows.set(v,row);this.phase.buffer=null;this.phase.refresh();return row;
  }
  prepareBuilding(building){
    if(!building.desc?.fx)return;let view=this.views.get(building);if(!view)this.views.set(building,(view=new BuildingView(building)));
    if(view.root){this.feet.write(view);this.write(view,this.dt);}
  }
  write(v,dt){
    if(!v.def?.fx||!v.root)return;const row=this.register(v);if(!row)return;const phase=this.phase;phase.refresh();if(this.count===this.active.length)return;this.phase.order[this.count]=row.id;this.active[this.count++]=v;
    const x=phase.input,at=row.id*I.STRIDE;x[at+I.PRESENT]=1;x[at+I.DT]=dt;x[at+I.GENERATION]=v.aiTarget?.generation??0;x[at+I.ALIVE]=v.alive?1:0;
    x[at+I.HEALTH]=v.health;x[at+I.MAX_HEALTH]=v.maxHealth;x[at+I.FLYING]=v.flying?1:0;x[at+I.FLIGHT_STATE]=FLIGHT_STATES.indexOf(v.flightState);x[at+I.BUILDING]=v.building?1:0;
    v.velocity.toArray(x,at+I.VELOCITY);v.root.position.toArray(x,at+I.POSITION);x[at+I.WATER_LEVEL]=this.water?.level??NaN;x[at+I.WATER_OVER]=row.descriptor.water!==null&&this.water?.over?.(v.root.position.x,v.root.position.z)?1:0;x[at+I.DYING]=v.dying??0;
    writeNodes(phase,row);
    for(let k=0;k<row.persistent;k++){const h=row.handles[k],at=(row.outputStart+k)*S.STRIDE;phase.status[at+S.PRESENT]=h?1:0;phase.status[at+S.DONE]=h?.done?1:0;}
  }
  update(vehicles,dt,turrets=null,objectives=null){
    this.dt=dt;this.phase.refresh();for(let i=0;i<this.phase.input.length;i+=I.STRIDE)this.phase.input[i+I.PRESENT]=0;this.count=0;
    this.feet.begin(dt);
    for(const v of vehicles||EMPTY){this.feet.write(v);this.write(v,dt);}
    for(const b of turrets||EMPTY)this.prepareBuilding(b);for(const b of objectives||EMPTY)this.prepareBuilding(b);
    this.feet.run();this.phase.header[F.COUNT]=this.count;this.phase.kernel.frame();
    let published=0,partial=null;
    for(;;){
      this.phase.refresh();const frame=this.phase.header;
      if(partial){const row=this.rows.get(partial);this.publishRange(partial,row.persistent+1,row.persistent+2);partial=null;published++;}
      const end=frame[F.CURSOR];
      while(published<end){const v=this.active[published++];this.publish(v);if(v.alive)this.feet.apply(v,this.effects);}
      if(!frame[F.PENDING])break;
      partial=this.active[published];const row=this.rows.get(partial);this.publishRange(partial,0,row.persistent+1);
      frame[F.RANDOM]=this.random();frame[F.RANDOM+1]=this.random();this.phase.kernel.resume();this.phase.refresh();
      if(this.phase.header[F.PENDING]===Q.MATRIX){writeNode(this.phase,row,this.phase.header[F.NODE]);this.phase.kernel.complete();}
    }
    for(let k=0;k<this.count;k++)this.active[k]=null;
  }

  updateBuildings(buildings,dt){this.update(null,dt,buildings);}
  publish(v){
    const row=this.rows.get(v),x=this.phase.input,at=row.id*I.STRIDE;
    this.stopRows(v);if(x[at+I.BLOW_APART]&&!x[at+I.FALLEN])this.blowApart(v);
    this.publishRange(v,0,row.persistent+row.events);
    if(x[at+I.BLOW_APART]&&x[at+I.FALLEN])this.blowApart(v);
  }
  stopRows(v){const row=this.rows.get(v),data=this.phase.output;for(let k=0;k<row.persistent;k++){const o=(row.outputStart+k)*O.STRIDE;if(data[o+O.STOP]||data[o+O.ACTION]===A.STOP){row.handles[k]?.stop();row.handles[k]=null;}}}
  publishRange(v,start,end){
    const row=this.rows.get(v),data=this.phase.output;
    for(let k=start;k<end;k++){
      const o=(row.outputStart+k)*O.STRIDE;let handle=row.handles[k];
      if(data[o+O.STOP]||data[o+O.ACTION]===A.STOP){handle?.stop();if(k<row.persistent)row.handles[k]=null;handle=null;}
      const action=data[o+O.ACTION];if(action!==A.PLAY&&action!==A.MOVE)continue;
      sceneTransform(this.position.fromArray(data,o+O.POSITION));sceneTransform(this.quaternion.fromArray(data,o+O.QUATERNION));this.velocity.fromArray(data,o+O.VELOCITY);
      const normal=data[o+O.NORMAL]?UP:null,velocity=k<row.persistent?this.velocity:null,quaternion=normal?null:this.quaternion;
      if(action===A.PLAY){handle=this.effects.play(row.names[data[o+O.EFFECT]],this.position,normal,velocity,quaternion);if(k<row.persistent)row.handles[k]=handle;}
      else handle.move(this.position,quaternion,velocity);
      if(handle&&k<row.persistent){handle.scale=data[o+O.SCALE];handle.inherit=data[o+O.INHERIT];}
    }
  }
  blowApart(v){
    const row=this.rows.get(v),x=this.phase.input,at=row.id*I.STRIDE;
    if(this.debris&&v.def.fx.chunks?.length){
      const fallen=Boolean(x[at+I.FALLEN]&&x[at+I.CARRIER_VALID]);if(fallen){sceneTransform(row.carrier.matrix.fromArray(x,at+I.CARRIER));sceneTransform(row.carrier.matrixWorld.copy(row.carrier.matrix));}
      this.origin.fromArray(v.root.matrixWorld.elements,12);
      row.debrisOptions.floor=this.origin.y;this.debris.blowApart(v.def.fx.chunks,fallen?row.fallenBones:row.normalBones,this.origin,v.velocity,-50,v.body??null,row.debrisOptions);
    }
  }
}
