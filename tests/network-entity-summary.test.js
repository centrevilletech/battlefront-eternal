import {EntityReceiver,EntityScheduler} from '../src/runtime/network-entity-stream.js';
import test from 'node:test';import assert from 'node:assert/strict';import * as summaryModule from '../src/runtime/network-entity-summary.js';import {captureWorldSummaries,captureBattleSummary,capturePostSummary,captureSpawnSummary,decodeWorldSummary,applyWorldSummary,createPostRegistry} from '../src/runtime/network-entity-summary.js';import {encodeEntityPacket,decodeEntityPacket} from '../src/runtime/network-entity-codec.js';
const posts=[{name:'alpha',team:1,toward:2,progress:.25,neutralize:.1,contested:true,active:false,counts:new Map([[1,2],[2,3]])},{name:'beta',team:2,toward:0,progress:0,neutralize:0,contested:false,counts:new Map()}],registry={posts:createPostRegistry(posts),classes:new Map([[1,'trooper'],[2,'pilot']])};
const battle={reinforcements:{1:150,2:120},start:{1:200,2:200},noPost:{1:20,2:10},bleed:{1:0,2:1},over:false,losing:false,gameOver:false,finished:false,loser:null,overTime:0,victoryClock:10,countdown:10};
const spawn={phase:'awaiting',className:'pilot',post:'alpha',spawnedOnce:true,ended:false,deadTime:4,pending:{className:'trooper',post:null},auto:null};
test('independent battle/post/spawn records are bounded and roundtrip authoritative eligibility',()=>{const options={tick:60,epoch:1,registry},records=[captureBattleSummary(battle,options),capturePostSummary(posts[0],options),captureSpawnSummary({unit:{id:8,generation:3},team:1,spawn},{...options,eligiblePosts:['alpha'],eligibleClasses:['trooper','pilot']})];const bytes=encodeEntityPacket({epoch:1,packetId:1,tick:60,records}),decoded=decodeEntityPacket(bytes).records;assert.ok(bytes.length<400);console.log(`Battle + CP + personal eligibility: ${bytes.length} bytes`);const game={battle:structuredClone(battle),posts:{posts:structuredClone(posts)},humanSpawn:{}};for(const record of decoded)applyWorldSummary(game,record,{registry});assert.deepEqual(game.humanSpawn,spawn);assert.deepEqual(game.posts.posts[0].counts,posts[0].counts);assert.equal(game.posts.posts[0].active,false);assert.deepEqual(decodeWorldSummary(decoded[2],registry).eligiblePosts,[1]);});
test('missing prerequisites and malformed summary never partially mutate gameplay',()=>{const r=capturePostSummary(posts[0],{tick:1,epoch:1,registry}),game={posts:{posts:structuredClone(posts)}};const old=structuredClone(game);assert.throws(()=>applyWorldSummary(game,r,{registry:{...registry,posts:new Map()}}));assert.deepEqual(game,old);assert.throws(()=>decodeWorldSummary({...r,payload:r.payload.slice(0,-1)},registry));assert.deepEqual(game,old);const bad=structuredClone(posts[0]);bad.progress=2;assert.throws(()=>decodeWorldSummary(capturePostSummary(bad,{tick:1,epoch:1,registry}),registry));});
test('auto spawn policy references verified content and full end-of-match authority survives',()=>{const s={...spawn,auto:{team:1,className:'pilot',post:'alpha'}},r=captureSpawnSummary({unit:{id:8,generation:4},team:1,spawn:s},{tick:100,epoch:2,registry,eligiblePosts:[],eligibleClasses:[]});assert.deepEqual(decodeWorldSummary(r,registry).auto,s.auto);const end={...battle,over:true,gameOver:true,finished:true,loser:2,overTime:9,victoryClock:null,countdown:null},game={battle:structuredClone(battle)};applyWorldSummary(game,captureBattleSummary(end,{tick:100,epoch:2}),{registry});assert.equal(game.battle.finished,true);assert.equal(game.battle.loser,2);assert.equal(game.battle.countdown,null);});

test('shared capture derives eligible sets from authoritative post and prepared-class policy',()=>{const world={players:new Map([['peer',{unit:{id:8,generation:3},team:1,spawn}]])},game={worldClock:{tick:60},battle:structuredClone(battle),posts:{posts:structuredClone(posts)},units:{relation:(a,b)=>a===b?1:-1},spawnPort:{roster:()=>[{className:'trooper'},{className:'pilot'}]},factory:{prepared:name=>name==='trooper'}};game.posts.posts[0].active=true;game.posts.posts[0].ai={spawnPath:{present:true}};game.posts.posts[1].ai={spawnPath:{present:true}};let records=captureWorldSummaries(game,world,registry,{epoch:1,playerId:'peer'}),s=decodeWorldSummary(records.at(-1),registry);assert.deepEqual(s.eligiblePosts,[1]);assert.deepEqual(s.eligibleClasses,[1]);game.battle.gameOver=true;records=captureWorldSummaries(game,world,registry,{epoch:1,playerId:'peer'});s=decodeWorldSummary(records.at(-1),registry);assert.deepEqual(s.eligiblePosts,[]);assert.deepEqual(s.eligibleClasses,[]);});

test('summary IDs never collide with actors or clear seats, and old partial summaries cannot roll back others',()=>{const receiver=new EntityReceiver(),s=new EntityScheduler(),options={tick:1,epoch:2,registry},post=capturePostSummary(posts[0],options),seat={kind:'seat',id:1,generation:1,tick:1,vehicleId:9,vehicleGeneration:1,seatIndex:0,leaseVersion:1};const packet=(id,tick,records)=>encodeEntityPacket({epoch:2,packetId:id,tick,records});receiver.apply(packet(1,1,[seat]));receiver.apply(packet(2,1,[post]));assert.equal(receiver.seatOwner(9,1,0).id,1);assert.equal(receiver.get('summary',1).schema,2);const next=capturePostSummary({...posts[0],team:2},{...options,tick:3});receiver.apply(packet(3,3,[next]));receiver.apply(packet(4,2,[{...post,tick:2}]));assert.equal(decodeWorldSummary(receiver.get('summary',1),registry).team,2);s.update(post);s.update({kind:'tombstone',id:1,generation:1,tick:1});assert.equal(s.records.size,2);});

test('personal summary retains one stable slot across body/class replacement',()=>{const options={tick:1,epoch:1,registry,eligiblePosts:['alpha'],eligibleClasses:['trooper']},scheduler=new EntityScheduler(),first=captureSpawnSummary({unit:{id:8,generation:1},team:1,spawn},options),second=captureSpawnSummary({unit:{id:42,generation:2},team:1,spawn:{...spawn,className:'trooper'}},{...options,tick:2});scheduler.update(first);scheduler.update(second);assert.equal(scheduler.records.size,1);assert.equal(first.id,second.id);assert.equal(decodeWorldSummary(scheduler.records.get('summary:65536').record,registry).playerId,42);});
test('publication eligibility shares team policy without sharing personal spawn state',()=>{
 let checks=0;const a={unit:{id:1,generation:1},team:1,spawn},b={unit:{id:2,generation:2},team:1,spawn:{...spawn,phase:'alive'}},world={players:new Map([['a',a],['b',b]])},game={worldClock:{tick:60},battle:structuredClone(battle),posts:{posts:[]},units:{relation:()=>1},spawnPort:{roster:()=>{checks++;return [{className:'trooper'}];}},factory:{prepared:()=>true}},cache=new Map();
 const get=(id,tick=60)=>decodeWorldSummary(captureWorldSummaries(game,world,registry,{tick,epoch:1,playerId:id,includeCommon:false,eligibilityCache:cache})[0],registry);
 assert.equal(get('a').playerId,1);assert.equal(get('b').playerId,2);assert.equal(get('b').phase,'alive');assert.equal(checks,1);
 game.factory.prepared=()=>false;assert.deepEqual(get('a',61).eligibleClasses,[]);assert.equal(checks,2);
 game.battle.gameOver=true;assert.deepEqual(get('b',61).eligiblePosts,[]);
});

test('the battle summary raises the Battle end announcement once and spawn accounts go through HumanSpawn',()=>{
 const announced=[],game={battle:{...structuredClone(battle),onOver:loser=>announced.push(loser)}};
 applyWorldSummary(game,captureBattleSummary({...battle,over:false},{tick:10,epoch:1}),{registry});assert.deepEqual(announced,[]);
 const end={...battle,over:true,loser:2,overTime:1,victoryClock:null,countdown:null};
 applyWorldSummary(game,captureBattleSummary(end,{tick:20,epoch:1}),{registry});applyWorldSummary(game,captureBattleSummary({...end,overTime:2},{tick:30,epoch:1}),{registry});
 assert.deepEqual(announced,[2],'Victory or Defeat once, as the local Battle.update raises it');
 const accounts=[],human={applyAuthoritative:value=>accounts.push(value)};
 applyWorldSummary({...game,humanSpawn:human},captureSpawnSummary({unit:{id:8,generation:3},team:1,spawn},{tick:40,epoch:1,registry,eligiblePosts:[],eligibleClasses:[]}),{registry});
 assert.equal(accounts.length,1);assert.equal(accounts[0].phase,'awaiting');assert.equal(human.phase,undefined,'fields are not assigned behind the flow');
});

test('the roster summary carries every Character\'s scores, name, ping and link by slot, in chunks, and lands in the local battle\'s Characters',()=>{
 const {captureRosterSummaries,ROSTER_CHUNK,linkCode}=summaryModule;
 const characters=[];for(let i=0;i<30;i++)characters.push({team:1+(i%2),human:i===0||i===4||i===17,name:i===0?'PLAYER 1':i===4?'Josh Lambert':i===17?'Rémi':`Bot ${i}`,kills:i,teamKills:i%3,deaths:2*i,captures:i%5});
 const links=new Map([[characters[4],{link:{rtt:41.6,path:{kind:'direct',family:'ipv6'}}}],[characters[17],{link:{rtt:120,path:{kind:'relay',family:'ipv4'}}}]]);
 const records=captureRosterSummaries({characters},links,{tick:9,epoch:1});
 assert.equal(records.length,Math.ceil(30/ROSTER_CHUNK));assert.ok(records.every(r=>r.payload.length<700),'a chunk fits a datagram');
 console.log(`Roster of 30: ${records.reduce((n,r)=>n+r.payload.length,0)} bytes in ${records.length} chunks`);
 const first=decodeWorldSummary(records[0],registry);assert.equal(first.schema,4);assert.equal(first.total,30);assert.equal(first.start,0);
 assert.deepEqual(first.characters[4],{team:1,human:true,kills:4,teamKills:1,deaths:8,captures:4,ping:42,link:'direct6',name:'Josh Lambert'});
 const second=decodeWorldSummary(records[1],registry);assert.deepEqual(second.characters[5],{team:2,human:true,kills:17,teamKills:2,deaths:34,captures:2,ping:120,link:'relay4',name:'Rémi'});
 assert.equal(first.characters[1].ping,null);assert.equal(first.characters[1].link,'unknown');
 assert.deepEqual([first.characters[0].team,first.characters[0].human],[0,false],'the headless engine\'s own human, no player\'s, has no row');
 assert.equal(linkCode({kind:'direct',family:'ipv4'}),1);assert.equal(linkCode({kind:'relay',family:'ipv6'}),4);assert.equal(linkCode({kind:'checking'}),0);
 // A local battle populated with its own guesses: the slots are overwritten, extra slots dropped, missing ones made.
 const game={battle:{characters:[{team:1,human:true,name:'PLAYER 1',kills:0,teamKills:0,deaths:0,captures:0}],human:null}};
 game.battle.human=game.battle.characters[0];
 const packet=decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:1,tick:9,records})).records;
 for(const record of packet)applyWorldSummary(game,record,{registry});
 assert.equal(game.battle.characters.length,30);assert.equal(game.battle.characters[4].name,'Josh Lambert');assert.equal(game.battle.characters[4].link.ping,42);assert.equal(game.battle.characters[4].link.kind,'direct6');
 assert.equal(game.battle.characters[0].human,false,'the local guess gives way to the server\'s roster');assert.equal(game.battle.characters[0].team,0);assert.equal(game.battle.characters[29].deaths,58);
 assert.equal(game.battle.characters[1].link,null);
 // The spawn summary names his slot: his Character, before or after the roster arrives.
 const mine=captureSpawnSummary({unit:{id:8,generation:3},team:1,spawn},{tick:9,epoch:1,registry,eligiblePosts:[],eligibleClasses:[],slot:4});
 const playerUnit={character:null};applyWorldSummary({...game,playerUnit,humanSpawn:{}},mine,{registry});
 assert.equal(game.battle.human,game.battle.characters[4]);assert.equal(playerUnit.character,game.battle.characters[4]);
 const fresh={battle:{characters:[],human:null},humanSpawn:{}};applyWorldSummary(fresh,mine,{registry});assert.equal(fresh.battle.human.index,4);assert.equal(fresh.battle.human.human,true);
 const none=captureSpawnSummary({unit:{id:8,generation:3},team:1,spawn},{tick:9,epoch:1,registry,eligiblePosts:[],eligibleClasses:[]});
 assert.equal(decodeWorldSummary(none,registry).slot,null);
 // Names are cut at the byte limit; malformed chunks are rejected whole.
 const long=captureRosterSummaries({characters:[{team:1,name:'x'.repeat(80)}]},new Map(),{tick:1,epoch:1});assert.equal(decodeWorldSummary(long[0],registry).characters[0].name.length,32);
 const bad=captureRosterSummaries({characters:[{team:1,name:'a'}]},new Map(),{tick:1,epoch:1})[0];bad.payload[5]=9;assert.throws(()=>decodeWorldSummary(bad,registry),/invalid roster entry/);
 const shifted={...records[1],id:records[1].id+1};assert.throws(()=>decodeWorldSummary(shifted,registry),/invalid roster summary/);
});
test('the shared capture publishes the roster with each player\'s link',()=>{
 const c1={team:1,human:true,name:'A',kills:1,teamKills:0,deaths:0,captures:0},c2={team:2,human:false,name:'B',kills:0,teamKills:0,deaths:0,captures:0};
 const world={players:new Map([['peer',{unit:{id:8,generation:3},team:1,spawn,character:c1,link:{rtt:33,path:{kind:'direct',family:'ipv4'}}}]])};
 const game={worldClock:{tick:60},battle:{...structuredClone(battle),characters:[c2,c1]},posts:{posts:[]},units:{relation:()=>1},spawnPort:{roster:()=>[]},factory:{prepared:()=>true}};
 const records=captureWorldSummaries(game,world,registry,{epoch:1,playerId:'peer'}),roster=records.filter(r=>r.schema===4),own=decodeWorldSummary(records.at(-1),registry);
 assert.equal(roster.length,1);const s=decodeWorldSummary(roster[0],registry);assert.equal(s.characters[1].ping,33);assert.equal(s.characters[1].link,'direct4');assert.equal(s.characters[0].link,'unknown');
 assert.equal(own.slot,1,'his own slot rides the spawn summary');
});

test('end-of-match records travel as datagram-sized chunks and show the same DETAILS and AWARDS',async()=>{
 const {BattleStats,personalStats,awards,recordOf}=await import('../src/battle-stats.js');
 const {captureRecordSummaries,OWN_RECORD_ID,RECORD_SUMMARY_ID}=summaryModule;
 const names=['rep_inf_clone_trooper','rep_weap_inf_rifle','rep_hover_fightertank','turret_gun'];
 const registry={classes:new Map([[1,names[0]],[3,names[2]]]),classIds:new Map([[names[0],1],[names[2],3]]),weapons:new Map([[2,names[1]]]),weaponIds:new Map([[names[1],2]]),posts:new Map()};
 const make=()=>Array.from({length:40},(_,index)=>({index,team:index%2?2:1,name:`C${index}`}));
 const world=make(),stats=new BattleStats();
 for(let i=0;i<40;i++)for(let k=0;k<i%5;k++)stats.kill(world[i],world[(i*7+k)%40],-1);
 stats.headshot(world[3]);stats.vehicleDestroyed(world[5]);stats.death(world[8]);
 const body=(weapon,vehicle)=>({position:{x:0,y:0,z:0},alive:true,combat:{health:1,maxHealth:1,arsenal:{current:()=>({name:weapon,def:{displayName:'GUN'}})}},
  seat:vehicle?{vehicle:{name:vehicle,label:'TANK',seats:[{weapons:[{state:{name:weapon,def:{displayName:'GUN'}}}]}]},index:0}:null,classDef:{displayName:'TROOPER'}});
 for(const [i,c] of world.entries()){c.className=names[0];c.unit=body(i===6?names[3]:names[1],i===7?names[2]:null);}
 for(let i=0;i<120;i++)stats.tick(1/60,world);
 const options={tick:600,epoch:1},records=captureRecordSummaries({characters:world},registry,options);
 assert.ok(records.length>1,'a whole roster is several chunks');
 records.forEach((r,i)=>assert.equal(r.id,RECORD_SUMMARY_ID+i));
 const page=make(),game={battle:{characters:page}};
 // A datagram each, as the scheduler sends them.
 records.forEach((record,i)=>{const [decoded]=decodeEntityPacket(encodeEntityPacket({epoch:1,packetId:i+1,tick:600,records:[record]})).records;applyWorldSummary(game,decoded,{registry});});
 const label=(kind,key)=>({class:{[names[0]]:'TROOPER'},weapon:{[names[1]]:'GUN'},vehicle:{[names[2]]:'TANK'}})[kind]?.[key]??null;
 for(const i of [0,3,5,7,8,13])assert.deepEqual(personalStats(page[i],page,{label}),personalStats(world[i],world),`Character ${i}`);
 const shown=list=>list.map(a=>[a.key,a.value,a.text]);
 assert.deepEqual(shown(awards(page)),shown(awards(world)));
 assert.equal(recordOf(page[6]).slots[0].weapons.length,0,'a favourite the registry cannot name is left out');
 // His own record mid-match: one record, to the second.
 const own=captureRecordSummaries({characters:world},registry,{...options,only:world[3],id:OWN_RECORD_ID,quantum:1000});
 assert.equal(own.length,1);assert.equal(own[0].id,OWN_RECORD_ID);
 const decoded=decodeWorldSummary(own[0],registry);
 // Character 3 fights for team 2, whose tallies are his second slot.
 assert.deepEqual(decoded.records.map(r=>r.slot),[3]);assert.equal(decoded.records[0].packed[1].lifeBest,Math.trunc(recordOf(world[3]).slots[1].lifeBest),'his life, in whole seconds as the table shows it');
 assert.equal(decoded.records[0].packed[0],null,'a team slot he never played in is left out');
});

test('roster chunks and the spawn summary arriving out of order leave no holes for a spectator to trip on',async()=>{
 const {captureRosterSummaries}=summaryModule,{Battle}=await import('../src/battle.js');
 const characters=[];for(let i=0;i<30;i++)characters.push({team:1+(i%2),human:i===25,name:`Bot ${i}`});
 const records=captureRosterSummaries({characters},new Map([[characters[25],{link:null}]]),{tick:9,epoch:1});
 // A spectator's battle: no player of its own, so Battle.human walks the roster.
 const battle=new Battle({teams:{1:{name:'rep'},2:{name:'cis'}},posts:[],own:1,foe:2}),game={battle};
 applyWorldSummary(game,records[2],{registry});
 assert.equal(battle.characters.length,30);assert.ok(battle.characters.every(Boolean),'no holes before the last chunk');
 assert.doesNotThrow(()=>battle.human);assert.equal(battle.human,battle.characters[25]);
 assert.equal(battle.characters[3].team,0,'a slot made early has no row until its chunk');
 applyWorldSummary(game,records[0],{registry});assert.equal(battle.characters[3].team,2);
 const fresh={battle:new Battle({teams:{1:{name:'rep'}},posts:[],own:1,foe:2}),humanSpawn:{}};
 applyWorldSummary(fresh,captureSpawnSummary({unit:{id:8,generation:3},team:1,spawn},{tick:9,epoch:1,registry,eligiblePosts:[],eligibleClasses:[],slot:4}),{registry});
 assert.ok(fresh.battle.characters.length===5&&fresh.battle.characters.every(Boolean));assert.equal(fresh.battle.human.index,4);
});
