import {entitySpawnPosts} from './entity-spawn-posts.js';
import {eligiblePosts as eligibleSpawnPosts} from '../human-spawn.js';
import {encodeEntityRecord} from './network-entity-codec.js';
import {Character} from '../battle.js';
import {packRecord,applyRecord,SHARED_TALLIES,FAVOURITES} from '../battle-stats.js';
const PHASES=['unspawned','alive','dying','awaiting'];
// The roster (schema 4): every Character's team, kind, scores, link and name,
// in chunks small enough for one datagram, by slot in battle.characters. The
// scoreboard, the HUD's stats and the Tab player list read it as they read a
// local battle's. A chunk that has not changed only heartbeats.
export const ROSTER_SUMMARY_ID=70000, ROSTER_CHUNK=12, ROSTER_NAME_BYTES=32, NO_SLOT=255;
export const LINK_KINDS=Object.freeze(['unknown','direct4','direct6','relay4','relay6']);
export function linkCode(path){if(!path||!path.kind||!path.family)return 0;const relay=path.kind==='relay',six=String(path.family).includes('6');return (relay?3:1)+(six?1:0);}
const encoder=new TextEncoder(),decoder=new TextDecoder();
class Fields{
 constructor(bytes=new Uint8Array(1000)){this.bytes=bytes;this.view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);this.at=0;}
 field(type,...args){const n={Uint8:1,Uint16:2,Uint32:4,Float64:8}[type];if(this.at+n>this.bytes.length)throw Error('summary exceeds bounds');const at=this.at;this.at+=n;if(!args.length)return this.view['get'+type](at,true);const v=args[0];if(!Number.isFinite(v)||type!=='Float64'&&(!Number.isInteger(v)||v<0||v>2**(n*8)-1))throw Error('invalid summary scalar');this.view['set'+type](at,v,true);}
 u8(...v){return this.field('Uint8',...v)}u16(...v){return this.field('Uint16',...v)}u32(...v){return this.field('Uint32',...v)}f64(...v){const x=this.field('Float64',...v);if(!v.length&&!Number.isFinite(x))throw Error('invalid summary float');return x;}
 bool(...v){if(v.length){if(typeof v[0]!=='boolean')throw Error('invalid summary boolean');this.u8(v[0]?1:0);return;}const b=this.u8();if(b>1)throw Error('invalid summary boolean');return !!b;}
 nullable(...v){if(v.length){this.bool(v[0]!==null);if(v[0]!==null)this.f64(v[0]);return;}return this.bool()?this.f64():null;}
}
const lookup=(map,name,nullable=true)=>{if(name==null&&nullable)return 0;for(const[id,value]of map)if(value===name)return id;throw Error(`summary registry missing ${name}`);};
const resolve=(map,id)=>{if(!id)return null;if(!map?.has(id))throw Error('summary registry prerequisite missing');return map.get(id);};
function record(w,{id,generation,tick,schema}){const r={kind:'summary',id,generation,tick,schema,payload:w.bytes.slice(0,w.at)};encodeEntityRecord(r);return r;}
export function createPostRegistry(posts){const names=posts.map(p=>p.name).sort();if(new Set(names).size!==names.length||names.length>65535)throw Error('invalid post registry');return new Map(names.map((name,i)=>[i+1,name]));}
export function captureBattleSummary(battle,{tick,epoch}){const w=new Fields(),teams=Object.keys(battle.reinforcements).map(Number).sort((a,b)=>a-b);if(teams.length>7)throw Error('summary team limit');w.u8(teams.length);for(const team of teams){w.u8(team);for(const key of ['reinforcements','start','noPost','bleed'])w.f64(battle[key]?.[team]??0);}for(const k of ['over','gameOver','finished','losing'])w.bool(!!battle[k]);w.u8(battle.loser||0);w.f64(battle.overTime);w.nullable(battle.victoryClock);w.nullable(battle.countdown);return record(w,{id:0,generation:epoch,tick,schema:1});}
export function capturePostSummary(post,{tick,epoch,registry}){const w=new Fields(),id=lookup(registry.posts,post.name,false);w.u16(id);w.u8(post.team);w.u8(post.toward);w.f64(post.progress);w.f64(post.neutralize);w.bool(post.contested);w.bool(post.active!==false);const counts=[...post.counts];if(counts.length>7)throw Error('summary post team limit');w.u8(counts.length);for(const[team,count]of counts){w.u8(team);w.u16(count);}return record(w,{id,generation:epoch,tick,schema:2});}
// Eligibility is provided by authoritative spawn policy, never derived from
// partial CP samples on the client. Allowed classes and posts are complete sets.
export function captureSpawnSummary(player,{tick,epoch,registry,eligiblePosts,eligibleClasses,slot=NO_SLOT}){if(!Array.isArray(eligiblePosts)||!Array.isArray(eligibleClasses)||eligiblePosts.length>64||eligibleClasses.length>64)throw Error('spawn eligibility limit');const s=player.spawn,w=new Fields();w.u32(player.unit?.id||0);w.u32(player.unit?.generation||0);w.u8(player.team);const phase=PHASES.indexOf(s.phase);if(phase<0)throw Error('invalid spawn phase');w.u8(phase);w.u16(lookup(registry.classes,s.className));w.u16(lookup(registry.posts,s.post));w.bool(!!s.spawnedOnce);w.bool(!!s.ended);w.f64(s.deadTime);w.bool(!!s.pending);if(s.pending){w.u16(lookup(registry.classes,s.pending.className,false));w.u16(lookup(registry.posts,s.pending.post));}w.bool(!!s.auto);if(s.auto){w.u8(s.auto.team);w.u16(lookup(registry.classes,s.auto.className,false));w.u16(lookup(registry.posts,s.auto.post));}for(const[list,map]of [[eligiblePosts,registry.posts],[eligibleClasses,registry.classes]]){w.u8(list.length);for(const name of list)w.u16(lookup(map,name,false));}
 // His own slot in the roster (battle.characters): the scoreboard's and the HUD's personal stats.
 w.u8(Number.isInteger(slot)&&slot>=0&&slot<NO_SLOT?slot:NO_SLOT);return record(w,{id:65536,generation:epoch,tick,schema:3});}
// players: Map of Character to { link } for every player attached. A
// Character flagged human that belongs to no player (the headless engine's
// own, never spawned) goes out as team 0: no row anywhere.
export function captureRosterSummaries(battle,players=new Map(),{tick,epoch}){
 const characters=battle?.characters||[],records=[];if(characters.length>ROSTER_CHUNK*200)throw Error('roster summary limit');
 for(let start=0;start<characters.length;start+=ROSTER_CHUNK){
  const w=new Fields(),slice=characters.slice(start,start+ROSTER_CHUNK);w.u16(characters.length);w.u16(start);w.u8(slice.length);
  for(const c of slice){const player=players.get(c),link=player?.link,human=players.has(c);w.u8(!human&&c.human?0:Math.max(0,Math.min(255,c.team|0)));w.u8(human?1:0);
   for(const key of ['kills','teamKills','deaths','captures'])w.u16(Math.max(0,Math.min(65535,c[key]|0)));
   w.u16(link&&Number.isFinite(link.rtt)?Math.min(65534,Math.max(0,Math.round(link.rtt))):65535);w.u8(linkCode(link?.path));
   let bytes=encoder.encode(String(c.name??''));if(bytes.length>ROSTER_NAME_BYTES)bytes=bytes.subarray(0,ROSTER_NAME_BYTES);w.u8(bytes.length);for(const b of bytes)w.u8(b);}
  records.push(record(w,{id:ROSTER_SUMMARY_ID+start/ROSTER_CHUNK,generation:epoch,tick,schema:4}));
 }
 return records;
}
export function decodeWorldSummary(record,registry){if(record.kind!=='summary'||!(record.payload instanceof Uint8Array))throw Error('invalid world summary');const r=new Fields(record.payload),s={schema:record.schema};if(s.schema===1){s.teams=[];const count=r.u8();if(count>7)throw Error('summary team limit');for(let i=0;i<count;i++){const team=r.u8();if(team<1||team>7||s.teams.some(x=>x.team===team))throw Error('invalid summary team');const t={team};for(const k of ['reinforcements','start','noPost','bleed'])t[k]=r.f64();s.teams.push(t);}for(const k of ['over','gameOver','finished','losing'])s[k]=r.bool();s.loser=r.u8()||null;if(s.loser>7)throw Error('invalid losing team');s.overTime=r.f64();s.victoryClock=r.nullable();s.countdown=r.nullable();if(record.id!==0)throw Error('summary identity mismatch');}
 else if(s.schema===2){s.postId=r.u16();s.name=resolve(registry.posts,s.postId);s.team=r.u8();s.toward=r.u8();s.progress=r.f64();s.neutralize=r.f64();s.contested=r.bool();s.active=r.bool();const count=r.u8();if(count>7||s.team>7||s.toward>7||s.progress<0||s.progress>1||s.neutralize<0||s.neutralize>1||record.id!==s.postId)throw Error('invalid post summary');s.counts=new Map();for(let i=0;i<count;i++){const team=r.u8(),n=r.u16();if(team<1||team>7||s.counts.has(team))throw Error('invalid post count');s.counts.set(team,n);}}
 else if(s.schema===3){s.playerId=r.u32();s.generation=r.u32();s.team=r.u8();s.phase=PHASES[r.u8()];s.className=resolve(registry.classes,r.u16());s.post=resolve(registry.posts,r.u16());s.spawnedOnce=r.bool();s.ended=r.bool();s.deadTime=r.f64();s.pending=r.bool()?{className:resolve(registry.classes,r.u16()),post:resolve(registry.posts,r.u16())}:null;s.auto=r.bool()?{team:r.u8(),className:resolve(registry.classes,r.u16()),post:resolve(registry.posts,r.u16())}:null;if(s.auto&&(s.auto.team<1||s.auto.team>7))throw Error('invalid spawn policy');if(!s.phase||s.team<1||s.team>7||s.deadTime<0||record.id!==65536)throw Error('invalid spawn summary');for(const[key,map]of [['eligiblePosts',registry.posts],['eligibleClasses',registry.classes]]){const count=r.u8();if(count>64)throw Error('spawn eligibility limit');s[key]=Array.from({length:count},()=>{const id=r.u16();resolve(map,id);return id;});if(new Set(s[key]).size!==count)throw Error('duplicate spawn eligibility');}
  const slot=r.u8();s.slot=slot===NO_SLOT?null:slot;}
 else if(s.schema===4){s.total=r.u16();s.start=r.u16();const count=r.u8();if(count>ROSTER_CHUNK||s.start%ROSTER_CHUNK||s.start+count>s.total||record.id!==ROSTER_SUMMARY_ID+s.start/ROSTER_CHUNK)throw Error('invalid roster summary');s.characters=[];
  for(let i=0;i<count;i++){const team=r.u8(),flags=r.u8();if(team>7||flags>1)throw Error('invalid roster entry');const entry={team,human:flags===1};for(const key of ['kills','teamKills','deaths','captures'])entry[key]=r.u16();const ping=r.u16(),link=r.u8();if(link>=LINK_KINDS.length)throw Error('invalid roster link');entry.ping=ping===65535?null:ping;entry.link=LINK_KINDS[link];const length=r.u8();if(length>ROSTER_NAME_BYTES)throw Error('invalid roster name');const bytes=new Uint8Array(length);for(let n=0;n<length;n++)bytes[n]=r.u8();entry.name=decoder.decode(bytes);s.characters.push(entry);}}
 else if(s.schema===5){if(record.id<OWN_RECORD_ID)throw Error('invalid record summary');const count=r.u8();s.records=[];
  for(let i=0;i<count;i++){const slot=r.u16(),mask=r.u8();if(mask<1||mask>3)throw Error('invalid record slots');const packed=[null,null];
   for(let k=0;k<2;k++){if(!(mask&1<<k))continue;const p={};
    for(const key of SHARED_TALLIES)p[key]=RECORD_TIMES.has(key)?r.u32()/1000:RECORD_U16.has(key)?r.u16():r.u8();
    for(const table of FAVOURITES){const id=r.u16();p[table]=id?[resolve(table==='weapons'?registry.weapons:registry.classes,id),null,r.u32()/1000]:null;}
    p.victims=[];const victims=r.u8();for(let v=0;v<victims;v++)p.victims.push([r.u16(),r.u8()]);
    packed[k]=p;}
   s.records.push({slot,packed});}}
 else throw Error('unknown summary schema');if(r.at!==record.payload.length)throw Error('trailing world summary');return s;}
// The roster's Character at slot, made if missing with every slot before it:
// chunks and the spawn summary arrive in any order, and a hole would break
// whatever walks the roster whole (Battle.human, the scoreboard), as a
// spectator's first frames did. A slot made early stays team 0, no row
// anywhere, until its chunk arrives.
function rosterSlot(characters,slot,team,human){for(let i=0;i<slot;i++)characters[i]||=new Character(i,0,false);return characters[slot]||=new Character(slot,team,human);}
// Validate all prerequisites before mutating persistent presentation state.
// The caller/EntityReceiver enforces epoch+per-record version ordering first.
export function applyWorldSummary(game,record,{registry,spawn=game.humanSpawn}={}){const s=decodeWorldSummary(record,registry);if(s.schema===1){if(!game.battle||s.teams.some(t=>!Object.hasOwn(game.battle.reinforcements,t.team)))throw Error('battle prerequisite missing');const wasOver=Boolean(game.battle.over);for(const t of s.teams)for(const k of ['reinforcements','start','noPost','bleed']){game.battle[k] ||= {};game.battle[k][t.team]=t[k];}for(const k of ['over','gameOver','finished','losing','loser','overTime','victoryClock','countdown'])game.battle[k]=s[k];
 // The battle's own end announcement (Victory or Defeat), once, as the local
 // Battle.update would have raised it.
 if(!wasOver&&s.over&&typeof game.battle.onOver==='function')game.battle.onOver(s.loser);}
 else if(s.schema===2){const post=game.posts?.posts.find(p=>p.name===s.name);if(!post)throw Error('post prerequisite missing');const previous=post.networkTeam,wasActive=post.networkActive;post.networkTeam=s.team;post.networkActive=s.active;
  // A destructible post's building destroyed on the server: lost, as the local CommandPosts would announce it.
  if(wasActive===true&&!s.active&&post.building){const event={kind:'lost',post,from:s.team,destroyed:true};game.audio?.on('post',event);game.hud?.message?.(game.hud.text('game.infomsg.lost',game.battle.names?.[s.team]??'',post.desc?.displayName||post.label),game.posts.relation?.(s.team));}
  else if(previous!==undefined&&previous!==s.team){const event={kind:s.team?'captured':'lost',post,by:s.team,from:previous};game.audio?.on('post',event);const team=s.team||previous,name=post.desc?.displayName||post.label,key=s.team?'game.infomsg.captured':'game.infomsg.lost';game.hud?.message?.(game.hud.text(key,game.battle.names?.[team]??'',name),game.posts.relation?.(team));}for(const k of ['team','toward','progress','neutralize','contested','active'])post[k]=s[k];post.counts=new Map(s.counts);}
 else if(s.schema===4){const battle=game.battle;if(!battle||!Array.isArray(battle.characters))throw Error('battle prerequisite missing');
  // The server's roster replaces the local battle's, slot for slot, so the
  // scoreboard and the HUD read it as they read a local battle's Characters.
  if(battle.characters.length>s.total)battle.characters.length=s.total;
  for(let i=0;i<s.characters.length;i++){const entry=s.characters[i],c=rosterSlot(battle.characters,s.start+i,entry.team,entry.human);
   c.team=entry.team;c.human=entry.human;c.name=entry.name;for(const key of ['kills','teamKills','deaths','captures'])c[key]=entry[key];c.link=entry.ping===null&&entry.link==='unknown'?null:{ping:entry.ping,kind:entry.link};}}
 else if(s.schema===5){const characters=game.battle?.characters;if(!Array.isArray(characters))throw Error('battle prerequisite missing');
  // Until the scores stop, the player's own record comes only from his own
  // (OWN_RECORD_ID, ten times a second): everyone's lag up to 5 s behind it.
  const own=record.id!==OWN_RECORD_ID&&!game.battle.gameOver?game.battle.local:null;
  // A Character the roster has not made yet gets this record with the next heartbeat.
  for(const {slot,packed} of s.records)if(characters[slot]&&characters[slot]!==own)applyRecord(characters[slot],packed,characters);}
 else {if(!spawn)throw Error('spawn prerequisite missing');
  // His slot in the roster is his Character, whichever record arrives first.
  if(s.slot!==null&&Array.isArray(game.battle?.characters)){const human=rosterSlot(game.battle.characters,s.slot,s.team,true);game.battle.human=human;if(game.playerUnit)game.playerUnit.character=human;}
  if(typeof spawn.applyAuthoritative==='function')spawn.applyAuthoritative(s);else for(const k of ['phase','className','post','spawnedOnce','ended','deadTime','pending','auto'])spawn[k]=s[k];}return s;}

// End-of-match records (schema 5): each Character's tallies, victims and
// favourites (battle-stats.js packRecord) by roster slot, in chunks that fit
// one datagram, so the end table's DETAILS and AWARDS read them as they read
// a local battle's. Times in milliseconds, or whole seconds (`quantum` 1000)
// for a player's own record mid-match, which then changes once a second. A
// favourite the content registry cannot name (a turret's gun) is left out;
// the page names the rest from its own content (the scoreboard's label).
export const RECORD_SUMMARY_ID=80000, OWN_RECORD_ID=79999, RECORD_BYTES=900;
const RECORD_U16=new Set(['headshots']),RECORD_TIMES=new Set(['lifeBest','campBest']);
const ms=(seconds,quantum)=>Math.min(0xffffffff,Math.max(0,Math.floor(seconds*1000/quantum)*quantum));
const favouriteIds=(registry,table)=>table==='weapons'?registry.weaponIds:registry.classIds;
function writeRecord(w,slot,packed,registry,quantum){
 w.u16(slot);w.u8(packed.reduce((mask,p,i)=>p?mask|1<<i:mask,0));
 for(const p of packed){if(!p)continue;
  for(const key of SHARED_TALLIES){const v=p[key];if(RECORD_TIMES.has(key))w.u32(ms(v,quantum));else if(RECORD_U16.has(key))w.u16(Math.min(65535,v|0));else w.u8(Math.min(255,v|0));}
  for(const table of FAVOURITES){const best=p[table],id=best?favouriteIds(registry,table)?.get(best[0])??0:0;w.u16(id);if(id)w.u32(ms(best[2],quantum));}
  const victims=p.victims.filter(([index])=>index<65535).slice(0,255);w.u8(victims.length);for(const [index,n] of victims){w.u16(index);w.u8(Math.min(255,n|0));}
 }
}
export function captureRecordSummaries(battle,registry,{tick,epoch,only=null,id=RECORD_SUMMARY_ID,quantum=1}){
 const characters=battle?.characters||[],records=[];
 let w=null,count=0,countAt=0;
 const close=()=>{if(!w)return;w.bytes[countAt]=count;records.push(record(w,{id:id+records.length,generation:epoch,tick,schema:5}));w=null;};
 for(let slot=0;slot<characters.length&&slot<65535;slot++){
  if(only&&characters[slot]!==only)continue;
  // A team slot he never played in (all nought) is left out; nothing at all, he is.
  const packed=packRecord(characters[slot],characters)?.map(p=>SHARED_TALLIES.some(key=>p[key])||p.victims.length||FAVOURITES.some(table=>p[table])?p:null);
  if(!packed||packed.every(p=>!p))continue;
  const entry=new Fields(new Uint8Array(4096));writeRecord(entry,slot,packed,registry,quantum);
  if(entry.at>RECORD_BYTES)continue;
  if(w&&(w.at+entry.at>RECORD_BYTES||count===255))close();
  if(!w){w=new Fields();countAt=w.at;w.u8(0);count=0;}
  w.bytes.set(entry.bytes.subarray(0,entry.at),w.at);w.at+=entry.at;count++;
 }
 close();
 return records;
}

// The players attached, by Character, with the link (ping and path) the session reports for each.
function attachedPlayers(world){const players=new Map();for(const p of world.players?.values?.()||[])if(p.character)players.set(p.character,{link:p.link||null});return players;}
export function captureWorldSummaries(game,world,registry,{tick=game.worldClock.tick,epoch=1,playerId,includeCommon=true,eligibilityCache}={}){
 const records=includeCommon?[captureBattleSummary(game.battle,{tick,epoch}),...game.posts.posts.map(post=>capturePostSummary(post,{tick,epoch,registry})),...captureRosterSummaries(game.battle,attachedPlayers(world),{tick,epoch}),
  // Everyone's records once the scores stop (they no longer change), for the end table.
  ...(game.battle.gameOver?captureRecordSummaries(game.battle,registry,{tick,epoch}):[])]:[];
 const player=world.players.get(playerId);if(!player||player.spectator)return records;
 const over=game.battle.gameOver;
 // This optional cache belongs to one synchronous publication, never a match.
 let eligibility=eligibilityCache?.get(player.team);
 if(!eligibility||eligibility.tick!==tick||eligibility.epoch!==epoch||eligibility.over!==over){
  eligibility={tick,epoch,over,eligiblePosts:over?[]:eligibleSpawnPosts(entitySpawnPosts(game),player.team,(a,b)=>game.units.relation(a,b)).map(p=>p.name),eligibleClasses:over?[]:game.spawnPort.roster(player.team).filter(c=>game.factory.prepared(c.className)).map(c=>c.className)};
  eligibilityCache?.set(player.team,eligibility);
 }
 const {eligiblePosts,eligibleClasses}=eligibility;
 const slot=game.battle?.characters?.indexOf(player.character)??-1;
 records.push(captureSpawnSummary(player,{tick,epoch,registry,eligiblePosts,eligibleClasses,slot:slot>=0?slot:NO_SLOT}));
 // His own record all match long (DETAILS on his row), to the second.
 if(player.character)records.push(...captureRecordSummaries(game.battle,registry,{tick,epoch,only:player.character,id:OWN_RECORD_ID,quantum:1000}));
 return records;
}
