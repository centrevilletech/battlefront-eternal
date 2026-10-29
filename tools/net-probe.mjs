#!/usr/bin/env node
// Real-transport multiplayer probe without a browser. The dedicated server
// repository's world worker, UdpPeer and GameSession run on one side;
// production NetworkSession/EntityInputSender clients run on the other, over
// the real native UDP transport (src/net, BFE-UDP/1). It reports what a
// player would see: how often each remote actor is refreshed and how far it
// would visibly jump when a late sample lands, own correction cadence, how
// late shot events arrive, transport drops, and the server's simulation load.
//
//   BFE_DATA_DIR=<imported game data> BFE_SERVER_REPO=<server checkout> \
//   node tools/net-probe.mjs --players=2 --seconds=20
//
// --shape-up-kbps=200 --shape-down-kbps=2000 creates a disposable Linux
// network namespace with per-client HTB/netem queues. Requires passwordless
// sudo, ip, tc and tcpdump. --competing-up-kbps=20 adds real competing UDP.
// --pcap=/tmp/probe.pcap retains all-IP captures, including acknowledgements.
// --stall-seconds=1.5 --stall-after=5 --stall-client=0 drops that client's
// uplink for the stall window inside the measurement (scenario E).
// Remote split: --serve [--port=18452] [--udp-port=18451] runs only the
// server side (world worker, UDP game port, sessions) with a WebSocket
// metrics bridge; client boxes run --remote=<host>:<port> [--offset=32]
// [--token=...], connect straight to the UDP game port and print the same
// per-client report (server metrics are relayed over the bridge). Namespace
// shaping and capture stay local-only; the real network provides the path.
// --direct=<host> joins a running bfeserver (IPv4 or IPv6 literal) straight
// at its game port as desktop clients do, reading its identity and content
// from /info (TCP, --info-port=18451); there is no server metrics bridge.
// --input-hz=60 compares the previous held-state cadence.
// This input driver uses fixed --lead ticks; it is not the renderer/InputLead
// benchmark. Nothing touches public services or host interface qdiscs.
import {enterProbeNamespace,shapeProbeTraffic,captureProbeTraffic} from './net-probe-shaping.mjs';
import {Worker} from 'node:worker_threads';
import {EventEmitter} from 'node:events';
import {createRequire} from 'node:module';
import {mkdtemp,rm} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const arg=(name,fallback)=>{const found=process.argv.find(value=>value.startsWith(`--${name}=`));return found===undefined?fallback:found.slice(name.length+3);};
const players=Number(arg('players',2)),seconds=Number(arg('seconds',20)),warmup=Number(arg('warmup',8)),lead=Number(arg('lead',6));
// Downstream state impairment (application level, like scripts/load-peers.mjs
// in the server repository): one-way delay plus jitter in ms and a loss ratio.
const latency=Number(arg('latency',0)),jitter=Number(arg('jitter',0)),loss=Number(arg('loss',0)),smoothing=arg('smoothing','1')==='1';
const map=arg('map','nab2'),side=arg('side','rep'),armySize=Number(arg('army',32)),moving=arg('moving','1')==='1',firing=arg('fire','1')==='1';
// Scenario E: drop one client's uplink for a bounded window that starts
// --stall-after seconds into the measurement, then let it recover.
const stallClient=Number(arg('stall-client',0)),stallAfter=Number(arg('stall-after',5)),stallSeconds=Number(arg('stall-seconds',0));
const directHost=arg('direct',null),serveMode=process.argv.includes('--serve'),remoteHost=arg('remote',directHost),remoteMode=remoteHost!==null,servePort=Number(arg('port',18452)),probeToken=arg('token','bfe-probe'),clientOffset=Number(arg('offset',0));
let measurementBegunAt=null;
const engine=resolve(arg('engine',process.env.BFE_ENGINE_DIR??'.'));
const serverRepo=resolve(arg('server-repo',process.env.BFE_SERVER_REPO??join(engine,'../battlefront-eternal-server')));
const data=resolve(arg('data',process.env.BFE_DATA_DIR??join(engine,'public')));
if(!serveMode&&(!Number.isInteger(players)||players<1||players>64||!(seconds>=1)||!(warmup>=0)))throw Error('Invalid probe arguments');
if(!Number.isInteger(lead)||!Number.isInteger(stallClient)||stallClient<0||!(stallAfter>=0)||!(stallSeconds>=0)||!Number.isInteger(clientOffset)||clientOffset<0)throw Error('Invalid probe arguments');

await enterProbeNamespace(process.argv.slice(2));
const serverModule=name=>import(pathToFileURL(join(serverRepo,'src',name)));
const engineModule=name=>import(pathToFileURL(join(engine,'src/runtime',name)));
const netModule=name=>import(pathToFileURL(join(engine,'src/net',name)));
const [{UdpPeer},{GameSession},{loadIdentity},{validateNetworkContract,wireCompatibility}]=await Promise.all(['udp-peer.js','game-session.js','identity.js','network-contract.js'].map(serverModule));
const [{UdpEndpoint,CLOSE_CODE},{identityKeys}]=await Promise.all(['endpoint.js','crypto.js'].map(netModule));
const [{SnapshotClient},{decodeOwnPlayer},{decodeWorldSummary},{decodeCombat},{SnapshotTimeline,SnapshotPresentation}]=await Promise.all(['snapshot-client.js','network-entity-player.js','network-entity-summary.js','network-entity-combat.js','snapshot-timeline.js'].map(engineModule));
const {PlayoutClock}=await engineModule('playout.js');
const {readSnapshotHeader,SNAPSHOT_ENTITIES_MAGIC}=await engineModule('snapshot-codec.js');
const {restoreCompactSoldier,COMPACT_SOLDIER_SCHEMA}=await engineModule('network-entity-soldier-compact.js');
const {Vector3}=await import('three');
const [{NetworkSession},{EntityInputSender}]=await Promise.all(['network-session.js','network-entity-sender.js'].map(engineModule));

// The client end of a probe connection: the surface ProbeClient used from
// the old RTC Peer (JSON and binary events, send, transmit), over one
// session of a client UDP endpoint.
class UdpProbePeer extends EventEmitter {
  constructor(keys){super();this.keys=keys;this.authenticated=false;this.closed=false;this.sequence=0;this.lastSequence=0;
    this.channels=new Map([['state',{bufferedAmount:()=>0}]]);this.pc={getSelectedCandidatePair:()=>this.pair??null,rtt:()=>this.session?.rtt??null};}
  async connect({serverId,address,port,contract,content,name}){
    this.endpoint=new UdpEndpoint({identity:identityKeys(this.keys)});
    await this.endpoint.listen({host:address==='127.0.0.1'?'127.0.0.1':null,families:[address.includes(':')?'IPv6':'IPv4']});
    this.session=await this.endpoint.connect({serverId,candidates:[{address,port}],hello:{...wireCompatibility(contract),content,name},timeoutMs:30000});
    this.pair={local:{address:address==='127.0.0.1'?'127.0.0.1':address.includes(':')?'::':'0.0.0.0',port:this.endpoint.port},remote:{address,port}};
    this.session.on('message',(lane,payload)=>{
      if(typeof payload!=='string'){this.emit('binary',payload,lane);return;}
      let m;try{m=JSON.parse(payload);}catch{this.emit('fault','invalid JSON');return;}
      if(lane==='state'){if(!Number.isSafeInteger(m.sequence)||m.sequence<=this.lastSequence)return;this.lastSequence=m.sequence;}
      this.emit('message',lane,m);
    });
    this.session.once('close',info=>{this.closed=true;this.authenticated=false;this.emit('closed',info.reason||'closed');});
    this.authenticated=true;this.session.resume();
  }
  send(label,value){if(this.closed||!this.session)return false;return this.session.send(label,JSON.stringify(label==='state'?{...value,sequence:++this.sequence}:value));}
  transmit(label,_max,write){if(this.closed||!this.session)return false;write({sendMessage:value=>this.session.send(label,value),sendMessageBinary:value=>this.session.send(label,value)});return true;}
  close(){if(this.closed)return;this.closed=true;this.session?.close(CLOSE_CODE.NORMAL,'probe finished');setTimeout(()=>this.endpoint?.close(),50);}
}

const percentile=(values,p)=>{if(!values.length)return null;const sorted=[...values].sort((a,b)=>a-b);return sorted[Math.min(sorted.length-1,Math.floor(sorted.length*p))];};
const round=value=>value===null?null:Math.round(value*100)/100;
const summary=values=>({count:values.length,p50:round(percentile(values,.5)),p95:round(percentile(values,.95)),p99:round(percentile(values,.99)),max:round(percentile(values,1))});
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
// Same-process transit times (local mode only, one clock): the moment the
// session hands a datagram to the transport until the other end's transport
// delivers it. Independent of simulation speed, unlike the tick-anchored ages.
const transitStamps=new Map(),datagramKey=bytes=>Buffer.from(bytes.buffer??bytes,bytes.byteOffset??0,Math.min(20,bytes.byteLength)).toString('hex')+':'+bytes.byteLength;
const stampTransit=(client,direction,bytes)=>{const key=`${client}|${direction}|${datagramKey(bytes)}`;transitStamps.set(key,performance.now());if(transitStamps.size>200000)transitStamps.delete(transitStamps.keys().next().value);};
const takeTransit=(client,direction,bytes)=>{const key=`${client}|${direction}|${datagramKey(bytes)}`,at=transitStamps.get(key);if(at===undefined)return null;transitStamps.delete(key);return performance.now()-at;};
const once=(emitter,type)=>new Promise((res,rej)=>{const handler=m=>{if(m.type==='error'){emitter.off('message',handler);rej(Error(m.message+'\n'+(m.stack||'')));}else if(m.type===type){emitter.off('message',handler);res(m);}};emitter.on('message',handler);});
// Match the production aggregate playout clock, including wall-time advancement
// between publications. Sparse actor coverage is measured separately below.

const directory=await mkdtemp(join(tmpdir(),'bfe-net-probe-'));
let failure=null;
const worldMaxPlayers=serveMode?Math.min(100,Math.max(2,Number(arg('max-players',64)))):Math.max(2,players);
const world=remoteMode?null:new Worker(new URL('src/world-worker.js',pathToFileURL(serverRepo+'/')),{workerData:{engine,data,query:`?map=${map}&side=${side}&ai=classic&seed=1`,maxPlayers:worldMaxPlayers,armySize,stateBytesPerSecond:Number(arg('budget',100000)),metrics:true,profile:arg('profile',null)||undefined}});
world?.on('message',m=>{if(m.type==='error')failure=Error(m.message+'\n'+(m.stack||''));});
world?.on('error',error=>{failure=error;});
const waitFor=async(condition,label,timeoutMs=60000)=>{const begun=performance.now();for(;;){if(failure)throw failure;if(condition())return;if(performance.now()-begun>timeoutMs)throw Error(`Timeout: ${label}`);await sleep(50);}};

// Remote server side: the world worker, the UDP game port and a
// token-checked WebSocket bridge for metrics. Client processes on other
// machines connect to the game port over the real network, exactly as the
// paired in-process mode, minus shared cores.
const wsLibrary=createRequire(join(serverRepo,'package.json'))('ws');
if(serveMode){
  const ready=await once(world,'ready');
  world.networkContract=validateNetworkContract(ready.networkContract);
  const contract=world.networkContract,content=ready.content,serverKeys=loadIdentity(join(directory,'server.json'));
  world.postMessage({type:'start'});
  const bridge=new wsLibrary.WebSocketServer({host:'0.0.0.0',port:servePort});
  const livePeers=new Map();
  // --server-thread=main|worker runs bfeserver's own game port
  // (udp-server.js): in-process on this thread, or on its network thread
  // with client traffic crossing straight to the world. Without it, the
  // earlier probe-local wiring below (endpoint, UdpPeer, GameSession).
  const serverThread=arg('server-thread',null);
  let udpPort,udp=null,gameEndpoint=null;
  const {monitorEventLoopDelay}=await import('node:perf_hooks');
  const mainLoop=monitorEventLoopDelay({resolution:1});mainLoop.enable();
  if(serverThread){
    const {startUdpServer,startNetworkThread}=await serverModule('udp-server.js');
    udp=await (serverThread==='worker'?startNetworkThread:startUdpServer)({engine,keys:serverKeys,port:Number(arg('udp-port',18451)),host:null,maxSessions:256,maxPerAddress:Infinity,profile:arg('net-profile',null)});
    udp.setBattle({world,content,name:()=>'probe',welcome:()=>({}),isBanned:()=>false,maxPlayers:worldMaxPlayers});
    udpPort=udp.port;
  } else {
    // Clients connect straight to this UDP game port; the bridge carries only
    // the server's identity, address and metrics.
    gameEndpoint=new UdpEndpoint({identity:identityKeys(serverKeys),maxSessions:256,server:{hello:()=>({...wireCompatibility(contract),content}),admit:()=>({accept:true})}});
    udpPort=(await gameEndpoint.listen({port:Number(arg('udp-port',18451))})).port;
    gameEndpoint.on('session',session=>{
      const peer=new UdpPeer(session,{contract,content,closeCodes:CLOSE_CODE});
      const entry={peer,session:new GameSession(peer,world,session.id,{})};livePeers.set(session.id,entry);
      peer.once('closed',()=>{entry.session.close();livePeers.delete(session.id);});
      console.error(`[probe-serve] client ${session.hello?.name??session.id.slice(0,8)} ready (${livePeers.size} peers)`);
    });
  }
  // Timing of the thread that runs the game port, and of this one.
  const loopReport=()=>({mean:mainLoop.mean/1e6,p50:mainLoop.percentile(50)/1e6,p99:mainLoop.percentile(99)/1e6,max:mainLoop.max/1e6});
  const networkMetrics=async()=>({thread:serverThread??'probe',network:udp?await udp.metrics():null,mainLoopMs:loopReport()});
  let metricsWaiters=[],resetWaiters=[];
  world.on('message',async m=>{
    if(m.type==='metrics'){const pending=metricsWaiters;metricsWaiters=[];const extra=await networkMetrics();for(const send of pending)send({type:'metrics',metrics:{...m,...extra}});}
    else if(m.type==='metricsReset'){await udp?.resetMetrics();mainLoop.reset();const pending=resetWaiters;resetWaiters=[];for(const send of pending)send({type:'metricsReset',tick:m.tick});}
    else if(m.type==='error')console.error('[probe-serve] world error',m.message);
  });
  bridge.on('connection',(socket,request)=>{
    const url=new URL(request.url,'http://probe');
    if(url.searchParams.get('token')!==probeToken){socket.close(1008,'bad token');return;}
    const send=value=>{if(socket.readyState===wsLibrary.WebSocket.OPEN)socket.send(JSON.stringify(value));};
    const mine=new Set();
    socket.on('message',raw=>{
      let m;try{m=JSON.parse(raw);}catch{return;}
      if(m.type==='hello')send({type:'info',contract,content,serverId:identityKeys(serverKeys).id,udpPort});
      else if(m.type==='resetMetrics'){resetWaiters.push(send);world.postMessage({type:'resetMetrics'});}
      else if(m.type==='metrics'){metricsWaiters.push(send);world.postMessage({type:'metrics'});}
      else if(m.type==='transportBaseline'){if(udp)udp.diagnostics().then(values=>send({type:'transportBaseline',values}));else send({type:'transportBaseline',values:[...livePeers.values()].map(e=>e.session?.transportDiagnostics()??null)});}
    });
    socket.on('close',()=>{for(const id of mine){if(udp)udp.kick(id,'probe finished');const entry=livePeers.get(id);entry?.session?.close();entry?.peer.close();livePeers.delete(id);}console.error(`[probe-serve] bridge closed (${udp?udp.players().length:livePeers.size} peers left)`);});
    socket.on('message',raw=>{let m;try{m=JSON.parse(raw);}catch{return;}if(m.type==='mine'&&typeof m.id==='string')mine.add(m.id);});
  });
  console.error(`[probe-serve] world ready (map ${map}, army ${armySize}, max ${worldMaxPlayers}); UDP game port :${udpPort}, bridge on :${servePort}`);
  setInterval(()=>{if(failure){console.error(failure);process.exit(1);}},1000);
  await new Promise(()=>{});
}

class ProbeClient {
  constructor(index,{contract,content}){
    this.index=index;this.errors=[];this.reset();
    this.sequence=0;this.edgeId=0;this.generation=0;this.ownId=null;this.ownClassId=null;this.ownDead=false;this.estimate=null;
    this.deliveryLag=[];this.ackLag=[];this.feedbackLag=[];this.stateTransit=[];this.inputTransit=[];
    this.entityAck=null;
    this.receiver=new SnapshotClient();this.samples=new Map();this.ownTick=-1;this.ownReceivedAt=0;this.seenCombat=new Set();
    this.playout=new PlayoutClock({minDelay:4,maxDelay:10});this.lastFrameAt=null;this.wireVersions=new Map();
    this.ownBody=Object.fromEntries(['position','velocity','normal','previous','settled','contactMean'].map(key=>[key,new Vector3()]));this.hasOwnPosition=false;
    this.registry=null;this.spawn=null;this.lastSpawnEdgeAt=0;this.look=0;this.fireAt=0;this.firing=false;this.welcome=null;this.inspection=null;
    // The real client's remote presentation path, driven at 60 Hz below.
    this.timeline=new SnapshotTimeline();this.view=new SnapshotPresentation(this.timeline);this.targets=new Map();this.latestTick=0;this.frameTick=null;
    this.keys=loadIdentity(join(directory,`client-${index}.json`));this.id=identityKeys(this.keys).id;this.contract=contract;this.content=content;
    this.peer=new UdpProbePeer(this.keys);
    this.session=new NetworkSession({lifecycle:null});this.session.authenticated=this.session.entityMode=true;
    this.sentBytes=0;this.sentMessages=0;this.actionTimes=new Map();this.resultLatencies=[];this.edgeTrace=[];
    const owner=this;
    this.session.channels.set('state',{readyState:'open',get bufferedAmount(){return owner.peer.channels.get('state')?.bufferedAmount()??0;},send(value){
      if(stallSeconds>0&&owner.index===stallClient&&measurementBegunAt!==null){const elapsed=(performance.now()-measurementBegunAt)/1000;if(elapsed>=stallAfter&&elapsed<stallAfter+stallSeconds)return;}
      owner.sentBytes+=typeof value==='string'?Buffer.byteLength(value):value.byteLength;owner.sentMessages++;
      if(typeof value!=='string')stampTransit(owner.index,'up',value);
      const write=()=>{if(!owner.peer.closed)owner.peer.transmit('state',32768,ch=>typeof value==='string'?ch.sendMessage(value):ch.sendMessageBinary(Buffer.from(value)));};
      if(Number(arg('up-loss',0))&&Math.random()<Number(arg('up-loss',0)))return;
      if(Number(arg('up-delay',0)))setTimeout(write,Number(arg('up-delay',0)));else write();
    }});
    this.peer.on('fault',error=>this.errors.push(String(error)));
    this.peer.once('closed',reason=>{if(reason&&reason!=='closed')this.errors.push('closed: '+reason);});
    this.peer.on('message',(label,m)=>{try{this.message(label,m);}catch(error){this.errors.push(error.message);}});
    this.peer.on('binary',(bytes,label)=>{try{label==='state'&&this.entity(bytes);}catch(error){this.errors.push(error.message);}});
  }
  reset(){this.stats={kindBytes:{},poseDuplicates:0,stalePoses:0,nearGaps:[],nearWallGaps:[],nearFrames:0,nearHeld:0,nearCapped:0,packets:0,bytes:0,remoteGaps:[],remoteWallGaps:[],jumps:[],frameJumps:[],frozenFrames:0,drawnFrames:0,afterStallFrozen:0,afterStallDrawn:0,afterStallFrozenLate:0,jumpsByKind:{},frozenByKind:{},jumpsDisc:0,jumpsNoDisc:0,ownGaps:[],ownWallGaps:[],launchDelays:[],lateLaunches:0,launches:0,ownLaunches:0,ownLaunchDelays:[],corrections:0,actorsSeen:new Set(),inputsSent:0,fireEdges:0,spawnEdges:0,edgeResults:{}};}
  // One rendered frame of the remote actors at the presentation clock: the
  // per-frame movement beyond what the actor's own velocity explains is what
  // a player sees as a hitch or a teleport.
  frame(now){
    if(!this.latestTick)return;
    const dt=this.lastFrameAt===null?1/60:Math.max(0,(now-this.lastFrameAt)/1000);this.lastFrameAt=now;
    const tick=this.playout.advance(dt);this.frameTick=tick;
    this.view.update(tick,{playerId:this.ownId});
    for(const [id,target] of this.targets){
      const pose=target.networkPose;if(!pose){target.last=null;continue;}
      if(this.hasOwnPosition&&pose.health>0&&Math.hypot(...pose.velocity)>.5&&Math.hypot(pose.position[0]-this.ownBody.position.x,pose.position[1]-this.ownBody.position.y,pose.position[2]-this.ownBody.position.z)<35){
        this.stats.nearFrames++;if(pose.fromTick===pose.toTick)this.stats.nearHeld++;
        if(pose.extrapolatedTicks>=this.timeline.extrapolateTicks)this.stats.nearCapped++;
      }
      if(target.last&&dt>0){
        let moved=0,expected=0;
        for(let i=0;i<3;i++){moved+=(pose.position[i]-target.last[i])**2;expected+=(target.lastVelocity[i]*dt)**2;}
        const excess=Math.max(0,Math.sqrt(moved)-Math.sqrt(expected));this.stats.frameJumps.push(excess);this.stats.drawnFrames++;
        if(excess>1){this.stats.jumpsByKind[pose.kind]=(this.stats.jumpsByKind[pose.kind]??0)+1;if(pose.disc)this.stats.jumpsDisc++;else this.stats.jumpsNoDisc++;}
        const afterStall=stallSeconds>0&&this.index===stallClient&&measurementBegunAt!==null&&(now-measurementBegunAt)/1000>=stallAfter+stallSeconds+.1;
        if(afterStall)this.stats.afterStallDrawn++;
        if(moved<1e-10&&expected>1e-4){this.stats.frozenFrames++;this.stats.frozenByKind[pose.kind]=(this.stats.frozenByKind[pose.kind]??0)+1;if(afterStall)this.stats.afterStallFrozen++;if(stallSeconds>0&&this.index===stallClient&&measurementBegunAt!==null&&(now-measurementBegunAt)/1000>=stallAfter+stallSeconds+.6)this.stats.afterStallFrozenLate++;}
      }
      target.last??=[0,0,0];target.lastVelocity??=[0,0,0];
      for(let i=0;i<3;i++){target.last[i]=pose.position[i];target.lastVelocity[i]=pose.velocity[i];}
    }
  }
  serverTick(){return this.estimate?this.estimate.tick+(performance.now()-this.estimate.at)*60/1000:null;}
  message(label,m){
    if(m.type==='welcome'){this.welcome=m;this.generation=m.generation;this.ownId=m.playerId;this.peer.send('events',{type:'call',id:1,method:'start'});}
    else if(m.type==='entity-registry'){const r={...m.registry};for(const key of ['classes','weapons','posts','bindingProfiles'])r[key]=new Map(m.registry[key]);this.registry=r;r.classIds=new Map([...r.classes].map(([id,name])=>[name,id]));r.postIds=new Map([...r.posts].map(([id,name])=>[name,id]));this.session.entityInput=new EntityInputSender({registry:r,send:bytes=>this.session.sendEntityInput(bytes),snapshotAck:()=>this.session.snapshotAck,getAim:()=>({aimYaw:this.look/2048,aimPitch:0}),stateIntervalTicks:Number(arg('input-hz',30))===30?2:1});
    }
    else if(m.type==='input-ack'){this.session.receiveInputAck(m);const now=performance.now();if(!this.estimate||m.serverTick>this.serverTick())this.estimate={tick:m.serverTick,at:now};this.ackLag.push(Math.max(0,this.serverTick()-m.serverTick)*1000/60);}
    else if(m.type==='entity-edge-results'){
      this.session.entityInput.feedback(m);this.session.sendEntityEdgeAcknowledgement();
      for(const r of m.results){if(this.actionTimes.has(r.id)){this.resultLatencies.push(performance.now()-this.actionTimes.get(r.id));if(this.edgeTrace.length<128)this.edgeTrace.push({id:r.id,status:r.status,ageMs:performance.now()-this.actionTimes.get(r.id),commandTick:this.session.entityInput.outcomes.get(r.id)?.edge.tick,resultTick:r.tick,estimatedTick:this.serverTick(),receiptServerTick:this.session.transportAck?.serverTick});this.actionTimes.delete(r.id);this.stats.edgeResults[r.status]=(this.stats.edgeResults[r.status]??0)+1;}}
    }
    else if(m.type==='reply'){if(m.resultType==='inspection')this.inspection=m;if(m.error)this.errors.push('reply: '+m.error);}
    else if(m.type==='gameplay-error'||m.type==='version-error')this.errors.push(m.message);
    else if(m.type==='entity-diagnostics')this.diagnostics=m.actors;
    else if(m.type==='gameplay-events'){for(const r of m.records){try{const event=decodeCombat({kind:'combat',...r,payload:Uint8Array.from(Buffer.from(r.payload,'base64'))});if(event.schema===3||event.schema===4)this.feedbackLag.push(Math.max(0,this.serverTick()-event.tick)*1000/60);}catch{}}}
  }
  entity(bytes){
    const transit=takeTransit(this.index,'down',bytes);if(transit!==null)this.stateTransit.push(transit);
    const header=readSnapshotHeader(new Uint8Array(bytes));
    const datagramKind=header.magic===SNAPSHOT_ENTITIES_MAGIC?'entities':'own';
    this.stats.kindBytes[datagramKind]=(this.stats.kindBytes[datagramKind]??0)+bytes.length;
    const packet=this.receiver.receive(new Uint8Array(bytes));const now=performance.now();if(!packet)return;
    for(const record of packet.records)if(['soldier','vehicle','droideka'].includes(record.kind)){const previous=this.wireVersions.get(record.id);if(previous&&record.generation===previous.generation){if(record.tick===previous.tick)this.stats.poseDuplicates++;else if(record.tick<previous.tick)this.stats.stalePoses++;}if(!previous||record.generation>previous.generation||record.generation===previous.generation&&record.tick>previous.tick)this.wireVersions.set(record.id,{generation:record.generation,tick:record.tick});}
    const s=this.stats;s.packets++;s.bytes+=bytes.length;
    if(!this.estimate||packet.tick>this.serverTick())this.estimate={tick:packet.tick,at:now};
    // A tick gap can look healthy while a transport delivers an entire stream
    // seconds late. Compare against the wall-time clock anchored by its least
    // delayed arrival; this is excess delay, not synchronized one-way latency.
    this.deliveryLag.push(Math.max(0,this.serverTick()-packet.tick)*1000/60);
    if(packet.tick>this.latestTick){this.latestTick=packet.tick;this.playout.arrive(packet.tick,now/1000,packet.interval);}
    const beforePoses=new Map();
    for(const record of packet.records)if(['soldier','vehicle','droideka'].includes(record.kind)){const pose=this.timeline.sample(record.id,this.playout.clock);if(pose)beforePoses.set(record.id,pose);}
    if(packet.sample)this.timeline.addSnapshot(packet.sample.tick,packet.sample.poses,packet.sample.epoch);
    for(const record of packet.records){
      if(record.kind==='tombstone'){const target=this.targets.get(record.id);if(target){this.view.unbind(record.id);this.targets.delete(record.id);}continue;}
      if(record.kind!=='soldier'&&record.kind!=='vehicle'&&record.kind!=='droideka'||record.id===this.ownId)continue;
      const target=this.targets.get(record.id);
      if(!target||target.generation!==record.generation){const fresh={networkPose:null,generation:record.generation};this.targets.set(record.id,fresh);this.view.bind(record.id,record.generation,record.kind==='vehicle'?'vehicle':record.kind,fresh);}
    }
    if(packet.snapshotAck?.tick)this.session.setSnapshotAck(packet.snapshotAck);
    this.entityAck=packet.snapshotAck;
    const clock=this.playout.clock;
    for(const record of packet.records){
      if(record.kind==='correction'){
        const own=decodeOwnPlayer(record);this.hasOwnPosition=own.bodySchema===COMPACT_SOLDIER_SCHEMA&&!own.dead;if(this.hasOwnPosition)restoreCompactSoldier(this.ownBody,own.body);this.session.entityInput?.ack(packet.ackInput);this.session.entityInput?.applied(record.tick);
        if(record.generation!==this.generation){this.generation=record.generation;}
        this.ownId=record.id;this.ownClassId=own.classId;this.ownDead=own.dead;s.corrections++;
        if(this.ownTick>=0&&record.tick>this.ownTick){s.ownGaps.push(record.tick-this.ownTick);s.ownWallGaps.push(now-this.ownReceivedAt);}
        if(record.tick>this.ownTick){this.ownTick=record.tick;this.ownReceivedAt=now;}
        continue;
      }
      if(record.kind==='summary'){if(record.schema===3&&this.registry){try{const value=decodeWorldSummary(record,this.registry);if(value.playerId===this.ownId){if(value.phase!==this.spawn?.phase)(this.phases??=[]).push(value.phase);this.spawn=value;}}catch(error){this.errors.push('summary: '+error.message);}}continue;}
      if(record.kind==='combat'){
        if(this.seenCombat.has(record.id))continue;this.seenCombat.add(record.id);if(this.seenCombat.size>8192)this.seenCombat.delete(this.seenCombat.values().next().value);
        let event;try{event=decodeCombat(record);}catch{continue;}
        if(event.schema===3||event.schema===4)this.feedbackLag.push(Math.max(0,this.serverTick()-event.tick)*1000/60);
        if(event.schema===1||event.schema===6){s.launches++;const delay=packet.tick-event.launchTick;s.launchDelays.push(delay);if(delay>18)s.lateLaunches++;if(event.ownerId===this.ownId){s.ownLaunches++;s.ownLaunchDelays.push(delay);}}
        continue;
      }
      if(record.kind==='tombstone'){this.samples.delete(record.id);continue;}
      if(record.kind!=='soldier'&&record.kind!=='vehicle'&&record.kind!=='droideka')continue;
      if(record.id===this.ownId)continue;
      s.actorsSeen.add(record.id);
      const previous=this.samples.get(record.id);
      if(previous&&previous.generation===record.generation&&record.tick>previous.tick){
        s.remoteGaps.push(record.tick-previous.tick);s.remoteWallGaps.push(now-previous.at);
        if(this.hasOwnPosition&&record.health>0&&Math.hypot(...record.velocity)>.5&&Math.hypot(...record.position.map((value,i)=>value-this.ownBody.position.getComponent(i)))<35){s.nearGaps.push(record.tick-previous.tick);s.nearWallGaps.push(now-previous.at);}
        // Compare actual timeline samples, including its bounded braking and
        // discontinuity guards. Frame excess below includes presentation smoothing.
        const before=beforePoses.get(record.id),after=this.timeline.sample(record.id,clock);
        if(before&&after&&before.generation===after.generation)s.jumps.push(Math.hypot(...after.position.map((value,i)=>value-before.position[i])));
      }
      this.samples.set(record.id,{tick:record.tick,generation:record.generation,position:record.position,velocity:record.velocity,at:now});
    }
  }
  pump(now){
    if(!this.welcome||!this.peer.authenticated||this.peer.closed||!this.session.entityInput)return;
    this.session.sendEntityAcknowledgement();
    if(now-(this.ackTimerAt??0)>100){this.session.sendEntityEdgeAcknowledgement();this.ackTimerAt=now;}
    const estimate=this.serverTick();if(estimate===null)return;
    const tick=Math.floor(estimate)+lead,s=this.stats,human=[];
    if((this.spawn?.phase==='awaiting'||this.spawn?.phase==='unspawned')&&this.ownClassId&&now-this.lastSpawnEdgeAt>1500){this.lastSpawnEdgeAt=now;human.push({kind:'spawn',className:this.registry.classes.get(this.ownClassId),post:null});s.spawnEdges++;}
    if(firing&&!this.ownDead&&now-this.fireAt>(this.firing?200:1300)){this.fireAt=now;this.firing=!this.firing;s.fireEdges++;}
    if(moving)this.look+=3;
    const down=moving&&Math.floor(now/1500)%2===0?['KeyW']:[];if(this.firing)down.push('Mouse0');
    const command={epoch:1,generation:this.generation,sequence:++this.sequence,tick,down,look:moving?[[3,0]]:[],actions:[],human,mouseScale:1/2048,aspect:16/9,viewTick:Math.max(0,Math.floor(estimate)-8),firstPerson:true,invertMouse:false,flightSettings:{bindings:{},sensitivity:1,curve:1.35,invert:false}};
    const previous=this.session.entityInput.id;
    if(this.session.inputs([command]))s.inputsSent++;
    for(let id=previous+1;id<=this.session.entityInput.id;id++)this.actionTimes.set(id,now);
  }
  report(elapsedSeconds,transportBefore){
    const s=this.stats,transport=this.inspection?.transport??null,before=transportBefore??{};
    const scheduling=this.inspection?.snapshotHost??null;
    return {index:this.index,inspectionWarning:this.inspection?null:'Final reliable-channel inspection did not arrive within the bounded wait',edgeTrace:this.edgeTrace,inputDiagnostics:this.inspection?.inputDiagnostics,errors:this.errors,spawnPhases:(this.phases??[]).slice(0,12),welcomeTeamSelect:this.welcome?.teamSelect,packetsPerSecond:round(s.packets/elapsedSeconds),bytesPerSecond:round(s.bytes/elapsedSeconds),inputsPerSecond:round(s.inputsSent/elapsedSeconds),
      stateTransitMs:summary(this.stateTransit),inputTransitMs:summary(this.inputTransit),
      estimatedStateExcessDelayMs:summary(this.deliveryLag),estimatedAckExcessDelayMs:summary(this.ackLag),estimatedFeedbackAgeMs:summary(this.feedbackLag),
      kindBytesPerSecond:Object.fromEntries(Object.entries(s.kindBytes).map(([kind,bytes])=>[kind,round(bytes/elapsedSeconds)])),samePoseDuplicates:s.poseDuplicates,olderPoseRecords:s.stalePoses,
      nearMovingGapTicks:summary(s.nearGaps),nearMovingWallGapMs:summary(s.nearWallGaps),nearMovingFrames:s.nearFrames,nearMovingHeldFraction:s.nearFrames?s.nearHeld/s.nearFrames:null,nearMovingCappedFraction:s.nearFrames?s.nearCapped/s.nearFrames:null,
      actorsSeen:s.actorsSeen.size,remoteGapTicks:summary(s.remoteGaps),remoteWallGapMs:summary(s.remoteWallGaps),visibleJumpMetres:summary(s.jumps),
      drawnFrameExcessMetres:summary(s.frameJumps),drawnFramesOver20cm:s.frameJumps.filter(value=>value>.2).length,drawnFramesOver1m:s.frameJumps.filter(value=>value>1).length,frozenMovingFrames:s.frozenFrames,drawnFrames:s.drawnFrames,afterStallFrozenMovingFrames:s.afterStallFrozen,afterStallDrawnFrames:s.afterStallDrawn,afterStallFrozenLate:s.afterStallFrozenLate,drawnJumpsOver1mByKind:s.jumpsByKind,frozenByKind:s.frozenByKind,drawnJumpsDisc:s.jumpsDisc,drawnJumpsNoDisc:s.jumpsNoDisc,ownLaunches:s.ownLaunches,ownLaunchDelayTicks:summary(s.ownLaunchDelays),
      jumpsOverOneMetre:s.jumps.filter(value=>value>1).length,jumpsOverThreeMetres:s.jumps.filter(value=>value>3).length,
      ownCorrectionGapTicks:summary(s.ownGaps),ownCorrectionWallGapMs:summary(s.ownWallGaps),corrections:s.corrections,
      launches:s.launches,launchDelayTicks:summary(s.launchDelays),launchesTooLateForFlash:s.lateLaunches,fireEdges:s.fireEdges,spawnEdges:s.spawnEdges,edgeResults:s.edgeResults,
      transport:transport?Object.fromEntries(Object.entries(transport).map(([key,value])=>[key,typeof value==='number'&&key in before&&!['stateQueued','stateBuffered','pacedDelayMaxMs','rtt','minRtt','loss'].includes(key)?value-before[key]:value])):null,
      scheduling:scheduling?{snapshots:scheduling.snapshots,datagrams:scheduling.datagrams,bytes:scheduling.bytes,cosmeticsDropped:scheduling.cosmeticsDropped,emptyBaselines:scheduling.emptyBaselines,baselineTick:scheduling.baselineTick,encodes:scheduling.encodes}:null};
  }
}

const clients=[],servers=[],sessions=[];
let pumpTimer=null,shaping=null,capture=null,bridgeSocket=null,gameEndpoint=null;
try {
  let contract,content,serverKeys=null,remoteServer=null;
  const byIdentity=new Map();
  const bridgeWaiters={info:[],metrics:[],metricsReset:[]};
  const bridgeOnce=type=>new Promise(resolve=>bridgeWaiters[type].push(resolve));
  const bridgeSend=value=>bridgeSocket.send(JSON.stringify(value));
  const requestReset=async()=>{if(directHost)return null;if(remoteMode){if(clientOffset!==0)return null;const pending=bridgeOnce('metricsReset');bridgeSend({type:'resetMetrics'});return await pending;}world.postMessage({type:'resetMetrics'});return await once(world,'metricsReset');};
  const requestMetrics=async()=>{if(directHost)return null;if(remoteMode){const pending=bridgeOnce('metrics');bridgeSend({type:'metrics'});return (await pending).metrics;}const pending=once(world,'metrics');world.postMessage({type:'metrics'});return await pending;};
  if(directHost){
    // A real bfeserver: its public /info names the identity, content and game
    // port; the wire contract is this engine's own, as a desktop client's is.
    const {NETWORK_CONTRACT}=await engineModule('network-contract.js');
    const host=directHost.includes(':')?`[${directHost}]`:directHost;
    const info=await (await fetch(`http://${host}:${arg('info-port',18451)}/info`)).json();
    contract=validateNetworkContract(NETWORK_CONTRACT);content=info.content;remoteServer={id:info.id,address:directHost,port:info.port};
    console.error(`[net-probe] direct to ${info.name} (${directHost}:${info.port}), ${info.players} players, ${info.query}`);
  } else if(remoteMode){
    bridgeSocket=new wsLibrary.WebSocket(`ws://${remoteHost}/?token=${encodeURIComponent(probeToken)}`);
    bridgeSocket.on('message',raw=>{let m;try{m=JSON.parse(raw);}catch{return;}
      if(bridgeWaiters[m.type]){const pending=bridgeWaiters[m.type];bridgeWaiters[m.type]=[];for(const f of pending)f(m);}
    });
    await new Promise((resolve,reject)=>{bridgeSocket.once('open',resolve);bridgeSocket.once('error',reject);});
    bridgeSend({type:'hello'});
    const info=await bridgeOnce('info');contract=validateNetworkContract(info.contract);content=info.content;remoteServer={id:info.serverId,address:remoteHost.replace(/:\d+$/,'').replace(/^\[|\]$/g,''),port:info.udpPort};
  } else {
    const ready=await once(world,'ready');
    world.networkContract=validateNetworkContract(ready.networkContract);
    contract=world.networkContract;content=ready.content;serverKeys=loadIdentity(join(directory,'server.json'));
    world.postMessage({type:'start'});
    // One UDP game port for every client, as bfeserver runs it.
    gameEndpoint=new UdpEndpoint({identity:identityKeys(serverKeys),maxSessions:256,server:{hello:()=>({...wireCompatibility(contract),content}),admit:()=>({accept:true})}});
    await gameEndpoint.listen({host:'127.0.0.1'});
    gameEndpoint.on('session',session=>{
      const index=byIdentity.get(session.id),client=clients[index];if(index===undefined)return session.close(CLOSE_CODE.REJECTED,'unknown probe client');
      const server=new UdpPeer(session,{contract,content,closeCodes:CLOSE_CODE});servers[index]=server;
      const transportSend=server.sendStatePacket.bind(server);
      server.sendStatePacket=frames=>{for(const frame of frames)stampTransit(client.index,'down',frame);return transportSend(frames);};
      server.on('binary',(bytes,label)=>{if(label!=='state')return;const transit=takeTransit(client.index,'up',bytes);if(transit!==null)client.inputTransit.push(transit);});
      if(latency||jitter||loss){
        // Impair only downstream state: delay (plus jitter) and loss before the
        // real transport, as a lossy, jittery link would. Everything else is exact.
        const sendStatePacket=server.sendStatePacket.bind(server),sendBinary=server.sendBinary.bind(server);
        let seed=12345+index;const random=()=>((seed=(Math.imul(seed,1664525)+1013904223)>>>0)/4294967296);
        const defer=fn=>setTimeout(fn,latency+random()*jitter);
        server.sendStatePacket=frames=>{if(server.closed)return false;const kept=frames.filter(()=>random()>=loss);if(kept.length)defer(()=>{if(!server.closed)sendStatePacket(kept);});return true;};
        server.sendBinary=(value,label='bulk')=>{if(label!=='state')return sendBinary(value,label);if(server.closed)return false;if(random()<loss)return true;defer(()=>{if(!server.closed)sendBinary(value,label);});return true;};
      }
      server.on('fault',error=>client.errors.push('server fault: '+error));
      server.once('closed',reason=>{if(reason&&reason!=='closed')client.errors.push('server closed: '+reason);});
      sessions[index]=new GameSession(server,world,session.id,{});
    });
    // Let the battle deploy before anyone joins, as on a live server.
    await sleep(3000);
  }
  for(let index=0;index<players;index++){
    const client=new ProbeClient(index+clientOffset,{contract,content});clients.push(client);byIdentity.set(client.id,index);
    const target=remoteMode?remoteServer:{id:identityKeys(serverKeys).id,address:'127.0.0.1',port:gameEndpoint.port};
    if(remoteMode&&!directHost)bridgeSend({type:'mine',id:client.id});
    await client.peer.connect({serverId:target.id,address:target.address,port:target.port,contract,content,name:`probe-${client.index}`});
    await waitFor(()=>client.welcome,`welcome ${client.index}`,remoteMode?60000:30000);
  }
  const pairs=clients.map(c=>{try{return c.peer.pc.getSelectedCandidatePair();}catch{return null;}});
  shaping=remoteMode?null:await shapeProbeTraffic(pairs,{upKbps:Number(arg('shape-up-kbps',0)),downKbps:Number(arg('shape-down-kbps',2000)),delayMs:Number(arg('shape-delay-ms',25)),jitterMs:Number(arg('shape-jitter-ms',10)),lossPercent:Number(arg('shape-loss-percent',1)),competingUpKbps:Number(arg('competing-up-kbps',0)),queuePackets:Number(arg('shape-queue-packets',32))});
  pumpTimer=setInterval(()=>{const now=performance.now();for(const client of clients){client.pump(now);client.frame(now);}},1000/60);
  await waitFor(()=>clients.every(client=>client.stats.packets>0),'first entity packets',60000);
  console.error(`[net-probe] ${players} client(s) receiving entity state; warming up ${warmup}s`);
  await sleep(warmup*1000);
  for(const client of clients){client.reset();client.deliveryLag=[];client.ackLag=[];client.feedbackLag=[];client.stateTransit=[];client.inputTransit=[];}
  const transportBefore=sessions.map(session=>session.transportDiagnostics());
  const reset=await requestReset();const metricsBegun=performance.now();
  capture=!remoteMode&&(shaping||arg('pcap',null))?await captureProbeTraffic(pairs,{path:arg('pcap',null),extraPorts:shaping?.ports??[]}):null;
  for(const client of clients)client.reset();
  const countersBefore=clients.map(c=>({sent:c.sentBytes,messages:c.sentMessages}));
  const networkBefore=shaping?.stats();
  clients.forEach(c=>c.resultLatencies=[]);
  const begun=performance.now(),begunUnix=Date.now()/1000;measurementBegunAt=begun;
  await sleep(seconds*1000);
  const endedUnix=Date.now()/1000,elapsed=endedUnix-begunUnix;
  const clientStats=clients.map(c=>structuredClone(c.stats));
  const countersAfter=clients.map(c=>({sent:c.sentBytes,messages:c.sentMessages,rtt:c.peer.pc.rtt?.()??null,latencies:summary(c.resultLatencies)}));
  const wire=await capture?.stop({begunUnix,endedUnix});capture=null;
  const network=shaping?.stats();

  for(const client of clients)client.peer.send('events',{type:'call',id:2,method:'inspect'});
  const inspectionDeadline=performance.now()+3000;
  while(clients.some(client=>!client.inspection)&&performance.now()<inspectionDeadline)await sleep(25);
  clearInterval(pumpTimer);pumpTimer=null;
  world?.postMessage({type:'stop'});
  const metrics=await requestMetrics();
  const metricsElapsed=(performance.now()-metricsBegun)/1000;
  const report={
    measurement:{begunUnix,endedUnix,pairs,countersBefore,countersAfter,wire,networkBefore,network},
    config:{players,seconds:round(elapsed),warmup,remote:remoteMode?remoteHost:null,clientOffset,transport:'native UDP (BFE-UDP/1)',map,armySize,lead,inputHz:Number(arg('input-hz',30)),moving,firing,impairment:{latencyMs:latency,jitterMs:jitter,loss},stall:{client:stallClient,afterSeconds:stallAfter,seconds:stallSeconds},smoothing},
    server:!metrics?null:{measurementSeconds:metricsElapsed,metricsWindowOwned:reset!==null,ticksPerSecond:reset?round((metrics.tick-reset.tick)/metricsElapsed):null,simulationMs:summary(metrics.sim),publicationMs:summary(metrics.encode),droppedTicks:round(metrics.droppedTicks),liveUnits:metrics.liveUnits,units:metrics.units,humans:metrics.humans,armies:metrics.armies,worldThreadCores:metrics.threadCpu?round((metrics.threadCpu.user+metrics.threadCpu.system)/1e6/metricsElapsed):null,
      // Where the game port ran, its event loop, the snapshot hop (world
      // publication to send) and the input hop (network receipt to the world).
      gamePortThread:metrics.thread??null,network:metrics.network??null,mainLoopMs:metrics.mainLoopMs??null,inputHopMs:summary(metrics.inputHop??[])},
    clients:clients.map((client,index)=>{client.stats=clientStats[index];return client.report(elapsed,transportBefore[index]);}),
  };
  console.log(JSON.stringify(report,null,1));
} catch(error) {
  // Exiting from the finally block below would otherwise hide the reason.
  failure??=error;
} finally {
  clearInterval(pumpTimer);
  await capture?.stop();shaping?.stop();
  for(const session of sessions)session?.close();
  for(const peer of [...servers,...clients.map(client=>client.peer)])peer?.close();
  bridgeSocket?.close();
  await new Promise(resolve=>setTimeout(resolve,100));gameEndpoint?.close();
  await world?.terminate();
  await rm(directory,{recursive:true,force:true});
  if(failure){console.error(failure);process.exit(1);}
  process.exit(0);
}
