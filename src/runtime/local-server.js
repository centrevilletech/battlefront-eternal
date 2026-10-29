import { createMultiplayerWorld } from './multiplayer-world.js';
import { createSnapshotHost, publicationBuffersFor } from './snapshot-host.js';
import { WORLD_SETTINGS } from '../settings.js';

// Transport-free local server, also used by the integration harness. A local
// human has the same mailbox, action edges, seat leases and combat as online.
export async function createLocalServer(game, {name='Player', send=()=>{}} = {}) {
  const id='local';
  const position=game.playerUnit.position.clone(),yaw=game.soldier.aimYaw;
  const world=createMultiplayerWorld(game,{maxPlayers:1,armySize:null});
  await world.addPlayer(id,{team:game.playerTeam,className:game.policy.className,name,
    spawn:game.policy.auto,teamSelect:game.humanSpawn.canSwitchTeams,reliableFire:true,separateActions:true});
  const actor=world.players.get(id);
  actor.autoRespawn=!!game.policy.auto;
  if(game.policy.auto){actor.unit.soldier.spawn(position.x,position.y,position.z,yaw);actor.unit.presentation.reset();}
  const host=createSnapshotHost({game,world,local:true});
  host.addClient(id);host.setReady(id);host.setStarted(id,true);
  let running=false,stepping=false;
  const event=message=>send('events',JSON.stringify(message));
  function publish(){
    const out=host.publish();
    if(out.faults.length)throw out.faults[0].error;
    for(const m of out.inputAcks)send('state',JSON.stringify({type:'input-ack',...m}));
    for(const m of out.edgeResults)event({type:'entity-edge-results',...m});
    for(const m of out.diagnostics)event({type:'entity-diagnostics',...m});
    for(const m of out.reliableEvents??[])event({type:'gameplay-events',tick:m.tick,records:m.records.map(r=>({...r,payload:btoa(String.fromCharCode(...r.payload))}))});
    for(const buffer of publicationBuffersFor(out,id))send('state',buffer);
  }
  function step(){
    const faults=host.advanceEdges();if(faults.length)throw faults[0].error;
    const tick=world.step();
    const consumed=host.consumedEdges(tick);if(consumed.length)throw consumed[0].error;
    publish();return tick;
  }
  return {game,world,host,id,actor,step,publish,
    get running(){return running;},get stepping(){return stepping;},
    welcome(){event({type:'entity-registry',registry:host.descriptor()});event({type:'welcome',epoch:1,snapshotMode:'snapshot-delta-v1',...world.player(id)});publish();},
    receive(label,raw){
      if(raw instanceof ArrayBuffer){if(label!=='state')throw Error('Invalid local input channel');if(running||stepping)host.input(id,raw);return;}
      const message=JSON.parse(raw);
      if(message.type==='entity-edge-ack'){host.edgeAck(id,message);return;}
      if(message.type!=='call')throw Error('Invalid local control message');
      const {method,args={},id:request}=message;
      try {
        let result={};
        if(method==='start'){running=true;stepping=false;}
        else if(method==='suspend'||method==='lockstep'){running=false;stepping=method==='lockstep';world.clearInput(id);}
        else if(method==='advance'){
          if(running)throw Error('Cannot step a running world');
          if(!Number.isInteger(args.ticks)||args.ticks<1||args.ticks>120)throw Error('Invalid step count');
          for(let i=0;i<args.ticks;i++)step();
        }else if(method==='settings'){for(const [key,value]of Object.entries(args.values||{}))if(WORLD_SETTINGS.includes(key))game.settings.set(key,value);}
        else if(method==='resync')host.resync(id);
        else if(method!=='inspect'&&method!=='worldState')throw Error('Unknown local control: '+method);
        event({type:'reply',id:request,epoch:1,tick:game.worldClock.tick,running,player:world.player(id),entityScheduling:method==='inspect'?host.inspect(id):undefined,...result});
      }catch(error){event({type:'reply',id:request,error:error.message});}
    },
    dispose(){running=false;host.dispose();}
  };
}
