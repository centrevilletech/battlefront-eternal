// Qualification uses the packaged host's selected factory and production worker.
// This module never upgrades an unavailable native world or retries with WASM.
import {resolve,dirname} from 'node:path';
import {pathToFileURL} from 'node:url';
import {createHash} from 'node:crypto';
import {maximum} from './telemetry.mjs';
import {createRequire} from 'node:module';
import {isMainThread,workerData,parentPort} from 'node:worker_threads';
export const qualificationBackend='native';
const moduleAt=(root,name)=>import(pathToFileURL(resolve(root,name)));
export async function createQualificationBackend({engine,manifest,sourceRoot}){
  const server=resolve(dirname(engine),'server');
  const {loadHostRuntime}=await moduleAt(server,'src/host-runtime.js');
  const runtime=await loadHostRuntime(engine);
  if(!runtime.manifest||runtime.manifest.engineBuild!==manifest.engineBuild)throw Error('Unpaired host runtime');
  if(runtime.worldBackend!=='native' || !runtime.addonIdentity?.nativeWorld || !runtime.addonIdentity?.nativePublication)throw Error('Native world operations unavailable');
  const entry=manifest.targets.find(entry=>entry.target===runtime.addonIdentity.target);
  if(!entry)throw Error('Native qualification target missing');
  const addon=createRequire(import.meta.url)(resolve(engine,entry.artifact));
  // Capture constructs source entities. Loading its bridge from that graph
  // preserves the generated reference classes; the artifact stays the bundle's.
  let simulationFactory=runtime.simulationFactory;
  if(sourceRoot){
    const source=resolve(sourceRoot),{engineBuild}=await moduleAt(source,'tools/host-contract.mjs');
    if(engineBuild(source)!==manifest.engineBuild)throw Error('Capture source differs from the paired bundle');
    const {loadNativeSim}=await moduleAt(source,'src/sim/native-backend.js');
    const reserve=Object.freeze({inputWords:8192,outputWords:8192,publicationBytes:65536});
    simulationFactory=()=>loadNativeSim(addon,reserve,manifest);
  }
  const counts=()=>{const [created,released]=addon.lifecycleCounts();return {created,released,live:created-released};};
  const {dataReader}=await moduleAt(server,'src/content.js');
  const [{createGame},{createMultiplayerWorld},{createSnapshotHost,publicationBuffersFor},{SnapshotClient},{createWireInputBridge},{worldState}]=await Promise.all([
    'src/game.js','src/runtime/multiplayer-world.js','src/runtime/snapshot-host.js','src/runtime/snapshot-client.js','src/runtime/wire-input.js','src/runtime/world-state.js',
  ].map(name=>moduleAt(sourceRoot??engine,name)));
  return {capabilities:{backend:'native',nativeWorld:true,nativePublication:true},simulationFactory,counts,async open(config){
    globalThis.fetch=await dataReader(config.data);
    let game,world,host,backend,closed=false,peak={},finishedAt=null;
    const clients=new Map(),weak=[];
    const fail=stage=>{if(config.failAfter===stage)throw Error(`Construction fixture: ${stage}`);};
    const close=()=>{
      if(closed)return false;closed=true;
      for(const client of clients.values())client.receiver.dispose();clients.clear();
      try { host?.dispose(); }
      finally {
        try { game?.dispose(); }
        finally { backend?.close();host=null;world=null;game=null;backend=null; }
      }
      return true;
    };
    try{
      game=await createGame({headless:true,query:config.query,simulationFactory:async()=>{
        backend=await simulationFactory();weak.push(new WeakRef(backend),new WeakRef(backend.owner));fail('instance');return backend;
      }});
      weak.push(new WeakRef(game));fail('game');
      world=createMultiplayerWorld(game,{maxPlayers:config.clients,armySize:32});fail('world');
      host=createSnapshotHost({game,world,stateBytesPerSecond:100000});fail('publisher');
    }catch(error){close();error.references=weak;throw error;}
    const check=()=>{if(closed)throw Error('Battle closed');};
    return {
      references:weak,
      async join(id){
        check();if(clients.has(id))throw Error('Duplicate harness client');
        await world.addPlayer(id,{spawn:true,teamSelect:false,reliableFire:true,separateActions:true,name:'Qualification player'});
        const receiver=new SnapshotClient();const encoder=createWireInputBridge(receiver.network);
        clients.set(id,{receiver,encoder,packets:0,complete:0});
        host.addClient(id);host.setReady(id);host.setStarted(id,true);fail('client');return world.player(id);
      },
      leave(id){check();const client=clients.get(id);if(!client)return false;client.receiver.dispose();clients.delete(id);host.removeClient(id);world.removePlayer(id);return true;},
      step(count=1){
        check();for(let i=0;i<count;i++){
          const faults=host.advanceEdges();if(faults.length)throw faults[0].error;
          const tick=world.step();const consumed=host.consumedEdges(tick);if(consumed.length)throw consumed[0].error;
          if(game.battle.finished&&finishedAt===null)finishedAt=tick;
          const publication=host.publish();if(publication.faults.length)throw publication.faults[0].error;
          for(const [id,client]of clients){
            for(const bytes of publicationBuffersFor(publication,id)){const result=client.receiver.receive(new Uint8Array(bytes));client.packets++;if(result?.complete)client.complete++;}
            host.input(id,client.encoder.keepalive(client.receiver.snapshotAck));
          }
        }
        return game.worldClock.tick;
      },
      identity(label){check();const id=game.simBridge.strings.id(label);return {stringId:id,string:game.simBridge.strings.at(id),unitId:game.playerUnit.id,generation:game.playerUnit.generation};},
      tick(){check();return game.worldClock.tick;},
      checksum(){check();return createHash('sha256').update(JSON.stringify(worldState(game))).digest('hex');},
      inspect(){check();return {tick:game.worldClock.tick,players:clients.size,finishedAt,clients:[...clients.values()].map(c=>({packets:c.packets,complete:c.complete}))};},
      diagnostics(){
        check();const js=process.memoryUsage(),native=backend.memoryDiagnostics();
        peak=maximum(peak,{reservedWords:native.reservedWords??0,heapUsed:js.heapUsed,external:js.external,arrayBuffers:js.arrayBuffers});
        return {tick:game.worldClock.tick,js,peak,native:{...native,available:true},
          clients:clients.size,units:game.units.all().length,finishedAt,
          publications:[...clients.keys()].map(id=>host.inspect(id))};
      },close,
    };
  }};
}

// --world-worker points here, but the message protocol and tick loop stay in
// the paired server. Its loader validates native capability before readiness.
if(!isMainThread && workerData?.engine) {
  try {
    const server=resolve(dirname(workerData.engine),'server');
    const {loadHostRuntime}=await moduleAt(server,'src/host-runtime.js');
    const runtime=await loadHostRuntime(workerData.engine);
    if(runtime.worldBackend!=='native')throw Error('Native world operations unavailable');
    await moduleAt(server,'src/world-worker.js');
  } catch(error) {parentPort.postMessage({type:'error',message:String(error?.stack??error)});}
}
