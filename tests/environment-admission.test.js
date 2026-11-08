// Exercise the displayed producers across real admission and memory growth.
// Values must remain native rows, including paused mode changes and clocks.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {loadEnvironment,SurfaceOwner,VisibilityHost} from '../src/environment/module.js';
import {Weather} from '../src/weather.js';
import {Snow} from '../src/snow.js';
import {FountainWater} from '../src/fountain-water.js';
import {EnvironmentWakes} from '../src/environment-volume.js';
import {EFFECT} from '../src/rust-render/effect-layout.js';
import {stepSurface} from './helpers/surfaces.js';
await loadEnvironment();
test('rain mode and sheet uniforms borrow native rows through cold growth',()=>{
  const owner=new SurfaceOwner(),scene=new THREE.Scene(),camera=new THREE.PerspectiveCamera();camera.updateMatrixWorld();
  let mode='classic',listener;
  const settings={get:()=>mode,onChange:fn=>{listener=fn;return()=>{};}};
  const data={precipitation:{ParticleDensity:[64],Range:[10],Velocity:[8],Color:[64,128,192],AlphaMinMax:[.2,.4]},wind:[3,4]};
  const rain=new Weather({data,scene,collision:{cast:()=>null},water:{over:()=>true,level:2},settings,surfaceOwner:owner});
  const old=rain.values;owner.x.memory.grow(1);owner.refresh();
  assert.equal(old.byteLength,0);assert.equal(rain.values.buffer,owner.x.memory.buffer);assert.equal(rain.sheets.values.buffer,owner.x.memory.buffer);
  mode='enhanced';listener('rainEffects');assert.equal(rain.count,2048);assert.equal(rain.cover.size,32);
  assert.equal(rain.values[EFFECT.RAIN.COVER_CELL],(32*2+8)/32);
  const values=rain.values,sheets=rain.sheets.values;
  for(let i=0;i<10;i++)stepSurface(rain,.05,camera);
  assert.equal(rain.values,values);assert.equal(rain.sheets.values,sheets);
  assert.equal(sheets[EFFECT.RAIN_SHEETS.RAIN_TIME],values[EFFECT.RAIN.RAIN_TIME]);
  assert.deepEqual(Array.from(sheets.subarray(EFFECT.RAIN_SHEETS.RADII,EFFECT.RAIN_SHEETS.RADII+3)),[4,9,18]);
  assert.ok(rain.cover.heights.every(h=>h===2));rain.dispose();
});
test('snow and fountain values retain their native identity and authored appearance',()=>{
  const owner=new SurfaceOwner(),scene=new THREE.Scene();
  const snow=new Snow({data:{precipitation:{ParticleDensity:[4],Range:[4]}},scene,collision:{cast:()=>null},texture:()=>null,surfaceOwner:owner});
  const host=new VisibilityHost(owner,0);host.start();const wakes=new EnvironmentWakes(host);
  const original=new THREE.Mesh();original.name='basin';original.visible=true;original.renderOrder=6;scene.add(original);
  const fountain=new FountainWater({scene},[{texture:'basin',center:[1,2,3],radius:4}],wakes,null,{});
  const old=fountain.values;owner.x.memory.grow(1);owner.refresh();assert.equal(old.byteLength,0);
  assert.equal(snow.values.buffer,owner.x.memory.buffer);assert.equal(fountain.values.buffer,owner.x.memory.buffer);assert.equal(fountain.basinData.buffer,owner.x.memory.buffer);
  assert.deepEqual(Array.from(fountain.basinData.subarray(0,8)),[1,2,3,4,1e6,1e6,1e6,0]);
  fountain.setEnabled(true);assert.equal(original.visible,false);assert.equal(original.renderOrder,-17);
  fountain.setEnabled(false);assert.equal(original.visible,true);assert.equal(original.renderOrder,6);
  fountain.dispose();snow.dispose();
});
