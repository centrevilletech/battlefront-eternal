export function bindSpawnSelection(actor,registry) { return actor.mailbox.bridge.backend.prepare('bindSpawnSelection',actor,registry); }
// Input owners are admitted once, before ingress begins.
export class InputMailbox {
  constructor(epoch=1,capacity=256,bridge=null,registry) {
    if(!bridge)throw Error('InputMailbox requires a battle simulation bridge');
    return bridge.backend.prepare('InputMailbox',epoch,capacity,bridge,registry);
  }
}
