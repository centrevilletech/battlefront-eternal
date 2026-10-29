import test from 'node:test';
import assert from 'node:assert/strict';
import { contentHash } from '../src/runtime/content-hash.js';

test('gameplay fingerprint detects changed or missing definitions across peer imports', async () => {
  const files = new Map([['/maps/nab2/map.json', JSON.stringify({ mission: { sides: ['cis'] } })], ['/sides/rep/units.json', '{"health":100}']]);
  const fetcher = async path => files.has(path) ? new Response(files.get(path)) : new Response('', { status: 404 });
  const first = await contentHash('?map=nab2&side=rep', fetcher);
  assert.match(first, /^bfe-content-sha256-[a-f0-9]{64}$/);
  assert.equal(await contentHash('?side=rep&map=nab2&network=1', fetcher), first);
  files.set('/sides/rep/units.json', '{"health":101}');
  assert.notEqual(await contentHash('?map=nab2&side=rep', fetcher), first);
  files.delete('/sides/rep/units.json');
  assert.notEqual(await contentHash('?map=nab2&side=rep', fetcher), first);
  await assert.rejects(contentHash('?map=../outside', fetcher), /Invalid/);
});

test('independent animation bone ordering hashes equally without hiding changed gameplay', async () => {
  const files = new Map([['/maps/nab2/map.json', '{}'], ['/common/anims.bin', new Uint8Array([1, 2, 3])]]);
  const fetcher = async path => files.has(path) ? new Response(files.get(path)) : new Response('', { status: 404 });
  const put = data => files.set('/common/anims.json', JSON.stringify(data));
  put({ bones: ['root', 'spine'], clips: { walk: { frames: 2, offset: 0, tracks: [0, 1], travel: [1, 0, 0] } } });
  const original = await contentHash('', fetcher);
  const permuted = { clips: { walk: { travel: [1, 0, 0], tracks: [1, 0], offset: 0, frames: 2 } }, bones: ['spine', 'root'] };
  put(permuted);
  assert.equal(await contentHash('', fetcher), original);
  for (const change of [value => value.clips.walk.tracks.reverse(), value => value.clips.walk.travel[0]++, value => value.clips.walk.offset++, value => value.clips.walk.frames++]) {
    const changed = structuredClone(permuted); change(changed); put(changed);
    assert.notEqual(await contentHash('', fetcher), original);
  }
  put(permuted); files.set('/common/anims.bin', new Uint8Array([1, 2, 4]));
  assert.notEqual(await contentHash('', fetcher), original);
  put({ bones: ['root'], clips: { walk: { tracks: [1] } } });
  await assert.rejects(contentHash('', fetcher), /Invalid animation track/);
});

test('all gameplay JSON including the map ignores Windows line endings, whitespace and object key order', async () => {
  const files=new Map([
    ['/maps/nab2/map.json',{mission:{sides:['rep','cis'],name:'nab2c'},spawns:{rep:[[1,2,3]]}}],
    ['/maps/nab2/ai-mission.json',{teams:{1:{units:32,classes:['trooper','sniper']}},goals:[{weight:1,kind:'capture'}]}],
    ['/sides/rep/ai.json',{trooper:{speed:7,aim:{spread:.1,enabled:true},label:'line\nvalue'}}],
    ['/common/common.json',{cameraMotions:{run:[[1,2],[3,4]]}}],
    ['/sides/cis/units.json',{units:{droid:{health:100}}}],
  ]);
  const reverse=value=>Array.isArray(value)?value.map(reverse):value&&typeof value==='object'?Object.fromEntries(Object.entries(value).reverse().map(([key,v])=>[key,reverse(v)])):value;
  const hash=formatter=>contentHash('?map=nab2&side=rep',async path=>files.has(path)?new Response(formatter(files.get(path))):new Response('',{status:404}));
  const compact=await hash(value=>JSON.stringify(value));
  assert.equal(await hash(value=>JSON.stringify(reverse(value),null,1).replace(/\n/g,'\r\n')),compact);
  assert.equal(await hash(value=>'\n\t'+JSON.stringify(reverse(value),null,4)+'\n'),compact);
  files.get('/maps/nab2/map.json').spawns.rep[0][0]++;
  assert.notEqual(await hash(JSON.stringify),compact,'map gameplay changes remain incompatible');
});

test('canonical content still distinguishes array order, primitive types, missing files and binary changes',async()=>{
  const files=new Map([['/maps/nab2/map.json','{}'],['/sides/rep/ai.json','{"path":[1,2],"flag":true,"n":0}'],['/maps/nab2/collision.bin',new Uint8Array([1,2,3])]]);
  const hash=()=>contentHash('',async path=>files.has(path)?new Response(files.get(path)):new Response('',{status:404}));
  const original=await hash();
  for(const changed of ['{"path":[2,1],"flag":true,"n":0}','{"path":[1,2],"flag":1,"n":0}','{"path":[1,2],"flag":true,"n":-0}','{"path":[1,2],"flag":true}']){
    files.set('/sides/rep/ai.json',changed);assert.notEqual(await hash(),original,changed);
  }
  files.delete('/sides/rep/ai.json');assert.notEqual(await hash(),original);
  files.set('/sides/rep/ai.json','{"path":[1,2],"flag":true,"n":0}');
  files.set('/maps/nab2/collision.bin',new Uint8Array([1,2,4]));assert.notEqual(await hash(),original,'collision bytes remain strict');
  files.set('/sides/rep/ai.json','{"n":1e400}');await assert.rejects(hash(),/Invalid JSON number/);
  files.set('/sides/rep/ai.json','not JSON');await assert.rejects(hash(),SyntaxError);
});

test('a battle\'s fingerprint leaves out its flyers\' clips (drawn only) but keeps its walkers\' and every class', async () => {
  const clip = (frames) => ({ fps: 30, bones: ['dummyroot', 'gear'], clips: { takeoff: { frames, flags: 0, offset: 0, tracks: [0, 1], travel: [0, 0, 0] } } });
  const pack = (animations, maxSpeed = '72.0') => ({ version: 1, animations,
    classes: { rep_fly_jedifighter: { base: 'flyer', model: 'rep_fly_jedifighter', settings: { MaxSpeed: maxSpeed } }, rep_walk_atte: { base: 'walker', model: 'rep_walk_atte', settings: {} } },
    spawns: [], models: {} });
  const files = new Map([['/maps/bes1/map.json', JSON.stringify({ missions: { bes1r: { mission: { era: 'cw', sides: ['rep', 'cis'], name: 'bes1r' }, vehicles: 'vehicles/bes1r/vehicles.json' } } })]]);
  const fetcher = async path => files.has(path) ? new Response(files.get(path)) : new Response('', { status: 404 });
  const hash = (value) => { files.set('/maps/bes1/vehicles/bes1r/vehicles.json', JSON.stringify(value)); return contentHash('?map=bes1&side=rep', fetcher); };
  const walker = { rep_walk_atte: { ...clip(41), clips: { idle: { frames: 41, flags: 0, offset: 0, tracks: [0], travel: [0, 0, 0] } } } };
  const before = await hash(pack(walker));
  assert.equal(await hash(pack({ ...walker, rep_fly_jedifighter: clip(80) })), before, 'a pack with its flyers\' clips plays with one prepared before them');
  assert.equal(await hash(pack({ rep_fly_jedifighter: clip(81), ...walker })), before);
  assert.notEqual(await hash(pack({ rep_walk_atte: { ...walker.rep_walk_atte, clips: { idle: { ...walker.rep_walk_atte.clips.idle, frames: 42 } } } })), before, 'a walker\'s clips stay gameplay');
  assert.notEqual(await hash(pack({ ...walker, rep_fly_jedifighter: clip(80) }, '73.0')), before, 'a flyer\'s class stays gameplay');
  // A model a walker draws too is never left out.
  const shared = pack({ ...walker, rep_fly_jedifighter: clip(80) });
  shared.classes.rep_walk_jedi = { base: 'walker', model: 'rep_fly_jedifighter', settings: {} };
  const kept = await hash(shared);
  shared.animations.rep_fly_jedifighter = clip(81);
  assert.notEqual(await hash(shared), kept);
});
