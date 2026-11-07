// Native AI captures voice facts at the source emission point. The observer
// only journals those facts; it never rereads a controller, pose or team.
export class AiNotices {
  constructor(events, items) {
    this.events=events;this.items=items;this.fields=[];
    // Source VO has absent optional keys, rather than explicit null fields.
    // Prepare each presence shape once so ordinary notices retain their layout.
    for(let mask=0;mask<16;mask++) {
      const fields={source:null,team:0,position:null,human:false,event:'',property:'',time:0};
      if(mask&1)fields.command=0;if(mask&2)fields.context=0;
      if(mask&4)fields.positional=false;if(mask&8)fields.notice=false;
      this.fields.push(fields);
    }
  }
  voice(event) {
    if(!this.events)return;
    const mask=(event.command!=null?1:0)|(event.context!=null?2:0)|(event.positional!=null?4:0)|(event.notice!=null?8:0);
    const fields=this.fields[mask];
    // SimEvents retains its source reference shallowly. Give each actual VO
    // its immutable gameplay generation, as squads.emitVo does; reusing this
    // tiny reference would silently rewrite an older journal record's life.
    fields.source={unit:event.actor,generation:event.generation};
    fields.team=event.team;fields.position=event.position;fields.human=event.human;
    fields.event=event.event;fields.property=event.property;fields.time=event.time;
    if(mask&1)fields.command=event.command;if(mask&2)fields.context=event.context;
    if(mask&4)fields.positional=event.positional;if(mask&8)fields.notice=event.notice;
    this.events.emit('vo',fields);
  }
  itemOwner(event) {
    // ownedBy numbers its owner even when the returned item list is empty.
    // This first-use identity notice is not another simulation journal event.
    this.items.ownerKey(event.actor);
  }
}
