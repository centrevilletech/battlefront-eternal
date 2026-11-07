import test from 'node:test';
import assert from 'node:assert/strict';
import { loadSim } from '../src/sim/module.js';
import { missionTeams } from '../src/sim/wasm/mission-teams.js';

// The page uses the native parser without creating an AI battle or consuming
// gameplay draws. Keep the historical faction aliases and directed alliances.
test('cold mission metadata preserves faction aliases and directed capture teams',async()=>{
  const instance=await loadSim();
  const calls=[{name:'SetTeamName',args:[1,'Republic']},{name:'SetTeamName',args:[2,'CIS']},
    {name:'SetTeamName',args:[3,'locals']},{name:'SetTeamAsFriend',args:[3,1]}];
  const result=missionTeams(instance,calls,{names:['Alliance','all','Empire','IMP','Republic','rep','CIS','neutral','locals','Local','Naboo Guard','']});
  assert.deepEqual(result.names,[1,1,2,2,3,3,4,0,5,5,5,5]);
  assert.deepEqual(result.factions,[0,3,4,5,0,0,0,0]);
  assert.equal(result.relations[3][1],1);assert.equal(result.relations[1][3],-1);
  assert.deepEqual(result.capture,[0,1,2,1,4,5,6,7]);
  assert.deepEqual(missionTeams(instance,calls),{...result,names:[]});
});

test('cold mission metadata retains extracted faction overrides and neutral locals',async()=>{
  const result=missionTeams(await loadSim(),[{name:'SetTeamName',args:[3,'unknown'],faction:4},
    {name:'SetTeamAsFriend',args:[3,1]},{name:'SetTeamAsFriend',args:[3,2]}]);
  assert.equal(result.factions[3],4);assert.equal(result.capture[3],0);
  assert.equal(result.relations[0][0],0);
});
