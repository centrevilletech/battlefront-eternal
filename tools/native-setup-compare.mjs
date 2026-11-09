// Checks the native cold setup against the whole-world input corpus
// (docs/world-replay.md): for every recorded battle, the query, the water
// setup row, the fetched files (URLs and SHA-256) and every tick row the
// host stamped, against what the setup-rows example computes from the
// player's content. Node only decodes the recording and compares; every
// number comes from the Rust side.
//
//   node tools/native-setup-compare.mjs --corpus DIR --content DIR --driver BIN [--units-driver BIN] [--only REGEX] [--out FILE]
//
// The driver is rust/target/<profile>/examples/setup-rows (cargo build -p
// bfe-content-native --example setup-rows); tick rows come from sim's
// admission-rows-spawn example. Exit 1 when any case differs.
// --spawn-calls FILE checks construction inputs; --require-all-spawn requires
// every recorded spawn export. A partial capture needs --allow-prefix and
// is reported as incomplete. Its companion corpus supplies battle options.
// --registry-case DIR --registry-driver TEST_BIN checks archived tick rows
// and lends F3 registry facts to the native World counter's fixture test.
// TEST_BIN is the executable built by cargo test -p sim --lib --no-run.
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { createHash } from 'node:crypto';
import { createGunzip, gunzipSync } from 'node:zlib';
import { createInterface } from 'node:readline';
import { spawn, spawnSync } from 'node:child_process';
import { BlockHeaderLayout as H } from '../src/sim/gen/boundary.js';
import { KIND_UNIT, KIND_VEHICLE, KIND_UNITS } from '../src/sim/gen/index.js';
import { WorldTickInputLayout } from '../src/sim/gen/world.js';
import { F2RiderBirthMirror, syncF2RiderBirth } from '../src/sim/gen/f2-riders.js';
import { F3ChangeMirror, syncF3Change, F3BodySkeletonMirror, syncF3BodySkeleton, F3ColdStageMirror, syncF3ColdStage, F3AdoptionMirror, syncF3Adoption } from '../src/sim/gen/units.js';
const argv = process.argv.slice(2);
const opt = (name, fallback = null) => { const at = argv.indexOf(`--${name}`); return at < 0 ? fallback : argv[at + 1]; };
const corpus = opt('corpus'), content = opt('content', process.env.BFE_CONTENT_DIR), driver = opt('driver'), only = opt('only'), out = opt('out'), unitsDriver = opt('units-driver', driver && path.join(path.dirname(driver), 'admission-rows-spawn'));
if(opt('registry-case')) {
  const result=await registryCase(opt('registry-case'));
  console.log(JSON.stringify(result));process.exit(result.differences||result.inputDifferences||result.rows!==result.expectedRows?1:0);
}
if (opt('spawn-calls')) {
  if (!content || !unitsDriver) throw Error('spawn call comparison needs --content and --units-driver');
  await compareSpawnCalls(opt('spawn-calls'));
  process.exit(process.exitCode || 0);
}
if (!corpus || !content || !driver) { console.error('usage: node tools/native-setup-compare.mjs --corpus DIR --content DIR --driver BIN [--units-driver BIN] [--only REGEX] [--out FILE]'); process.exit(2); }
const cases = fs.readdirSync(corpus).filter(name => fs.existsSync(path.join(corpus, name, 'setup.json'))).filter(name => !only || new RegExp(only).test(name)).sort();
if (!cases.length) throw Error('no recorded battle matches');
// world-hash.mjs's own argument reading, for the options the driver takes.
function battleArgs(recorded) {
  const take = (name, fallback) => { const i = recorded.indexOf(`--${name}`); return i < 0 ? fallback : recorded[i + 1]; };
  const args = ['--map', take('map', 'nab2'), '--side', take('side', 'rep'), '--seed', take('seed', '1'), '--ai', take('ai', 'classic')];
  if (recorded.includes('--heroes')) args.push('--heroes');
  if (take('unit', null) !== null) args.push('--unit', take('unit'));
  return args;
}
// Tick rows cross as hex f64 bits: a signed zero or NaN payload survives.
const rowWords = encoded => { const bytes = Buffer.from(encoded, 'base64'), words = []; for (let at = 0; at < bytes.length; at += 8) words.push(bytes.readBigUInt64LE(at).toString(16).padStart(16, '0')); return words; };
function checkRows(args, frames) {
  return new Promise((resolve, reject) => {
    const child = spawn(unitsDriver, [content, ...args, '--check-units'], { stdio: ['pipe', 'pipe', 'inherit'] });
    let text = '';
    child.stdout.on('data', chunk => { text += chunk; });
    child.on('error', reject);
    child.on('close', code => code === 0 ? resolve(text.trim().split('\n').pop()) : reject(Error(`admission-rows-spawn exited ${code}`)));
    child.stdin.on('error', error => { if (error.code !== 'EPIPE') reject(error); });
    child.stdin.end(frames.map(frame=>JSON.stringify({battle:frame.battle,frame,worldRows:frame.world.map(rowWords)})).join('\n')+'\n');
  });
}
const results = [];
let failures = 0;
for (const label of cases) {
  const dir = path.join(corpus, label), setup = JSON.parse(fs.readFileSync(path.join(dir, 'setup.json'), 'utf8'));
  const args = battleArgs(setup.argv), problems = [];
  const run = spawnSync(driver, [content, ...args], { encoding: 'utf8', maxBuffer: 1 << 26 });
  if (run.status !== 0) { problems.push(`driver: ${run.stderr.trim()}`); }
  const native = run.status === 0 ? JSON.parse(run.stdout.trim().split('\n')[0]) : null;
  let rows = null;
  if (native) {
    if (native.query !== setup.options.query) problems.push(`query ${native.query} vs ${setup.options.query}`);
    if (native.era !== setup.options.era) problems.push(`era ${native.era} vs ${setup.options.era}`);
    for (const battle of setup.battles) if (battle.worldSetup !== native.worldSetup) problems.push(`setup row ${native.worldSetup} vs ${battle.worldSetup} (battle ${battle.index})`);
    const recorded = setup.assets, urls = new Set(native.assets.map(a => a.url));
    for (const url of Object.keys(recorded)) if (!urls.has(url)) problems.push(`not fetched natively: ${url}`);
    // The page fetches a soldier's body when its class first spawns; the
    // native loader prepares every soldier and droideka class before the
    // battle (units.js prepareSoldiers), so the recording's set is a subset.
    // Bodies prepared ahead are counted; any other extra file is a difference.
    let ahead = 0;
    for (const asset of native.assets) {
      if (!Object.hasOwn(recorded, asset.url)) { if (/^\/sides\/[^/]+\/actors\/[^/]+(\.json|\.bin|_index\.bin)$/.test(asset.url)) ahead++; else problems.push(`fetched only natively: ${asset.url}`); continue; }
      const want = recorded[asset.url];
      if ((want?.sha256 ?? null) !== asset.sha256 || (want?.bytes ?? 0) !== asset.bytes) problems.push(`bytes differ: ${asset.url}`);
    }
    const order = native.assets.map(a => a.url).filter(u => Object.hasOwn(recorded, u)), recordedOrder = Object.keys(recorded).filter(u => urls.has(u));
    const sameOrder = order.length === recordedOrder.length && order.every((u, i) => u === recordedOrder[i]);
    const frames = JSON.parse(gunzipSync(fs.readFileSync(path.join(dir, 'inputs.json.gz'))));
    if (frames.length !== setup.frames) problems.push(`frame count ${frames.length} vs ${setup.frames}`);
    rows = JSON.parse(await checkRows(args, frames));
    // This driver advances an actual native World. Every clock/options
    // word and its F3 registry count are computed before each recorded tick.
    const expectedRows=frames.reduce((n,frame)=>n+frame.world.length,0);
    if (!rows.unitsComputed || rows.rows!==expectedRows) problems.push('native unit count did not cover every tick');
    if (rows.unitDifferences) problems.push(`units word: ${rows.unitDifferences} differ`);
    if (rows.differences) problems.push(`tick rows: ${rows.differences} differ, first ${JSON.stringify(rows.first)}`);
    results.push({ label, args, problems, assets: native.assets.length, assetsRecorded: Object.keys(recorded).length, bodiesAhead: ahead, sameOrder, rows: rows.rows, rowDifferences: rows.differences, units: {computed:rows.unitsComputed,differences:rows.unitDifferences}, first: rows.first });
  } else results.push({ label, args, problems });
  if (problems.length) failures++;
  console.log(`${problems.length ? 'DIFF ' : 'ok   '}${label.padEnd(28)} setup ${native?.worldSetup ?? '-'} assets ${native?.assets.length ?? '-'}${native ? ` (${results.at(-1).bodiesAhead} bodies ahead${results.at(-1).sameOrder ? '' : ', order differs'})` : ''} rows ${rows?.rows ?? '-'}${problems.length ? '\n     ' + problems.join('\n     ') : ''}`);
}
const summary = { corpus, content, driver, unitsDriver, cases: results.length, failures, rows: results.reduce((n, r) => n + (r.rows || 0), 0), results };
if (out) fs.writeFileSync(out, JSON.stringify(summary, null, 1) + '\n');
console.log(`${results.length} battles, ${summary.rows} tick rows, ${failures} with differences`);
process.exit(failures ? 1 : 0);

// Recover only host import buffers from the call recorder's byte patches. sim_import_room
// clears its block before the host writes it; no simulation snapshot is used.
// Rust's generated Unit decoder resolves class/body fields in the driver.
async function compareSpawnCalls(file) {
  // A full recording can exceed V8's maximum string length. Stream it,
  // retaining only nested calls until their enclosing earlier call arrives.
  let complete=false;
  async function* orderedCalls() {
    const input = fs.createReadStream(file);
    const lines = createInterface({input:file.endsWith('.gz') ? input.pipe(createGunzip()) : input,crlfDelay:Infinity});
    const pending = new Map(); let next = 0;
    for await (const line of lines) {
      if (!line.trim()) continue;
      const row = JSON.parse(line);
      if(row.type==='end') {complete=true;if(row.calls!==next || pending.size)throw Error('end before every call returned');}
      if (row.type !== 'call') continue;
      if (row.sequence < next || pending.has(row.sequence)) throw Error('duplicate call sequence');
      pending.set(row.sequence,row);
      while (pending.has(next)) {const call=pending.get(next);pending.delete(next++);yield call;}
    }
    if (pending.size) throw Error(`incomplete call sequence at ${next}`);
    if(!complete&&!argv.includes('--allow-prefix'))throw Error('recording has no end; use --allow-prefix only for inspection');
  }
  const number = bits => { const bytes = Buffer.alloc(8); bytes.writeBigUInt64LE(BigInt(`0x${bits}`)); return bytes.readDoubleLE(); };
  const requests = [], imports = new Map(), current = new Map(), lives = new Map();
  let holders, completedClip, constructing, registryUnit, tail, roster, playersStarted=false;
  let registryAdds=[];const clipRooms=new Map();
  const spawnExports=new Set(['f3_factory_reserve','f3_factory_stage','f3_factory_resident','f3_factory_player_bind','f3_factory_player_spawn','f3_factory_finish','f3_factory_acquire','f3_adopt','f3_pose','units_droideka_attach','f2_seats_adopt','f2_turret_adopt','f2_rider_birth','f3_body_skeleton','units_muzzle_attach']);
  const recorded={};
  let strings = [], battle, covered = { f3_body_skeleton:0, f3_factory_stage:0, f3_factory_resident:0, f3_adopt:0, units_muzzle_attach:0, f2_rider_birth:0, units_droideka_attach:0 };
  for await (const call of orderedCalls()) {
    if(spawnExports.has(call.export))recorded[call.export]=(recorded[call.export]||0)+1;
    if (call.battle !== battle) {
      battle = call.battle; imports.clear(); current.clear(); lives.clear(); strings = [];
      holders={kind:'holders',vehicles:[],hulls:[],turrets:[],clips:[],walkerClasses:[],factory:[],strings:[]};requests.push(holders);clipRooms.clear();completedClip=undefined;constructing=undefined;registryUnit=undefined;tail=undefined;roster=undefined;playersStarted=false;registryAdds=[];
    }
    for (const write of call.writes || []) {
      const bytes = Buffer.from(write.bytes,'base64');
      for (const block of [...imports.values(),...clipRooms.values()]) {
        const from = Math.max(write.offset,block.at), to = Math.min(write.offset+bytes.length,block.at+block.bytes.length);
        if (from < to) bytes.copy(block.bytes,from-block.at,from-write.offset,to-write.offset);
      }
    }
    if (completedClip!==undefined) {
      clipRooms.delete(`data:${completedClip}`);clipRooms.delete(`tracks:${completedClip}`);completedClip=undefined;
    }
    if (call.intern) strings[call.intern.id] = call.intern.text.split('').map(c=>c.charCodeAt(0));
    const args = call.arguments.map(number);
    if(call.export==='players_init')playersStarted=true;
    if(call.export==='f4_tail_init')tail={row:call.args.words,heap:call.args.heap};
    if(call.export==='f4_roster_command'&&call.returned?.block)roster=call.returned.block;
    if(call.export==='f3_registry_add')registryAdds.push(call.returned.scalar);
    if(call.export==='f3_factory_reserve'||call.export==='f3_factory_player_bind') {
      holders.factory.push({export:call.export,arguments:call.arguments,row:call.args.words,heap:call.args.heap,
        registryUnit,registryAdds:registryAdds.slice(),tail,roster,local:!playersStarted});
    }
    if(call.export==='units_droideka_class') {
      if(!constructing)throw Error('droideka class before its body skeleton');
      holders.walkerClasses.push({unit:constructing.unit,id:call.returned.scalar});
    }
    if (call.export === 'clip_new') {
      const [frames,tracks]=args;
      holders.clips.push({id:number(call.returned.scalar),arguments:call.arguments,data:Buffer.alloc(frames*tracks*7*4),tracks:Buffer.alloc(tracks*4)});
    }
    if (call.export === 'clip_data' || call.export === 'clip_tracks') {
      const clip=holders.clips.find(clip=>clip.id===args[0]);if(!clip)throw Error('clip loan without creation');
      const field=call.export==='clip_data'?'data':'tracks';clipRooms.set(`${field}:${clip.id}`,{at:number(call.returned.scalar),bytes:clip[field]});
      if(field==='tracks')completedClip=clip.id;
    }
    if (call.export === 'sim_import_room' && ([KIND_UNIT,KIND_VEHICLE,KIND_UNITS].includes(args[0]))) {
      const [kind,count,stride,heap] = args, bytes = Buffer.alloc((H.words+count*stride+heap)*8);
      for (const [at,value] of [[H.rows,count],[H.stride,stride],[H.heap,H.words+count*stride],[H.heapWords,heap]]) bytes.writeDoubleLE(value,at*8);
      imports.set(kind,{at:number(call.returned.scalar),bytes,count,stride,heap});
    }
    if (call.export === 'f2_seats_adopt') holders.hulls.push({row:call.args.words,heap:call.args.heap,prepared:args[0]!==0});
    if (call.export === 'f2_turret_adopt') holders.turrets.push({row:call.args.words,heap:call.args.heap});
    if (call.export === 'sim_import' && ([KIND_UNIT,KIND_VEHICLE,KIND_UNITS].includes(args[0]))) {
      const block=imports.get(args[0]);if(!block)throw Error('body import without a room');
      let rows,heap;
      if(call.input?.block && call.args.words.length) {
        if(call.args.words.length%block.stride)throw Error('import row width');
        rows=[];for(let at=0;at<call.args.words.length;at+=block.stride)rows.push(call.args.words.slice(at,at+block.stride));
        heap=call.args.heap;
      } else {
        // Older recordings keep the same host input as byte patches.
        const word=at=>block.bytes.readDoubleLE(at*8),stride=word(H.stride),count=word(H.rows);
        const words=(at,n)=>Array.from({length:n},(_,i)=>block.bytes.readBigUInt64LE((at+i)*8).toString(16).padStart(16,'0'));
        rows=[];for(let i=0;i<count;i++)rows.push(words(H.words+i*stride,stride));
        heap=words(word(H.heap),word(H.heapWords));
      }
      if(args[0]===KIND_VEHICLE)for(const row of rows)holders.vehicles.push({row,heap});
      else if(args[0]===KIND_UNITS)registryUnit={row:rows[0],heap};
      else for(const row of rows) {
        const request={strings:strings.slice(),unit:{row,heap},checks:[]};
        holders.strings=strings.slice();requests.push(request);current.set(number(row[0]),request);lives.delete(number(row[0]));
      }
    }
    const decode = (sync, Mirror) => {
      const words=call.args.words.map(number),heap=call.args.heap.map(number);
      return sync({strings:{at:id=>id},refs:new Proxy({},{get:()=>({at:(slot,generation)=>({slot,generation})})})},words.concat(heap),0,words.length,new Mirror());
    };
    // F3 owns life changes after construction. Bind later rider calls from
    // preceding lifecycle commands, never from the rider row being checked.
    const retainLife=(key,life=key.life)=>{
      const old=lives.get(key.slot);
      lives.set(key.slot,{generation:key.generation,life:old?.generation===key.generation?Math.max(old.life,life):life});
    };
    if(call.export==='f3_command'&&call.args.words.length) {
      const change=decode(syncF3Change,F3ChangeMirror);
      retainLife(change.key,change.metadata?.life??(change.action==='spawn'?change.key.life+1:change.key.life));
    }
    let slot, cookie;
    if (call.export === 'f3_body_skeleton') {const key=decode(syncF3BodySkeleton,F3BodySkeletonMirror).key;slot=key.slot;retainLife(key);}
    if (call.export === 'f3_adopt' && call.args.words.length) {const key=decode(syncF3Adoption,F3AdoptionMirror).key;slot=key.slot;retainLife(key);}
    if (call.export === 'f3_factory_stage' || call.export === 'f3_factory_resident') { const stage=decode(syncF3ColdStage,F3ColdStageMirror);slot=stage.adoption.key.slot;cookie=stage.cookie; }
    if (call.export === 'units_muzzle_attach' || call.export==='units_droideka_attach') slot=args[0];
    if (call.export === 'f2_rider_birth') slot=decode(syncF2RiderBirth,F2RiderBirthMirror).unit.slot;
    if (slot !== undefined) {
      const request=current.get(slot); if (!request) throw Error(`${call.export} has no earlier Unit import for ${slot}`);
      if (call.export==='f3_body_skeleton') {request.strings=strings.slice();constructing=request;}
      const check={export:call.export,arguments:call.arguments,row:call.args.words,heap:call.args.heap};
      if (call.export==='f3_factory_stage' || call.export==='f3_factory_resident') check.cookie=cookie;
      if(call.export==='f2_rider_birth') {if(!lives.has(slot))throw Error('rider has no preceding F3 life');check.life=lives.get(slot).life;}
      request.checks.push(check);covered[call.export]++;
    }
  }
  const setupPath=`${file}.corpus/setup.json`;
  const setup=fs.existsSync(setupPath)?JSON.parse(fs.readFileSync(setupPath,'utf8')):null;
  const args=battleArgs(setup?.argv||[]);
  for(const name of ['map','side','seed','ai','unit']) {
    const value=opt(name);if(value===null)continue;
    const at=args.indexOf(`--${name}`);if(at<0)args.push(`--${name}`,value);else args[at+1]=value;
  }
  if(argv.includes('--heroes')&&!args.includes('--heroes'))args.push('--heroes');
  const child=spawn(unitsDriver,[content,...args,'--compare-imports'],{stdio:['pipe','pipe','inherit']});
  let output='';child.stdout.on('data',chunk=>{output+=chunk;});
  child.stdin.on('error',error=>{if(error.code!=='EPIPE')throw error;});
  const done=new Promise((resolve,reject)=>{child.on('error',reject);child.on('close',code=>code===0?resolve():reject(Error(`spawn comparison driver exited ${code}`)));});
  child.stdin.end(requests.map(r=>JSON.stringify(r.kind==='holders'?{...r,clips:r.clips.map(c=>({...c,data:c.data.toString('hex'),tracks:c.tracks.toString('hex')}))}:r)).join('\n')+'\n');
  await done;
  const summary=JSON.parse(output.trim().split('\n').pop());summary.covered=covered;summary.complete=complete;summary.recorded=recorded;
  summary.unchecked=Object.fromEntries(Object.entries(recorded).filter(([name,count])=>(summary.checked?.[name]||0)<count).map(([name,count])=>[name,count-(summary.checked?.[name]||0)]));
  if(out)fs.writeFileSync(out,JSON.stringify(summary,null,1)+'\n');
  console.log(JSON.stringify(summary));
  if(!summary.rows||summary.differences||(argv.includes('--require-all-spawn')&&Object.keys(summary.unchecked).length))process.exitCode=1;
}

// Unit/body owners supply identities and flags; the counter owns the count.
// Capture those explicit F3 registry inputs while recreating the archived
// scenario, then lend them to a native World. No expected count seeds F3.
async function registryCase(directory) {
  const driver=opt('registry-driver');if(!driver)throw Error('registry-case requires --registry-driver TEST_BIN');
  const bytes=fs.readFileSync(path.join(directory,'inputs.json.gz'));
  const setup=JSON.parse(fs.readFileSync(path.join(directory,'setup.json'),'utf8'));
  if(setup.version!==1||createHash('sha256').update(bytes).digest('hex')!==setup.inputsSha256)throw Error('invalid tick corpus');
  if(setup.runtime.v8!==process.versions.v8)throw Error('tick corpus V8 mismatch');
  const frames=JSON.parse(gunzipSync(bytes));
  if(frames.length!==setup.frames)throw Error('tick corpus frame count mismatch');
  const expected=frames.flatMap(frame=>frame.world);
  const directoryWork=fs.mkdtempSync(path.join(os.tmpdir(),'bfe-registry-'));
  const fixture=path.join(directoryWork,'registry.jsonl'),plugin=path.join(directoryWork,'watch.mjs');
  const monitor=Symbol.for('bfe.registry.oracle'),previousMonitor=globalThis[monitor];
  globalThis[monitor]={game:null};
  fs.writeFileSync(plugin,`export function battle({game}){globalThis[Symbol.for('bfe.registry.oracle')].game=game;}\n`);
  const fd=fs.openSync(fixture,'w');
  const instantiate=WebAssembly.instantiate,exit=process.exit,write=process.stdout.write;
  const previousArgv=process.argv,previousContent=process.env.BFE_CONTENT_DIR;
  const stopped={};let status=0,rows=0,inputDifferences=0,first=null,previousGame=null;const registry=new Map();
  WebAssembly.instantiate=async function(...args) {
    const result=await instantiate.apply(this,args),instance=result.instance??result,x=instance.exports;
    if(!x.world_step_run)return result;
    let address=null;
    const exports={...x,world_step_init(...args){address=x.world_step_init(...args);return address;},world_step_run(...args){
      if(address===null||rows>=expected.length)throw Error('unexpected registry-query tick');
      const recorded=Buffer.from(expected[rows],'base64');
      const input=Buffer.from(x.memory.buffer,address,WorldTickInputLayout.words*8);
      // The broad host fingerprint includes unrelated player layouts. Check
      // every tick word directly without rewriting the archived metadata.
      if(!recorded.equals(input)) {inputDifferences++;first??={row:rows,input:'world input differs'};}
      const game=globalThis[monitor].game;if(!game)throw Error('registry scenario has no battle');
      const reset=game!==previousGame;if(reset){registry.clear();previousGame=game;}
      const phase=game.unitsSim,changes=[],seen=new Set();
      for(const unit of phase.factory.built) {
        const binding=phase.bindings[unit.id];if(!binding)throw Error('built unit has no F3 binding');
        const key=binding.view.key;
        const fact={slot:key.slot,generation:key.generation,life:key.life,removed:Boolean(unit.removed),human:unit.networkHuman??null,native:Boolean(binding.nativeIntent)};
        seen.add(key.slot);const encoded=JSON.stringify(fact);
        if(registry.get(key.slot)?.encoded!==encoded){changes.push(fact);registry.set(key.slot,{encoded,fact});}
      }
      for(const [slot,old] of registry)if(!seen.has(slot)) {
        changes.push({...old.fact,removed:true});registry.delete(slot);
      }
      fs.writeSync(fd,JSON.stringify({reset,players:phase.nativePlayers,changes,units:recorded.readBigUInt64LE(WorldTickInputLayout.units*8).toString(16)})+'\n');
      rows++;return x.world_step_run(...args);
    }};
    const wrapped={exports};return result.instance?{...result,instance:wrapped}:wrapped;
  };
  process.exit=code=>{status=code;throw stopped;};
  process.stdout.write=function(chunk,encoding,callback){if(typeof encoding==='function')callback=encoding;callback?.();return true;};
  if(content)process.env.BFE_CONTENT_DIR=content;
  process.argv=[process.argv[0],new URL('./world-hash.mjs',import.meta.url).pathname,...setup.argv,'--plugin',plugin];
  try {
    try{await import('./world-hash.mjs');}catch(error){if(error!==stopped)throw error;}
  } finally {
    WebAssembly.instantiate=instantiate;process.exit=exit;process.stdout.write=write;process.argv=previousArgv;fs.closeSync(fd);
    if(previousContent===undefined)delete process.env.BFE_CONTENT_DIR;else process.env.BFE_CONTENT_DIR=previousContent;
    if(previousMonitor===undefined)delete globalThis[monitor];else globalThis[monitor]=previousMonitor;
  }
  try {
    if(status)throw Error(`world scenario exited ${status}`);
    const run=spawnSync(driver,['--exact','world::admit::bodies::count::tests::recorded_foreign_registry_words','--nocapture'],
      {encoding:'utf8',env:{...process.env,BFE_F3_REGISTRY_ORACLE:fixture}});
    const match=run.stdout?.match(/REGISTRY_ORACLE (.*)/);
    if(!match)throw Error(`native registry driver failed: ${run.stderr||run.stdout||run.error}`);
    const result=JSON.parse(match[1]);if(run.status&& !result.differences)throw Error('native registry test failed');
    if(result.rows!==rows)throw Error('native registry test ended early');
    return {target:'native Rust World with explicit F3 registry inputs',...result,expectedRows:expected.length,inputDifferences,first:result.first??first};
  } finally {fs.rmSync(directoryWork,{recursive:true,force:true});}
}
