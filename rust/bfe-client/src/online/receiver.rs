//! network-session.js's delivery boundary, without a JavaScript entity facade.
//! Packet callbacks consume leases before the next datagram can recycle them;
//! only resource identities and the own camera checkpoint are retained here.
use bfe_presentation::{network::{Session,Shared},network_layout::{net_header as h,net_kind as k,net_record as r,net_limits as l}};
use bfe_sim::prediction::{corrections::{decode_own_player_header,decode_compact_soldier_fixed},reconcile::EstimatorState};
use serde::Deserialize;
use super::feedback::{Ids,Results};
use std::{cell::RefCell,rc::Rc};
pub const BOUNDS:[f64;6]=[-1024.,2048.,-1024.,2048.,-400.,700.];
#[derive(Default,Clone,Copy)]
pub struct Own {pub id:f64,pub generation:f64,pub class:usize,pub team:u8,pub position:[f64;3],pub yaw:f64,pub pitch:f64,pub alive:bool,pub tick:f64,pub posture:usize,pub air:u8,pub zoom:f64,pub dive:f64,pub lock:f64}
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct Message<'a>{#[serde(rename="type")]kind:&'a str,#[serde(default)]epoch:Option<f64>,#[serde(default)]generation:Option<f64>,#[serde(default)]player_id:Option<f64>,#[serde(default)]tick:Option<f64>,#[serde(default)]server_tick:Option<f64>,#[serde(default)]ack:Option<f64>,#[serde(default)]spectator:bool,#[serde(default)]message:Option<&'a str>,#[serde(default)]received:Ids,#[serde(default)]results:Results}
#[derive(Clone,Copy)]
struct Early {bytes:[u8;l::PAYLOAD],length:usize,now:f64}
pub struct Receiver {
    pub network:Shared,pub clock:super::clock::Clock,pub facts:Vec<[f64;r::STRIDE]>,indices:super::index::Index,free:Vec<usize>,
    pub own:Own,pub welcomed:bool,pub spectator:bool,pub epoch:f64,pub latest:f64,pub complete:bool,pub registry:bool,
    expected:serde_json::Value,pub estimator:EstimatorState,pub ack:f64,pub packets:u64,pub corrections:u64,pub corrected_sequence:f64,pub refused:u64,
    pub fault:Option<&'static str>,pub received:Ids,pub outcomes:Ids,early:Vec<Early>,early_at:usize,early_count:usize,
}
impl Receiver {
    // Reserve both receiver baselines and the early-transport queue before
    // any live callback can arrive. Resource facts retain no payload pointer.
    pub fn new(expected:serde_json::Value)->Self{
        let mut session=Session::new();session.reserve_receive();
        Self{network:Rc::new(RefCell::new(session)),clock:Default::default(),facts:vec![[0.;r::STRIDE];l::SLOTS],indices:super::index::Index::new(l::SLOTS),free:(0..l::SLOTS).rev().collect(),own:Own::default(),welcomed:false,spectator:false,epoch:1.,latest:0.,complete:false,registry:false,expected,
            estimator:EstimatorState{hz:60.,window:64.,at:0.,count:0.,offset:None,dropped:None,offsets:vec![0.;64]},ack:0.,packets:0,corrections:0,corrected_sequence:0.,refused:0,fault:None,received:Ids::default(),outcomes:Ids::default(),early:vec![Early{bytes:[0;l::PAYLOAD],length:0,now:0.};256],early_at:0,early_count:0}
    }
    pub fn message(&mut self,text:bool,bytes:&[u8],now:f64){
        if self.fault.is_some(){return;}
        if text{
            self.text(bytes,now);
            // State and reliable bootstrap have independent transport order.
            // Keep bounded early state until registry/welcome admission, just
            // as NetworkSession's initial entity queue did in the browser.
            if self.registry&&self.welcomed{while self.early_count!=0&&self.fault.is_none(){
                let early=self.early[self.early_at];self.early_at=(self.early_at+1)%self.early.len();self.early_count-=1;
                self.message(false,&early.bytes[..early.length],early.now);
            }}return;
        }
        if !self.registry||!self.welcomed{
            if !(20..=l::PAYLOAD).contains(&bytes.len()){self.fault=Some("invalid early snapshot");return;}
            if self.early_count==self.early.len(){self.early_at=(self.early_at+64)%self.early.len();self.early_count-=64;self.refused+=64;}
            let at=(self.early_at+self.early_count)%self.early.len();self.early[at].bytes[..bytes.len()].copy_from_slice(bytes);self.early[at].length=bytes.len();self.early[at].now=now;self.early_count+=1;return;
        }
        // Session.receive is the same decoder used by SnapshotClient. Its
        // publication lease lasts only until this callback receives again.
        let mut owner=self.network.borrow_mut();
        if let Err(e)=owner.receive(bytes,&BOUNDS){self.fault=Some(e);return;}
        // Count delivered datagrams separately from valid publications:
        // duplicate or incomplete parts may contribute no changed records.
        self.packets+=1;
        if owner.header[h::VALID]==0.{return;}
        // An epoch invalidates every resource identity and arrival estimate.
        // Keep reservations, but never pair a new map with an old pose.
        if owner.header[h::EPOCH_CHANGED]!=0.{self.indices.clear();self.free.clear();self.free.extend((0..l::SLOTS).rev());self.facts.fill([0.;r::STRIDE]);self.complete=false;self.estimator.reset();self.clock.reset();}
        self.epoch=owner.header[h::EPOCH];let tick=owner.header[h::TICK];
        if tick>self.latest{self.latest=tick;self.estimator.arrive(tick,now/1000.,0.);}
        if let Err(error)=self.clock.arrive(tick,now/1000.,owner.header[h::INTERVAL]){self.fault=Some(error);return;}
        self.complete|=owner.header[h::COMPLETE]!=0.;self.ack=self.ack.max(owner.header[h::ACK_INPUT]);
        for at in 0..(owner.header[h::RECORDS]as usize).min(owner.leases.publications.len()){
            let lease=owner.leases.publications[at]as usize;let row=&owner.leases.records[lease*r::STRIDE..][..r::STRIDE];
            let kind=row[r::KIND]as u32;let id=row[r::ID]as u32;
            // Read only the shared correction prefix needed for the own
            // camera. Opaque arsenal state remains in the native receiver.
            if kind==k::CORRECTION&&row[r::ID]==self.own.id{
                let offset=row[r::PAYLOAD_OFFSET]as usize;let length=row[r::PAYLOAD_LENGTH]as usize;
                let Some(bytes)=owner.leases.payloads.get(offset..offset+length)else{continue;};
                let Ok(head)=decode_own_player_header(bytes)else{self.fault=Some("invalid own correction");return;};
                self.own.generation=row[r::GENERATION];self.own.class=head.class_id as usize;self.own.team=head.team;self.own.alive=head.dead==0&&head.removed==0;self.own.tick=tick;self.own.zoom=head.zoom;
                if head.body_schema==6{
                    if let Ok(body)=decode_compact_soldier_fixed(head.body.get(bytes),&[true;6]){self.own.position=body.vectors[0];self.own.yaw=body.numbers[1];self.own.pitch=body.numbers[2];self.own.posture=body.posture as usize;self.own.air=body.air;self.own.dive=body.numbers[3];self.own.lock=body.numbers[5];}
                }self.corrected_sequence=owner.header[h::ACK_INPUT];self.corrections+=1;continue;
            }
            if kind==k::TOMBSTONE{
                // Identity is shared across entity kinds, but a late tombstone
                // cannot remove the newer generation that reused that identity.
                for removed in k::SOLDIER..=k::REMOTE{if let Some(&index)=self.indices.get(&(removed,id)){
                    if self.facts[index][r::GENERATION]==row[r::GENERATION]{self.indices.remove(&(removed,id));self.facts[index].fill(0.);self.free.push(index);}
                }}continue;
            }
            // Summary/combat leases are not model identities. Resource
            // admission below retains only the body/hull identity facts.
            if kind>k::REMOTE{continue;}
            let index=if let Some(&index)=self.indices.get(&(kind,id)){index}else{
                let Some(index)=self.free.pop()else{self.refused+=1;continue;};self.indices.insert((kind,id),index);index
            };
            self.facts[index].copy_from_slice(row);
        }
    }
    // Reliable bootstrap is cold. Repeated input receipts instead borrow
    // strings and fixed arrays directly from their current transport message.
    fn text(&mut self,bytes:&[u8],now:f64){
        if bytes.len()>262144{self.fault=Some("server message exceeds bounds");return;}
        let Ok(m)=serde_json::from_slice::<Message>(bytes)else{self.fault=Some("invalid server message");return;};
        match m.kind{
            "entity-registry"=>{
                if self.registry{self.fault=Some("duplicate entity registry");return;}
                let Ok(value)=serde_json::from_slice::<serde_json::Value>(bytes)else{return;};let mut wanted=self.expected.clone();
                // Epoch is an authority lease, not authored content. All table
                // names and input profiles still have to match local content.
                let epoch=value["registry"]["contentEpoch"].as_u64().unwrap_or(0);if epoch==0||epoch>u32::MAX as u64{self.fault=Some("invalid content epoch");return;}
                wanted["contentEpoch"]=epoch.into();
                if value["registry"]!=wanted{self.fault=Some("entity content/input registry mismatch");return;}self.registry=true;
            }
            // Registry ordering is a protocol prerequisite. An authenticated
            // connection is not yet subscribed until Client sends start.
            "welcome"=>{if !self.registry||self.welcomed{self.fault=Some("unexpected battle welcome");return;}self.welcomed=true;self.spectator=m.spectator;self.own.id=m.player_id.unwrap_or(-1.);self.epoch=m.epoch.unwrap_or(1.);if let Some(t)=m.tick{self.latest=t;self.estimator.arrive(t,now/1000.,0.);}}
            "input-ack"=>{if m.epoch==Some(self.epoch)&&m.generation==Some(self.own.generation){self.ack=self.ack.max(m.ack.unwrap_or(0.));if let Some(t)=m.server_tick{self.latest=self.latest.max(t);self.estimator.arrive(t,now/1000.,0.);}}}
            "entity-edge-results"=>{if m.epoch==Some(self.epoch)&&m.generation==Some(self.own.generation){for &id in &m.received.values[..m.received.count]{self.received.add(id);}for &id in &m.results.0.values[..m.results.0.count]{self.outcomes.add(id);}}}
            "gameplay-error"|"version-error"=>{let _=m.message;self.fault=Some("server rejected battle admission");}
            _=>{},
        }
    }
    pub fn active(&self)->impl Iterator<Item=&[f64;r::STRIDE]>{self.indices.values().map(|&index|&self.facts[index])}
}
