// Capture synthetic whole surface frames for the native surface-replay tool.
// Query replies are recorded at the phase boundary, so replay exercises the
// real cache, intersection, selection, commands and draw-record construction.
import fs from 'node:fs';
import * as THREE from 'three';
import { loadEnvironment, SurfaceOwner } from '../src/environment/module.js';
import { SurfaceHost } from '../src/environment/surfaces.js';
import { Surface as LIMITS, SurfaceCommand as COMMAND, SurfaceConfig as CONFIG, SurfaceFrame as FRAME, SurfaceResult as RESULT } from '../src/environment/layout.js';

await loadEnvironment();
const file=process.argv[2];if(!file)throw Error('surface-replay.mjs CAPTURE');
const host=new SurfaceHost(new SurfaceOwner(undefined, true)),camera=new THREE.PerspectiveCamera(63,1.6,.5,250);
const chunks=[];
const u32=value=>{const b=Buffer.alloc(4);b.writeUInt32LE(value);chunks.push(b);};
const doubles=values=>{const b=Buffer.alloc(values.length*8);for(let i=0;i<values.length;i++)b.writeDoubleLE(values[i],i*8);chunks.push(b);};
const floats=values=>{const b=Buffer.alloc(values.length*4);for(let i=0;i<values.length;i++)b.writeFloatLE(values[i],i*4);chunks.push(b);};
const frames=240;
host.config[CONFIG.HEIGHT]=-2;host.config[CONFIG.BOBBING]=1;
host.config[CONFIG.NORMAL_COUNT]=12;host.config[CONFIG.NORMAL_FPS]=7;
host.config[CONFIG.BUMP_COUNT]=5;host.config[CONFIG.BUMP_FPS]=4;
host.config[CONFIG.MASK_COUNT]=3;host.config[CONFIG.MASK_FPS]=2.4;
u32(0x31434653);u32(frames);doubles(host.config);
let tiles=[],probes=[];
host.owner.attach({host_shore_tile:()=>{
  const data=[0,-4,-40,0,4,-40,0,4,40,0,-4,-40,0,4,40,0,-4,40];
  host.triangles.set(data);tiles.push(data);return 2;
},host_water_over:()=>1,host_water_level:()=>-2,host_shore_exposed:(x,y,z)=>{const exposed=x>=0;probes.push(exposed);return exposed?-Infinity:Infinity;}});
const contact={},velocity=new THREE.Vector3(2,-1,3),point=new THREE.Vector3();
for(let i=0;i<frames;i++) {
  tiles=[];probes=[];
  camera.position.set(Math.sin(i*.03)*12,3,Math.cos(i*.05)*23);camera.lookAt(0,-2,0);camera.updateMatrixWorld();
  const dt=i===0?6.06:i%29===0?0:i%17===0?.1:1/60;
  point.set(camera.position.x,-2,camera.position.z);
  if(i%3===0)host.ripple(point,2,.25,i%2?velocity:null);
  if(i===100)for(let j=0;j<5000;j++)host.ripple(point,4,1,velocity);
  host.wader(contact,point,velocity,1.5);
  host.prepare(dt,camera);host.frame[FRAME.LEVEL]=-2;
  host.frame[FRAME.SUN]=.25;host.frame[FRAME.SUN+1]=-.75;host.frame[FRAME.SUN+2]=1;
  host.frame[FRAME.OCEAN]=i<180?1:0;host.frame[FRAME.STORM]=1;host.frame[FRAME.SHORE_QUALITY]=i<160?2:i<180?0:1;
  doubles(host.frame);for(let j=0;j<host.frame[FRAME.COMMANDS];j++){const at=((host.frame[FRAME.COMMAND_START]+j)%LIMITS.COMMANDS)*COMMAND.STRIDE;doubles(host.commands.subarray(at,at+COMMAND.STRIDE));}
  host.owner.update();
  u32(tiles.length);for(const tile of tiles){u32(tile.length/9);doubles(tile);}
  u32(probes.length);for(const probe of probes)u32(probe?1:0);
  doubles(host.result);
  const vertices=host.result[RESULT.QUADS]*4,count=host.result[RESULT.SHORE_COUNT];
  floats(host.positions.subarray(0,vertices*3));floats(host.uvs.subarray(0,vertices*2));floats(host.alpha.subarray(0,vertices));
  floats(host.shore.subarray(0,count*4));floats(host.phases.subarray(0,count));
  const storm=host.result[RESULT.STORM_COUNT];floats(host.stormPositions.subarray(0,storm*3));floats(host.stormTangents.subarray(0,storm*3));
  floats(host.stormEdges.subarray(0,storm));floats(host.stormWidths.subarray(0,storm));floats(host.stormEnergies.subarray(0,storm));
}
fs.writeFileSync(file,Buffer.concat(chunks));console.log(`${frames} surface frames captured`);
