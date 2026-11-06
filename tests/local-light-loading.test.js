// Shader accumulation order must survive asynchronous source decoding. Shared
// images can become ready together while an intervening image is still pending.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {LocalLights} from '../src/local-lights.js';

test('lamp discovery preserves scene order when shared images finish first',()=>{
  const lights=new LocalLights(),geometry=new THREE.PlaneGeometry(1,1);
  const ready=new THREE.DataTexture(new Uint8Array([255,255,255,255]),1,1);
  const delayed=new THREE.DataTexture(null,1,1);
  const mesh=(name,map)=>{
    const object=new THREE.Mesh(geometry,new THREE.ShaderMaterial({uniforms:{map:{value:map}}}));
    object.name=name;return object;
  };
  const emitted=[];
  // Observe publication through the actual register/begin boundary. Emitter
  // geometry and colour sampling have separate coverage; readiness is the input.
  lights.emitters=({mesh})=>emitted.push(mesh.name);
  lights.register(mesh('first',ready));
  lights.register(mesh('middle',delayed));
  lights.register(mesh('last',ready));
  lights.begin(true);
  assert.deepEqual(emitted,['first']);
  assert.equal(lights.pending.length,2);
  // A shared source becoming ready cannot let its later mesh overtake the
  // earlier mesh whose pixel decode has not completed yet.
  delayed.image.data=new Uint8Array([255,255,255,255]);
  lights.begin(true);
  assert.deepEqual(emitted,['first','middle','last']);
  assert.equal(lights.pending.length,0);
});
