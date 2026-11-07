// Authenticated production UDP sessions, input sender and snapshot receiver.
// Only the human input pattern and intentional ACK withholding are synthetic.
import {pathToFileURL} from 'node:url';
import {resolve,join} from 'node:path';
import {until,sleep} from './common.mjs';
const load=(root,name)=>import(pathToFileURL(resolve(root,name)));
export async function clientFactory(engine,server,directory){
  const [{UdpEndpoint},{identityKeys},{CLOSE_CODE},{NetworkSession},{EntityInputSender},{SnapshotClient},{loadIdentity},{decodeOwnPlayer},{decodeWorldSummary}]=await Promise.all([
    load(engine,'src/net/endpoint.js'),load(engine,'src/net/crypto.js'),load(engine,'src/net/connection.js'),
    load(engine,'src/runtime/network-session.js'),load(engine,'src/runtime/network-entity-sender.js'),load(engine,'src/runtime/snapshot-client.js'),load(server,'src/identity.js'),
    load(engine,'src/runtime/network-entity-player.js'),load(engine,'src/runtime/network-entity-summary.js')]);
  return async function connect(index,info){
    const keys=loadIdentity(join(directory,`client-${index}.json`)),identity=identityKeys(keys),errors=[];
    const endpoint=new UdpEndpoint({identity});await endpoint.listen({host:'127.0.0.1',families:['IPv4']});
    const receiver=new SnapshotClient(),session=new NetworkSession({lifecycle:null});
    session.authenticated=session.entityMode=true;
    let transport,closing=false,slow=false,registry=null,welcome=null,sequence=0,estimate=null,ownGeneration=0,latestTick=0,complete=0,packets=0,bytes=0,lastInput=0,commandTick=-1,spawnPhase=null,lastSpawn=0;
    const calls=new Map();let nextCall=10;
    const error=e=>{if(errors.length<16)errors.push(String(e?.message??e));};
    // Encoders return reusable WASM views; transport flushes asynchronously.
    // Own the bytes before another client can grow that shared linear arena.
    const send=(lane,value)=>{if(transport&&!transport.closed)transport.send(lane,typeof value==='string'?value:Buffer.from(value));};
    session.channels.set('state',{readyState:'open',bufferedAmount:0,send:value=>send('state',value)});
    session.channels.set('events',{readyState:'open',bufferedAmount:0,send:value=>send('events',value)});
    const receive=(lane,payload)=>{
      try{
        if(typeof payload!=='string'){
          packets++;bytes+=payload.byteLength;
          if(slow)return;
          const result=receiver.receive(new Uint8Array(payload));
          if(result){
            latestTick=Math.max(latestTick,result.tick);if(result.complete)complete++;
            if(result.snapshotAck?.epoch>=1)session.setSnapshotAck(result.snapshotAck);
            for(const record of result.records){
              if(record.kind==='correction'){
                const own=decodeOwnPlayer(record);ownGeneration=record.generation;
                if(registry?.classes.has(own.classId))welcome.className=registry.classes.get(own.classId);
                session.entityInput?.ack(result.ackInput);session.entityInput?.applied(record.tick);
              }
              else if(record.kind==='summary'&&record.schema===3&&registry)spawnPhase=decodeWorldSummary(record,registry).phase;
            }
          }
          return;
        }
        const m=JSON.parse(payload);
        if(m.type==='entity-registry'){
          registry={...m.registry};for(const key of ['classes','weapons','posts','bindingProfiles'])registry[key]=new Map(m.registry[key]);
          registry.classIds=new Map([...registry.classes].map(([id,name])=>[name,id]));registry.postIds=new Map([...registry.posts].map(([id,name])=>[name,id]));
          session.entityRegistry=registry;
          session.entityInput=new EntityInputSender({registry,send:packet=>session.sendEntityInput(packet),snapshotAck:()=>session.snapshotAck,
            getAim:()=>({aimYaw:sequence/2048,aimPitch:0}),stateIntervalTicks:2});
        }else if(m.type==='welcome'){
          welcome=m;ownGeneration=m.generation;session.welcome=m;send('events',JSON.stringify({type:'call',id:1,method:'start'}));
        }else if(m.type==='input-ack'){
          session.receiveInputAck(m);ownGeneration=m.generation;estimate={tick:m.serverTick,at:performance.now()};
        }else if(m.type==='entity-edge-results'){session.entityInput?.feedback(m);session.sendEntityEdgeAcknowledgement();}
        else if(m.type==='reply'){
          const p=calls.get(m.id);if(p){calls.delete(m.id);clearTimeout(p.timer);m.error?p.reject(Error(m.error)):p.resolve(m);}
          if(m.error)error(m.error);
        }else if(m.type==='gameplay-error'||m.type==='version-error')error(m.message);
      }catch(e){error(e);}
    };
    try{
      transport=await endpoint.connect({serverId:info.id,candidates:[{address:'127.0.0.1',port:info.port}],
        hello:{protocol:info.protocol,profile:info.profile,simulation:info.simulation,content:info.content,name:`Qualification ${index}`},timeoutMs:30000});
      transport.on('message',receive);transport.on('close',value=>{if(!closing)error(`Unexpected transport close: ${value.reason}`);});transport.resume();
      await until(()=>{if(errors.length)throw Error(errors[0]);return welcome&&registry;},'authenticated welcome');
      const pump=setInterval(()=>{
        try{
          const now=performance.now();session.sendEntityAcknowledgement();session.sendEntityEdgeAcknowledgement();
          if(!session.entityInput||now-lastInput<30)return;lastInput=now;
          // Delayed ACKs can revise the wall estimate backwards. The input
          // clock still advances monotonically, as the real page clock does.
          const tick=Math.max(commandTick+1,Math.floor(estimate?estimate.tick+(now-estimate.at)*60/1000:Math.max(welcome.tick,latestTick))+6);commandTick=tick;
          const down=Math.floor(now/2000)%2===0?['KeyW']:[];if(Math.floor(now/1300)%2===0)down.push('Mouse0');
          const human=[];
          if(['awaiting','unspawned'].includes(spawnPhase)&&now-lastSpawn>1500){human.push({kind:'spawn',className:welcome.className,post:null});lastSpawn=now;}
          session.inputs([{epoch:1,generation:ownGeneration,sequence:++sequence,tick,down,look:[[1,0]],actions:[],human,
            mouseScale:1/2048,aspect:16/9,viewTick:Math.max(0,tick-12),firstPerson:true,invertMouse:false,
            flightSettings:{bindings:{},sensitivity:1,curve:1.35,invert:false}}]);
        }catch(e){error(e);}
      },16);
      const close=async()=>{
        if(closing)return false;closing=true;clearInterval(pump);
        for(const p of calls.values()){clearTimeout(p.timer);p.reject(Error('Client closed'));}calls.clear();
        session.entityInput?.dispose();receiver.dispose();
        // Registry encoders share a separate page owner with the sender.
        session.entityInput?.network?.dispose();
        transport.close(CLOSE_CODE.NORMAL,'Qualification finished');await sleep(30);endpoint.close();return true;
      };
      return {id:identity.id,pair:{local:{address:'127.0.0.1',port:endpoint.port},remote:{address:'127.0.0.1',port:info.port}},
        get receiver(){return receiver;},get errors(){return errors;},get complete(){return complete;},get tick(){return latestTick;},
        slow(value){slow=!!value;},report:()=>({index,identity:identity.id,packets,bytes,complete,tick:latestTick,errors:[...errors]}),close,
        inspect(){const id=++nextCall;return new Promise((resolve,reject)=>{const timer=setTimeout(()=>{calls.delete(id);reject(Error('Client inspection timeout'));},10000);calls.set(id,{resolve,reject,timer});send('events',JSON.stringify({type:'call',id,method:'inspect'}));});}};
    }catch(e){closing=true;receiver.dispose();session.entityInput?.dispose();transport?.close(CLOSE_CODE.NORMAL,'Qualification failed');endpoint.close();throw e;}
  };
}
