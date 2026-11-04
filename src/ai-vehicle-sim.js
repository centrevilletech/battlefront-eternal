// Vehicle algorithms join the battle's AI bridge. The service and its
// controller-facing objects keep their identity when native ownership binds.
import { Vector3 } from 'three';
import { AiVehicleServiceOp_INDEX } from './sim/gen/ai-vehicle.js';
import { vehicleCommands } from './ai/classic/vehicles/live-commands.js';
import { vehicleWorld } from './ai/classic/vehicles/live-world.js';
import { encode,decoder } from './ai/classic/vehicles/wire.js';
import { requestView } from './ai/classic/vehicles/live-views.js';

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
      return kind===1 ? new classes.stationaryHelper(agent):kind===2 ? new classes.flyHelper(agent.controller,service.world,{agent}):null;
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
      this.frames.push({control,intent,options});
      try {this.command('seatInput',['Obj','f64',['option','Vector3']],[u,dt,aim],'Stick');return intent;}
      finally {this.frames.pop();}
    };
    this.bindReservations(service.reservations);
    service.reserve=(candidate,owner,kind=1)=>service.reservations.reserve(candidate,owner,kind);
    service.reserveTarget=(candidate,owner)=>service.reservations.reserve(candidate,owner,2);
  }
  command(name,types,args,result) {
    const values=[];
    for(let i=0;i<types.length;i++) {
      const t=types[i],v=args[i];
      encode(t,t==='Obj' ? this.sim.id(v):Array.isArray(t) && t[0]==='option' && t[1]==='Obj' ? (v?this.sim.id(v):null):v,this.sim,values);
    }
    this.sim.words(values);
    this.sim.x.ai_vehicle_service(AiVehicleServiceOp_INDEX.get(name));
    if(result==='void')return;
    const d=decoder([...this.sim.out()],this.sim),value=d.read(result);d.done();
    if(result==='Obj')return this.sim.objects[value];
    if(Array.isArray(result) && result[1]==='Obj')return result[0]==='list' ? value.map(id=>this.sim.objects[id]):this.sim.objects[value] ?? null;
    return value;
  }
  bindReservations(reservations) {
    const records=new Map();
    const read=()=>{
      const entries=this.command('reservationRead',[],[],['list','Reservation']);
      return entries.map(r=>{
        const key=`${r.candidate.obj}:${r.candidate.generation}:${r.owner.obj}:${r.owner.generation}:${r.kind}`;
        let row=records.get(key);
        if(!row){row={candidate:{unit:this.sim.objects[r.candidate.obj],generation:r.candidate.generation},owner:{unit:this.sim.objects[r.owner.obj],generation:r.owner.generation},time:r.time,type:r.kind};records.set(key,row);}
        row.time=r.time;return row;
      });
    };
    Object.defineProperty(reservations,'records',{configurable:true,enumerable:true,get:read});
    reservations.clear=()=>this.command('reservationClear',[],[],'void');
    reservations.release=(c,u,k=1)=>this.command('reservationRelease',['Obj','Obj','f64'],[c,u,k],'void');
    reservations.has=(c,u,k=1)=>this.command('reservationHas',['Obj','Obj','f64'],[c,u,k],'bool');
    reservations.count=(c,t,k=1)=>this.command('reservationCount',['Obj','f64','f64'],[c,t,k],'f64');
    reservations.find=(c,u,k=1)=>reservations.has(c,u,k) ? read().find(r=>r.candidate.unit===c && r.owner.unit===u && r.type===k) ?? null:null;
    reservations.reserve=(c,u,k=1)=>{
      if(!c || !u)return null;
      this.command('reserve',['Obj','Obj','f64'],[c,u,k],'void');
      return read().find(r=>r.candidate.unit===c && r.owner.unit===u && r.type===k) ?? null;
    };
    reservations.live=()=>{reservations.count(null,0,0);return read();};
  }
}
