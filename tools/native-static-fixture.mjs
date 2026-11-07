// Compare cold native algorithms with the browser constructors on synthetic
// authored inputs. The oracle reads source directly; it never changes src/.
import fs from 'node:fs';
import {execFileSync} from 'node:child_process';
import assert from 'node:assert/strict';
import * as THREE from 'three';
const source=fs.readFileSync(new URL('../src/foliage.js',import.meta.url),'utf8');
const body=source.slice(source.indexOf('class Seeded'),source.indexOf('// One batch of camera-facing cards'));
const sun=new THREE.Vector3(0.2,-0.9,0.3).normalize();
const oracle=new Function('THREE','lighting','UP',body+';return {Seeded,buildGrass,buildLeaves};')(THREE,{sunDirection:{value:sun}},new THREE.Vector3(0,1,0));
const probe=process.env.BFE_STATIC_FIXTURE||'rust/target/debug/examples/static-fixture';
const native=v=>JSON.parse(execFileSync(probe,{input:JSON.stringify(v),encoding:'utf8'}));
const definitions=[
 {base:'grasspatch',NumParticles:35,BoxSize:[2.8,2.8],MinSize:1.2,MaxSize:1.5,DarknessMin:0.4,DarknessMax:1},
 {base:'grasspatch',NumParticles:50,RadiusFadeMin:2,RadiusFadeMax:8,MinSize:1,MaxSize:3,YOffset:0.2},
 {base:'leafpatch',NumParticles:55,Seed:321651321,Radius:4,Height:4,HeightScale:1,MinSize:1.7,MaxSize:2,DarknessMin:0.4,DarknessMax:1,Offset:[1,2,3]},
 {base:'leafpatch',NumParticles:50,Seed:651684651,Radius:1.3,Height:13,HeightScale:1.6,ConeHeight:8,MinSize:1.2,MaxSize:1.4},
 {base:'leafpatch',NumParticles:50,Seed:937377218,Vine:[2,0],MinSize:2,MaxSize:2.5,VineLength:[2,4],VineSpread:0.6},
 {base:'leafpatch',NumParticles:13,Seed:328552794,BoxSize:[4,5,6],MinSize:1,MaxSize:2},
];
for(const definition of definitions){
 const random=new oracle.Seeded(37),saved=Math.random;Math.random=()=>random.next();
 let cards;try{cards=definition.base==='grasspatch'?oracle.buildGrass(definition):oracle.buildLeaves(definition);}finally{Math.random=saved;}
 const expected=cards.flatMap(c=>definition.base==='grasspatch'?[c.x,c.y,c.z,c.grey,c.width,c.height,c.phase,c.rate,c.part]:[c.p.x,c.p.y,c.p.z,c.grey,c.size,0,c.phase,0,c.part]);
 const actual=native({kind:'foliage',definition,sun:sun.toArray()});
 assert.equal(actual.length,expected.length);
 // The uploaded geometry rounds authored cards to Float32. Keep a precise
 // f64 delta too, so platform Math.hypot differences remain visible.
 let delta=0;for(let i=0;i<expected.length;i++){delta=Math.max(delta,Math.abs(actual[i]-expected[i]));assert.equal(Math.fround(actual[i]),Math.fround(expected[i]),`${definition.base} scalar ${i}`);}
 console.log(JSON.stringify({kind:definition.base,count:cards.length,maxFloat64Delta:delta,uploadedScalarsExact:true}));
}
// Water.surface itself only constructs Three shells after these arrays. Run
// that method with a synthetic receiver and compare its typed geometry.
const water=fs.readFileSync(new URL('../src/water.js',import.meta.url),'utf8');
const method=water.slice(water.indexOf('  surface(divisions'),water.indexOf('  ringMesh(map)'));
const surface=new Function('THREE',`return ({${method}}).surface`)(THREE);
const data={cell:32,height:-4,offset:[16,16],cells:[[0,0,0],[1,0,1]],patches:[{positions:[[0,-4,0],[1,-4,0],[0,-4,1]],uvs:[[0,0],[1,0],[0,1]],indices:[0,1,2]}]};
for(const [divisions,rows] of [[1,1],[3,7],[16,16]]){
 const g=surface.call({...data,data},divisions,rows),p=g.attributes.position.array,uv=g.attributes.uv.array;
 const vertices=[];for(let i=0;i<p.length/3;i++)vertices.push(...p.subarray(i*3,i*3+3),...uv.subarray(i*2,i*2+2));
 const actual=native({kind:'water',data,divisions,rows});assert.deepEqual(actual,{vertices,index:Array.from(g.index.array)});console.log(JSON.stringify({kind:'water',divisions,rows,vertices:p.length/3,exact:true}));
}

const oceanSource=fs.readFileSync(new URL('../src/ocean-surface.js',import.meta.url),'utf8');
const ocean=new Function('THREE',oceanSource.slice(oceanSource.indexOf('export function oceanGeometry'),oceanSource.indexOf('export class OceanSurface')).replace('export function','function')+';return oceanGeometry;')(THREE)();
assert.deepEqual(native({kind:'ocean'}),{positions:Array.from(ocean.attributes.position.array),index:Array.from(ocean.index.array)});console.log(JSON.stringify({kind:'ocean',vertices:ocean.attributes.position.count,exact:true}));
