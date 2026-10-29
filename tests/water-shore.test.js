import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { WaterShore, waterlineSegment } from '../src/water-shore.js';

const triangle = (a,b,c) => new THREE.Triangle(...[a,b,c].map(p => new THREE.Vector3(...p)));

test('shore detection intersects the water plane with solid triangles', () => {
  const segment = waterlineSegment(triangle([0,-2,-4],[0,2,-4],[0,2,4]),0);
  assert.ok(segment);
  assert.ok(segment.every(p => p.y === 0 && p.x === 0));
  assert.equal(waterlineSegment(triangle([0,2,-4],[0,3,-4],[0,3,4]),0),null);
  assert.equal(waterlineSegment(triangle([0,0,-4],[1,0,-4],[0,0,4]),0),null);
});

function fixture(walls = true, dry = true) {
  let quality = 'high';
  let gathers = 0;
  const water = { height:0, group:new THREE.Group(), over:() => true };
  const wall = [triangle([0,-2,-20],[0,2,-20],[0,2,20]),triangle([0,-2,-20],[0,2,20],[0,-2,20])];
  const collision = {
    gather() { gathers++; return walls ? [{triangles:wall}] : []; },
    cast(origin, direction, far, hit) {
      hit.point.set(origin.x,dry && origin.x<0 ? 2 : -2,origin.z);
      hit.distance=origin.y-hit.point.y;
      return hit;
    },
  };
  const graphics = {settings:{get:()=>quality}};
  const shore = new WaterShore({water,collision,graphics});
  return {shore,water,setQuality:q=>{quality=q;},gathers:()=>gathers};
}

test('lapping patches sit on the exposed side of actual walls', () => {
  const f = fixture();
  const points = f.shore.sampleTile(0,0);
  assert.ok(points.length > 0);
  assert.ok(points.every(([x,z,nx,nz]) => x===0 && z>=0 && z<=8 && nx>0.99 && Math.abs(nz)<1e-6));
  f.shore.dispose();
});

test('water-cell borders and submerged walls never manufacture shore foam', () => {
  const grid = fixture(false);
  grid.water.over=x=>x>0;
  assert.equal(grid.shore.sampleTile(0,0).length,0);
  grid.shore.dispose();
  const submerged = fixture(true,false);
  assert.equal(submerged.shore.sampleTile(0,0).length,0);
  submerged.shore.dispose();
});

test('shore work is cached, bounded, disabled on low and disposed cleanly', () => {
  const f = fixture();
  const camera={position:new THREE.Vector3(2,3,0)};
  f.shore.update(1/60,camera);
  assert.ok(f.gathers()<=2);
  for(let i=0;i<60;i++)f.shore.update(1/60,camera);
  assert.ok(f.shore.count>0 && f.shore.count<=192);
  const before=f.gathers();
  for(let i=0;i<60;i++)f.shore.update(1/60,camera);
  assert.equal(f.gathers(),before,'stationary camera reuses its completed shore cache');
  f.setQuality('low');
  f.shore.update(1/60,camera);
  assert.equal(f.shore.group.visible,false);
  assert.equal(f.gathers(),before);
  f.setQuality('ultra');
  for(let i=0;i<100;i++) {camera.position.x+=8;f.shore.update(1/60,camera);}
  assert.ok(f.shore.cache.size<=169);
  f.shore.dispose();
  assert.equal(f.water.group.children.length,0);
});
