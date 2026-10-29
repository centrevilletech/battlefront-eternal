import { createGame } from '../game.js';
import { createLocalServer } from './local-server.js';
import { contentHash } from './content-hash.js';
import { FixedClock } from './fixed-clock.js';
import { worldState } from './world-state.js';

const clock=new FixedClock({maxSteps:4,budget:.05,maxDebt:8});
let server=null,timer=null,last=0,initializing=false;
const send=(channel,payload)=>self.postMessage({channel,payload},payload instanceof ArrayBuffer?[payload]:[]);
function fail(error){clearTimeout(timer);server?.dispose();send('events',JSON.stringify({type:'gameplay-error',message:error.message}));}
function pump(){
  if(!server?.running)return;
  try{
    const now=performance.now();clock.advance((now-last)/1000,()=>server.step());last=now;
    timer=setTimeout(pump,clock.debt?0:Math.max(1,(clock.dt-(clock.elapsed-clock.time))*1000));
  }catch(error){fail(error);}
}
let messages=Promise.resolve();
self.onmessage=({data})=>{messages=messages.then(async()=>{
  if(data.type==='init'){
    if(server||initializing)throw Error('Local world already initialized');initializing=true;
    if(await contentHash(data.query)!==data.content)throw Error('Local content changed during loading');
    const game=await createGame({query:data.query,headless:true,savedSettings:data.settings});
    server=await createLocalServer(game,{name:data.playerName,send});
    self.__bf=game;self.__server=server;
    server.welcome();return;
  }
  if(!server)throw Error('Local world not initialized');
  const before=server.running;
  // Debug state is deliberately outside the entity protocol, available only
  // on this local transport for deterministic parity checks.
  if(typeof data.payload==='string'){
    const m=JSON.parse(data.payload);
    if(m.type==='call'&&m.method==='worldState'){send('events',JSON.stringify({type:'reply',id:m.id,state:worldState(server.game)}));return;}
  }
  server.receive(data.channel,data.payload);
  if(server.running&&!before){clock.tick=server.game.worldClock.tick;clock.rebase();last=performance.now();timer=setTimeout(pump,0);}
  if(!server.running&&before){clearTimeout(timer);timer=null;}
}).catch(fail);};
