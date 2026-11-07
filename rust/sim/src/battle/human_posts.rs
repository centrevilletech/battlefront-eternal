//! Human spawn choices (human-spawn.js and flight/spawn.js). Human picks use
//! gameplay uniforms, including a draw for a one-point path; Classic bot
//! choices retain their separate raw-modulo RNG path in spawning.rs.
use super::{mobile, posts::{self, SpawnFacts}, world::PostsWorld};
use crate::gen::{FlightClass, FlightState, SpawnPost, Store, Vehicle, Post};
use crate::store::{Ref, StrId};
use three::{Quaternion, Vector3};
const FLT_MAX:f64=3.4028234663852886e38;
#[derive(Clone,Copy,Debug,PartialEq)]
pub enum Key { Post(SpawnPost), Transport(Ref<Vehicle>) }
#[derive(Clone,Copy)]
pub struct Candidate { pub key:Key, pub name:StrId, pub facts:SpawnFacts, point:Option<[f64;4]> }
struct Raw { post:Ref<Post>, points:Option<Vec<[f64;4]>> }
pub struct Choices {
    pub rows:Vec<Candidate>, points:Vec<[f64;4]>,
    hulls:Vec<Option<(Ref<Vehicle>,StrId)>>, raw:Vec<Raw>,
    placed:usize,
}
impl Choices {
    pub fn new(posts:usize,hulls:usize,points:usize)->Self {
        Self{rows:Vec::with_capacity(posts+hulls),points:Vec::with_capacity(points),hulls:Vec::new(),raw:Vec::with_capacity(posts),placed:posts}
    }
    /// Names and unmodified world spawn paths are prepared once. A fallback
    /// uses the raw path, which can differ from the mission's AI spawn path.
    pub fn raw_post(&mut self,post:Ref<Post>,points:Option<Vec<[f64;4]>>) {self.raw.push(Raw{post,points});}
    pub fn hull(&mut self,hull:Ref<Vehicle>,name:StrId) {
        self.hulls.resize(self.hulls.len().max(hull.slot as usize+1),None);
        self.hulls[hull.slot as usize]=Some((hull,name));
        self.rows.reserve((self.placed+self.hulls.len()).saturating_sub(self.rows.len()));
    }
    pub fn reserve_points(&mut self,count:usize) {self.points.reserve(count.saturating_sub(self.points.len()));}
    pub fn begin<W:PostsWorld>(&mut self,s:&mut Store,world:&mut W,
        mut floor:impl FnMut(Vector3,Ref<crate::gen::Body>)->Option<(Vector3,Vector3)>) {
        self.begin_with(s, world, |_, at, body| floor(at, body));
    }
    /// Lend the same post owner to each grounding query without capturing it
    /// while transport admission already holds its mutable loan.
    pub fn begin_with<W:PostsWorld>(&mut self,s:&mut Store,world:&mut W,
        mut floor:impl FnMut(&mut W,Vector3,Ref<crate::gen::Body>)->Option<(Vector3,Vector3)>) {
        mobile::refresh_all(s,world); self.rows.clear();
        for &r in &s.command_posts.posts {
            let p=s.post.get(r).expect("placed human post"); if !p.active {continue;}
            self.rows.push(Candidate{key:Key::Post(SpawnPost::Post(r)),name:p.name,
                facts:posts::spawn_facts(s,SpawnPost::Post(r),world),point:None});
        }
        for &r in &s.mobile_posts.posts {
            let p=s.mobile_post.get(r).expect("mobile human post"); if !p.active {continue;}
            self.rows.push(Candidate{key:Key::Post(SpawnPost::MobilePost(r)),name:p.name,
                facts:posts::spawn_facts(s,SpawnPost::MobilePost(r),world),point:None});
        }
        for &r in &s.vehicles.vehicles {
            let v=s.vehicle.get(r).expect("human transport hull");
            let team=v.team.unwrap_or(0.0);
            if !v.alive || v.flight_class!=Some(FlightClass::Transport) || v.flight_state!=Some(FlightState::Landed) || team==0.0 || team.is_nan() {continue;}
            let mut at=Vector3::new(v.bounds_max[0]+2.0,1.0,0.0);
            let [x,y,z,w]=v.quaternion; at.apply_quaternion(&Quaternion::new(x,y,z,w));
            at.add(&Vector3::new(v.position[0],v.position[1],v.position[2]));
            let Some((hit,normal))=floor(world,at,v.body) else {continue}; if normal.y<0.8 {continue;}
            at.y=hit.y+0.1;
            let name=self.hulls.get(r.slot as usize).copied().flatten().filter(|(h,_)|*h==r).expect("prepared transport post name").1;
            self.rows.push(Candidate{key:Key::Transport(r),name,
                facts:SpawnFacts{active:true,owner:jsrt::conv::to_int32(team) as f64,position:[at.x,at.y,at.z],spawn_path:true,..SpawnFacts::default()},
                point:Some([at.x,at.y,at.z,v.yaw])});
        }
    }
    /// Game.spawnPosts refreshes mobile ownership, then keeps only placed
    /// posts with authored raw paths. URL auto-spawn does not use AI paths,
    /// allied posts, transport exits or the spawn screen's nearest-post rule.
    pub fn auto_posts<W:PostsWorld>(&mut self,s:&mut Store,team:f64,world:&mut W)->usize {
        mobile::refresh_all(s,world);
        self.rows.clear();
        let own=s.command_posts.posts.iter().any(|&r| {
            let p=s.post.get(r).expect("placed auto post");
            p.active && p.team==team && self.raw.iter().any(|raw|raw.post==r && raw.points.is_some())
        });
        for &r in &s.command_posts.posts {
            let p=s.post.get(r).expect("placed auto post");
            if !p.active || (own && p.team!=team) || !self.raw.iter().any(|raw|raw.post==r && raw.points.is_some()) {continue;}
            self.rows.push(Candidate{key:Key::Post(SpawnPost::Post(r)),name:p.name,
                facts:SpawnFacts::default(),point:None});
        }
        self.rows.len()
    }
    /// The readiness callback indexes directly, before respawn normalizes a
    /// URL index. An out-of-range choice leaves the queued post unchanged.
    pub fn auto_post(&self,index:f64)->Option<SpawnPost> {
        if !index.is_finite() || index<0.0 || index.fract()!=0.0 {return None;}
        self.rows.get(index as usize).and_then(|p|match p.key {Key::Post(key)=>Some(key),_=>None})
    }
    /// Game.respawn consumes one gameplay uniform even for a single point.
    /// Empty authored arrays remain present and reject after consuming it.
    pub fn auto_point(&self,index:&mut f64,fallback:&[[f64;4]],mut uniform:impl FnMut()->f64)->Option<(Option<StrId>,[f64;4])> {
        let (name,points)=if self.rows.is_empty() {(None,fallback)} else {
            let count=self.rows.len() as f64;
            *index=((*index%count)+count)%count;
            if !index.is_finite() || index.fract()!=0.0 {return None;}
            let p=self.rows.get(*index as usize)?;
            let Key::Post(SpawnPost::Post(r))=p.key else {return None};
            (Some(p.name),self.raw.iter().find(|raw|raw.post==r).and_then(|raw|raw.points.as_deref())?)
        };
        let chosen=v8math::floor(uniform()*points.len() as f64) as usize;
        Some((name,*points.get(chosen)?))
    }
    pub(crate) fn name(&self,index:usize)->StrId {self.rows[index].name}
    pub fn named(&self,name:Option<StrId>)->Option<usize> {self.rows.iter().position(|p|Some(p.name)==name)}
    fn friendly(p:&Candidate,team:f64,relation:&mut impl FnMut(f64,f64)->f64)->bool {
        if !p.facts.active || p.facts.owner==0.0 {return false;}
        if p.facts.owner==team {p.facts.spawn_path || p.facts.class_spawn_points>0.0}
        else {relation(p.facts.owner,team)>0.0 && p.facts.ally_path}
    }
    pub fn choose(&self,team:f64,previous:Option<StrId>,mut relation:impl FnMut(f64,f64)->f64,mut uniform:impl FnMut()->f64)->Option<usize> {
        self.choose_with(team,previous,&mut (),|_,a,b|relation(a,b),|_|uniform())
    }
    pub fn choose_with<W>(&self,team:f64,previous:Option<StrId>,world:&mut W,mut relation:impl FnMut(&mut W,f64,f64)->f64,mut uniform:impl FnMut(&mut W)->f64)->Option<usize> {
        let previous=self.named(previous); let mut best=FLT_MAX; let mut pick=None;
        if let Some(old)=previous {
            if Self::friendly(&self.rows[old],team,&mut |a,b|relation(world,a,b)) {return Some(old);}
            for (i,p) in self.rows.iter().enumerate() {
                if !Self::friendly(p,team,&mut |a,b|relation(world,a,b)) {continue;}
                let d=distance(p.facts.position,self.rows[old].facts.position);
                if best>d {best=d;pick=Some(i);}
            }
        } else {
            for (i,p) in self.rows.iter().enumerate() {
                if !Self::friendly(p,team,&mut |a,b|relation(world,a,b)) {continue;}
                for enemy in &self.rows {
                    if !(relation(world,team,enemy.facts.owner)<0.0) {continue;}
                    let d=distance(p.facts.position,enemy.facts.position);
                    if best>d {best=d;pick=Some(i);}
                }
            }
        }
        if pick.is_some() {return pick;}
        let count=self.rows.iter().filter(|p|Self::friendly(p,team,&mut |a,b|relation(world,a,b))).count();
        if count==0 {return None;}
        let chosen=v8math::min(v8math::floor(uniform(world)*count as f64),count as f64-1.0) as usize;
        self.rows.iter().enumerate().filter(|(_,p)|Self::friendly(p,team,&mut |a,b|relation(world,a,b))).nth(chosen).map(|(i,_)|i)
    }
    /// Game.spawnPosts' fallback is limited to placed posts with raw paths.
    /// An own post wins; otherwise the first active placed path is retained.
    pub fn fallback(&self,team:f64)->Option<usize> {
        let raw=|p:&Candidate|match p.key {Key::Post(SpawnPost::Post(r))=>self.raw.iter().any(|p|p.post==r && p.points.is_some()),_=>false};
        self.rows.iter().position(|p|p.facts.owner==team && raw(p)).or_else(||self.rows.iter().position(raw))
    }
    pub fn point<W:PostsWorld>(&mut self,s:&Store,index:usize,team:f64,world:&mut W,
        mut relation:impl FnMut(f64,f64)->f64,uniform:impl FnMut()->f64)->Option<[f64;4]> {
        self.point_with(s, index, team, world, |_, a, b| relation(a, b), uniform)
    }
    /// Relation and path reads borrow one owner in sequence. A recursive body
    /// operation never needs to reacquire a facade during post admission.
    pub fn point_with<W:PostsWorld>(&mut self,s:&Store,index:usize,team:f64,world:&mut W,
        mut relation:impl FnMut(&mut W,f64,f64)->f64,mut uniform:impl FnMut()->f64)->Option<[f64;4]> {
        self.point_context(s,index,team,world,relation,|_|uniform())
    }
    pub fn point_context<W:PostsWorld>(&mut self,s:&Store,index:usize,team:f64,world:&mut W,
        mut relation:impl FnMut(&mut W,f64,f64)->f64,mut uniform:impl FnMut(&mut W)->f64)->Option<[f64;4]> {
        let p=*self.rows.get(index)?;
        if !Self::friendly(&p,team,&mut |a,b|relation(world,a,b)) {
            if let Key::Post(SpawnPost::Post(r))=p.key {
                if let Some(points)=self.raw.iter().find(|p|p.post==r).and_then(|p|p.points.as_ref()) {
                    let chosen=v8math::floor(uniform(world)*points.len() as f64) as usize;
                    return points.get(chosen).copied();
                }
            }
        }
        if let Some(point)=p.point {uniform(world);return Some(point);}
        let Key::Post(key)=p.key else {unreachable!()};
        let allied=p.facts.owner!=team;
        if let SpawnPost::Post(r)=key {
            let ai=s.post.get(r).expect("human path post").ai.as_ref();
            let path=ai.and_then(|ai|if allied {ai.ally_path.as_ref()} else {ai.spawn_path.as_ref()});
            if let Some(path)=path.filter(|path|path.present && !path.points.is_empty()) {
                let i=v8math::min(v8math::floor(uniform(world)*path.points.len() as f64),path.points.len() as f64-1.0) as usize;
                return path.points.get(i).copied();
            }
        }
        if !allied && p.facts.class_spawn_points>0.0 {
            if let SpawnPost::MobilePost(r)=key {world.class_spawn_world_into(s,r,&mut self.points);}
            else if let SpawnPost::Post(r)=key {
                self.points.clear();if let Some(ai)=&s.post.get(r).unwrap().ai {self.points.extend_from_slice(&ai.class_spawn_world);}
            }
            if !self.points.is_empty() {
                let i=v8math::min(v8math::floor(uniform(world)*p.facts.class_spawn_points),self.points.len() as f64-1.0) as usize;
                return self.points.get(i).copied().or_else(||self.points.first().copied());
            }
        }
        let yaw=if p.facts.yaw==0.0 || p.facts.yaw.is_nan() {0.0} else {p.facts.yaw};
        Some([p.facts.position[0],p.facts.position[1],p.facts.position[2],yaw])
    }
}
fn distance(a:[f64;3],b:[f64;3])->f64 {let (x,y,z)=(a[0]-b[0],a[1]-b[1],a[2]-b[2]);(x*x+y*y+z*z) as f32 as f64}
