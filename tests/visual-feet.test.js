import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {VisualFeet,cycleHeights} from '../src/presentation/feet.js';
import {FootEvent as O,FootLimits as L,FootFrame as F,FootInput as I} from '../src/presentation/layout.js';

// A no-cycle primitive uses the original falling/landed/rising detector.
// Read one coarse phase, then apply only the resource event it published.
test('native visual feet plant once, preserve primitive turn and reset a new life',()=>{
 const root=new THREE.Group(),bone=new THREE.Bone();root.add(bone);
 const turn=new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0,1,0),.3);
 const v={root,bones:[bone],byName:{foot:bone},alive:true,aiTarget:{generation:1},
  def:{fx:{feet:[{bone:'foot',at:[1,0,2],turn:turn.toArray()}],stomp:{effect:'dust'}}}};
 const feet=new VisualFeet(),played=[];const effects={play(name,p,n,velocity,q){played.push([name,p.toArray(),q?.toArray()]);}};
 const frame=y=>{bone.position.y=y;root.updateMatrixWorld(true);feet.begin(.1);feet.write(v);feet.run();feet.apply(v,effects);};
 frame(2);frame(1);frame(1);frame(1);assert.equal(played.length,1);
 assert.deepEqual(played[0],['dust',[1,0,2],turn.toArray()]);
 frame(2);frame(1);frame(1);assert.equal(played.length,2);
 v.aiTarget.generation++;frame(1);assert.equal(played.length,2);
});

test('cold native cycle samples retain scales, bind positions and f32 stores',()=>{
 const root=new THREE.Group(),bone=new THREE.Bone();root.add(bone);bone.scale.set(1,2,1);
 const data=new Float32Array([0,0,0,1,0,1,0, 0,0,0,1,0,2,0, 0,0,0,1,0,1,0]);
 const clip={frames:3,fps:2,rootMotion:true,tracks:[0],data};
 const v={bones:[bone],byName:{foot:bone},bindPositions:[new THREE.Vector3()],bindQuaternions:[new THREE.Quaternion()],
  pose:{root:-1},walkLength:2,clips:new Map([['walk_leftfoot_rightfoot',clip],['walk_rightfoot_leftfoot',clip]]),def:{fx:{}}};
 assert.deepEqual(cycleHeights(v,[{bone:'foot',at:[0,.25,0]}]),[[1.5,2.5,1.5,2.5]]);
});

test('an oversized authored foot class is bounded at registration',()=>{
 const root=new THREE.Group();root.updateMatrixWorld(true);
 const v={root,alive:true,aiTarget:{generation:1},def:{fx:{feet:Array.from({length:4096},()=>({at:[0,0,0]})),stomp:{effect:'dust'}}}};
 const feet=new VisualFeet();feet.begin(.1);feet.write(v);feet.run();
 assert.equal(feet.rows.get(v).nodes.length,L.PER_BODY);assert.equal(feet.output.length,L.PER_BODY*O.STRIDE);
 // The next class begins exactly after the bounded prefix. Rewriting the
 // oversized body cannot overwrite its neighbour's correction record.
 const other={...v,aiTarget:{generation:9},def:{fx:{feet:[{at:[0,0,0]}],stomp:{effect:'dust'}}}};
 feet.write(other);const row=feet.rows.get(other);assert.equal(row.start,L.PER_BODY);
 feet.write(v);assert.equal(feet.input[F.STRIDE+row.start*I.STRIDE+I.GENERATION],9);
 assert.equal(feet.output.length,(L.PER_BODY+1)*O.STRIDE);
});
