// Unordered state must not turn into an ordered decode backlog on a slow page.
// One decode runs; every newer complete snapshot replaces the waiting one.
export class LatestSnapshot {
  constructor(consume){this.consume=consume;this.pending=null;this.running=null;}
  clear(){this.pending=null;}
  push(value){this.pending=value;if(!this.running)this.running=this.drain();return this.running;}
  async drain(){try{while(this.pending){const value=this.pending;this.pending=null;await this.consume(value);}}finally{this.running=null;}}
}
