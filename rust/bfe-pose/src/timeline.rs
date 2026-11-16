//! snapshot-timeline.js: one retained, bounded snapshot ring and one sample
//! batch for every remote body and hull. Histories pair by entity id, never
//! by a packet's or a renderer's row number. The frame allocates nothing.
use crate::timeline_layout::{timeline_frame as f, timeline_record as r, timeline_output as o, timeline_limits as lim, VERSION};
use three::Quaternion;

// A soldier's step from one record to the next that his speed does not
// explain is drawn as a slide. The world does this on purpose, as the 2004
// game does: a soldier who spawns on the spot where another still stands is
// pushed half a body aside in one tick with him, a vehicle shoves a soldier
// out of its way, a soldier is lifted onto a ledge. Drawn as it is that is a
// pop. The part of the step beyond the unit's speed is held back as an offset
// beside the sampled position (the way smoothing.rs keeps a correction apart
// from the collided position) and eased out over SLIDE_TICKS from the step's
// start. Teleports and seat changes (marked by the server), new lives, the
// dead and hulls are never slid, nor is any step of SLIDE_UNDER or more.
const SLIDE_TICKS:f64=8.0;
const SLIDE_OVER:f64=0.2;
const SLIDE_UNDER:f64=1.5;
// A unit that is carried along, step after step beyond its speed, is drawn
// where it is sampled: only a step that follows explained ones slides.
// `gap` is the newest snapshot between the two poses that came without the
// unit: a unit at rest stayed where it was until then. Returns the step's
// start and the part of it the unit's speed does not explain, if any.
fn unexplained(older:&[f64],from:f64,gap:Option<f64>,newer:&[f64],to:f64,hz:f64)->Option<(f64,[f64;3],f64)> {
    if newer[r::KIND]==1.0 || newer[r::DISC]!=0.0 {return None;}
    let speed=|p:&[f64]|p[r::VELOCITY]*p[r::VELOCITY]+p[r::VELOCITY+1]*p[r::VELOCITY+1]+p[r::VELOCITY+2]*p[r::VELOCITY+2];
    let (was,is)=(speed(older),speed(newer));
    let start=if was<1e-4 {gap.unwrap_or(from)} else {from};
    if to<=start {return None;}
    let seconds=(to-start)/hz;
    let step:[f64;3]=std::array::from_fn(|k|newer[r::POSITION+k]-older[r::POSITION+k]);
    let length=(step[0]*step[0]+step[1]*step[1]+step[2]*step[2]).sqrt();
    if length-was.max(is).sqrt()*seconds<=SLIDE_OVER {return None;}
    let living=|p:&[f64]|p[r::HEALTH]>0.0 && p[r::DEAD]==0.0 && (p[r::KIND]!=0.0 || p[r::STATE]!=7.0 && p[r::STATE]!=8.0);
    if !living(older) || !living(newer) {return None;}
    Some((start,std::array::from_fn(|k|step[k]-(older[r::VELOCITY+k]+newer[r::VELOCITY+k])*0.5*seconds),length))
}
// What of such a step is still held back at `tick`. The sample has drawn
// `done` of the whole step; of the part held back only `ease` is shown yet
// (no speed at either end of the slide).
fn held(step:Option<(f64,[f64;3],f64)>,to:f64,tick:f64)->Option<[f64;3]> {
    let (start,excess,length)=step?;
    if length>=SLIDE_UNDER || tick<=start || tick-start>=SLIDE_TICKS {return Some([0.0;3]);}
    let x=(tick-start)/SLIDE_TICKS;let ease=x*x*(3.0-2.0*x);let done=((tick-start)/(to-start)).min(1.0);
    Some(excess.map(|part|part*(ease-done)))
}

// Stable entity slots make a sparse history lookup constant time. A packet
// can arrive in any order without making sampling quadratic in population.
struct Snapshot { tick: f64, records: Vec<f64> }
struct Entity { id: f64, generation: f64, fire_tick: f64, raw: f64, total: f64,
    // The newest pose as it arrived (its life, tick, place and squared speed)
    // and the tick of the newest step from one pose to the next that may be
    // beyond the unit's speed: only then are older poses read for a slide.
    life: f64, seen: f64, at: [f64; 3], speed: f64, odd: f64 }
pub struct Timeline {
    pub input: Vec<f64>,
    pub output: Vec<f64>,
    snapshots: Vec<Snapshot>,
    order: Vec<usize>,
    entities: Vec<Entity>,
    buckets: Vec<usize>,
    free: Vec<usize>,
    epoch: f64,
    maximum: usize,
    pub refused: u64,
    presented: Option<f64>,
    // A body at rest sends nothing. Its last pose, once its snapshot has
    // left the ring, stays here so the body keeps its place on the playout
    // clock instead of showing the newest record ahead of it.
    rest: Vec<f64>,
    rest_tick: Vec<f64>,
}
impl Default for Timeline { fn default() -> Self { Self::new() } }
impl Timeline {
    // The 512 wire slots bound simultaneous display identities. Retain all
    // 64 historical ticks without reserving a second population of slots.
    // Reserve the largest supported ring at birth. A stalled page or burst
    // of partial packets never grows any native collection.
    pub fn new() -> Self {
        Self { input: vec![0.0;f::STRIDE+lim::ENTITIES*r::STRIDE], output:vec![0.0;lim::ENTITIES*o::STRIDE],
            snapshots:(0..lim::SNAPSHOTS).map(|_|Snapshot { tick:0.0,records:vec![0.0;lim::ENTITIES*r::STRIDE] }).collect(),
            order:Vec::with_capacity(lim::SNAPSHOTS),entities:Vec::with_capacity(lim::ENTITIES),buckets:vec![usize::MAX;lim::ENTITIES*2],free:Vec::with_capacity(lim::ENTITIES),epoch:0.0,maximum:lim::SNAPSHOTS,refused:0,presented:None,
            rest:vec![0.0;lim::ENTITIES*r::STRIDE],rest_tick:vec![0.0;lim::ENTITIES] }
    }
    // Epoch changes discard history and fire edges together, while retaining
    // the allocation for the next battle state.
    pub fn reset(&mut self, epoch:f64) { self.epoch=epoch;self.order.clear();self.entities.clear();self.buckets.fill(usize::MAX);self.free.clear();self.output.fill(0.0);self.presented=None;self.rest.fill(0.0); }
    // Removal clears every bracket for this identity. Its reserved slot can
    // safely serve a later generation without exposing an older pose.
    pub fn remove(&mut self,id:f64) {
        let bucket=self.bucket(id);let index=self.buckets[bucket];
        if index==usize::MAX {return;}
        for snapshot in &mut self.snapshots {snapshot.records[index*r::STRIDE+r::VALID]=0.0;}
        self.rest[index*r::STRIDE+r::VALID]=0.0;
        self.entities[index].id = -1.0;self.entities[index].fire_tick=f64::NEG_INFINITY;self.entities[index].generation = -1.0;
        self.free.push(index);self.buckets[bucket]=usize::MAX;
        // Close this probe cluster instead of retaining tombstones. Long
        // battles can reuse slots indefinitely without degrading lookups.
        let mask=self.buckets.len()-1;let mut next=(bucket+1)&mask;
        while self.buckets[next]!=usize::MAX {
            let slot=self.buckets[next];self.buckets[next]=usize::MAX;
            let to=self.bucket(self.entities[slot].id);self.buckets[to]=slot;
            next=(next+1)&mask;
        }
    }
    // Stable slots and a half-full hash table resolve sparse wire identities.
    // Canonical zero matches Map's SameValueZero treatment of signed zero.
    fn bucket(&self,id:f64)->usize {
        let mut hash=if id==0.0 {0}else{id.to_bits()};
        hash=(hash^(hash>>30)).wrapping_mul(0xbf58476d1ce4e5b9);
        hash=(hash^(hash>>27)).wrapping_mul(0x94d049bb133111eb);
        let mask=self.buckets.len()-1;let mut at=((hash^(hash>>31))as usize)&mask;
        while self.buckets[at]!=usize::MAX && self.entities[self.buckets[at]].id!=id {at=(at+1)&mask;}
        at
    }
    // A snapshot leaving the ring hands each soldier's and droideka's pose
    // to `rest`. Hulls keep posing their current record once parked.
    fn retire(&mut self,slot:usize) {
        let snapshot=&self.snapshots[slot];
        for (entity,e) in self.entities.iter().enumerate() {
            let pose=&snapshot.records[entity*r::STRIDE..][..r::STRIDE];
            if pose[r::VALID]==0.0 || pose[r::ID]!=e.id || pose[r::KIND]==1.0 {continue;}
            let kept=&mut self.rest[entity*r::STRIDE..][..r::STRIDE];
            if kept[r::VALID]!=0.0 && kept[r::ID]==e.id && self.rest_tick[entity]>snapshot.tick {continue;}
            kept.copy_from_slice(pose);self.rest_tick[entity]=snapshot.tick;
        }
    }
    /// addSnapshot: later parts merge into a tick. The oldest whole snapshot
    /// is evicted only when a new tick needs its slot. Counts clamp before
    /// any slice, and a full entity store refuses additional identities.
    pub fn push(&mut self) {
        if self.input[f::VERSION]!=VERSION as f64 {return;}
        let epoch=self.input[f::EPOCH];if !epoch.is_finite() {return;}
        if epoch<self.epoch {return;}
        if epoch>self.epoch {self.reset(epoch);}
        self.maximum=(self.input[f::SNAPSHOTS] as usize).clamp(1,lim::SNAPSHOTS);
        let tick=self.input[f::TICK];if !tick.is_finite() {return;}
        while self.order.len()>self.maximum {let old=self.order.remove(0);self.retire(old);}
        if self.order.len()>=self.maximum && self.order.first().is_some_and(|&s|tick<self.snapshots[s].tick) {return;}
        // A second part of a tick reuses that whole snapshot. Otherwise its
        // slot is inserted in tick order and the oldest allocation is reused.
        let slot=if let Some(&slot)=self.order.iter().find(|&&s|self.snapshots[s].tick==tick) {slot} else {
            let slot=if self.order.len()>=self.maximum {let old=self.order.remove(0);self.retire(old);old} else { (0..lim::SNAPSHOTS).find(|s|!self.order.contains(s)).unwrap_or(0) };
            self.snapshots[slot].tick=tick;self.snapshots[slot].records.fill(0.0);
            let at=self.order.iter().position(|&s|self.snapshots[s].tick>tick).unwrap_or(self.order.len());
            self.order.insert(at,slot);slot
        };
        let count=(self.input[f::COUNT] as usize).min(lim::ENTITIES);
        let hz=if self.input[f::HZ]>0.0 {self.input[f::HZ]} else {60.0};
        for row in 0..count {
            let at=f::STRIDE+row*r::STRIDE;
            self.input[at+r::SLOT] = -1.0;
            let id=self.input[at+r::ID];
            if !id.is_finite() || id<0.0 || id.fract()!=0.0 {continue;}
            let kind=self.input[at+r::KIND];if !(0.0..=2.0).contains(&kind) || kind.fract()!=0.0 {continue;}
            // Ids are not array indices: maps can have sparse ids, and
            // packet order changes when entities enter or leave interest.
            let bucket=self.bucket(id);
            let entity=if self.buckets[bucket]!=usize::MAX {self.buckets[bucket]} else {
                let fresh=Entity{id,generation:-1.0,fire_tick:f64::NEG_INFINITY,raw:0.0,total:0.0,life:-1.0,seen:f64::NEG_INFINITY,at:[0.0;3],speed:0.0,odd:f64::NEG_INFINITY};
                let index=if let Some(index)=self.free.pop(){self.entities[index]=fresh;self.rest[index*r::STRIDE+r::VALID]=0.0;index}else{
                    if self.entities.len()==lim::ENTITIES {self.refused+=1;continue;}
                    self.entities.push(fresh);self.entities.len()-1
                };
                self.buckets[bucket]=index;index
            };
            let e=&mut self.entities[entity];let generation=self.input[at+r::GENERATION];
            let snapshot=&mut self.snapshots[slot];
            // Overwrite only this record. Other parts already installed at
            // the same tick remain available to the display bracket search.
            let target=entity;
            let out=&mut snapshot.records[target*r::STRIDE..][..r::STRIDE];
            out.copy_from_slice(&self.input[at..at+r::STRIDE]);out[r::VALID]=1.0;out[r::SLOT]=entity as f64;
            // Never misses a step `unexplained` would find: a unit at rest may
            // have stood until one tick before this pose.
            if out[r::KIND]!=1.0 {
                let place=[out[r::POSITION],out[r::POSITION+1],out[r::POSITION+2]];
                let speed=out[r::VELOCITY]*out[r::VELOCITY]+out[r::VELOCITY+1]*out[r::VELOCITY+1]+out[r::VELOCITY+2]*out[r::VELOCITY+2];
                if e.life!=generation {e.life=generation;e.seen=f64::NEG_INFINITY;e.odd=f64::NEG_INFINITY;}
                if tick>e.seen && e.seen.is_finite() {
                    let far=(0..3).map(|k|(place[k]-e.at[k])*(place[k]-e.at[k])).sum::<f64>();
                    if far>SLIDE_OVER*SLIDE_OVER {
                        let reach=e.speed.max(speed).sqrt()*(if e.speed<1e-4 {1.0} else {tick-e.seen})/hz+SLIDE_OVER;
                        if far>reach*reach {e.odd=e.odd.max(tick);}
                    }
                } else if tick<e.seen {e.odd=e.odd.max(e.seen);}
                if tick>=e.seen {e.seen=tick;e.at=place;e.speed=speed;}
            }
            self.input[at+r::SLOT]=entity as f64;
            // Fire counters unwrap per life in arrival order. Stale snapshot
            // parts retain their own raw value, matching the original ring.
            if out[r::HAS_FIRE_COUNTER]!=0.0 && (e.generation!=generation || tick>=e.fire_tick) {
                let raw=out[r::FIRE_COUNTER];
                let delta=(raw-e.raw+256.0)%256.0;
                e.total=if e.generation!=generation {raw} else {e.total+if delta<=128.0 {delta} else {0.0}};
                e.generation=generation;e.fire_tick=tick;e.raw=raw;out[r::FIRE_COUNTER]=e.total;
            }
        }
    }
    /// Resource binding resolves this stable slot once, without a page lookup.
    pub fn slot(&self,id:f64)->Option<usize>{let index=self.buckets[self.bucket(id)];(index!=usize::MAX).then_some(index)}
    /// SnapshotTimeline.sample for the entire retained population. Output
    /// rows carry their id so host bindings can reject stale or foreign rows.
    pub fn present(&mut self) {
        if self.input[f::VERSION]!=VERSION as f64 {return;}
        let tick=self.input[f::TICK];if !tick.is_finite() {return;}
        let hz=self.input[f::HZ];let extrapolate=self.input[f::EXTRAPOLATE];
        self.input[f::DT]=self.presented.map_or(0.0,|previous|three::clamp((tick-previous)/60.0,0.0,0.25));
        self.presented=Some(tick);
        if !hz.is_finite() || hz <= 0.0 {return;}
        // One batch covers every retained identity. The host copies only
        // bound rows and verifies the published id before using its slot.
        for (entity,e) in self.entities.iter().enumerate() {
            let out=&mut self.output[entity*o::STRIDE..][..o::STRIDE];out[o::VALID]=0.0;out[o::ID]=e.id;
            if e.id<0.0 {continue;}
            let mut a:Option<&[f64]>=None;let mut b:Option<&[f64]>=None;let mut atick=0.0;let mut btick=0.0;
            // The newest pose names the life on display: a body is bound to
            // the newest record's life, so an earlier life's poses are no
            // longer offered. `hole` is the newest snapshot older than `b`
            // that came without this unit: it had not changed by then.
            // `cut` is the newest pose ahead of the clock that the server
            // marked as a teleport or a seat change. (The mark stays set on
            // later records of a unit that has not moved since: with no
            // change of place there is nothing to cut.)
            let mut life=None;let mut hole=None;let mut cut=None;let mut newer:Option<(&[f64],f64)>=None;
            // `gap` is `hole` for the pose in `newer`. Poses older than the
            // bracket are read only while a step between them still slides.
            let mut gap=None;let mut offset=[0.0;3];let mut searching=true;
            // The slide of the pair newer than the one in hand, and whether the
            // unit rested before it: kept unless the older step is unexplained too.
            let mut slide:Option<[f64;3]>=None;let mut alone=false;let watch=tick-e.odd<SLIDE_TICKS;
            let cuts=|older:&[f64],newer:Option<(&[f64],f64)>|newer.is_some_and(|(pose,at)|at>tick && pose[r::DISC]!=0.0 && (0..3).any(|k|older[r::POSITION+k]!=pose[r::POSITION+k]));
            for &slot in self.order.iter().rev() {
                let snapshot=&self.snapshots[slot];
                let pose=&snapshot.records[entity*r::STRIDE..][..r::STRIDE];
                if pose[r::VALID]==0.0 || pose[r::ID]!=e.id {if newer.is_some()&&gap.is_none() {gap=Some(snapshot.tick);}continue;}
                let key=(pose[r::GENERATION],pose[r::KIND]);
                if *life.get_or_insert(key)!=key {searching=false;break;}
                if cut.is_none() && cuts(pose,newer) {cut=newer;}
                if let Some((next,at))=newer.filter(|_|watch) {
                    let step=held(unexplained(pose,snapshot.tick,gap,next,at,hz),at,tick);
                    if let Some(part)=slide.filter(|_|alone || step.is_none()) {for k in 0..3 {offset[k]+=part[k];}}
                    slide=step;alone=gap.is_some();
                    // Behind a teleport or seat change nothing older is drawn.
                    if at<=tick && next[r::DISC]!=0.0 && (0..3).any(|k|pose[r::POSITION+k]!=next[r::POSITION+k]) {searching=false;break;}
                }
                newer=Some((pose,snapshot.tick));
                if snapshot.tick>tick {b=Some(pose);btick=snapshot.tick;gap=None;continue;}
                if a.is_none() {a=Some(pose);atick=snapshot.tick;hole=gap;}
                gap=None;
                if !watch || snapshot.tick+SLIDE_TICKS<=tick {searching=false;break;}
            }
            if searching {
                let kept=&self.rest[entity*r::STRIDE..][..r::STRIDE];
                if kept[r::VALID]!=0.0 && kept[r::ID]==e.id && life.is_none_or(|key|key==(kept[r::GENERATION],kept[r::KIND])) {
                    if let Some((next,at))=newer.filter(|_|watch) {
                        let step=held(unexplained(kept,self.rest_tick[entity],gap,next,at,hz),at,tick);
                        if let Some(part)=slide.filter(|_|alone || step.is_none()) {for k in 0..3 {offset[k]+=part[k];}}
                        slide=step;
                    }
                    if a.is_none() {if cut.is_none() && cuts(kept,newer) {cut=newer;}a=Some(kept);atick=self.rest_tick[entity];hole=gap;}
                }
            }
            if let Some(part)=slide {for k in 0..3 {offset[k]+=part[k];}}
            if cut.is_some() {offset=[0.0;3];}
            // A life whose first pose is still ahead of the clock, and a
            // teleport or a seat change ahead of it, are shown at that pose
            // at once and held: the body is where it will next be seen, not
            // at a place it has already left.
            let (a,atick,b,btick)=match (cut,a,b) {
                (Some((pose,at)),_,_)=>(pose,at,pose,at),
                (None,Some(a),Some(b))=>(a,atick,b,btick),
                (None,None,Some(b))=>(b,btick,b,btick),
                (None,Some(a),None)=>(a,atick,a,atick),
                (None,None,None)=>continue,
            };
            // A unit at rest (no speed) that was absent from a later
            // snapshot stayed where it was until then: its first step or
            // turn starts at that snapshot, not at its last pose long ago.
            let still=a[r::VELOCITY]*a[r::VELOCITY]+a[r::VELOCITY+1]*a[r::VELOCITY+1]+a[r::VELOCITY+2]*a[r::VELOCITY+2]<1e-4;
            let start=if btick>atick && still {hole.unwrap_or(atick)} else {atick};
            let blend=if btick>start {three::clamp((tick-start)/(btick-start),0.0,1.0)} else {0.0};
            // State 8 is the thrown corpse that still travels. Other dead
            // entities hold their last pose instead of extrapolating it.
            let extra=if btick==atick && tick>=atick && (a[r::HEALTH]>0.0 || a[r::STATE]==8.0) {extrapolate.min(tick-atick)/hz} else {0.0};
            out[..r::STRIDE].copy_from_slice(a);out[o::VALID]=1.0;
            out[o::FROM_TICK]=atick;out[o::TO_TICK]=btick;out[o::EXTRAPOLATED_TICKS]=extra*hz;
            for k in 0..3 {
                out[o::VELOCITY+k]=a[r::VELOCITY+k]+(b[r::VELOCITY+k]-a[r::VELOCITY+k])*blend;
                out[o::POSITION+k]=a[r::POSITION+k]+(b[r::POSITION+k]-a[r::POSITION+k])*blend+a[r::VELOCITY+k]*extra+offset[k];
                out[o::PATH_VELOCITY+k]=if btick>start {if tick>=start {(b[r::POSITION+k]-a[r::POSITION+k])*hz/(btick-start)} else {0.0}} else if extra>0.0 && tick-atick<extrapolate {a[r::VELOCITY+k]} else {0.0};
                out[o::COAST_ORIGIN+k]=a[r::POSITION+k];
            }
            // Hulls use normalized quaternion slerp. Bodies use shortest-arc
            // yaw and retain discrete animation state from the older bracket.
            if a[r::KIND]==1.0 {
                let q=|row:&[f64]|Quaternion::new(row[r::QUATERNION],row[r::QUATERNION+1],row[r::QUATERNION+2],row[r::QUATERNION+3]);
                let mut turn=q(a);turn.slerp(&q(b),blend).normalize();
                out[o::QUATERNION..o::QUATERNION+4].copy_from_slice(&[turn.x,turn.y,turn.z,turn.w]);
            } else {
                let angle=|a:f64,b:f64,t:f64|a+v8math::atan2(v8math::sin(b-a),v8math::cos(b-a))*t;
                out[o::YAW]=angle(a[r::YAW],b[r::YAW],blend);out[o::PITCH]=a[r::PITCH]+(b[r::PITCH]-a[r::PITCH])*blend;
                let turning=btick>start && tick>=start;
                out[o::YAW_RATE]=if turning {(angle(a[r::YAW],b[r::YAW],1.0)-a[r::YAW])*hz/(btick-start)} else {0.0};
                out[o::PITCH_RATE]=if turning {(b[r::PITCH]-a[r::PITCH])*hz/(btick-start)} else {0.0};
                if a[r::KIND]==2.0 && a[r::MODE]==b[r::MODE] {for key in [r::TIME,r::ROLL,r::LEAN,r::LEGS] {out[key]=a[key]+(b[key]-a[key])*blend;}}
            }
        }
    }
}
