//! snapshot-timeline.js: one retained, bounded snapshot ring and one sample
//! batch for every remote body and hull. Histories pair by entity id, never
//! by a packet's or a renderer's row number. The frame allocates nothing.
use crate::timeline_layout::{timeline_frame as f, timeline_record as r, timeline_output as o, timeline_limits as lim, VERSION};
use three::Quaternion;

// Stable entity slots make a sparse history lookup constant time. A packet
// can arrive in any order without making sampling quadratic in population.
struct Snapshot { tick: f64, records: Vec<f64> }
struct Entity { id: f64, generation: f64, fire_tick: f64, raw: f64, total: f64 }
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
            order:Vec::with_capacity(lim::SNAPSHOTS),entities:Vec::with_capacity(lim::ENTITIES),buckets:vec![usize::MAX;lim::ENTITIES*2],free:Vec::with_capacity(lim::ENTITIES),epoch:0.0,maximum:lim::SNAPSHOTS,refused:0,presented:None }
    }
    // Epoch changes discard history and fire edges together, while retaining
    // the allocation for the next battle state.
    pub fn reset(&mut self, epoch:f64) { self.epoch=epoch;self.order.clear();self.entities.clear();self.buckets.fill(usize::MAX);self.free.clear();self.output.fill(0.0);self.presented=None; }
    // Removal clears every bracket for this identity. Its reserved slot can
    // safely serve a later generation without exposing an older pose.
    pub fn remove(&mut self,id:f64) {
        let bucket=self.bucket(id);let index=self.buckets[bucket];
        if index==usize::MAX {return;}
        for snapshot in &mut self.snapshots {snapshot.records[index*r::STRIDE+r::VALID]=0.0;}
        self.entities[index].id=-1.0;self.entities[index].fire_tick=f64::NEG_INFINITY;self.entities[index].generation=-1.0;
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
        while self.order.len()>self.maximum {self.order.remove(0);}
        if self.order.len()>=self.maximum && self.order.first().is_some_and(|&s|tick<self.snapshots[s].tick) {return;}
        // A second part of a tick reuses that whole snapshot. Otherwise its
        // slot is inserted in tick order and the oldest allocation is reused.
        let slot=if let Some(&slot)=self.order.iter().find(|&&s|self.snapshots[s].tick==tick) {slot} else {
            let slot=if self.order.len()>=self.maximum {self.order.remove(0)} else { (0..lim::SNAPSHOTS).find(|s|!self.order.contains(s)).unwrap_or(0) };
            self.snapshots[slot].tick=tick;self.snapshots[slot].records.fill(0.0);
            let at=self.order.iter().position(|&s|self.snapshots[s].tick>tick).unwrap_or(self.order.len());
            self.order.insert(at,slot);slot
        };
        let count=(self.input[f::COUNT] as usize).min(lim::ENTITIES);
        for row in 0..count {
            let at=f::STRIDE+row*r::STRIDE;
            self.input[at+r::SLOT]=-1.0;
            let id=self.input[at+r::ID];
            if !id.is_finite() || id<0.0 || id.fract()!=0.0 {continue;}
            let kind=self.input[at+r::KIND];if !(0.0..=2.0).contains(&kind) || kind.fract()!=0.0 {continue;}
            // Ids are not array indices: maps can have sparse ids, and
            // packet order changes when entities enter or leave interest.
            let bucket=self.bucket(id);
            let entity=if self.buckets[bucket]!=usize::MAX {self.buckets[bucket]} else {
                let fresh=Entity{id,generation:-1.0,fire_tick:f64::NEG_INFINITY,raw:0.0,total:0.0};
                let index=if let Some(index)=self.free.pop(){self.entities[index]=fresh;index}else{
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
        if !(hz>0.0) || !hz.is_finite() {return;}
        // One batch covers every retained identity. The host copies only
        // bound rows and verifies the published id before using its slot.
        for (entity,e) in self.entities.iter().enumerate() {
            let out=&mut self.output[entity*o::STRIDE..][..o::STRIDE];out[o::VALID]=0.0;out[o::ID]=e.id;
            if e.id<0.0 {continue;}
            let mut a=None;let mut b=None;let mut atick=0.0;let mut btick=0.0;
            for &slot in self.order.iter().rev() {
                let snapshot=&self.snapshots[slot];
                let pose=&snapshot.records[entity*r::STRIDE..][..r::STRIDE];
                if pose[r::VALID]==0.0 || pose[r::ID]!=e.id {continue;}
                if snapshot.tick>tick {b=Some(pose);btick=snapshot.tick;continue;}
                a=Some(pose);atick=snapshot.tick;if b.is_none() {b=Some(pose);btick=atick;}break;
            }
            let Some(a)=a else {continue;};let mut b=b.unwrap_or(a);
            // A death, teleport or new life cannot interpolate through the
            // newer frame. Hold/coast the earlier bracket until it arrives.
            if b[r::GENERATION]!=a[r::GENERATION] || b[r::KIND]!=a[r::KIND] || b[r::DISC]!=0.0 {b=a;btick=atick;}
            let blend=if btick>atick {three::clamp((tick-atick)/(btick-atick),0.0,1.0)} else {0.0};
            // State 8 is the thrown corpse that still travels. Other dead
            // entities hold their last pose instead of extrapolating it.
            let extra=if btick==atick && tick>=atick && (a[r::HEALTH]>0.0 || a[r::STATE]==8.0) {extrapolate.min(tick-atick)/hz} else {0.0};
            out[..r::STRIDE].copy_from_slice(a);out[o::VALID]=1.0;
            out[o::FROM_TICK]=atick;out[o::TO_TICK]=btick;out[o::EXTRAPOLATED_TICKS]=extra*hz;
            for k in 0..3 {
                out[o::VELOCITY+k]=a[r::VELOCITY+k]+(b[r::VELOCITY+k]-a[r::VELOCITY+k])*blend;
                out[o::POSITION+k]=a[r::POSITION+k]+(b[r::POSITION+k]-a[r::POSITION+k])*blend+a[r::VELOCITY+k]*extra;
                out[o::PATH_VELOCITY+k]=if btick>atick {(b[r::POSITION+k]-a[r::POSITION+k])*hz/(btick-atick)} else if extra>0.0 && tick-atick<extrapolate {a[r::VELOCITY+k]} else {0.0};
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
                out[o::YAW_RATE]=if btick>atick {(angle(a[r::YAW],b[r::YAW],1.0)-a[r::YAW])*hz/(btick-atick)} else {0.0};
                out[o::PITCH_RATE]=if btick>atick {(b[r::PITCH]-a[r::PITCH])*hz/(btick-atick)} else {0.0};
                if a[r::KIND]==2.0 && a[r::MODE]==b[r::MODE] {for key in [r::TIME,r::ROLL,r::LEAN,r::LEGS] {out[key]=a[key]+(b[key]-a[key])*blend;}}
            }
        }
    }
}
