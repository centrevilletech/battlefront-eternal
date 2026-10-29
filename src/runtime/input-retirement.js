// A discard releases history without asserting that its input was simulated.
// Only a consumed ACK contributes to lateness estimation or fallback held keys.
export function retireAuthoritativeInputs(client, {ack=0,discardedInput=0,heldInput,viewInput,generation,tick,ackTick=tick,measureLead=true}) {
  const retired=Math.max(ack,discardedInput);
  while(client.pending.length&&client.pending[0].sequence<=retired){
    const command=client.pending.shift();
    if(measureLead&&command.sequence===ack)client.lead.acknowledge(ackTick-command.tick);
    if(heldInput===undefined&&command.sequence<=ack&&command.generation===generation){client.held=command.down;client.heldGeneration=generation;}
  }
  if(heldInput!==undefined){client.held=[...heldInput];client.heldGeneration=generation;}
  if(viewInput!==undefined)client.replayView=structuredClone(viewInput);
  if(discardedInput>(client.discardedInput??0)){
    client.predictionWindow?.reset();client.vehicleWindow?.reset();
  }
  client.discardedInput=Math.max(client.discardedInput??0,discardedInput);
  client.ackInput=ack;
  return retired;
}
