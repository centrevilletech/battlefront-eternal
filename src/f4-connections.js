// Connection admission prepares the native lifecycle once. Each command is
// qualified by the roster's Character life; ordinary input never feeds body
// or spawn tables back into the owner.
import {PhaseCounters} from './sim/phase-contract.js';
import * as F from './sim/gen/f4.js';
const COMMAND={words:F.F4ConnectionCommandLayout.words,put:F.putF4ConnectionCommand};
const RESULT={Mirror:F.F4ConnectionViewMirror,sync:F.syncF4ConnectionView};
export class F4Connections {
  constructor(tail) {
    this.counters=new PhaseCounters();this.tail=tail;this.phase=tail.phase;this.change=new F.F4ConnectionCommandMirror();
    // Character slots are bounded by the battle's cold roster admission.
    this.phase.x.f4_connections_init(tail.characterCapacity);this.phase.sync();
  }
  command(op,player,className=null,post=null) {
    const p=this.phase,b=p.bridge,c=this.change,active=p.active,previous=b.phaseCounters,counters=this.counters;
    c.op=op;c.target=this.tail.entries.get(player.character).handle;
    c.className=className;c.post=post;c.ordinal=player.ordinal;c.autoRespawn=Boolean(player.autoRespawn);
    p.random.push();p.active=true;b.phaseCounters=counters;
    counters.wasmCalls++;counters.commandCalls++;counters.inputFields+=COMMAND.words;
    let pointer;
    try {b.callArgs(COMMAND,c);pointer=p.x.f4_connection_command();}
    finally {p.active=active;b.phaseCounters=previous;}
    // Bind a cold structural Unit birth before decoding its result reference.
    p.random.pull();p.sync();
    const result=b.callResult(RESULT,pointer);counters.outputFields+=F.F4ConnectionViewLayout.words;
    if(result.accepted) {
      player.unit=result.unit;player.className=result.className;
      const view=player.spawn;view.phase=result.phase;view.className=result.className;view.post=result.post;
      view.spawnedOnce=result.spawnedOnce;view.deadTime=result.deadTime;view.ended=result.ended;
    }
    return result.accepted;
  }
}
