import test from 'node:test';
import assert from 'node:assert/strict';
import {HumanSpawn} from '../src/human-spawn.js';
const fixture=()=>new HumanSpawn({policy:{auto:null,className:'soldier'},
  world:{team:()=>1,roster:()=>['soldier','pilot'],over:()=>false}});

test('the page follows published human phases and emits their ordered notices',()=>{
  const human=fixture(),events=[];
  for(const name of ['spawned','playerDied','awaitingSpawn'])human.on(name,()=>events.push(name));
  human.applyAuthoritative({phase:'alive',className:'soldier',post:'home',spawnedOnce:true});
  human.applyAuthoritative({phase:'dying',className:'soldier',spawnedOnce:true,deadTime:1});
  human.applyAuthoritative({phase:'awaiting',className:'pilot',spawnedOnce:true,deadTime:4});
  assert.deepEqual(events,['spawned','playerDied','awaitingSpawn']);
  assert.deepEqual(human.state(),{phase:'awaiting',team:1,className:'pilot',classIndex:1,post:'home',canSwitchTeams:true,over:false});
});
test('a failed page listener does not suppress the next spawn listener',()=>{
  const human=fixture(),heard=[],error=console.error;console.error=()=>{};
  human.on('spawned',()=>{throw Error('listener');});human.on('spawned',()=>heard.push('spawned'));
  try {human.applyAuthoritative({phase:'alive',className:'soldier'});}finally{console.error=error;}
  assert.deepEqual(heard,['spawned']);
});
