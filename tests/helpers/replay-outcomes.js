// Synthetic transport outcomes corresponding to an existing reference input.
// The native frame must join these facts back to the frozen command stream.
export function replayOutcomes(commands){const result=new Map();let id=0;for(const c of commands)for(const action of c.actions||[])result.set(++id,{sequence:c.sequence,edge:{kind:'action',action,tick:c.tick,generation:c.generation}});return result;}
