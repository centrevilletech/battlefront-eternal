// Connections own stable native Character slots. Reservation, reuse and team
// changes publish their touched rows without feeding the roster back in.
import { MAX_TEAM_SIZE } from './battle-limits.js';
import { botNameGenerator } from './bot-names.js';
import { PhaseCounters } from './sim/phase-contract.js';
import * as F from './sim/gen/f4.js';
const SETUP={words:F.F4RosterSetupLayout.words,put:F.putF4RosterSetup};
const COMMAND={words:F.F4RosterCommandLayout.words,put:F.putF4RosterCommand};
const RESULT={Mirror:F.F4RosterResultMirror,sync:F.syncF4RosterResult};
export class F4Roster {
  constructor(tail,game,armySize=MAX_TEAM_SIZE){
    if(armySize!==null&&(!Number.isInteger(armySize)||armySize<1||armySize>MAX_TEAM_SIZE))throw Error('invalid army size');
    this.tail=tail;this.bridge=tail.bridge;this.x=tail.x;this.counters=new PhaseCounters();this.change=new F.F4RosterCommandMirror();
    this.classic=Boolean(game.ai?.spawning&&game.ai?.classic);
    this.countRows=this.classic?[{team:1,humans:0,total:0},{team:2,humans:0,total:0}]:[{team:1,humans:0},{team:2,humans:0}];
    const spawning=game.ai?.spawning,names=[];
    if(this.classic)for(const team of spawning.teams){
      const source=botNameGenerator(team.botNames),faction=team.faction,side=faction>=1&&faction<=4?faction:0;
      names.push({faction,state:source?.random?.state??null,first:source?.first??spawning.names?.[side]?.first??[],
        last:source?.last??spawning.names?.[side]?.last??[],local:source?.local??spawning.names?.local??null});
    }
    this.bridge.callArgs(SETUP,{classic:this.classic,hasAi:Boolean(game.ai),armySize,local:game.battle.human??null,names});
    const p=tail.phase,active=p.active;p.random.push();p.active=true;
    try{this.x.f4_roster_init();}finally{p.active=active;}
    p.random.pull();p.sync();
  }
  command(op,character=null,team=0,id=undefined,name='',handle=null){
    const p=this.tail.phase,b=this.bridge,c=this.change,counters=this.counters,previous=b.phaseCounters,active=p.active;
    c.op=op;c.target=handle??(character?this.tail.entries.get(character).handle:null);c.team=team;c.playerId=id;c.name=name;
    const output=this.tail.mirrors.words,unitWords=p.counters.outputFields;
    p.random.push();b.phaseCounters=counters;p.active=true;
    counters.wasmCalls++;counters.commandCalls++;counters.inputFields+=COMMAND.words;
    let result;
    try{
      b.callArgs(COMMAND,c);result=b.callResult(RESULT,this.x.f4_roster_command());
      result.character=result.handle?this.tail.ensureCharacter(result.handle):null;
    }finally{p.active=active;b.phaseCounters=previous;}
    p.random.pull();p.sync();counters.outputFields+=this.tail.mirrors.words-output+p.counters.outputFields-unitWords;
    if(result.full)throw Error('team full');return result;
  }
  reserve(team,id,name){return this.command('reserve',null,team,id,String(name)).character;}
  release(character){this.command('release',character);}
  switchTeam(character,team){return this.command('switchTeam',character,team).flag;}
  counts(){
    const result=this.command('counts');
    for(let i=0;i<this.countRows.length;i++){
      const out=this.countRows[i],row=result.counts[i];out.team=row.team;out.humans=row.humans;if(this.classic)out.total=row.total;
    }
    return this.countRows;
  }
}
