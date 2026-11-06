import {rustItems} from './helpers/rust-simulation.js';
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";

class FakeUnit {
  constructor(id, team) { this.id = id; this.generation = 1; this.team = team; }
}

const explosion = { class: "boom", Damage: 60, DamageRadiusInner: 1, DamageRadiusOuter: 6, DroidScale: 0.5 };
const mine = { class: "mine", base: "mine", GeometryName: "mine", Gravity: 1, LifeSpan: 30, MaxHealth: 40, ExplosionDeath: explosion, ExplosionTrigger: explosion, TriggerRadius: 2, SuppressRadius: 1 };
const bomb = { class: "bomb", base: "mine", GeometryName: "bomb", Gravity: 1, LifeSpan: 1.5, MaxHealth: 10, ExplosionExpire: explosion };
const health = { class: "health", base: "powerupitem", GeometryName: "health", Gravity: 1, Rebound: 0.4, Friction: 0.6, LifeSpan: 20 };
const ammo = { class: "ammo", base: "powerupitem", Gravity: 1, LifeSpan: 20 };

// The ground at y 0; a model a sphere of radius 0.4 a little off its origin.
const collision = {
  cast(origin, dir, far, out) {
    out.body = null;
    if (!(dir.y < 0)) return null;
    const t = -origin.y / dir.y;
    if (t < 0 || t > far) return null;
    out.distance = t;
    out.point.copy(origin).addScaledVector(dir, t);
    out.normal.set(0, 1, 0);
    out.terrain = true;
    return out;
  },
};
const model = (name) => () => {
  const geometry = new THREE.SphereGeometry(name === "bomb" ? 0.25 : 0.4);
  geometry.translate(0, 0.1, 0.05);
  geometry.computeBoundingSphere();
  return new THREE.Mesh(geometry);
};

// Soldiers walking a line through the field: an enemy of team 1 and a
// friend, each taking what they touch.
function targets(log) {
  const make = (name, team, x, z, vz) => ({ name, team, position: new THREE.Vector3(x, 0, z), center: new THREE.Vector3(x, 1, z), radius: 0.5, velocity: new THREE.Vector3(0, 0, vz),
    relation: (other) => (other === 0 ? 0 : other === team ? 1 : -1),
    give(def) { log.push(["give", name, def.class]); return def.class !== "ammo"; } });
  return [make("enemy", 2, 0, -6, 2), make("friend", 1, 3, 8, -1.5)];
}
function walk(list, dt) {
  for (const t of list) { t.position.addScaledVector(t.velocity, dt); t.center.set(t.position.x, 1, t.position.z); }
}


import {createHash} from 'node:crypto';

test('Rust items synthetic fixture matches recorded flight and events',async()=>{
const log=[], owner=new FakeUnit(1,1);
const items = rustItems({scene:new THREE.Scene(),collision,effects:{play:()=>null},lights:{add(){}},model,explode:(def,p)=>log.push(['explode',def.class,p.toArray()])});
const digest=createHash('sha256'), list=targets(log), v=(x,y,z)=>new THREE.Vector3(x,y,z);
items.spawn(health,v(0,1,0),v(0,0,0),.3);
items.spawn(ammo,v(3,2,6),v(.5,3,0),1.1);
for(let tick=0;tick<480;tick++){
 if(tick===10||tick===30||tick===50||tick===70) items.dispense('minelayer',3,mine,v(tick/20,1,2),v(0,2,1),.5,owner,1);
 if(tick===40) items.dispense('timebomb',0,bomb,v(-2,1,1),v(0,1,0),0,owner,1);
 if(tick===200) items.blast(explosion,v(3.1,.5,2));
 walk(list,1/60);items.update(1/60,list);
 digest.update(JSON.stringify([items.items.map(p=>[p.def.class,p.position.toArray(),p.velocity.toArray(),p.up.toArray(),p.life,p.age,p.health,p.gone,p.landed,p.sinking]),log]));
}
assert.equal(digest.digest('hex'),'bbc593bc4b5492a1a9a54048b142cbeca949203d54e2b636a940e2a892724011');
});
