//! CombatPresentation's retained launch, terminal, flash and playout policy.
//! IDs qualify host resource handles; array order never pairs separate stores.
use crate::ordnance_layout::{combat_input as i, combat_output as o, combat_output_kind as k,
    combat_result as r, ordnance_limits as limits};
use std::collections::HashMap;
use three::Vector3;
use v8math::{max, min};

pub const LAUNCH_TICKS: f64 = 24.0;
const IMPACT_TICKS: f64 = 18.0;

// The physical flight clock may be fractional after server rewind. Wire tick
// and execution tick remain separate: all pellets still share one trigger.
#[derive(Clone, Copy, Default)]
struct Event {
    id: u32, generation: u32, schema: u32,
    tick: f64, expiry: f64, definition: u32,
    owner: u32, owner_generation: u32, fire: u32, channel: u32,
    launch: f64, execution: f64, position: Vector3, direction: Vector3,
    offset: Vector3, speed: f64, life: f64, merge: f64, surface: u32,
    fade: f64, velocity: f64, effect: u32,
}
// Only numeric flight facts live here. Muzzle nodes, trails and effect
// handles belong to the host and are never retained across this boundary.
#[derive(Clone, Copy, Default)]
struct Bolt {
    position: Vector3, direction: Vector3, offset: Vector3,
    speed: f64, life: f64, merge: f64, flight: f64, execution: f64,
    started: bool,
}
impl Bolt {
    fn launch(e: Event) -> Self {
        Self { position: e.position, direction: e.direction, offset: e.offset,
            speed: e.speed, life: e.life, merge: e.merge, flight: e.launch,
            execution: e.execution, started: false }
    }
    // network-entity-combat.js reach and stopBolt. Retain the operation order
    // of the dot product, and the minimum merge distance of fading streaks.
    fn reach(&mut self, point: Vector3, stop: bool) {
        let along = max(0.0, (point.x-self.position.x)*self.direction.x
            +(point.y-self.position.y)*self.direction.y+(point.z-self.position.z)*self.direction.z);
        if stop {
            self.life = min(self.life, along / max(self.speed, 1e-6));
            self.merge = max(min(self.merge, along), 1e-3);
        } else if self.speed > 0.0 { self.life = min(self.life, along / self.speed); }
    }
}
// A terminal can keep its launch bolt, while an explosion waits independently
// for delayed playout. Neither state is an authoritative damage operation.
#[derive(Clone, Copy, Default)]
struct State {
    event: Event, bolt: Option<Bolt>, predicted: bool,
    explosion: bool, impact: bool, impact_tick: Option<f64>,
}
// Confirmation identifies a weapon execution rather than a projectile ID.
// This also deduplicates each pellet without confusing later automatic fire.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Flash(u32,u32,u32,u64,u32,u32);

// Reservation bounds all stores, including a burst while no frame advances.
// A full bank refuses a new identity instead of throwing from the draw path.
pub struct Combat {
    pub input: Vec<f64>, pub result: Vec<f64>, pub output: Vec<f64>,
    states: Vec<State>, order: Vec<usize>, free: Vec<usize>,
    ids: HashMap<u32, usize>, flashes: HashMap<Flash, f64>, capacity: usize,
    epoch: Option<f64>, tick: f64, presented: f64, pruned: Option<f64>,
    incoming: Event, previous: Option<State>, pending: Option<usize>, revive: bool,
}
impl Combat {
    // Reserve the largest requested bank once. The zero-capacity instance
    // used for feedback needs only its scalar exchanges, not a projectile pool.
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.min(limits::RECORDS);
        Self { input: vec![0.0; i::STRIDE], result: vec![0.0;r::STRIDE],
            output: vec![0.0;(capacity+1)*o::STRIDE], states: vec![State::default();capacity],
            order: Vec::with_capacity(capacity), free: (0..capacity).rev().collect(),
            ids: HashMap::with_capacity(capacity), flashes: HashMap::with_capacity(capacity), capacity,
            epoch: None, tick: 0.0, presented: 0.0, pruned: None,
            incoming: Event::default(), previous: None, pending: None, revive: false }
    }
    // Output rows are a borrowed prefix. Clearing its count invalidates old
    // commands without touching or reallocating the reserved backing store.
    fn begin(&mut self) { self.result.fill(0.0); self.clocks(); }
    // Every publication names the wire ID. The host cannot accidentally bind
    // a reused storage slot to a previous life's resource object.
    fn row(&mut self, kind: u32, id: u32, bolt: Option<Bolt>) {
        let n = self.result[r::ROWS] as usize;
        if n >= self.output.len()/o::STRIDE { return; }
        let at = n*o::STRIDE;
        let a = &mut self.output[at..at+o::STRIDE];
        a.fill(0.0); a[o::KIND]=kind as f64; a[o::ID]=id as f64;
        if let Some(b)=bolt {
            a[o::BOLT]=1.0;
            a[o::POSITION..o::POSITION+3].copy_from_slice(&[b.position.x,b.position.y,b.position.z]);
            a[o::DIRECTION..o::DIRECTION+3].copy_from_slice(&[b.direction.x,b.direction.y,b.direction.z]);
            a[o::OFFSET..o::OFFSET+3].copy_from_slice(&[b.offset.x,b.offset.y,b.offset.z]);
            a[o::SPEED]=b.speed;a[o::LIFE]=b.life;a[o::MERGE]=b.merge;
            a[o::FLIGHT_TICK]=b.flight;a[o::EXECUTION_TICK]=b.execution;a[o::STARTED]=b.started as u8 as f64;a[o::COPY_OFFSET]=(!b.started)as u8 as f64;
        }
        self.result[r::ROWS]+=1.0;
    }
    fn clocks(&mut self) { self.result[r::TICK]=self.tick; self.result[r::PRESENTED_TICK]=self.presented; }
    // A new session discards its entire identity namespace, including flashes.
    // Refill the existing free list so reconnecting never grows the bank.
    pub fn reset(&mut self, epoch: f64) {
        self.epoch=Some(epoch);self.tick=0.0;self.presented=0.0;self.pruned=None;
        self.ids.clear();self.flashes.clear();self.order.clear();self.free.clear();
        self.free.extend((0..self.capacity).rev());self.pending=None;
    }
    // CombatPresentation.prune uses the displayed clock, not the latest
    // snapshot: early delivery must not erase a delayed explosion or impact.
    fn prune(&mut self, tick: f64) {
        if self.pruned==Some(tick) {return;}
        self.pruned=Some(tick);
        self.flashes.retain(|_,expiry|*expiry>=tick);
        // Compact survivors once. Removing each expired entry separately
        // would repeatedly shift the whole bank after a long render pause.
        let mut kept=0;
        for n in 0..self.order.len() {
            let slot=self.order[n];let s=self.states[slot];
            // An accepted delayed explosion must still happen after a long
            // pause, even when its retransmission window has already expired.
            if s.event.expiry<tick&&!s.explosion {
                self.ids.remove(&s.event.id);self.free.push(slot);
                self.row(k::REMOVE,s.event.id,None);
            } else {self.order[kept]=slot;kept+=1;}
        }
        self.order.truncate(kept);
    }
    // Input offsets come from the shared schema. No variable count or host
    // index is used to address a Rust-owned array.
    fn read_event(&self) -> Event {
        let a=&self.input;let v=|at|Vector3::new(a[at],a[at+1],a[at+2]);
        Event {id:a[i::ID] as u32,generation:a[i::GENERATION] as u32,schema:a[i::SCHEMA] as u32,
            tick:a[i::TICK],expiry:a[i::EXPIRY_TICK],definition:a[i::DEFINITION_ID] as u32,
            owner:a[i::OWNER_ID] as u32,owner_generation:a[i::OWNER_GENERATION] as u32,
            fire:a[i::FIRE_ID] as u32,channel:a[i::CHANNEL] as u32,launch:a[i::LAUNCH_TICK],execution:a[i::EXECUTION_TICK],
            position:v(i::POSITION),direction:v(i::DIRECTION),offset:v(i::OFFSET),
            speed:a[i::SPEED],life:a[i::LIFE],merge:a[i::MERGE],surface:a[i::SURFACE] as u32,
            fade:a[i::FADE_OUT],velocity:a[i::VELOCITY],effect:impact_effect(a[i::SURFACE] as u32,a[i::IMPACT_MASK] as u32)}
    }
    // Receive is split at the foreign prediction claim service. Admission
    // runs first so a duplicate cannot consume a speculative shot's identity.
    pub fn receive(&mut self, epoch: f64, tick: f64) -> bool {
        self.begin();self.pending=None;
        if !epoch.is_finite()||!tick.is_finite()||self.epoch.is_some_and(|e|epoch<e) {return false;}
        if self.epoch!=Some(epoch) {self.reset(epoch);self.row(k::RESET,0,None);}
        self.tick=max(self.tick,tick);self.prune(self.presented);self.clocks();
        let e=self.read_event();
        if !e.expiry.is_finite()||e.expiry<self.tick {return false;}
        let old=self.ids.get(&e.id).map(|slot|self.states[*slot]);
        // Lost launches may arrive after their terminals. They can still fly
        // to that saved impact while their original execution window is live.
        self.revive=old.is_some_and(|s|e.schema==1&&s.event.schema==2&&s.event.generation==e.generation
            &&s.bolt.is_none()&&!s.predicted&&self.tick-e.execution<=LAUNCH_TICKS+e.life*60.0);
        // Terminal records outrank launches at the same generation, including
        // repeated samples delivered out of order by independent snapshots.
        if !self.revive&&old.is_some_and(|s|s.event.generation>e.generation||s.event.generation==e.generation
            &&(matches!(s.event.schema,2|7)||s.event.tick>e.tick||s.event.tick==e.tick
                &&!(matches!(e.schema,2|7)&&e.schema!=s.event.schema))) {return false;}
        // Replacement keeps its insertion position, just as Map.set did. A
        // fresh ID uses a free slot but appends to the ordered presentation.
        let slot=if let Some(slot)=self.ids.get(&e.id) {*slot} else {
            let Some(slot)=self.free.pop() else {return false;};
            self.ids.insert(e.id,slot);self.order.push(slot);slot
        };
        self.incoming=e;self.previous=old;self.pending=Some(slot);
        self.result[r::ACCEPTED]=1.0;self.result[r::REVIVE]=self.revive as u8 as f64;
        // Non-projectile events never claim a weapon flash. Dynamic samples
        // use their launch tick as execution identity, as the old wire did.
        let claim=matches!(e.schema,1|2|6);
        self.result[r::CLAIM]=claim as u8 as f64;
        self.result[r::LANDED]=(e.schema==2&&e.fade>0.0&&old.is_none()) as u8 as f64;
        if claim {
            // Flash retention follows the displayed clock, while eligibility
            // follows the newest snapshot. Early snapshots cannot prune it.
            let key=Flash(e.owner,e.owner_generation,e.fire,e.execution.to_bits(),e.channel,e.definition);
            if self.tick-e.execution<=LAUNCH_TICKS&&!self.flashes.contains_key(&key) {
                // An exhausted flash bank suppresses extras until pruning;
                // keeping existing keys avoids replaying accepted sounds.
                if self.flashes.len()<self.capacity {
                    self.flashes.insert(key,e.execution+LAUNCH_TICKS);self.result[r::LAUNCH]=1.0;
                }
            }
        }
        true
    }
    // CombatPresentation.receive after the host answers prediction/muzzle
    // requests. Everything below changes retained numeric presentation state.
    pub fn resolve(&mut self, predicted: bool) -> bool {
        self.result[r::ROWS]=0.0;
        let Some(slot)=self.pending.take() else {return false;};
        let e=self.incoming;let old=self.previous;self.result[r::IMPACT_EFFECT]=e.effect as f64;
        use crate::ordnance_layout::combat_dispatch as dispatch;
        self.result[r::DISPATCH]=match e.schema{1=>dispatch::BOLT,2=>dispatch::TERMINAL,3|4=>dispatch::FEEDBACK,5=>dispatch::EXPLOSION,6|7=>dispatch::DYNAMIC,_=>dispatch::PRESENTATION}as f64;
        self.result[r::POINT]=matches!(e.schema,2|5)as u8 as f64;
        self.result[r::REUSE_BOLT]=(e.schema==2)as u8 as f64;
        self.result[r::FOLLOW_MUZZLE]=(e.schema==1)as u8 as f64;
        self.result[r::SPLASH]=(e.surface==4)as u8 as f64;self.result[r::CHARACTER]=(e.surface==0)as u8 as f64;

        // A predicted launch has already supplied the visible flight. Keep
        // the saved terminal but never create another bolt for that claim.
        if self.revive {
            if predicted {self.result[r::ACCEPTED]=0.0;return false;}
            let mut s=old.unwrap();let mut b=Bolt::launch(e);b.reach(s.event.position,false);
            s.bolt=Some(b);if s.impact {s.impact_tick=None;}
            self.states[slot]=s;self.row(k::STATE,e.id,s.bolt);return true;
        }
        let mut s=State{event:e,..State::default()};
        if e.schema==5 {s.explosion=true;}
        if matches!(e.schema,1|2) {
            // Fading sniper streaks stop at the hit instead of disappearing.
            // Missing launches may reconstruct a streak from the drawn gun.
            if e.schema==2&&e.fade>0.0 {
                if predicted {self.result[r::STOP_PREDICTED]=1.0;}
                else if let Some(mut b)=old.and_then(|s|s.bolt) {b.reach(e.position,true);s.bolt=Some(b);}
                else if old.is_none()&&self.input[i::HAS_MUZZLE]!=0.0&&e.velocity>0.0 {
                    let start=Vector3::new(self.input[i::MUZZLE],self.input[i::MUZZLE+1],self.input[i::MUZZLE+2]);
                    let mut direction=e.position;direction.sub(&start);let distance=direction.length();
                    if distance>1e-3 {
                        direction.divide_scalar(distance);
                        s.bolt=Some(Bolt{position:start,direction,offset:Vector3::default(),speed:e.velocity,
                            life:distance/e.velocity,merge:min(8.0,distance),flight:e.launch,execution:0.0,started:false});
                    }
                }
            }
            if e.schema==1 {
                s.predicted=predicted;if !predicted {s.bolt=Some(Bolt::launch(e));}
            } else {
                s.predicted=predicted;
                if let Some(previous)=old.filter(|s|s.event.generation==e.generation) {
                    if let Some(mut b)=previous.bolt {
                        // stopBolt changed the old mirror before reach in JS.
                        if e.fade>0.0&&!predicted {b.reach(e.position,true);}
                        b.reach(e.position,false);s.bolt=Some(b);
                    }
                }
                // Surface five is removal without an impact. Immediate own
                // prediction has a short effect window; remote flight waits.
                if e.surface!=5 {
                    if predicted&&s.bolt.is_none()&&self.input[i::DYNAMIC]==0.0 {
                        if self.tick-e.tick<=IMPACT_TICKS {self.result[r::IMPACT]=1.0;}
                    } else {s.impact=true;s.impact_tick=if s.bolt.is_some(){None}else{Some(e.tick)};}
                }
            }
        }
        self.states[slot]=s;self.row(k::STATE,e.id,s.bolt);
        self.result[r::BOLT]=s.bolt.is_some() as u8 as f64;
        self.result[r::PENDING_IMPACT]=s.impact as u8 as f64;self.result[r::IMPACT_AT]=s.impact_tick.unwrap_or(f64::NAN);
        true
    }
    // CombatPresentation.present advances every retained bolt in one call.
    // The returned rows also order retries and sniper-impact queue requests.
    pub fn present(&mut self, tick: f64) {
        self.begin();if !tick.is_finite(){return;}
        self.tick=max(self.tick,tick);self.presented=max(self.presented,tick);self.prune(self.presented);
        for n in 0..self.order.len() {
            let slot=self.order[n];let s=&mut self.states[slot];
            let resolve=s.bolt.is_some_and(|b|!b.started);
            if let Some(b)=&mut s.bolt {
                if !b.started {
                    b.started=true;
                    // A late delivery starts at the gun on its first visible
                    // frame, bounded by the weapon's execution grace window.
                    if tick>b.flight {b.flight=min(tick,max(b.flight,b.execution)+LAUNCH_TICKS);}
                }
            }
            let s=*s;self.row(k::DRAW,s.event.id,s.bolt);let at=(self.result[r::ROWS]as usize-1)*o::STRIDE;self.output[at+o::RESOLVE_MUZZLE]=resolve as u8 as f64;
        }
        self.clocks();
    }
    // CombatPresentation.settle runs after all draw records, so a sniper's
    // deferred impact finds the trace its first draw created this frame.
    pub fn settle(&mut self, tick: f64) {
        self.begin();if !tick.is_finite(){return;}
        for n in 0..self.order.len() {
            let slot=self.order[n];let mut s=self.states[slot];
            if s.explosion&&tick>=s.event.tick {s.explosion=false;self.row(k::EXPLODE,s.event.id,None);}
            if s.impact {
                // Terminals paired with a bolt use its resolved flight clock.
                // An unstarted bolt cannot settle before its first draw.
                let at=s.impact_tick.or_else(||s.bolt.filter(|b|b.started).map(|b|b.flight+b.life*60.0));
                if let Some(at)=at.filter(|at|tick>=*at) {
                    s.impact=false;if tick-at<=IMPACT_TICKS {self.row(k::IMPACT,s.event.id,None);}
                }
            }
            self.states[slot]=s;
        }
        self.clocks();
    }
}


// CombatPresentation.impact selects authored names, not resource objects.
// Water's nullish default deliberately differs from the other truthy fallbacks.
pub fn impact_effect(surface: u32, mask: u32) -> u32 {
    let has=|i:u32| -> bool { mask & (1u32 << i) != 0u32 };
    match surface {
        4=>if has(4){4}else{5},
        0=>0,
        2=>if has(2){2}else{1},
        3=>if has(3){3}else{1},
        _=>if has(1){1}else{0},
    }
}

// presentFeedback's event phase: health is never touched. The host supplies
// generation-qualified resource facts and the mission relationship result.
pub fn feedback(a: &[f64], out: &mut [f64]) {
    use crate::ordnance_layout::{feedback_input as i, feedback_output as o};
    out.fill(0.0);
    if a.len()<i::STRIDE||out.len()<o::STRIDE{return;}
    let target=a[i::TARGET_PRESENT]!=0.0&&a[i::TARGET_GENERATION]==a[i::TARGET_ACTUAL_GENERATION];
    let attacker=a[i::ATTACKER_PRESENT]!=0.0&&a[i::ATTACKER_GENERATION]==a[i::ATTACKER_ACTUAL_GENERATION];
    let target_own=target&&a[i::TARGET_OWN]!=0.0;
    out[o::NOTE_ATTACKER]=target_own as u8 as f64;
    out[o::ATTACKER_KIND]=if attacker {2.0}else if a[i::ATTACKER_ID]!=0.0{1.0}else{0.0};
    if a[i::SCHEMA]==3.0 {
        out[o::HIT]=(attacker&&a[i::ATTACKER_OWN]!=0.0) as u8 as f64;
        out[o::DAMAGE]=(target_own&&a[i::HAS_SOURCE]!=0.0) as u8 as f64;
        let x=a[i::SOURCE]-a[i::POSITION];let z=a[i::SOURCE+2]-a[i::POSITION+2];
        let yaw=a[i::YAW];
        out[o::ANGLE]=v8math::atan2(-x*v8math::cos(yaw)+z*v8math::sin(yaw),
            x*v8math::sin(yaw)+z*v8math::cos(yaw));
    } else {out[o::DEATH]=target as u8 as f64;}
    // targeting-facts.js radarRelation: locals retain their authored colour
    // except when the viewer belongs to that same team.
    out[o::RELATION]=if a[i::ATTACKER_TEAM]==0.0{0.0}
        else if a[i::ATTACKER_TEAM]!=a[i::OWN_TEAM]&&a[i::LOCALS]!=0.0{3.0}
        else if a[i::RELATION]>0.0{1.0}else if a[i::RELATION]<0.0{2.0}else{0.0};
}


// CombatCapture.launch retains a bolt through its entire fractional flight
// and the retransmission window, rounding only at the final tick boundary.
pub fn bolt_expiry(launch: f64, life: f64) -> f64 { (launch+life*60.0).ceil()+120.0 }

// Admission of producer facts. Browser references qualify a projectile, while
// Rust owns defaults, launch clocks and the retained retransmission window.
pub fn capture_bolt(text:&str)->Result<String,String>{
    use serde_json::{json,Value};
    let f:Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
    let n=|k:&str|f[k].as_f64().unwrap_or(0.0);
    let launch=f["sampleTick"].as_f64().unwrap_or(n("tick"));
    let owner=if f["sample"].as_bool()==Some(true){&f["sampleOwner"]}else{&f["boltOwner"]};
    Ok(json!({"launchTick":launch,"executionTick":n("tick"),"id":n("id"),"generation":n("generation"),"expiryTick":bolt_expiry(launch,n("life")),"definitionId":n("definitionId"),"ownerId":owner[0].as_f64().unwrap_or(0.0),"ownerGeneration":owner[1].as_f64().unwrap_or(0.0),"fireId":owner[2].as_f64().unwrap_or(0.0),"channel":owner[3].as_f64().unwrap_or(0.0)}).to_string())
}
pub fn capture_terminal(text:&str,tick:f64)->Result<String,String>{
    use serde_json::{json,Value};let mut state:Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
    if state["executionTick"].is_null(){state["executionTick"]=state["launchTick"].clone();}
    if state["expiryTick"].is_null(){state["expiryTick"]=json!(tick+18.0);}
    Ok(state.to_string())
}

impl Combat {
    // Raw envelope numbers and authored resource facts are cold admission
    // inputs. Missing fields, effect precedence and dispatch stay native.
    pub fn receive_facts(&mut self,epoch:f64,tick:f64,definition:&str,has_definition:bool)->Result<bool,String>{
        let d:serde_json::Value=serde_json::from_str(definition).map_err(|e|e.to_string())?;
        let schema=self.input[i::SCHEMA]as u32;
        if matches!(schema,1|2|5)&&!has_definition{return Err("unknown combat definition".into());}
        if self.input[i::SURFACE].is_nan(){self.input[i::SURFACE]=5.0;}
        if self.input[i::EXECUTION_TICK].is_nan(){self.input[i::EXECUTION_TICK]=self.input[i::LAUNCH_TICK];}
        for value in &mut self.input{if value.is_nan(){*value=0.0;}}
        let mut mask=0u32;for(j,key)in IMPACT_FIELDS.iter().enumerate(){if if j==4{!d[*key].is_null()}else{d[*key].as_str().is_some_and(|v|!v.is_empty())}{mask|=1<<j;}}
        self.input[i::IMPACT_MASK]=mask as f64;
        Ok(self.receive(epoch,tick))
    }
}
const IMPACT_FIELDS:[&str;5]=["ImpactEffectTerrain","ImpactEffectStatic","ImpactEffectSoft","ImpactEffectRigid","ImpactEffectWater"];
pub fn impact_name(effect:usize,text:&str)->Result<String,String>{
    if effect==5{return Ok("watersplash_ord".into());}
    let d:serde_json::Value=serde_json::from_str(text).map_err(|e|e.to_string())?;
    Ok(IMPACT_FIELDS.get(effect).and_then(|k|d[*k].as_str()).unwrap_or("").to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn launch(c:&mut Combat,id:f64,tick:f64) {
        c.input.fill(0.0);c.input[i::ID]=id;c.input[i::GENERATION]=1.0;c.input[i::SCHEMA]=1.0;
        c.input[i::TICK]=tick;c.input[i::EXPIRY_TICK]=tick+120.0;c.input[i::EXECUTION_TICK]=tick;
        c.input[i::LAUNCH_TICK]=tick;c.input[i::SPEED]=60.0;c.input[i::LIFE]=1.0;
        c.input[i::MERGE]=8.0;c.input[i::DIRECTION+2]=1.0;
    }
    #[test]
    fn paused_burst_and_full_bank_never_panic_or_replace_identity() {
        let mut c=Combat::new(4);
        for id in 1..=10000 {
            launch(&mut c,id as f64,10.0);
            let accepted=c.receive(1.0,10.0);
            assert_eq!(accepted,id<=4);if accepted {assert!(c.resolve(false));}
        }
        assert_eq!(c.ids.len(),4);assert_eq!(c.flashes.len(),1);
        c.present(200.0);assert!(c.ids.is_empty());
        launch(&mut c,10001.0,200.0);assert!(c.receive(1.0,200.0));assert!(c.resolve(false));
        assert!(c.ids.contains_key(&10001));assert!(!c.ids.contains_key(&1));
    }
    #[test]
    fn terminal_waits_for_playout_and_repeated_launch_is_rejected() {
        let mut c=Combat::new(8);launch(&mut c,7.0,10.0);
        assert!(c.receive(1.0,10.0));c.resolve(false);c.present(10.0);
        c.input[i::SCHEMA]=2.0;c.input[i::TICK]=12.0;c.input[i::POSITION+2]=3.0;c.input[i::SURFACE]=2.0;
        assert!(c.receive(1.0,12.0));c.resolve(false);
        c.settle(12.0);assert_eq!(c.result[r::ROWS],0.0);
        c.settle(13.0);assert_eq!(c.output[o::KIND],k::IMPACT as f64);
        c.settle(14.0);assert_eq!(c.result[r::ROWS],0.0);
        launch(&mut c,7.0,10.0);assert!(!c.receive(1.0,14.0));
    }
}
