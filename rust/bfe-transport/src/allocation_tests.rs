//! First-use allocation accounting includes protection, receive, stream
//! assembly, ACKs, loss recovery and key rotation. No fixture observer copies
//! a message or grows a queue while the protocol reservation is measured.
use crate::{crypto::Aead,handshake::Keys,connection::{Connection,Sink,Lane},layout::limits as lim};
use std::{alloc::{GlobalAlloc,Layout,System},cell::Cell,net::SocketAddr};
thread_local!{static TRACK:Cell<bool>=const{Cell::new(false)};static ALLOCS:Cell<usize>=const{Cell::new(0)};}
struct Counting;
unsafe impl GlobalAlloc for Counting{
 unsafe fn alloc(&self,l:Layout)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.alloc(l)}
 unsafe fn dealloc(&self,p:*mut u8,l:Layout){System.dealloc(p,l)}
 unsafe fn realloc(&self,p:*mut u8,l:Layout,n:usize)->*mut u8{if TRACK.try_with(Cell::get).unwrap_or(false){ALLOCS.with(|n|n.set(n.get()+1));}System.realloc(p,l,n)}
}
#[global_allocator]static ALLOCATOR:Counting=Counting;
struct Wire{bytes:Vec<[u8;lim::MAX_DATAGRAM]>,lengths:Vec<usize>,count:usize,messages:usize}
impl Wire{fn new()->Self{Self{bytes:vec![[0;lim::MAX_DATAGRAM];512],lengths:vec![0;512],count:0,messages:0}}}
impl Sink for Wire{
 fn packet(&mut self,bytes:&[u8],_:SocketAddr){assert!(self.count<self.bytes.len());self.bytes[self.count][..bytes.len()].copy_from_slice(bytes);self.lengths[self.count]=bytes.len();self.count+=1;}
 fn message(&mut self,_:Lane,_:bool,bytes:&[u8]){assert!(bytes.iter().all(|&n|n==73));self.messages+=1;}
}
fn transfer(a:&mut Connection,b:&mut Connection,up:&mut Wire,down:&mut Wire,at:f64,drop:bool){
 let path=SocketAddr::from(([127,0,0,1],18451));
 for pass in 0..4{
  let n=up.count;for i in 0..n{if !drop||(i+pass)%17!=0{b.receive(&up.bytes[i][..up.lengths[i]],path,at,down);}}up.count=0;
  let n=down.count;for i in 0..n{if !drop||(i+pass)%19!=0{a.receive(&down.bytes[i][..down.lengths[i]],path,at,up);}}down.count=0;
  if up.count==0{break;}
 }
}
#[test]
fn first_packet_streams_loss_and_rekey_use_only_reserved_storage(){
 let path=SocketAddr::from(([127,0,0,1],18451));let c=Aead::new([7;32]).unwrap();let s=Aead::new([9;32]).unwrap();
 let mut a=Connection::new(Keys{send:c.clone(),receive:s.clone(),local:1,remote:2},path,0.,100.);
 let mut b=Connection::new(Keys{send:s,receive:c,local:2,remote:1},path,0.,100.);a.fixture_key_interval(64);b.fixture_key_interval(64);
 let mut up=Wire::new();let mut down=Wire::new();let payload=[73;8000];
 ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 for tick in 0..1000{
  let now=tick as f64*17.;a.queue(Lane::State,false,&payload[..80]).unwrap();
  if tick%3==0{a.queue(Lane::Events,false,&payload[..2048]).unwrap();}if tick%10==0{b.queue(Lane::Bulk,false,&payload).unwrap();}
  a.poll(now,&mut up).unwrap();b.poll(now,&mut down).unwrap();transfer(&mut a,&mut b,&mut up,&mut down,now,tick%5==0);
 }
 for tick in 1000..1400{let now=tick as f64*17.;a.poll(now,&mut up).unwrap();b.poll(now,&mut down).unwrap();transfer(&mut a,&mut b,&mut up,&mut down,now,false);}
 TRACK.with(|n|n.set(false));assert_eq!(ALLOCS.with(Cell::get),0);assert_eq!(a.backlog()+b.backlog(),0);
 assert!(a.closed.is_none()&&b.closed.is_none());assert!(up.messages>0&&down.messages>0);
}
#[test]
fn paused_state_queue_and_reliable_byte_limit_refuse_without_growth(){
 let path=SocketAddr::from(([127,0,0,1],18451));let mut c=Connection::new(Keys{send:Aead::new([7;32]).unwrap(),receive:Aead::new([9;32]).unwrap(),local:1,remote:2},path,0.,100.);
 let mut sink=Wire::new();let payload=[73;65536];ALLOCS.with(|n|n.set(0));TRACK.with(|n|n.set(true));
 for _ in 0..5000{c.queue(Lane::State,false,&payload[..1100]).unwrap();}c.flush(0.,&mut sink).unwrap();sink.count=0;
 let mut refused=false;for _ in 0..100{if c.queue(Lane::Bulk,false,&payload).is_err(){refused=true;break;}}
 c.flush(1.,&mut sink).unwrap();TRACK.with(|n|n.set(false));assert!(refused);assert_eq!(c.closed.unwrap().code,2);assert_eq!(ALLOCS.with(Cell::get),0);
}
