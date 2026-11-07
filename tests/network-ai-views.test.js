import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { networkViews } from '../src/runtime/network-views.js';

// Visibility and carried LOD decisions live in Rust. This host supplies the
// ordered authoritative camera facts, including remote humans and departures.
test('online AI camera facts follow every authoritative human',()=>{
  const world={},game={ai:{worldData:world}};
  const player=x=>({unit:{alive:true,position:new THREE.Vector3(x,0,0),soldier:{aimYaw:0,aimPitch:0}}});
  const first=player(0),second=player(1000),players=new Map([['a',first],['b',second]]);
  const update=networkViews(game,players),direction=new THREE.Vector3();update();
  assert.equal(world.networkViews,true);assert.equal(world.cameras.length,2);
  assert.deepEqual(world.cameras.map(camera=>camera.position.toArray()),[[0,1.7,0],[1000,1.7,0]]);
  world.cameras[1].getWorldDirection(direction);assert.ok(direction.z>.999);
  const retained=world.cameras[1];second.unit.soldier.aimYaw=Math.PI;update();
  assert.equal(world.cameras[1],retained);retained.getWorldDirection(direction);assert.ok(direction.z<-.999);
  second.disconnectedAt=1;update();assert.equal(world.cameras.length,1);
  first.unit.alive=false;update();assert.equal(world.cameras.length,0);
});
