import './fixtures/native-render-descriptors.js';
import test from 'node:test';
import assert from 'node:assert/strict';
import {Scene,Vector3} from 'three';
import {actorLook} from '../src/actor-resources.js';
import {modelPack,modelGroup} from '../src/models.js';
import {radiusOf} from '../src/items.js';
import {nativeItems} from './helpers/native-items.js';

// Frozen modelPack bounds: two equal-radius parts with different centers.
// Items.radiusOf keeps the first on a tie, using the immutable bind sphere
// even when a renderer changes the node's culling sphere for a posed view.
function model() {
 const vertices=new Float32Array([0,0,0, 4,0,0, 0,6,0, 20,0,0, 24,0,0, 20,6,0].flatMap((v,i,a)=>i%3?[]:[v,a[i+1],a[i+2],0,0,1,0,0]));
 const indices=new Uint32Array([0,1,2,3,4,5]);
 const parts=[0,1].map(i=>({vertexOffset:i*3*32,vertexCount:3,indexOffset:i*3*4,indexCount:3}));
 const source=modelPack({item:parts},vertices.buffer,indices.buffer).get('item');
 return ()=>{const group=modelGroup(source,()=>actorLook('dynamic'));for(const node of group.children)node.boundingSphere.radius=100;return group;};
}

test('item bounds retain the frozen largest bind sphere and first tied center',()=>{
 const group=model()();assert.equal(radiusOf(group),Math.sqrt(13));
 assert.deepEqual(group.userData.center.toArray(),[2,3,0]);
});

test('native item admission retains the model radius and pickup center',async()=>{
 const make=model();
 const items=await nativeItems({scene:new Scene(),effects:{},lights:{add(){}},model:()=>make,explode(){},pools:{powerupitem:4}});
 const item=items.spawn({base:'powerupitem',GeometryName:'item',Gravity:1,LifeSpan:30},new Vector3(0,20,0),new Vector3(),0);
 assert.equal(item.radius,Math.sqrt(13));assert.deepEqual(item.center.toArray(),[2,3,0]);
});
