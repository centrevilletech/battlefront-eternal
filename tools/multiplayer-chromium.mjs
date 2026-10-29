import {actorCoverage} from './multiplayer-coverage.mjs';
// Retired: this measured the WebRTC transport (protocol 2) with the per-actor
// entity stream (BNE1), and decodes neither the snapshot-delta datagrams nor
// the native UDP transport of protocol 3. Use tools/net-probe.mjs for
// transport measurement and tests/multiplayer-browser.mjs for rendered clients.
// Actual browser SCTP, production NetworkHost and desktop identity bridge,
// without a renderer. Requires locally owned content and Playwright Chromium.
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'vite';
import { createNetworkBridge } from '../electron/network.js';

const number = (name, fallback) => Number(process.argv.find(value => value.startsWith(`--${name}=`))?.split('=')[1] ?? fallback);
// The gameplay protocol is mandatory; historical profiles require old checkouts.
assert.ok(!process.argv.some(arg=>/^--(fresh|journal|transients|state-life)=/.test(arg)), 'Legacy transport switches are not supported');
const players=number('players',2),seconds=number('seconds',20),warmup=number('warmup',5),lead=number('input-lead-ms',130),stateLife=50;
assert([1,2,8,32,64].includes(players) && seconds>=1 && seconds<=600 && warmup>=0 && warmup<=120 && lead>=0 && lead<=1000);
const engine=resolve(process.env.BFE_ENGINE_DIR || '.'),data=resolve(process.env.BFE_DATA_DIR || join(engine,'public'));
const profilePath=process.argv.find(value=>value.startsWith('--profile='))?.slice(10);
const serverRepo=resolve(process.env.BFE_SERVER_REPO || join(engine,'../battlefront-eternal-server'));
const revision=directory=>({revision:execFileSync('git',['rev-parse','HEAD'],{cwd:directory,encoding:'utf8'}).trim(),dirty:!!execFileSync('git',['status','--porcelain'],{cwd:directory,encoding:'utf8'}).trim()});
const source={engine:revision(engine),server:revision(serverRepo)};
function kernelCounterSnapshot() {
  try {
    const queues=JSON.parse(execFileSync('/usr/sbin/tc',['-j','-s','qdisc','show','dev','lo'],{encoding:'utf8',stdio:['ignore','pipe','pipe']}));
    const queue=queues.find(value=>value.kind==='netem');
    if(!queue)return null;
    return {sampledAt:performance.now(),handle:queue.handle,bytes:queue.bytes,packets:queue.packets,drops:queue.drops};
  } catch { return null; }
}
const {chromium}=await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const directory=await mkdtemp(join(tmpdir(),'bfe-chromium-rtc-'));
const stage=message=>console.error(`[Chromium qualification] ${message}`);
let browser,vite,child,initializationTimer,logs='';const bridges=[];
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const gamePort=number('port',18661),healthPort=number('health-port',18662);
try {
  stage('Starting local authoritative server');
  await writeFile(join(directory,'config.json'),JSON.stringify({name:'Local Chromium transport qualification',engine,data,maxPlayers:players,gamePort,bind:'0.0.0.0',discovery:false,onlineNoticeAccepted:false,rotation:['swbf1/nab2:rep:conquest']}),{mode:0o600});
  child=spawn(process.execPath,[join(serverRepo,'src/main.js'),'--serve'],{env:{...process.env,BFE_STATE_DIR:directory,PORT:String(healthPort),BFE_WORLD_METRICS:'1',...(profilePath?{BFE_WORLD_PROFILE:resolve(profilePath)}:{})},stdio:['ignore','pipe','pipe']});
  for(const stream of [child.stdout,child.stderr])stream.on('data',chunk=>{logs=(logs+chunk).slice(-12000);});
  let info;
  for(let attempt=0;attempt<120;attempt++) {
    if(child.exitCode!==null)throw Error(`Server stopped: ${logs}`);
    try {const response=await fetch(`http://127.0.0.1:${gamePort}/info`);if(response.ok){info=await response.json();break;}}catch{}
    await sleep(250);
  }
  assert(info,'Local gameplay server must become ready');
  const operatorToken=(await readFile(join(directory,'operator-token'),'utf8')).trim();
  const admin=async action=>{const response=await fetch(`http://127.0.0.1:${healthPort}/admin`,{method:'POST',headers:{'content-type':'application/json',authorization:'Bearer '+operatorToken},body:JSON.stringify({action})});const value=await response.json();assert(response.ok,value.error||'operator metrics failed');return value;};
  const processSample=async()=>{try{const raw=await readFile(`/proc/${child.pid}/stat`,'utf8'),fields=raw.slice(raw.lastIndexOf(')')+2).trim().split(/\s+/),status=await readFile(`/proc/${child.pid}/status`,'utf8');return {ticks:Number(fields[11])+Number(fields[12]),rssKiB:Number(status.match(/^VmRSS:\s+(\d+)/m)?.[1]||0)};}catch{return null;}};
  const clockTicks=process.platform==='linux'?Number(execFileSync('getconf',['CLK_TCK'],{encoding:'utf8'})):null;
  stage('Starting module server');
  vite=await createServer({configFile:false,root:engine,publicDir:data,logLevel:'error',optimizeDeps:{noDiscovery:true,include:[]},server:{host:'127.0.0.1',port:18663,strictPort:true,watch:null}});
  await vite.listen();
  stage('Launching Chromium');
  browser=await chromium.launch({args:['--disable-features=WebRtcHideLocalIpsWithMdns']});
  const errors=[],pages=[];
  const clientsPerRenderer=8;
  for(let group=0;group<Math.ceil(players/clientsPerRenderer);group++){
    const current=await browser.newPage();pages.push(current);
    current.on('pageerror',error=>{errors.push(error.message);stage(error.message);});
    current.on('console',message=>{if(message.type()==='error')stage(message.text());});
    await current.route('**/rtc-qualification',route=>route.fulfill({contentType:'text/html',body:'<!doctype html><title>RTC qualification</title>'}));
    await current.goto('http://127.0.0.1:18663/rtc-qualification');
    await current.exposeFunction('bfeRequest',(index,...args)=>bridges[index](...args));
  }
  // browser.newPage creates a separate context, avoiding one renderer event
  // loop becoming an artificial bottleneck for all 64 simulated clients.
  const page={evaluate:async(fn,arg)=>{
    const values=await Promise.all(pages.map((current,index)=>current.evaluate(fn,arg?.players?{...arg,players:Math.min(clientsPerRenderer,players-index*clientsPerRenderer),offset:index*clientsPerRenderer}:arg)));
    return values.flat();
  }};
  for(let index=0;index<players;index++)bridges.push(createNetworkBridge(join(directory,`client-${index}`),message=>pages[Math.floor(index/clientsPerRenderer)].evaluate(({index,message})=>window.dispatchEvent(new CustomEvent(`bfe-signal-${index}`,{detail:message})),{index,message}).catch(()=>{})));
  stage('Initializing browser transport clients');
  await Promise.race([page.evaluate(async ({players,offset,info,gamePort,lead,stateLife,probeFire})=>{
    const [{NetworkHost},{packetParts},{InputLead,advanceInputClock},{FixedClock}]=await Promise.all([import('/src/runtime/network-host.js'),import('/src/runtime/shared-packet.js'),import('/src/runtime/reconcile.js'),import('/src/runtime/fixed-clock.js')]);
    const {EntityReceiver}=await import('/src/runtime/network-entity-stream.js');
    const {decodeOwnPlayer}=await import('/src/runtime/network-entity-player.js');
    const {decodeWorldSummary}=await import('/src/runtime/network-entity-summary.js');
    window.clients=[];
    await Promise.all(Array.from({length:players},async (_,index)=>{
      index+=offset;
      const state={index,tick:0,ack:0,ackActions:0,packets:0,generation:0,sequence:0,lastPacket:0,gaps:[],actions:new Map(),actionLatency:[],errors:[],invalidatedActions:0,timings:[],transportAckCount:0};
      window.clients.push(state);
      const receiver=new EntityReceiver();state.observedActorLives=new Map();state.ownId=null;state.entityTicks=new Map();state.entityReceivedAt=new Map();state.diagnostics=[];state.ownTick=0;state.ownReceivedAt=0;state.progressReceivedAt=0;state.consumedActions=0;state.consumedFire=0;
      const inputClock=new FixedClock(),inputLead=new InputLead({maxLead:24,extrapolate:true});
      let inputAt=performance.now()/1000;
      if(probeFire)state.forceFire=false;
      const bridge={request:(...args)=>window.bfeRequest(index,...args),onSignal:callback=>{const listener=e=>callback(e.detail);window.addEventListener(`bfe-signal-${index}`,listener);return()=>window.removeEventListener(`bfe-signal-${index}`,listener);}};
      const host=state.host=new NetworkHost({bridge,serverId:info.id,address:'127.0.0.1',port:gamePort,onError:error=>state.errors.push(error.message),onInputAck:ack=>{state.transportAckCount++;inputLead.arrive(ack.serverTick,ack.receivedAt);},onEntityDiagnostics:actors=>{state.diagnostics=actors;},onEntityRegistry:descriptor=>{
        // Transport-only consumer: materialize authenticated registry names; no
        // content is executed or game definitions instantiated from this table.
        const registry={...descriptor};
        for(const key of ['classes','weapons','posts','bindingProfiles'])registry[key]=new Map(descriptor[key]);
        for(const [table,ids] of [['classes','classIds'],['weapons','weaponIds'],['posts','postIds']])registry[ids]=new Map([...registry[table]].map(([id,name])=>[name,id]));
        state.registry=registry;return registry;
      },onEntities:bytes=>{
        const packet=receiver.apply(bytes);if(!packet)return;
        const now=performance.now();if(state.lastPacket)state.gaps.push(now-state.lastPacket);
        if(packet.tick>state.tick)state.progressReceivedAt=now;state.tick=Math.max(state.tick,packet.tick);state.lastPacket=now;state.packets++;
        inputLead.arrive(packet.tick,now/1000);
        for(const record of packet.records){
          if(record.kind==='tombstone'){state.entityTicks.delete(record.id);state.entityReceivedAt.delete(record.id);continue;}
          if(record.kind==='soldier'||record.kind==='vehicle'){state.observedActorLives.set(`${record.kind}:${record.id}:${record.generation}`,now);if(record.tick>(state.entityTicks.get(record.id)??-1))state.entityReceivedAt.set(record.id,now);state.entityTicks.set(record.id,record.tick);}
          if(record.kind==='summary'&&record.schema===3)state.spawn=decodeWorldSummary(record,state.registry);
          if(record.kind!=='correction')continue;
          const own=decodeOwnPlayer(record);state.ownId=record.id;if(record.tick>state.ownTick)state.ownReceivedAt=now;state.ownTick=record.tick;state.ack=packet.ackInput;
          state.generation=record.generation;state.className=state.registry.classes.get(own.classId);
          for(const item of host.entityInput.outcomes.values())if(item.result&&item.result.tick<=record.tick){
            if(item.result.status==='consumed'){state.consumedActions++;if(item.edge.kind==='fire')state.consumedFire++;const sent=state.actions.get(item.sequence);if(sent!==undefined){state.actionLatency.push(now-sent);state.actions.delete(item.sequence);}}
          }
          host.entityInput.ack(Math.max(packet.ackInput,own.discardedInput));host.entityInput.applied(record.tick);
        }
        host.entityApplied(packet.ack);
      },onTicks:buffers=>{
        for(const buffer of buffers){
          const packet=packetParts(buffer).meta,now=performance.now();
          const timing=packet.transportTiming;
          if(timing&&Object.values(timing).every(Number.isFinite)){
            const {capturedAtUnixMs:c,compressionStartedAtUnixMs:s,compressionFinishedAtUnixMs:f,offeredAtUnixMs:o}=timing,received=Date.now();
            if([c,s,f,o].every(Number.isFinite))state.timings.push({captureToCompression:s-c,compression:f-s,compressionToOffer:o-f,offerToReceipt:received-o,captureToReceipt:received-c});
          }
          if(state.lastPacket)state.gaps.push(now-state.lastPacket);
          if(packet.generation!==state.generation){state.invalidatedActions+=state.actions.size;state.actions.clear();}
          Object.assign(state,{tick:packet.tick,ack:packet.ack,ackActions:packet.ackActions??0,generation:packet.generation,className:packet.playerClass,spawn:packet.personal?.spawn,lastPacket:now});state.packets++;
          inputLead.arrive(packet.tick,now/1000,packet.health?.dropped??0);
          for(const [id,sent] of state.actions)if(id<=state.ackActions){state.actionLatency.push(now-sent);state.actions.delete(id);}
          // This consumer materializes the transport state, not a rendered world.
          // It deliberately cannot certify collision, prediction or frame time.
          host.applied(packet,()=>null,()=>{});
        }
      }});
      await host.init({query:info.query});
      await host.call('start');
      state.timer=setInterval(()=>{
        const now=performance.now()/1000,dt=Math.min(.25,now-inputAt);inputAt=now;
        if(!state.tick||host.closed)return;
        const target=inputLead.target(now,state.tick,(host.path?.rtt||lead)/1000);
        const batch=[];
        advanceInputClock(inputClock,target,dt,(_h,tick)=>{
        const sequence=++state.sequence,actions=sequence%240===0?['jump']:[];
        const held=state.forceFire ?? sequence%120<60;
        const command={epoch:1,sequence,tick,generation:state.generation,down:[index%2?'KeyS':'KeyW',...(held?['Mouse0']:[])],actions,look:state.nextLook || [],mouseScale:1,invertMouse:false,firstPerson:false,aspect:16/9,viewTick:state.tick};
        if(state.spawn?.phase==='awaiting'&&sequence%30===0)command.human=[{kind:'spawn',className:state.className,post:null}];
        if(actions.length||command.human)state.actions.set(sequence,performance.now());
        state.nextLook=null;batch.push(command);
        },target+7);
        if(batch.length)host.inputs(batch);
      },1000/60);
    }));
  },{players,info,gamePort,lead,stateLife,probeFire:number('probe-fire',0)===1}),new Promise((_,reject)=>{initializationTimer=setTimeout(()=>reject(Error('Browser initialization exceeded 60 seconds')),60000);})]);
  clearTimeout(initializationTimer);
  stage('Clients connected; warming up');
  await sleep(warmup*1000);
  let aimProbe,fireProbe;
  if(number('probe-fire',0)===1){
    const before=await page.evaluate(()=>Promise.all(clients.map(async c=>({...await c.host.call('inspect'),consumedFire:c.consumedFire}))));
    await page.evaluate(()=>{for(const c of clients)c.forceFire=true;});
    await sleep(250);
    await page.evaluate(()=>{for(const c of clients)c.forceFire=false;});
    await sleep(750);
    const after=await page.evaluate(()=>Promise.all(clients.map(async c=>({...await c.host.call('inspect'),consumedFire:c.consumedFire}))));
    fireProbe=before.map((previous,i)=>({before:previous,after:after[i]}));
    for(const probe of fireProbe){
      assert.equal(probe.before.player.generation,probe.after.player.generation,'Fire probe needs the same life');
      assert.ok(probe.after.consumedFire>probe.before.consumedFire,'Browser fire must be accepted');
      assert.ok(probe.after.player.weapon.clip<probe.before.player.weapon.clip,'Browser fire must consume authoritative ammunition');
    }
    await page.evaluate(()=>{for(const c of clients)delete c.forceFire;});
  }
  if(number('probe-aim',0)===1){
    const before=await page.evaluate(()=>Promise.all(clients.map(async c=>({...await c.host.call('inspect'),consumedFire:c.consumedFire}))));
    await page.evaluate(()=>{for(const c of clients)c.nextLook=[[80,20]];});
    await sleep(1000);
    const after=await page.evaluate(()=>Promise.all(clients.map(async c=>({...await c.host.call('inspect'),consumedFire:c.consumedFire}))));
    aimProbe=before.map((previous,i)=>({before:previous,after:after[i]}));
    for(const probe of aimProbe){
      assert.equal(probe.before.player.generation,probe.after.player.generation,'Aim probe needs the same life before and after input');
      assert.ok(Math.abs(probe.after.player.aimYaw-probe.before.player.aimYaw)>.005,'Mouse command must change authoritative yaw');
      assert.ok(Math.abs(probe.after.player.aimPitch-probe.before.player.aimPitch)>.005,'Mouse command must change authoritative pitch');
    }
  }
  await page.evaluate(async()=>{for(const c of clients){
    c.firstTick=c.tick;c.timings=[];c.transportAckCount=0;c.gaps=[];c.actionLatency=[];c.invalidatedActions=0;c.actions.clear();c.initialBytes={};
    for(const stat of (await c.host.pc.getStats()).values())if(stat.type==='data-channel')c.initialBytes[stat.label]={received:stat.bytesReceived||0,sent:stat.bytesSent||0};
  }});
  const metricsReset=await admin('reset-metrics'),cpuBefore=await processSample();
  stage('Measuring transport');
  const ages=[],ownAges=[],entityAges=[],arrivalAges=[],progressWallAges=[],ownWallAges=[],entityWallAges=[];const kernelBefore=kernelCounterSnapshot(),begin=performance.now();
  while(performance.now()-begin<seconds*1000){
    const health=await (await fetch(`http://127.0.0.1:${healthPort}/healthz`)).json();
    const states=await page.evaluate(()=>clients.map(c=>({tick:c.tick,ownTick:c.ownTick,entityTicks:[...c.entityTicks.values()],age:performance.now()-c.lastPacket,progressWallAge:c.progressReceivedAt?performance.now()-c.progressReceivedAt:null,ownWallAge:c.ownReceivedAt?performance.now()-c.ownReceivedAt:null,entityWallAges:[...c.entityReceivedAt.values()].map(at=>performance.now()-at)})));
    for(const state of states){ages.push(Math.max(0,health.tick-state.tick)*1000/60);arrivalAges.push(state.age);if(state.progressWallAge!==null)progressWallAges.push(state.progressWallAge);if(state.ownWallAge!==null)ownWallAges.push(state.ownWallAge);entityWallAges.push(...state.entityWallAges);if(state.ownTick)ownAges.push(Math.max(0,health.tick-state.ownTick)*1000/60);for(const tick of state.entityTicks)entityAges.push(Math.max(0,health.tick-tick)*1000/60);}
    await sleep(250);
  }
  const elapsed=(performance.now()-begin)/1000;
  const kernelAfter=kernelCounterSnapshot(),cpuAfter=await processSample(),worldMetrics=await admin('metrics');
  const results=await page.evaluate(async()=>Promise.all(clients.map(async c=>{
    clearInterval(c.timer);let received=0,sent=0,rtt=0;const channels={};
    for(const stat of (await c.host.pc.getStats()).values()){
      if(stat.type==='data-channel'){
        const baseline=c.initialBytes[stat.label]||{received:0,sent:0};
        const bytes={received:Math.max(0,(stat.bytesReceived||0)-baseline.received),sent:Math.max(0,(stat.bytesSent||0)-baseline.sent)};
        channels[stat.label]=bytes;received+=bytes.received;sent+=bytes.sent;
      }
      if(stat.type==='candidate-pair'&&stat.state==='succeeded')rtt=Math.max(rtt,(stat.currentRoundTripTime||0)*1000);
    }
    return {observedActorLives:[...c.observedActorLives].map(([identity,at])=>[identity,performance.now()-at]),ownId:c.ownId,entity:c.host.entityMode,diagnostics:c.diagnostics,consumedActions:c.consumedActions,fresh:c.host.freshMode,timings:c.timings,transportAckCount:c.transportAckCount,stateLife:c.host.channels.get('state')?.maxPacketLifeTime,journal:c.host.journalMode,transients:c.host.transientsMode,index:c.index,tick:c.tick,firstTick:c.firstTick,ack:c.ack,ackActions:c.ackActions,gaps:c.gaps,actionLatency:c.actionLatency,errors:c.errors,closed:c.host.closed,packets:c.packets,received,sent,channels,rtt,invalidatedActions:c.invalidatedActions};
  })));
  const stats=values=>{values.sort((a,b)=>a-b);const at=p=>values[Math.min(values.length-1,Math.floor(values.length*p))]??0;return {count:values.length,p50:at(.5),p95:at(.95),p99:at(.99),max:values.at(-1)??0};};
  const report={kind:'Chromium RTC transport only; no 3D renderer or prediction',source,profile:'entity-v1',inputClock:'Production InputLead extrapolation and FixedClock; input-lead-ms is fallback RTT until ICE stats arrive',aimProbe,fireProbe,stateLifeMs:stateLife,negotiated:results.map(c=>({entity:!!c.entity,stateLifeMs:c.stateLife})),players,seconds:elapsed,warmup,payloadBytesPerPlayerSecond:{received:results.reduce((n,c)=>n+c.received,0)/elapsed/players,sent:results.reduce((n,c)=>n+c.sent,0)/elapsed/players},channelPayloadBytes:results.map(c=>c.channels),bandwidthMeaning:'Delivered RTCDataChannel payload; excludes retransmissions, SCTP/DTLS/UDP/IP overhead',connected:results.filter(c=>!c.closed).length,acknowledgedClients:results.filter(c=>c.ack>0).length,actionAcknowledgedClients:results.filter(c=>c.consumedActions>0).length,stateAgeMs:stats(ages),ownCorrectionAgeMs:stats(ownAges),remoteEntityAgeMs:stats(entityAges),timeSincePacketMs:stats(arrivalAges),packetGapMs:stats(results.flatMap(c=>c.gaps)),actionLatencyMs:stats(results.flatMap(c=>c.actionLatency)),effectiveTicksPerSecond:stats(results.map(c=>(c.tick-c.firstTick)/elapsed)),iceRttMs:stats(results.map(c=>c.rtt)),errors:[...errors,...results.flatMap(c=>c.errors)]};
  report.receivedUpdateWallAgeMs={stateProgress:stats(progressWallAges),ownCorrection:stats(ownWallAges),remoteEntity:stats(entityWallAges)};
  report.receivedUpdateWallAgeMeaning='Browser monotonic wall milliseconds since a newer state tick, own correction, or individual remote actor update was received; duplicate/old ticks do not reset age. Samples taken about every 250ms. Excludes capture-to-receipt delay and never-received actors; therefore not total authoritative freshness or full roster coverage.';
  report.actorCoverage=actorCoverage(worldMetrics.actorRoster,results);
  report.worldMetrics={resetTick:metricsReset.tick,tick:worldMetrics.tick,simMs:stats(worldMetrics.sim),publishMs:stats(worldMetrics.encode),totalMs:stats(worldMetrics.total),droppedTicks:worldMetrics.droppedTicks,liveUnits:worldMetrics.liveUnits,humans:worldMetrics.humans,armies:worldMetrics.armies,threadCpu:worldMetrics.threadCpu};
  report.serverCpu={worldWorkerCores:(worldMetrics.threadCpu.user+worldMetrics.threadCpu.system)/1e6/elapsed,totalProcessCores:cpuBefore&&cpuAfter?(cpuAfter.ticks-cpuBefore.ticks)/clockTicks/elapsed:null,rssKiB:cpuAfter?.rssKiB??null,meaning:'Average CPU core equivalents during the measured window; process includes worker and native RTC threads'};
  if(profilePath)report.worldProfile=resolve(profilePath);
  report.clientTopology={browserContexts:pages.length,clientsPerRenderer,producer:'One compact input batch per simulation frame, matching production coalescing'};
  report.transportAckCounts=results.map(c=>c.transportAckCount);
  report.contentHash=info.content;
  report.query=info.query;
  report.unsupportedActors=results.map(c=>c.diagnostics);
  report.stageTimingMeaning='Semantic capture/offer timestamps are not instrumented here. State/own/remote age is authoritative tick lag divided by 60, not wall-clock freshness when simulation slows. timeSincePacketMs and packetGapMs measure browser wall time.';
  report.remoteEntityAgeMeaning='Sampled latest received soldier/vehicle ticks, including dead actors. Never-received actors are excluded; this is not authoritative live-roster coverage proof.';
  if(kernelBefore&&kernelAfter&&kernelBefore.handle===kernelAfter.handle){
    const delta=Object.fromEntries(['bytes','packets','drops'].map(key=>[key,kernelAfter[key]-kernelBefore[key]]));
    if(Object.values(delta).every(value=>Number.isFinite(value)&&value>=0)){
      const seconds=(kernelAfter.sampledAt-kernelBefore.sampledAt)/1000;
      report.kernelUdpCounters={...delta,seconds,bytesPerPlayerSecond:delta.bytes/seconds/players,
        meaning:'Isolated netem loopback UDP queue counters over measurement window; both directions and retransmissions, not physical-link wire bytes'};
    }
  }
  console.log(JSON.stringify(report,null,2));
  assert.ok(results.every(c=>c.entity),'Every client must use the mandatory entity protocol');
  assert.ok(results.every(c=>c.stateLife===50),'Every state channel must use the fixed lifetime');
  assert.ok(ownAges.length>0 && entityAges.length>0,'Own and remote state must both arrive');
  assert.equal(report.connected,players);assert.equal(report.acknowledgedClients,players);assert.equal(report.errors.length,0);
} catch(error){console.error(logs);throw error;}
finally {
  clearTimeout(initializationTimer);
  for(const bridge of bridges)await bridge('close').catch(()=>{});
  await browser?.close();await vite?.close();
  if(child&&child.exitCode===null){child.kill('SIGTERM');await Promise.race([new Promise(resolve=>child.once('exit',resolve)),sleep(3000)]);if(child.exitCode===null)child.kill('SIGKILL');}
  await rm(directory,{recursive:true,force:true});
}
