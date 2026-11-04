// Opaque nonnumeric state values retain their JavaScript identities. The
// common numeric states and all transition ordering live in Rust.
export class NativeFsmFields {
  constructor(sim,machine) {
    const id=this.id=sim.x.ai_fsm_new(),values=[],tokens=new Map();
    ['state','pending'].forEach((name,index)=>{
      const initial=machine[name];
      Object.defineProperty(machine,name,{enumerable:true,configurable:true,
        get:()=>{const kind=sim.x.ai_fsm_kind(id,index),value=sim.x.ai_fsm_field(id,index);return kind===0?value:values[value];},
        set:value=>{
          if(typeof value==='number'){sim.x.ai_fsm_write(id,index,0,value);return;}
          let token=tokens.get(value);if(token===undefined){token=values.length;values.push(value);tokens.set(value,token);}
          sim.x.ai_fsm_write(id,index,1,token);
        }});
      machine[name]=initial;
    });
  }
}
