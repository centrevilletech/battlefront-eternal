//! SniperTrace's retained flight, vapour, head shots and draw records.
//! The page supplies ordered events and displayed camera facts. Fixed stores
//! evict their oldest visual records; an overloaded battle never grows them.
use crate::sniper_layout::{sniper_frame as f,sniper_command as c,sniper_kind as k,sniper_event as e,sniper_event_kind as ek,sniper_match as m,sniper_limits as lim,sniper_counts as h,sniper_streak as streak,sniper_flare as flare};
use crate::cameras::math::{plus,sub,cross,Math,V8};
use three::{Vector3 as V,Matrix4};
const WHITE:[f64;3]=[1.0;3];
const GOLD:[f64;3]=[1.0,0.82,0.45];
const EMBER:[f64;3]=[1.0,0.5,0.15];
const VAPOUR:[f64;3]=[0.85,0.9,1.0];
fn unit(mut v:V)->V {v.normalize();v}
fn scale(mut v:V,n:f64)->V{v.multiply_scalar(n);v}
fn distance(a:V,b:V)->f64{sub(a,b).length()}
fn ease(x:f64)->f64{1.0-(1.0-x)*(1.0-x)}
fn vector(x:&[f64],at:usize)->V{V::new(x[at],x[at+1],x[at+2])}
fn matrix(x:&[f64],at:usize)->Matrix4{let mut m=Matrix4::default();m.elements.copy_from_slice(&x[at..at+16]);m}
// Every visual list had oldest-first eviction. Removing its first value
// preserves draw order while pushes stay inside the reserved allocation.
fn append<T:Copy>(items:&mut Vec<T>,cap:usize,value:T){if items.len()==cap{items.remove(0);}items.push(value);}
#[derive(Clone,Copy,Default)]
struct Callback {id:u32,seed:bool}
#[derive(Clone,Copy,Default)]
struct Record {
    id:u32,born:f64,landed:Option<f64>,ended:bool,touched:bool,
    start:V,head:V,target:V,direction:V,origin:V,drawn:V,fired:V,reach:f64,own:bool,
    local:Option<V>,color:[f64;3],seed:f64,headshot:bool,head_at:Option<V>,callbacks:[Callback;32],callback_count:usize,
}
#[derive(Clone,Copy)]
struct Source {id:f64,record:u32}
#[derive(Clone,Copy)]
struct Puff {at:V,direction:V,born:f64,seed:f64,wisps:[V;7]}
#[derive(Clone,Copy)]
struct Spark {at:V,velocity:V,life:f64,age:f64,color:[f64;3],ember:bool}
#[derive(Clone,Copy)]
struct Burst {at:V,color:[f64;3],radius:f64,life:f64,brightness:f64,turn:f64,born:f64}
#[derive(Clone,Copy)]
struct Head {at:V,time:f64}
#[derive(Clone,Copy)]
struct Waiting {at:V,until:f64}

pub struct Sniper {
    records:Vec<Record>,sources:Vec<Option<Source>>,puffs:Vec<Puff>,sparks:Vec<Spark>,bursts:Vec<Burst>,heads:Vec<Head>,waiting:Vec<Waiting>,
    time:Option<f64>,next:u32,max_source:f64,pub input:Vec<f64>,pub events:Vec<f64>,pub streaks:Vec<f32>,pub flares:Vec<f32>,pub matches:Vec<f64>,
    pub counts:[u32;h::STRIDE],eye:V,camera:Option<Matrix4>,inverse:Matrix4,muzzle:Option<V>,now:f64,
}
impl Default for Sniper {
    fn default()->Self {Self{records:Vec::with_capacity(48),sources:vec![None;lim::SOURCES],puffs:Vec::with_capacity(25),sparks:Vec::with_capacity(161),bursts:Vec::with_capacity(65),heads:Vec::with_capacity(512),waiting:Vec::with_capacity(512),time:None,next:0,max_source:0.0,
        input:vec![0.0;f::STRIDE+lim::COMMANDS*c::STRIDE],events:vec![0.0;lim::EVENTS*e::STRIDE],streaks:vec![0.0;lim::STREAKS*streak::STRIDE],flares:vec![0.0;lim::FLARES*flare::STRIDE],matches:vec![0.0;lim::COMMANDS*m::STRIDE],counts:[0;h::STRIDE],eye:V::new(0.0,1e6,0.0),camera:None,inverse:Matrix4::default(),muzzle:None,now:0.0}}
}
impl Sniper {
    // The displayed camera is already posed. Reading its matrices directly
    // avoids asking a browser scene to recompute any native-owned transform.
    fn facts(&mut self){self.now=self.input[f::TIME];self.eye=vector(&self.input,f::EYE);
        self.camera=(self.input[f::HAS_CAMERA]!=0.0).then(||matrix(&self.input,f::CAMERA));self.inverse=matrix(&self.input,f::INVERSE);
        self.muzzle=(self.input[f::HAS_MUZZLE]!=0.0).then(||vector(&self.input,f::MUZZLE));}
    /// The collect phase runs after all rounds have been drawn and before
    /// their terminal effects settle. Random draws keep their birth order.
    pub fn collect(&mut self,random:&mut impl FnMut()->f64){self.facts();self.counts[h::EVENTS]=0;
        if self.input[f::BEGIN]!=0.0{for r in &mut self.records{r.touched=false;}}
        let count=(self.input[f::COMMANDS]as usize).min(lim::COMMANDS);
        for index in 0..count {
            let at=f::STRIDE+index*c::STRIDE;let x:[f64;c::STRIDE]=self.input[at..at+c::STRIDE].try_into().unwrap();
            let point=vector(&x,c::POINT);
            match x[c::KIND]as u32 {
                k::TOUCH=>self.touch(&x,random),
                k::HEADSHOT=>self.headshot(point,random),
                k::DEFER=>{let token=x[c::TOKEN]as u32;let seed=x[c::SEED_EFFECT]!=0.0;
                    let record=if x[c::RECORD]>0.0{self.records.iter().position(|r|r.id==x[c::RECORD]as u32)}else{self.records.iter().position(|r|r.landed.is_none()&&self.now-r.born<=1.0&&off_line(r,point)<=1.5)};
                    if let Some(index)=record {let r=&mut self.records[index];if r.callback_count<r.callbacks.len(){r.callbacks[r.callback_count]=Callback{id:token,seed};r.callback_count+=1;}else{self.callback(Callback{id:token,seed:false},random);}}
                    else{self.callback(Callback{id:token,seed:false},random);}},

                _=>{}
            }
        }
        self.sizes();
    }
    /// Match all due impacts together. The resource callbacks are replayed
    /// in their original order, including unmatched effects and explosions.
    pub fn match_impacts(&mut self,count:usize){self.facts();for index in 0..count.min(lim::COMMANDS){let at=index*m::STRIDE;let p=vector(&self.matches,at+m::POINT);
        self.matches[at+m::RECORD]=self.records.iter().find(|r|r.landed.is_none()&&self.now-r.born<=1.0&&off_line(r,p)<=1.5).map_or(0.0,|r|r.id as f64);}}
    // SniperTrace.touch may see a replicated replacement for a cosmetic.
    // Only a fresh, untouched collinear record can claim that replacement.
    fn touch(&mut self,x:&[f64],random:&mut impl FnMut()->f64){let source=x[c::SOURCE];let start=vector(x,c::START);let head=vector(x,c::HEAD);let direction=vector(x,c::DIRECTION);
        if !source.is_finite()||source<1.0{return;}
        let source_slot=(source%lim::SOURCES as f64)as usize;
        let old=self.sources[source_slot].filter(|s|s.id==source).map(|s|s.record);
        let index=if let Some(id)=old{self.records.iter().position(|r|r.id==id)}else{
            // Source ids are assigned in collection order. Once a tombstone
            // leaves the bounded cache, an older id still cannot be reborn.
            if source<=self.max_source{return;}self.max_source=source;
            let existing=self.records.iter().position(|r|!r.touched&&self.now-r.born<0.5&&r.direction.dot(&direction)>0.999&&(distance(r.start,start)<0.6||distance(r.head,head)<0.6));
            let index=existing.unwrap_or_else(||self.add(x,start,direction,random));let id=self.records[index].id;
            self.sources[source_slot]=Some(Source{id:source,record:id});Some(index)};
        // An evicted source remains a tombstone: a later update must not
        // resurrect its puff or return a stale record's slot to a new shot.
        if let Some(index)=index {let r=&mut self.records[index];r.touched=true;r.target=head;r.ended|=x[c::ENDED]!=0.0;}
    }
    // SniperTrace.add keeps authored hue but deepens pale laser colours.
    // Eye ownership is fixed at birth, independent of later camera motion.
    fn add(&mut self,x:&[f64],start:V,direction:V,random:&mut impl FnMut()->f64)->usize{
        let peak=x[c::COLOR].max(x[c::COLOR+1]).max(x[c::COLOR+2]).max(1.0);
        let mut color=[0.0;3];for k in 0..3{color[k]=V8::pow((x[c::COLOR+k]/peak).max(0.0),1.4);}
        let saturation=1.0-color.iter().copied().fold(f64::INFINITY,f64::min);if saturation<0.65&&saturation>1e-3{for c in &mut color{*c=1.0-(1.0-*c)*(0.65/saturation);}}
        let own=distance(start,self.eye)<2.5;let local=if own&&self.camera.is_some(){let mut p=start;p.apply_matrix4(&self.inverse);Some(p)}else{None};
        self.next=self.next.wrapping_add(1).max(1);let r=Record{id:self.next,born:self.now,start,head:start,target:start,direction:unit(direction),origin:vector(x,c::ORIGIN),drawn:unit(direction),fired:start,reach:x[c::REACH],own,local,color,seed:random(),touched:true,..Default::default()};
        if self.records.len()==48 {
            let old=self.records[0];
            // Callbacks are host resource handles. Eviction releases them
            // without playing an impact whose drawn record has been lost.
            for callback in &old.callbacks[..old.callback_count]{self.emit(ek::RELEASE,callback.id as f64,0.0,V::default(),WHITE,0.0,0.0);}
        }
        append(&mut self.records,48,r);if !own{self.burst(start,color,0.45,0.07,1.2,0.0);}self.puff(start,r.direction,random);self.records.len()-1
    }
    // SniperTrace.puff rotates each wisp through the shot axis before
    // adding its forward drift. Preserve the original three random draws.
    fn puff(&mut self,at:V,direction:V,random:&mut impl FnMut()->f64){let across=unit(cross(direction,if direction.y.abs()<0.9{V::new(0.0,1.0,0.0)}else{V::new(1.0,0.0,0.0)}));let mut wisps=[V::default();7];
        for (k,w) in wisps.iter_mut().enumerate(){let angle=(k as f64+random()*0.6)/7.0*std::f64::consts::PI*2.0;*w=across;w.apply_axis_angle(&direction,angle);w.add_scaled_vector(&direction,0.3+random()*0.4).normalize().multiply_scalar(1.2+random()*1.2);}
        append(&mut self.puffs,25,Puff{at,direction,born:self.now,seed:random(),wisps});}
    // A burst is an impact or muzzle flare. It owns a point value, so
    // later movement of the source cannot carry an old flash along with it.
    fn burst(&mut self,at:V,color:[f64;3],radius:f64,life:f64,brightness:f64,turn:f64){append(&mut self.bursts,65,Burst{at,color,radius,life,brightness,turn,born:self.now});}
    // Resource events retain their numeric order. The store exceeds the
    // maximum callback releases and light requests in one command block.
    fn emit(&mut self,kind:u32,token:f64,seed:f64,at:V,color:[f64;3],reach:f64,time:f64){let index=self.counts[h::EVENTS]as usize;if index>=lim::EVENTS{self.counts[h::DROPPED]+=1;return;}self.counts[h::EVENTS]+=1;let out=&mut self.events[index*e::STRIDE..(index+1)*e::STRIDE];out[e::KIND]=kind as f64;out[e::TOKEN]=token;out[e::SEED]=seed;out[e::POSITION..e::POSITION+3].copy_from_slice(&[at.x,at.y,at.z]);out[e::COLOR..e::COLOR+3].copy_from_slice(&color);out[e::REACH]=reach;out[e::TIME]=time;}
    // Reserve an effect birth seed before making sparks, as land did.
    // The host consumes that seed when it applies the callback resource.
    fn callback(&mut self,callback:Callback,random:&mut impl FnMut()->f64){let seed=if callback.seed{(random()*4294967296.0)as u32 as f64}else{-1.0};self.emit(ek::CALLBACK,callback.id as f64,seed,V::default(),WHITE,0.0,0.0);}
    // SniperTrace.spray reflects forward-pointing samples back off the
    // struck surface, then adds its backward bias before normalizing.
    fn spray(&mut self,at:V,direction:V,color:[f64;3],count:usize,speed:f64,random:&mut impl FnMut()->f64){for _ in 0..count{let mut v=V::default();v.random_direction(random);let dot=v.dot(&direction);if dot>0.0{v.add_scaled_vector(&direction,-2.0*dot);}v.add_scaled_vector(&direction,-0.6).normalize().multiply_scalar((3.0+random()*11.0)*speed);let life=0.2+random()*0.45;let color=if random()<0.5{WHITE}else{color};append(&mut self.sparks,161,Spark{at,velocity:v,life,age:0.0,color,ember:false});}}
    // Embers keep their separate upward kick and slower cooling. Their
    // white-to-orange transition is evaluated from the remaining lifetime.
    fn embers(&mut self,at:V,direction:V,count:usize,random:&mut impl FnMut()->f64){for _ in 0..count{let mut v=V::default();v.random_direction(random).add_scaled_vector(&direction,-0.8).normalize().multiply_scalar(1.0+random()*3.0);v.y+=1.5;let life=0.6+random()*0.7;append(&mut self.sparks,161,Spark{at,velocity:v,life,age:0.0,color:EMBER,ember:true});}}
    fn flourish(&mut self,at:V,direction:V,random:&mut impl FnMut()->f64){self.burst(at,GOLD,1.4,0.16,1.9,0.785);self.spray(at,direction,GOLD,26,1.3,random);self.embers(at,direction,10,random);self.emit(ek::FLASH,0.0,0.0,at,[1.6,1.4,1.1],8.0,0.45);}
    // Landing callbacks precede impact randomness. A round that merely
    // exhausted its reach has no surface impact unless a head was reported.
    fn land(&mut self,r:&mut Record,random:&mut impl FnMut()->f64){r.ended=true;r.landed=Some(self.now);r.head=r.target;for callback in &r.callbacks[..r.callback_count]{self.callback(*callback,random);}r.callback_count=0;
        if r.reach!=0.0&&distance(r.head,r.start)>r.reach-1.0&&r.head_at.is_none(){return;}
        self.burst(r.head,WHITE,0.6*0.8,0.12,1.3,r.seed*3.0);self.burst(r.head,r.color,0.9*0.8,0.45,0.55,r.seed*3.0);self.spray(r.head,r.direction,r.color,12,1.0,random);self.embers(r.head,r.direction,6,random);
        self.emit(ek::FLASH,0.0,0.0,plus(r.head,r.direction,-0.4),r.color.map(|v|v*1.5),6.0+2.0*0.8,0.4);
        if let Some(at)=r.head_at.take(){self.flourish(at,r.direction,random);}}
    // A predicted and authoritative head report must produce one flourish.
    // A report ahead of its first draw waits briefly for a matching line.
    fn headshot(&mut self,at:V,random:&mut impl FnMut()->f64){self.heads.retain(|h|self.now-h.time<1.2);if self.heads.iter().any(|h|distance(h.at,at)<1.5){return;}append(&mut self.heads,512,Head{at,time:self.now});let mut index=None;let mut best=1.5;
        for(k,r)in self.records.iter().enumerate(){if r.headshot||self.now-r.born>1.0{continue;}let d=off_line(r,at);if d<best{index=Some(k);best=d;}}
        if let Some(k)=index{let r=&mut self.records[k];r.headshot=true;if r.landed.is_none(){r.head_at=Some(at);}else{let direction=r.direction;self.flourish(at,direction,random);}}
        else{append(&mut self.waiting,512,Waiting{at,until:self.now+0.15});}}
    // Waiting reports claim the first eligible line. Compaction retains
    // arrival order, including independent flourishes after their timeout.
    fn claim_waiting(&mut self,random:&mut impl FnMut()->f64){let mut kept=0;for k in 0..self.waiting.len(){let w=self.waiting[k];let record=self.records.iter_mut().find(|r|!r.headshot&&r.landed.is_none()&&off_line(r,w.at)<1.5);if let Some(r)=record{r.headshot=true;r.head_at=Some(w.at);}else if self.now<w.until{self.waiting[kept]=w;kept+=1;}else{self.flourish(w.at,V::new(0.0,0.0,1.0),random);}}self.waiting.truncate(kept);}
    // SniperTrace's GPU attributes use generated group offsets. Component
    // order and f32 conversion stay identical to the interleaved draw format.
    fn streak(&mut self,head:V,tail:V,color:[f64;3],brightness:f64,core:f64,glow:f64,kind:f64,seed:f64,age:f64,opacity:f64,offset:f64,near:f64){
        let k=self.counts[h::STREAKS]as usize;
        if k>=lim::STREAKS{self.counts[h::DROPPED]+=1;return;}
        self.counts[h::STREAKS]+=1;let row=&mut self.streaks[k*streak::STRIDE..][..streak::STRIDE];
        row[streak::HEAD..streak::HEAD+3].copy_from_slice(&[head.x as f32,head.y as f32,head.z as f32]);
        row[streak::TAIL..streak::TAIL+3].copy_from_slice(&[tail.x as f32,tail.y as f32,tail.z as f32]);
        row[streak::COLOR..streak::COLOR+4].copy_from_slice(&[color[0]as f32,color[1]as f32,color[2]as f32,brightness as f32]);
        row[streak::SIZE..streak::SIZE+4].copy_from_slice(&[core as f32,glow as f32,kind as f32,seed as f32]);
        row[streak::MISC..streak::MISC+4].copy_from_slice(&[age as f32,opacity as f32,offset as f32,near as f32]);
    }
    fn flare(&mut self,b:Burst,age:f64){
        let k=self.counts[h::FLARES]as usize;
        if k>=lim::FLARES{self.counts[h::DROPPED]+=1;return;}
        self.counts[h::FLARES]+=1;let t=(age/b.life).min(1.0);let row=&mut self.flares[k*flare::STRIDE..][..flare::STRIDE];
        row[flare::CENTER..flare::CENTER+3].copy_from_slice(&[b.at.x as f32,b.at.y as f32,b.at.z as f32]);
        row[flare::AXIS..flare::AXIS+3].fill(0.0);
        row[flare::COLOR..flare::COLOR+4].copy_from_slice(&[b.color[0]as f32,b.color[1]as f32,b.color[2]as f32,(b.brightness*(1.0-t)*(1.0-t))as f32]);
        row[flare::PARAM..flare::PARAM+4].copy_from_slice(&[(b.radius*(0.7+0.5*ease(t)))as f32,0.0,0.0,b.turn as f32]);
    }
    fn sizes(&mut self){self.counts[h::RECORDS]=self.records.len()as u32;self.counts[h::SPARKS]=self.sparks.len()as u32;}
    /// SniperTrace.end advances all retained visuals once. Counts describe
    /// prefixes of immutable-capacity buffers consumed directly by rendering.
    pub fn frame(&mut self,random:&mut impl FnMut()->f64){self.facts();let dt=self.time.map_or(0.0,|t|(self.now-t).clamp(0.0,0.1));self.time=Some(self.now);self.counts[h::STREAKS]=0;self.counts[h::FLARES]=0;self.counts[h::EVENTS]=0;
        self.claim_waiting(random);let mut kept=0;for k in 0..self.records.len(){let mut r=self.records[k];if self.draw_record(&mut r,random){self.records[kept]=r;kept+=1;}}self.records.truncate(kept);
        kept=0;for k in 0..self.puffs.len(){let p=self.puffs[k];if self.draw_puff(p){self.puffs[kept]=p;kept+=1;}}self.puffs.truncate(kept);
        kept=0;for k in 0..self.bursts.len(){let b=self.bursts[k];let age=self.now-b.born;if age>=0.0{self.flare(b,age);}if age<b.life{self.bursts[kept]=b;kept+=1;}}self.bursts.truncate(kept);
        kept=0;for k in 0..self.sparks.len(){let mut s=self.sparks[k];s.age+=dt;s.velocity.y-=9.8*dt;s.velocity.multiply_scalar(V8::exp(-dt*2.5));s.at.add_scaled_vector(&s.velocity,dt);let left=1.0-s.age/s.life;
            if left>0.0{let tail=plus(s.at,s.velocity,if s.ember{-0.012}else{-0.03});let color=if s.ember&&left>0.6{WHITE}else{s.color};self.streak(s.at,tail,color,if s.ember{1.1*left}else{1.4*left},if s.ember{0.01}else{0.012},if s.ember{0.035}else{0.05},2.0,0.0,0.0,0.0,0.0,0.0);}
            if s.age<s.life{self.sparks[kept]=s;kept+=1;}}self.sparks.truncate(kept);self.sizes();}
    // SniperTrace.drawRecord separates the moving bolt from its fixed
    // smoke origin. Arrival uses the target distance from the current gun.
    fn draw_record(&mut self,r:&mut Record,random:&mut impl FnMut()->f64)->bool{let age=self.now-r.born;if !r.touched{r.ended=true;}
        if let Some(muzzle)=self.muzzle.filter(|&p|r.own&&distance(p,r.start)<1.5){r.start=muzzle;}else if let(Some(mut p),Some(camera))=(r.local,self.camera){p.apply_matrix4(&camera);r.start=p;}
        let endpoint=if r.ended{r.target}else{plus(r.origin,r.direction,if r.reach!=0.0{r.reach}else{300.0})};r.drawn=sub(endpoint,r.start);r.drawn=if r.drawn.length_sq()>1e-8{unit(r.drawn)}else{r.direction};
        let reach=if r.ended{distance(r.target,r.start)}else{f64::INFINITY};let flown=reach.min(650.0*age);if r.landed.is_none(){r.head=plus(r.start,r.drawn,flown);if r.ended&&flown>=reach{self.land(r,random);}}
        let after=r.landed.map_or(0.0,|t|self.now-t);if r.landed.is_some()&&after>0.05&&after>0.6&&age>2.2{return false;}
        let length=distance(r.head,r.start);let clear=if r.own{0.6_f64.min(length)}else{0.0};let fade_in=if r.own{1.2}else{0.5};let back=(length-6.0).max(0.0);let collapse=if r.landed.is_none(){0.0}else{(after/0.05).min(1.0)};let tail_at=clear.max(back+(length-back)*collapse);
        if collapse<1.0&&length-tail_at>0.05{let thin=if r.own{0.6}else{1.0};self.streak(r.head,plus(r.start,r.drawn,tail_at),r.color,2.0*(1.0-0.5*collapse),0.045*thin,0.17*thin,2.0,r.seed,age,fade_in,tail_at,0.0);if !r.own{self.emit(ek::LIGHT,0.0,0.0,r.head,r.color,4.0,0.0);}}
        let glow_left=if r.landed.is_none(){1.0}else{1.0-after/0.6};let cooled=clear.max((age-0.03)*140.0);if glow_left>0.0&&length>cooled+0.5{self.streak(r.head,plus(r.start,r.drawn,cooled),r.color,0.6*glow_left*glow_left,0.03*0.3,0.03*(0.6+0.4*glow_left),0.0,r.seed,age,fade_in,cooled,0.0);}
        let left=(1.0-age/2.2).max(0.0);if left>0.0&&length>clear+2.5*if r.own{1.0}else{0.0}{let rise=0.12*age;let radius=0.03+0.22*ease((age/(2.2*0.6)).min(1.0));let opacity=0.5*V8::pow(left,1.3)*(age/0.04+0.2).min(1.0);let reach_smoke=distance(r.head,r.fired);let skip=if r.own{2.5_f64.min(reach_smoke)}else{0.0};let mut lift=r.head;lift.y+=rise;let mut tail=scale(sub(r.head,r.fired),skip/reach_smoke.max(1e-6));tail.add(&r.fired);tail.y+=rise;self.streak(lift,tail,r.color,0.4*V8::exp(-age*4.0),0.0,radius*2.0,1.0,r.seed,age,opacity*if r.own{0.75}else{1.0},skip,0.0);}
        true
    }
    // SniperTrace.drawPuff keeps exponential expansion and f32 instance
    // stores in their original order; it never integrates per-wisp objects.
    fn draw_puff(&mut self,p:Puff)->bool{let age=self.now-p.born;if age>0.9{return false;}let k=age/0.9;let out=1.0-V8::exp(-age*6.0);let opacity=0.42*V8::pow(1.0-k,1.6);let glow=0.35*V8::exp(-age*16.0);
        let mut lift=plus(p.at,p.direction,0.15+0.5*out);lift.y+=0.12*age;let mut tail=plus(p.at,p.direction,0.05*out);tail.y+=0.12*age;self.streak(lift,tail,VAPOUR,glow,0.0,(0.05+0.24*ease((age/0.5).min(1.0)))*2.0,1.0,p.seed,age*0.5,opacity,0.0,1.0);
        for(k,w)in p.wisps.iter().enumerate(){let mut lift=plus(p.at,*w,0.3*out);lift.y+=0.15*age;let tail=plus(lift,*w,-0.08);self.streak(lift,tail,VAPOUR,glow*0.3,0.0,(0.03+0.1*ease((age/0.4).min(1.0)))*2.0,1.0,p.seed+k as f64,age*0.5,opacity*0.7,0.0,1.0);}true}
}
fn off_line(r:&Record,point:V)->f64{let v=sub(point,r.origin);let along=v.dot(&r.direction);if along<0.0{f64::INFINITY}else{plus(v,r.direction,-along).length()}}
