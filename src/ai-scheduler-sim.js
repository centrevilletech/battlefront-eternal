// Decision ordering and LOD are native. Services read their current live
// objects at each callback, including changes made by earlier services.
import { ClassicScheduler } from './ai/classic/scheduler.js';

export class NativeScheduler extends ClassicScheduler {
  constructor(sim,ai) {
    super(ai);
    this.sim=sim;
    const field=(object,key,index)=>Object.defineProperty(object,key,{enumerable:true,configurable:true,
      get:()=>sim.x.ai_scheduler_read(index),set:value=>sim.x.ai_scheduler_write(index,value)});
    ['counter','parity','inputParity','skips'].forEach((key,index)=>field(this,key,index));
    ['serviced','skipped','considered','budget'].forEach((key,index)=>field(this.stats,key,index+4));
    sim.bridge.register({aiSchedule:args=>{
      // The bridge reuses its argument record. Copy before any callback
      // can enter another scheduler or navigation export on this instance.
      const op=args.op,object=sim.objects[args.id],number=args.number,phase=args.phase;
      const result={number:0,flag:false,values:[]};
      if(op==='config') { const c=this.config();result.values=[c.cycle,c.losBudget]; }
      else if(op==='notify') ai.vehicles?.notify?.(ai);
      else if(op==='humans') result.values=ai.humanUnits().map(unit=>sim.id(unit));
      else if(op==='broadcast') ai.combat?.broadcastAcoustic?.(object,ai);
      else if(op==='controllers') result.values=ai.controllers.map(c=>sim.id(c));
      else if(op==='controller') result.values=[Number(Boolean(object.processed)),Number(Boolean(object.lodSkipped)),Number(Boolean(object.live)),sim.id(object.unit)];
      else if(op==='processed') object.processed=Boolean(number);
      else if(op==='rebuild') ai.combat?.rebuildProtected?.(ai);
      else if(op==='lodSkip') result.flag=Boolean(this.lodSkip(object));
      else if(op==='service') this.service(object,number);
      else if(op==='plannerIdle') result.flag=Boolean(ai.nav?.plannerIdle?.());
      else if(op==='navService') ai.nav?.service?.(number);
      else if(op==='losService') result.flag=Boolean(ai.combat?.serviceLos?.(ai.world,number)??false);
      else if(op==='phase') object.update(number,phase);
      else if(op==='dropLos') ai.combat?.dropLosRequests?.(object);
      else if(op==='scan') ai.combat?.scan?.(object,ai.world);
      else if(op==='clearCandidate') ai.combat?.clearCandidate?.(object);
      else if(op==='lodDisabled') result.flag=Boolean(ai.lodDisabled());
      else if(op==='identity') result.number=ai.identity(object);
      else if(op==='velocity') {const v=object.velocity;if(v)result.values=[v.x,v.y,v.z];}
      else if(op==='cameras') {
        result.values=[Number(Boolean(ai.world?.networkViews))];
        for(const c of ai.cameras()) {
          const elements=c?.matrixWorld?.elements;
          result.values.push(Number(Boolean(elements)),c?.zoom??1,c?.near??.1,c?.far??Infinity,c?.fov??60,c?.aspect??1,
            ...(elements??new Array(16).fill(0)));
        }
      }
      else if(op==='position') {const p=object.position;result.values=[p.x,p.y,p.z];}
      else if(op==='writeLod') object.lodSkipped=Boolean(number);
      else if(op==='inputAt') result.number=sim.id(ai.controllers[number]);
      else if(op==='executeInput') this.executeInput(object,number);
      else if(op==='budgets') this.serviceBudgets(number);
      else if(op==='visible') result.flag=Boolean(this.visibleOrNear(object));
      else throw Error(`unknown AI scheduler callback ${op}`);
      return result;
    }});
  }
  update(dt) { this.sim.x.ai_scheduler_command(0,0,dt); }
  service(controller,dt) { this.sim.x.ai_scheduler_command(1,this.sim.id(controller),dt); }
  serviceBudgets(budget) { this.sim.x.ai_scheduler_command(2,0,budget); }
  lodSkip(controller) { return Boolean(this.sim.x.ai_scheduler_command(3,this.sim.id(controller),0)); }
  visibleOrNear(unit) { return Boolean(this.sim.x.ai_scheduler_command(4,this.sim.id(unit),0)); }
  executeInputs(dt) { this.sim.x.ai_scheduler_command(5,0,dt); }
}
