// Vehicle algorithms join the battle's AI bridge. The service and its
// controller-facing objects keep their identity when native ownership binds.
import { Vector3 } from 'three';
import { AiVehicleServiceOp_INDEX, AiVehicleAuxOp_INDEX } from './sim/gen/ai-vehicle.js';
import { vehicleCommands } from './ai/classic/vehicles/live-commands.js';
import { vehicleWorld } from './ai/classic/vehicles/live-world.js';
import { encode,decoder } from './ai/classic/vehicles/wire.js';
import { requestView,unitView } from './ai/classic/vehicles/live-views.js';

export function bindVehicleAI(service, classes) {
  const sim=service.world.aiSim;
  if(!sim || service.nativeVehicle)return service;
  const native=new NativeVehicleAI(service,sim,classes);
  service.nativeVehicle=native;
  return service;
}
class NativeVehicleAI {
  constructor(service,sim,classes) {
    this.service=service;this.sim=sim;this.frames=[];
    const id=o=>sim.id(o),optional=o=>o ? id(o):null;
    sim.bridge.register({aiVehicleWorld:args=>vehicleWorld(service,sim,args,this.frames.at(-1))});
    for(const [name,[types,result]] of Object.entries(vehicleCommands)) {
      service[name]=(...args)=>this.command(name,types,args,result);
    }
    const policies=[null,classes.mobile,classes.stationary,classes.fly];
    service.controllerPolicy=unit=>policies[this.command('controllerPolicy',[['option','Obj']],[unit],'u32')];
    service.mountedState=(c,state)=>this.command('mountedState',['u32','f64'],[policies.indexOf(c.policy),state],'f64');
    service.agentFor=(state,c)=>this.command('agentFor',['u32','f64'],[policies.indexOf(c.policy),state],'bool') ? ((controller,goal)=>new classes.flyAgent(controller,goal)):null;
    service.createHelper=agent=>{
      const kind=this.command('createHelper',[['option','Obj']],[agent.controller],'u32');
      if(kind===1) {
        const h=new classes.stationaryHelper(agent);h.goto=h.stop=()=>this.aux('stationary',['Obj'],[agent.controller],'void');return h;
      }
      return kind===2 ? new classes.flyHelper(agent.controller,service.world,{agent}):null;
    };
    service.freeSeats=(c,t,k=1)=>this.command('freeSeats',['Obj','f64','f64'],[c,t,k],'f64');
    service.requestExit=(c,p=null)=>this.command('requestExit',[['option','Obj'],['option','Obj']],[c,p],'bool');
    service.disembarkPassengers=(c,p=null)=>this.command('disembarkPassengers',[['option','Obj'],['option','Obj']],[c,p],'f64');
    service.boardPolicy=(a,c)=>this.command('boardPolicy',['Obj','Obj','Obj'],[a,a.controller,c],'bool');
    service.patrolMounted=p=>this.command('patrolMounted',['PatrolView'],[{obj:id(p),agent:id(p.agent),controller:id(p.agent.controller),post:optional(p.post),support:Boolean(p.agent.support),helper:Boolean(p.helper)}],'bool');
    service.supportPoint=(u,p,h,out=new Vector3())=>{
      const v=this.command('supportPoint',[['option','Obj'],['option','Obj'],['option','Obj']],[u,p,h],['option','Vector3']);return v ? out.copy(v):null;
    };
    service.dispatch=(c,u,t,allowed,world,out)=>{
      const answer=this.command('dispatch',['Obj','Obj','Obj','bool'],[c,u,t,allowed],'Dispatch');
      Object.assign(out,{pattern:answer.pattern,probability:answer.probability,aux:answer.aux,action:answer.action,inBand:answer.in_band,weapon:sim.objects[answer.weapon] ?? null});return out;
    };
    service.analogs=(u,r,aim,dt,out,options={})=>{
      const answer=this.command('analogs',['Obj',['option','Request'],['option','Vector3'],'f64','AnalogOptions'],
        [u,requestView(r),aim,dt,{controller:optional(options.controller ?? u.controller),stepped:Boolean(options.stepped)}],'Stick');
      Object.assign(out,{forward:answer.forward,right:answer.right,turn:answer.turn,pitch:answer.pitch});return out;
    };
    service.seatInput=(control,u,intent,dt,aim=null,options={})=>{
      this.command('seatInput',['Obj','f64',['option','Vector3']],[u,dt,aim],'Stick',false,{control,intent,options});return intent;
    };
    for(const [name,op] of [['enterPattern',0],['tickPattern',1],['exitPattern',2],['sidestep',3]]) {
      service[name]=(p,state=0)=>{
        const a=p.agent,c=p.controller,u=c.unit;
        const view={obj:id(p),agent:id(a),controller:id(c),control:id(u),tracker_point:a.tracker?.point ?? null,
          fresh_target:optional(a.tracker?.freshTarget),target:optional(a.tracker?.target),helper:Boolean(a.helper),helper_arrived:Boolean(a.helper?.arrived)};
        const values=encode('PatternsView',view,sim);encode('f64',state,sim,values);sim.words(values);sim.x.ai_vehicle_pattern(op);
        if(op===3)return Boolean(sim.out()[0]);
      };
    }
    for(const name of ['bodyOf','entryOf','leaseOf','controllerLease'])service[name]=o=>this.aux(name,[['option','Obj']],[o],['option','Obj']);
    for(const name of ['nominalSpeed','reachable','playerNear'])service[name]=o=>this.aux(name,['Obj'],[o],name==='nominalSpeed'?'f64':'bool');
    service.bodyReach=(body,point)=>this.aux('bodyReach',[['option','Obj'],'Vector3'],[body,point],'bool');
    service.walkerContacts=u=>this.aux('walkerContacts',['Obj'],[u],'void');
    service.walkerContact=(u,other)=>this.aux('walkerContact',['Obj','UnitView'],[u,unitView(other,id)],'void');
    service.boardTick=a=>this.aux('boardTick',['AgentCx',['option','Obj']],[{agent:id(a),controller:id(a.controller),unit:optional(a.unit)},a.vehicle?.unit],'void');
    const pad=service.padLeft;
    pad.get=o=>this.aux('padGet',['Obj'],[o],['option','f64']) ?? undefined;
    pad.has=o=>pad.get(o)!==undefined;
    pad.set=(o,time)=>{this.aux('padSet',['Obj','f64'],[o,time],'void');return pad;};
    Object.defineProperty(service.notifier,'counter',{configurable:true,enumerable:true,
      get:()=>this.aux('counterGet',[],[],['option','f64']),set:v=>this.aux('counterSet',[['option','f64']],[v],'void')});
    service.notifier.run=()=>service.notify();service.notifier.consider=(_,e)=>this.aux('consider',['Obj'],[e],'f64');
    const cache=service.supportCache;
    const refresh=()=>{
      const posts=this.aux('supportRead',[],[],['list','SupportCachePost']);
      for(const p of posts) {
        const post=sim.objects[p.post];let keys=Map.prototype.get.call(cache,post);
        if(!keys)Map.prototype.set.call(cache,post,keys=new Map());
        for(const row of p.keys)keys.set(row.key,row.hubs.map(h=>({hub:service.world.nav.graph.hubs[h.hub],distance:h.distance})));
      }
    };
    cache.get=post=>{refresh();return Map.prototype.get.call(cache,post);};
    cache.has=post=>{refresh();return Map.prototype.has.call(cache,post);};
    cache.entries=cache[Symbol.iterator]=()=>{refresh();return Map.prototype.entries.call(cache);};
    this.bindReservations(service.reservations);
    service.reserve=(candidate,owner,kind=1)=>service.reservations.reserve(candidate,owner,kind);
    service.reserveTarget=(candidate,owner)=>service.reservations.reserve(candidate,owner,2);
  }
  aux(name,types,args,result) {return this.command(name,types,args,result,true);}
  command(name,types,args,result,aux=false,frame=null) {
    const values=[];
    for(let i=0;i<types.length;i++) {
      const t=types[i],v=args[i];
      encode(t,t==='Obj' ? this.sim.id(v):Array.isArray(t) && t[0]==='option' && t[1]==='Obj' ? (v?this.sim.id(v):null):v,this.sim,values);
    }
    this.sim.words(values);
    this.frames.push(frame);
    try {
      if(aux)this.sim.x.ai_vehicle_aux(AiVehicleAuxOp_INDEX.get(name));
      else this.sim.x.ai_vehicle_service(AiVehicleServiceOp_INDEX.get(name));
    } finally {this.frames.pop();}
    if(result==='void')return;
    const d=decoder([...this.sim.out()],this.sim),value=d.read(result);d.done();
    if(result==='Obj')return this.sim.objects[value];
    if(Array.isArray(result) && result[1]==='Obj')return result[0]==='list' ? value.map(id=>this.sim.objects[id]):this.sim.objects[value] ?? null;
    return value;
  }
  bindAgent(agent,kind) {
    const sim=this.sim,id=o=>sim.id(o);
    const invoke=(op,dt=0,target=null)=>{
      const values=encode('AgentCx',{agent:id(agent),controller:id(agent.controller),unit:agent.unit ? id(agent.unit):null},sim);
      encode('f64',dt,sim,values);encode('Obj',id(target),sim,values);
      encode('BoardAgent',{vehicle:agent.vehicle ? {obj:id(agent.vehicle.unit),generation:agent.vehicle.generation}:null},sim,values);
      sim.words(values);sim.x.ai_vehicle_agent(kind,op);
      if([3,4,5,7].includes(op))return Boolean(sim.out()[0]);
    };
    agent.onEnter=()=>invoke(0);agent.behave=dt=>invoke(1,dt);
    if(kind===0) {
      agent.contested=()=>invoke(3);agent.acceptsTarget=t=>invoke(4,0,t);agent.acceptsAcoustic=t=>invoke(4,0,t);
    } else if(kind===2) {
      agent.onExit=()=>invoke(2);agent.acceptsTarget=t=>invoke(4,0,t);agent.clearAbove=()=>invoke(5);
      agent.pickPost=()=>invoke(6);agent.findHint=()=>invoke(7);agent.approach=()=>invoke(8);agent.landed=()=>invoke(9);
    }
  }
  bindHelper(helper) {
    const sim=this.sim;
    const invoke=(op,point={x:0,y:0,z:0},points=[],over=false,altitude=false)=>{
      const values=encode('Obj',sim.id(helper),sim);encode('Vector3',point,sim,values);
      encode(['list',['array','f64',3]],points,sim,values);encode('bool',over,sim,values);encode('bool',altitude,sim,values);
      sim.words(values);sim.x.ai_vehicle_helper(op);if(op===1)return Boolean(sim.out()[0]);
    };
    helper.goto=(p,options={})=>invoke(0,p,[],options.mode==='direct');
    helper.followPath=(p,over=false,alt=false)=>invoke(1,undefined,p ?? [],over,alt);
    helper.stop=()=>invoke(2);helper.land=(on=true)=>invoke(3,undefined,[],on);
    helper.setFlags=(over,alt)=>invoke(4,undefined,[],over,alt);
  }
  bindReservations(reservations) {
    const records=new Map();
    const read=()=>{
      const entries=this.command('reservationRead',[],[],['list','Reservation']);
      const present=new Set();
      const rows=entries.map(r=>{
        const key=`${r.candidate.obj}:${r.candidate.generation}:${r.owner.obj}:${r.owner.generation}:${r.kind}`;
        present.add(key);
        let row=records.get(key);
        if(!row){row={candidate:{unit:this.sim.objects[r.candidate.obj],generation:r.candidate.generation},owner:{unit:this.sim.objects[r.owner.obj],generation:r.owner.generation},time:r.time,type:r.kind};records.set(key,row);}
        row.time=r.time;return row;
      });
      for(const key of records.keys())if(!present.has(key))records.delete(key);
      return rows;
    };
    Object.defineProperty(reservations,'records',{configurable:true,enumerable:true,get:read});
    reservations.clear=()=>{this.command('reservationClear',[],[],'void');records.clear();};
    reservations.release=(c,u,k=1)=>this.command('reservationRelease',['Obj','Obj','f64'],[c,u,k],'void');
    reservations.has=(c,u,k=1)=>this.command('reservationHas',['Obj','Obj','f64'],[c,u,k],'bool');
    reservations.count=(c,t,k=1)=>this.command('reservationCount',['Obj','f64','f64'],[c,t,k],'f64');
    reservations.find=(c,u,k=1)=>reservations.has(c,u,k) ? read().find(r=>r.candidate.unit===c && r.owner.unit===u && r.type===k) ?? null:null;
    reservations.reserve=(c,u,k=1)=>{
      if(!c || !u)return null;
      this.command('reserve',['Obj','Obj','f64'],[c,u,k],'void');
      return read().find(r=>r.candidate.unit===c && r.owner.unit===u && r.type===k) ?? null;
    };
    reservations.live=()=>{this.aux('reservationLive',[],[],'void');return read();};
  }
}
