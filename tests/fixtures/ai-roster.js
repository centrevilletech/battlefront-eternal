import { Matrix4 } from 'three';
import { CommandPosts } from '../../src/commandposts.js';
export const ROSTER_SEED=293;
export const rosterCalls=[
  {name:'SetTeamName',args:[1,'rep']},{name:'SetTeamName',args:[2,'cis']},{name:'SetTeamName',args:[3,'locals']},
  ...[1,2].flatMap(team=>[
    {name:'AddUnitClass',args:[team,'soldier',1]},{name:'AddUnitClass',args:[team,'medic',1]},
    {name:'AddUnitClass',args:[team,'missing',4]},{name:'SetUnitCount',args:[team,4]},
    {name:'SetHeroClass',args:[team,team===1?'hero-one':'hero-two']},{name:'SetReinforcementCount',args:[team,80]},
  ]),
  {name:'AddUnitClass',args:[3,'soldier',0]},{name:'SetUnitCount',args:[3,2]},
  {name:'SetSpawnDelay',args:[9,.5]},
];
export const rosterPrepared=['soldier','medic','hero-one','hero-two'];
export const rosterNames=Object.fromEntries(Array.from({length:5},(_,index)=>[index,{first:[`F${index}A`,`F${index}B`],last:['ONE','TWO']}]));
rosterNames.local='LOCAL';
export function rosterPosts() {
  const posts=new CommandPosts(null,{commandPosts:[1,2,0,3].map((team,index)=>({name:`post${index}`,team,
    matrix:new Matrix4().makeTranslation(index*20,0,0).toArray(),hologram:[0,0,0]}))},{headless:true,playerTeam:1});
  for(const [index,post] of posts.posts.entries()) {
    const points=[[index*20,0,0,0]];
    post.desc.spawnPath=`path${index}`;post.desc.spawnPoints=points;
    post.ai={order:index,atk:[0,20,30,40,50,60],def:[0,60,50,40,30,20],bans:0,valueBleed:100,
      spawnPath:post.team?{name:`path${index}`,present:true,points}:null,allyPath:null,allyCount:0,classSpawnPoints:0,classSpawnWorld:[],capture:null,radius:0,yaw:0};
  }
  return posts;
}
// A compact cold reference checks ordering and values without depending on
// controller behavior. Queue keys are post identities, rather than positions.
export function rosterSnapshot(spawning,random) {
  return {seed:random.state,A:spawning.A,B:spawning.B,Q:spawning.Q,waveCountdown:spawning.waveCountdown,
    wave:spawning.wave,stagger:spawning.stagger,released:spawning.released,localsCap:spawning.localsCap,
    teams:spawning.teams.map(team=>({index:team.index,faction:team.faction,unitCount:team.unitCount,tickets:team.tickets,
      heroCount:team.heroCount,relations:Array.from(team.relations),classes:team.classes.map(type=>({name:type.name,target:type.target,hero:type.hero})),
      characters:team.characters.map(character=>character.managerIndex)})),
    characters:spawning.characters.map(character=>({index:character.index,slot:character.slot,managerIndex:character.managerIndex,
      playerId:character.playerId,team:character.team,human:character.human,name:character.name,hero:character.hero,
      className:character.className,classIndex:character.classIndex,objective:character.objective?.name??null,
      spawn:{...character.spawn,post:character.spawn.post?.name??null}})),
    queues:[...spawning.queues].map(([post,queue])=>[post.name,[...queue]])};
}
