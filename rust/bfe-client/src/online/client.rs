//! One native frame receives packets, consumes the input batch and publishes
//! an own camera. No authoritative World or Node host is created by a join.
use crate::{scene::{MapCamera,View},host::World,generated_input::{ClientCommand,ClientLook,ClientFlightCommand},input::ViewActions};
use super::{receiver::Receiver,input::Sender,replicas::Replicas,info};
use bfe_transport::{endpoint::Endpoint,connection::Lane};
use bfe_presentation::{aim::View as AimView,cameras::math::V8};
use std::{path::Path,time::Duration};
pub struct Client{pub receiver:Receiver,pub sender:Sender,pub endpoint:Endpoint,pub camera:MapCamera,dt:f64,started:bool,pub frames:u64}
impl Client{
    pub fn connect(address:&str,server:&info::Server,root:&Path,identity_path:&Path,name:&str,resources:&Replicas,camera:MapCamera)->Result<Self,String>{
        // NetworkSession computes the local gameplay hash independently from
        // /info. A mismatch remains a spectator decision made by the server.
        let files=bfe_content::files::DirFiles::new(root);let hash=bfe_content_native::fingerprint::hash(&files,&server.query)?;
        let identity=info::identity(identity_path)?;let hello=info::Server::hello(&hash,name);let mut receiver=Receiver::new(resources.descriptor.clone());
        let mut endpoint=Endpoint::connect(address,identity,server.identity()?,&hello).map_err(|e|e.to_string())?;
        // Loading/admission is complete before the socket opens. Early state
        // still retains its original receipt time while welcome is in flight.
        endpoint.establish(Duration::from_secs(30),&mut|lane,text,bytes,now|{if text||lane==Lane::State{receiver.message(text,bytes,now);}}).map_err(|e|e.to_string())?;
        info::authenticated(&endpoint.server_hello)?;
        if let Some(error)=receiver.fault{return Err(error.into());}
        Ok(Self{receiver,sender:Sender::new(&resources.turns),endpoint,camera,dt:0.,started:false,frames:0})
    }
    // Packet receipts use the endpoint monotonic clock, not display time.
    // Poll even when focus pauses generated input, so the lease stays alive.
    pub fn receive(&mut self,dt:f64)->Result<(),String>{
        self.dt=dt.clamp(0.,8./60.);let receiver=&mut self.receiver;
        self.endpoint.pump(&mut|lane,text,bytes,now|{if text||lane==Lane::State{receiver.message(text,bytes,now);}}).map_err(|e|e.to_string())?;
        if let Some(error)=receiver.fault{return Err(error.into());}
        // NetworkSession calls start after welcome; authentication alone does
        // not subscribe a client to the server's snapshot publication owner.
        if receiver.welcomed&&!self.started{
            self.started=self.endpoint.send(Lane::Events,true,br#"{"type":"call","id":1,"method":"start","args":{}}"#).map_err(|e|e.to_string())?;
        }
        self.sender.feedback(receiver);receiver.clock.advance(self.dt).map_err(str::to_string)?;Ok(())
    }
    // A queued input and its durable edges share one transport flush.
    // The sender advances delivery accounting only after queue admission.
    pub fn send(&mut self)->Result<(),String>{
        self.sender.spawn(&self.receiver);self.sender.send(&mut self.endpoint,&mut self.receiver)?;self.endpoint.flush().map_err(|e|e.to_string())
    }
    // Timed diagnostic output is cold: formatting never enters a frame.
    pub fn summary(&self)->serde_json::Value{
        let own=self.receiver.own;serde_json::json!({"frames":self.frames,"identity":self.endpoint.identity.iter().map(|b|format!("{b:02x}")).collect::<String>(),"welcomed":self.receiver.welcomed,"playerId":own.id,"generation":own.generation,"alive":own.alive,"position":own.position,"packets":self.receiver.packets,"corrections":self.receiver.corrections,"sent":self.sender.sent,"fireEdges":self.sender.fire_edges,"refused":self.receiver.refused+self.sender.refused})
    }
}
impl World for Client{
    type Frame=View;
    fn generation(&self)->u32{self.receiver.own.generation as u32}
    fn step(&mut self,c:&ClientCommand,look:&ClientLook,_:&ClientFlightCommand,_:&ViewActions,_:f64)->Result<(),String>{
        if !self.receiver.own.alive&&c.actions&1!=0{self.sender.spawned=false;}
        let rtt=self.endpoint.connection.as_ref().map_or(100.,|c|c.rtt);self.sender.tick(c,look,&self.receiver,rtt,self.endpoint.now());Ok(())
    }
    fn present(&mut self,alpha:f64)->&View{
        self.frames+=1;self.sender.preview(&self.receiver);let own=self.receiver.own;
        // AimView supplies the canonical first-person eye for posture/air.
        // Preview replays only uncorrected pointer deltas on the checkpoint.
        if self.receiver.corrections!=0{
            let position=three::Vector3::new(own.position[0],own.position[1],own.position[2]);
            let aim=AimView{position,feet:position,yaw:self.sender.yaw,pitch:self.sender.pitch,posture:own.posture,air:own.air!=0,zoom:own.zoom,..Default::default()}.sample::<V8>();
            self.camera.follow([aim.eye.x,aim.eye.y,aim.eye.z],self.sender.yaw,self.sender.pitch,self.dt);
        }self.camera.present(alpha)
    }
}
impl Drop for Client{
    fn drop(&mut self){self.endpoint.close(0,b"left battle");let _=self.endpoint.flush();}
}
