import { Vector3 } from 'three';
import { FlyGotoHelper as NavFlyGotoHelper } from '../nav/goto.js';
import { encode,decoder } from './wire.js';
export const hintView=(h,id)=>h ? {obj:id(h),position:h.position,forward:h.forward,up:h.up ?? null,command_post:h.commandPost ? id(h.commandPost):null}:null;
const fields=['toggle','entry','destination','best','progress_at','fallback','loop_','inverted','flip_a','flip_b'];
const key=k=>k==='loop_'?'loop':k.replace(/_([a-z])/g,(_,c)=>c.toUpperCase());
export function agentWorld(service,sim,op,values) {
  const d=decoder(values,sim),read=d.read,id=o=>sim.id(o),objects=sim.objects,obj=()=>objects[read('Obj')];
  const vector=()=>{const v=read('Vector3');return new Vector3(v.x,v.y,v.z);};
  const reply=(type,value)=>{d.done();return {values:type ? encode(type,value,sim):[]};};
  const none=()=>reply(null),ai=service.ai;
  switch(op) {
    case 'agentState': return reply('f64',obj().state);
    case 'agentRequestState': {const a=obj(),s=read('f64');a.requestState(s);return none();}
    case 'flyFields': {const a=obj();return reply('FlyAgent',{hint:hintView(a.hint,id),search_at:a.searchAt,flag:a.flag,deadline:a.deadline,post:a.post ? id(a.post):null});}
    case 'writeFlyFields': {const a=obj(),f=read('FlyAgent');a.hint=f.hint ? objects[f.hint.obj]:null;a.searchAt=f.search_at;a.flag=f.flag;a.deadline=f.deadline;a.post=f.post ? objects[f.post]:null;return none();}
    case 'patternFields': {const p=obj(),v=p.vehicle;return reply(['option','PatternState'],v ? Object.fromEntries(fields.map(k=>[k,v[key(k)]])):null);}
    case 'writePatternFields': {const p=obj(),v=read('PatternState');const out=p.vehicle ??= {};
      for(const k of fields) {const name=key(k);if(k==='entry'||k==='destination')(out[name] ??= new Vector3()).copy(v[k]);else out[name]=v[k];}return none();}
    case 'interruptsTick': return reply('bool',obj().interrupts.tick());
    case 'updateCombatTarget': obj().updateCombatTarget();return none();
    case 'aim': {const a=obj(),stance=read('f64');a.aim(stance);return none();}
    case 'tickHelper': {const a=obj(),dt=read('f64');a.tickHelper(dt);return none();}
    case 'canEngage': {const a=obj(),t=obj();return reply('bool',a.canEngage(t));}
    case 'hasHelper': return reply('bool',Boolean(obj().helper));
    case 'helperArrived': return reply('bool',Boolean(obj().helper?.arrived));
    case 'helperFailed': return reply('bool',Boolean(obj().helper?.failed));
    case 'posts': return reply(['list','Obj'],(service.world.posts?.posts ?? []).map(id));
    case 'queryHints': {const center=vector(),requester=obj(),now=read('f64'),query=ai.nav?.queryHints;
      return reply(['option',['list','HintView']],query ? (query({mask:32,center,halfExtent:80,cap:1,requester,now}) ?? []).map(h=>hintView(h,id)):null);}
    case 'flyUnit': {const u=obj().unit;return reply(['option','Obj'],u ? id(u):null);}
    case 'flySuperGoto': {const h=obj(),p=vector(),direct=read('bool');NavFlyGotoHelper.prototype.goto.call(h,p,direct?{mode:'direct'}:{});return none();}
    case 'flySuperStop': NavFlyGotoHelper.prototype.stop.call(obj());return none();
    case 'flyHasPlanner': return reply('bool',Boolean(obj().nav?.planner));
    case 'flyPlannerCancel': {const h=obj();h.nav.planner.cancel(h.control);return none();}
    case 'flyPathNew': return reply('u64',id(obj().nav.planner.path()));
    case 'flyPathPrepend': {const p=obj(),x=read('f64'),y=read('f64'),z=read('f64');return reply('bool',p.prepend(x,y,z));}
    case 'flyPathRelease': obj().release();return none();
    case 'flyInstall': {const h=obj(),p=obj(),over=read('bool'),alt=read('bool');h.control.install(p,over,alt);return none();}
    case 'flySetDestination': {const h=obj(),p=vector();h.destination.copy(p);return none();}
    case 'flySetFollowed': {const h=obj(),failed=read('bool'),state=read('f64'),pending=read('f64');h.failed=failed;h.state=state;h.pending=pending;return none();}
    default: throw Error(`unknown vehicle world call ${op}`);
  }
}
