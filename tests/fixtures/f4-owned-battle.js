import {Character} from '../../src/battle.js';
import {Matrix4} from 'three';
import {CommandPosts} from '../../src/commandposts.js';
import {F4Live} from '../../src/f4-live.js';
import {F4Tail} from '../../src/f4-tail.js';
import {HumanSpawnMirror} from '../../src/sim/gen/index.js';
import {f3World} from './f3-world.js';

// Construction is the only import. Tests exercise ordinary score commands
// and the actual native tail, without a JS rules or statistics implementation.
export async function ownBattle(battle,{counts={}}={}) {
  const source=battle.posts?.posts??[];
  const posts=new CommandPosts(null,{commandPosts:source.map((p,i)=>({
    name:`post${i}`,team:p.team,matrix:new Matrix4().makeTranslation(i*20,0,0).toArray(),
    hologram:[0,0,0],...p.desc,
  }))},{headless:true,playerTeam:battle.own});
  const soldier={class:'soldier',base:'soldier',MaxHealth:100,weapons:[]};
  const w=await f3World({soldier}, {prepareLive:()=>({buildings:[],animated:[],traps:[]})});
  for(let a=0;a<8;a++)for(let b=0;b<8;b++)w.units.setRelation(a,b,battle.relation(a,b));
  w.phase.construction.prepare('soldier',Object.values(counts).reduce((a,b)=>a+b,0));
  for(const [team,count] of Object.entries(counts))for(let i=0;i<count;i++){const u=w.spawn('soldier',i*3,0);u.team=u.combat.team=Number(team);const c=new Character(i,Number(team));c.unit=u;c.className='soldier';battle.characters.push(c);}
  new F4Live(w.phase,{sound(){}});
  const human=new HumanSpawnMirror();
  const tail=new F4Tail(w.phase,battle,{posts,human,gameplay:w.ordnance.random,onOver:battle.onOver});
  let tick=0;
  return {world:w,tail,step:dt=>tail.step(dt,++tick,true)};
}
