import {FixedClock} from '../../src/runtime/fixed-clock.js';
import {WorldTick} from '../../src/runtime/world-tick.js';
import {CommandPosts} from '../../src/commandposts.js';
import {F4Live} from '../../src/f4-live.js';
import {F4Tail} from '../../src/f4-tail.js';
import {HumanSpawnMirror} from '../../src/sim/gen/index.js';
import {battleFixture} from '../fixtures/f4-battle.js';

// Item assertions advance the same complete coordinator as Game.step. They
// cannot supply a second target roster or skip the unit/seat owner ordering.
export function worldFrame(world,unit=world.factory.built[0]) {
  const phase=world.phase,posts=new CommandPosts(null,{}, {headless:true,playerTeam:1});
  const battle=battleFixture(posts),character=battle.characters[0];
  character.unit=unit;character.className=unit.className;
  phase.registerViewpoint(unit);
  if(!phase.liveOwner)new F4Live(phase,{local:unit,sound(){}});
  if(!phase.tailOwner)new F4Tail(phase,battle,{posts,human:new HumanSpawnMirror(),local:unit,character,
    gameplay:world.ordnance.random,rosters:[[],[unit.className],[unit.className]],classes:[unit.className]});
  const tick=new WorldTick(phase,new FixedClock());
  return (dt=1/60)=>{tick.run(dt,tick.clock.tick+1,true,null,false,true,0,false,0);tick.finish();tick.complete();};
}
