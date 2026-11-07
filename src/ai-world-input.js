// The simulation journal crosses once per AI phase as sparse event facts.
// Unrelated events still consume ring positions. Resolving life, hearing and
// target eligibility remains at the native combat reader's observation point.
import { Vector3 } from 'three';
import { AiJournalEventMirror } from './sim/gen/ai-phase.js';
function reference(row, value) {
  row.member = null;row.generation = null;row.direct = false;
  if (!value) return;
  if (value.unit) {row.member=value.unit;row.generation=value.generation??null;}
  else if (value.alive !== undefined) {row.member=value;row.direct=true;}
}
export class AiJournalFacts {
  constructor(capacity = 4096) {
    this.sequence=0;this.cursor=0;this.count=0;this.records=[];this.points=[];
    // The active prefix has a scalar count. Truncating and regrowing an array
    // each tick would discard its backing storage even with pooled row objects.
    for(let i=0;i<capacity;i++){this.records.push(new AiJournalEventMirror());this.points.push(new Vector3());}
  }
  capture(journal) {
    const head=journal.seq;
    if(head<this.cursor)this.cursor=0;
    // A delayed or unexpectedly larger source retains its newest bounded
    // window. Overwritten facts stay lost, just as in the source ring.
    let start=Math.max(this.cursor,head-Math.min(journal.capacity,this.records.length));
    this.count=0;this.sequence=head;
    for(;start<head;start++) {
      const event=journal.ring[start%journal.capacity];
      if(!event || event.kind!=='shot' && event.kind!=='damage')continue;
      const index=this.count++,row=this.records[index];
      row.sequence=start;row.kind=event.kind;
      reference(row.source,event.source);reference(row.target,event.target);reference(row.attacker,event.attacker);
      row.attackerCharacter=event.attackerCharacter??null;
      row.amount=event.amount??null;row.generic=event.generic??null;row.typed=event.typed??null;row.known=Boolean(event.known);
      if(event.point) {
        const point=this.points[index];row.point=point;
        if(Array.isArray(event.point))point.set(event.point[0],event.point[1],event.point[2]);
        else point.copy(event.point);
      }else row.point=null;
    }
    this.cursor=head;return this;
  }
}
