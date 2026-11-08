// A declared compressed twin is a content choice, never a speculative loader.
// Transport, decoder and upload failures must preserve the selected format and
// reach the GPU-labelled loading error without issuing a PNG request.
import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import {TextureLibrary} from '../src/textures.js';

// The two errors straddle descriptor publication: the second arrives after
// compressed metadata and mip views have already filled the waiting Texture.
// Neither is a missing-file decision. Both must keep the PNG unrequested.
let mode='decode';
class DecoderPort{
 postMessage({id}){queueMicrotask(()=>this.onmessage({data:mode==='decode'?{id,error:'codec failed'}:{id,width:4,height:4,format:33779,mips:[{width:4,height:4,offset:0,length:16}],pixels:new ArrayBuffer(16)}}));}
 terminate(){}
}
// Loading-manager completion is the observation boundary, so assertions
// cannot race the rejection handler or accidentally accept an unfinished load.
function fixture(){
 let finish;const done=new Promise(resolve=>{finish=resolve;});
 const manager={resolveURL:url=>url,itemStart(){},itemError(){},itemEnd(){finish();}};
 const gl={RENDERER:7937,isContextLost:()=>false,getInternalformatParameter:()=>[4],getExtension:()=>null,getParameter:()=> 'Fixture GPU'};
 const renderer={getContext:()=>gl,capabilities:{maxTextureSize:2048},resources:{enqueue:async()=>{throw Error('upload failed');}}};
 const library=new TextureLibrary({manager,renderer,compressed:false});library.twins.set('/asset.png','/asset.ktx2');
 return {library,renderer,done};
}

test('declared KTX2 decode and late upload failures never fall through to PNG',async()=>{
 // The GL label is independent of the mocked transport and decoder port.
 const previousFetch=globalThis.fetch,previousWorker=globalThis.Worker;
 const requests=[];
 globalThis.Worker=DecoderPort;
 globalThis.fetch=async url=>{requests.push(url);return {ok:true,arrayBuffer:async()=>new ArrayBuffer(16)};};
 try{
  for(mode of ['decode','upload']){
   const {library,renderer,done}=fixture(),texture=library.load('/asset.png');await done;
   assert.match(library.images.failure.message,/Fixture GPU/);
   assert.match(library.images.failure.message,mode==='decode'?/codec failed/:/upload failed/);
   assert.equal(renderer.resources.failure,library.images.failure);
   assert.equal(library.stats.failed,1);
   // A late failure must not overwrite the format and reuse stale mip views.
   if(mode==='upload')assert.equal(texture.format,33779,'failure cannot reinterpret compressed metadata as PNG');
  }
  assert.deepEqual(requests,['/asset.ktx2','/asset.ktx2']);
 }finally{globalThis.fetch=previousFetch;globalThis.Worker=previousWorker;}
});

// Only an actual missing optional index permits the original PNG route.
// A malformed present index and an unavailable server are renderer errors.
test('compressed discovery distinguishes absent content from broken delivery',async()=>{
 const previous=globalThis.fetch;
 try{
  // Discovery is tested separately from asset decoding and upload.
  const {library}=fixture();library.compressed=true;
  globalThis.fetch=async()=>({status:404,ok:false});await library.prepare(['/folder/asset.png']);
  globalThis.fetch=async()=>({status:503,ok:false});await assert.rejects(library.prepare(['/folder/asset.png']),/Fixture GPU.*503/);
  globalThis.fetch=async()=>({status:200,ok:true,json:async()=>{throw Error('invalid index');}});
  await assert.rejects(library.prepare(['/folder/asset.png']),/Fixture GPU.*invalid index/);
  for(const value of [{}, {files:[]}, {files:{'asset.png':{}}}]){
   globalThis.fetch=async()=>({status:200,ok:true,json:async()=>value});
   await assert.rejects(library.prepare(['/folder/asset.png']),/Fixture GPU.*Invalid KTX2/);
  }
 }finally{globalThis.fetch=previous;}
});

// Filtering a pending PNG must not mark a synthetic 1x1 image ready. These
// descriptors have the same class and null-image contract as TextureLoader.
test('PNG descriptors preserve the pending TextureLoader contract',async()=>{
 const previousFetch=globalThis.fetch,previousWorker=globalThis.Worker;
 globalThis.Worker=DecoderPort;mode='decode';
 globalThis.fetch=async()=>({ok:true,arrayBuffer:async()=>new ArrayBuffer(16)});
 try{
  const {library,done}=fixture();library.twins.clear();
  const texture=library.load('/pending.png');
  // A pending source must not appear uploadable before decoded rows arrive.
  assert.equal(texture.constructor,THREE.Texture);
  assert.equal(texture.image,null);assert.equal(texture.version,0);
  await done;
 }finally{globalThis.fetch=previousFetch;globalThis.Worker=previousWorker;}
});
