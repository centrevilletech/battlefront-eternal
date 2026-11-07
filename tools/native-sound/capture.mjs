// Record production SoundControl and worklet messages with synthetic game facts.
// Native control recomputes decisions; recorded output bytes only verify it.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {installAudioHost} from '../../tests/audio-offline-host.js';
import {CONTROL_SCENES,renderControlFixture} from '../../tests/audio-control-fixture.js';
import * as L from '../../src/audio/control-layout.js';
const out=process.argv[2];if(!out)throw Error('usage: capture.mjs OUTPUT [SCENE]');
let scene=CONTROL_SCENES.find(s=>s.name===(process.argv[3]||'control-enhanced-stereo'));
if(!scene||scene.output!=='stereo')throw Error('native device capture requires stereo');
if(process.env.BFE_SOUND_BURST==='1')scene={...scene,burstStreams:8};
fs.mkdirSync(out,{recursive:true});installAudioHost();
const blobs=[],ops=[],messages=[];let bytes=0,block=0,seed,options,lastControl=-1;
const blob=a=>{const data=Buffer.from(a.buffer,a.byteOffset,a.byteLength);const span=[bytes,data.length];blobs.push(Buffer.from(data));bytes+=data.length;return span;};
const hash=a=>crypto.createHash('sha256').update(Buffer.from(a.buffer,a.byteOffset,a.byteLength)).digest('hex');
const instantiate=WebAssembly.instantiate.bind(WebAssembly);
WebAssembly.instantiate=async(...args)=>{const result=await instantiate(...args),instance=result.instance||result,e=instance.exports;if(!e.sound_new)return result;const wrapped={...e};
 wrapped.sound_new=n=>{seed=n;return e.sound_new(n);};
 for(const name of ['import','step','ready'])wrapped['sound_'+name]=(...a)=>{
  const n=name==='ready'?L.READY.STRIDE:a.at(-1),data=blob(new Float64Array(e.memory.buffer,e.sound_input(a[0]),n));
  const op={name,kind:name==='import'?a[1]:0,data};ops.push(op);const value=e['sound_'+name](...a);
  if(name!=='import'){
   const info=new Float64Array(e.memory.buffer,e.sound_info(a[0]),L.INFO.STRIDE);
   op.expected=[hash(info),hash(new Float64Array(e.memory.buffer,e.sound_commands(a[0]),info[L.INFO.COMMANDS])),hash(new Float64Array(e.memory.buffer,e.sound_placements(a[0]),info[L.INFO.PLACEMENTS]))];
   lastControl=ops.length-1;
  }
  return value;
 };
 const proxy=new Proxy(instance,{get:(t,k)=>k==='exports'?wrapped:Reflect.get(t,k,t)});return result.instance?{...result,instance:proxy}:proxy;
};
const Node=AudioWorkletNode;
globalThis.AudioWorkletNode=new Proxy(Node,{construct(Target,args){const node=Reflect.construct(Target,args);if(args[1]!=='audio-output')return node;
 options={...args[2].processorOptions};delete options.dsp;delete options.controlLayout;delete options.id;
 if(options.filters)throw Error('capture requires a speaker layout');
 const post=node.port.postMessage.bind(node.port);node.port.postMessage=m=>{
  const row={block,type:m.type};
  if(m.type==='sample'){Object.assign(row,{id:m.id,frames:m.frames,channels:m.channels,rate:m.rate,data:blob(m.data)});}
  else if(m.type==='controls'){row.control=lastControl;if(lastControl<0)throw Error('control has no native owner');}
  else if(m.type==='voices'||m.type==='params')row.data=blob(m.data);
  else if(m.type==='volume')Object.assign(row,{value:m.value,time:m.time});
  else if(m.type==='room')row.params=m.params;
  else if(m.type==='configure'){if(block)throw Error('capture cannot change device layout');options={...m.options};delete options.id;delete options.controlLayout;return post(m);}
  else if(m.type==='stream')throw Error('record streamed PCM credits separately');
  else if(!['sync','release-samples'].includes(m.type))throw Error('unknown output message '+m.type);
  if(!['sync','release-samples'].includes(m.type))messages.push(row);return post(m);
 };
 const process=node.processor.process.bind(node.processor);node.processor.process=(...a)=>{block++;return process(...a);};return node;
}});
const rendered=await renderControlFixture(scene);
if(!options)throw Error('output worklet not recorded');
fs.writeFileSync(out+'/blobs.bin',Buffer.concat(blobs));
const reference=Buffer.concat(rendered.encoded.map(s=>Buffer.from(s,'base64')));
fs.writeFileSync(out+'/reference.f32',reference);
fs.writeFileSync(out+'/sound.json',JSON.stringify({version:1,seed,rate:48000,frames:scene.seconds*48000,options,ops,messages}));
console.log(JSON.stringify({scene:scene.name,ops:ops.length,messages:messages.length,bytes,peak:rendered.peak,reference:crypto.createHash('sha256').update(reference).digest('hex')}));
