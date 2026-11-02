import { createGame } from '../game.js';
import { createLocalServer } from './local-server.js';
import { contentHash } from './content-hash.js';
import { FixedClock } from './fixed-clock.js';
import { worldState } from './world-state.js';
import { batchedPoster } from './local-message-batch.js';
import { createWorldPump } from './local-world-pump.js';

const clock=new FixedClock({maxSteps:4,budget:.05,maxDebt:8});
let server=null,initializing=false;
// Each step's messages go to the page together, right after the step.
const {send,flush}=batchedPoster((message,transfer)=>self.postMessage(message,transfer));
const pump=createWorldPump(clock,()=>{server.step();flush();},{fail});
function fail(error){pump.stop();server?.dispose();send('events',JSON.stringify({type:'gameplay-error',message:error.message}));}
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
  if(server.running&&!before){clock.tick=server.game.worldClock.tick;clock.rebase();pump.start();}
  if(!server.running&&before)pump.stop();
}).catch(fail);};
