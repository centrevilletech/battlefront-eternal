// Long idle-player battles exercise native seat acquisition and cold bodies
// that short scripted-player hashes rarely reach. Run with game content.
import { headlessGame } from './headless-world.mjs';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { createSnapshotHost } from '../src/runtime/snapshot-host.js';
import { KIND_BATTLE } from '../src/sim/gen/index.js';

const args=process.argv.slice(2);
const option=(name,fallback)=>{const at=args.indexOf(`--${name}`);return at<0?fallback:args[at+1];};
const map=option('map','nab2'),ticks=Number(option('ticks',60000)),players=Number(option('players',4));
if(!Number.isInteger(ticks)||ticks<1||!Number.isInteger(players)||players<1||players>64)throw Error('invalid soak bounds');
const game=await headlessGame(`?map=${map}&side=rep&ai=classic&seed=1`);
const world=createMultiplayerWorld(game,{maxPlayers:players,armySize:32});
const host=createSnapshotHost({game,world,stateBytesPerSecond:100000});
// Lift only the cold ticket bound so ordinary victory cannot end the test
// before the late acquisitions. All simulation and publication stay live.
for(const team of [1,2]){game.battle.reinforcements[team]=1000000;game.battle.start[team]=1000000;}
game.unitsSim.bridge.write(KIND_BATTLE,[game.battle],true);
for(let index=0;index<players;index++){
  const id=`idle-${index}`;
  await world.addPlayer(id,{spawn:true,teamSelect:false,reliableFire:true,separateActions:true});
  host.addClient(id);host.setReady(id);host.setStarted(id,true);
}
let completed=0;
try{
  for(;completed<ticks;completed++){
    const edges=host.advanceEdges();if(edges.length)throw edges[0].error;
    const tick=world.step(),consumed=host.consumedEdges(tick);if(consumed.length)throw consumed[0].error;
    const publication=host.publish();if(publication.faults.length)throw publication.faults[0].error;
    if(game.battle.finished)throw Error('battle ended before soak completed');
    if((completed+1)%10000===0)console.log(JSON.stringify({map,players,completed:completed+1}));
  }
  console.log(JSON.stringify({map,players,completed,worldTick:game.worldClock.tick,passed:true}));
}catch(error){console.error(JSON.stringify({map,players,completed,passed:false}),error);process.exitCode=1;}
