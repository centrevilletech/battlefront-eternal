//! connection.js::packDatagrams/fragment: state is replaceable and never
//! retransmitted. The byte budget covers 512 ordinary game datagrams; unusually
//! large queued state evicts older messages instead of reserving their maximum
//! possible size in every slot on a small-memory client.
use crate::{Error,Result,codec::{Writer,varint_size},layout::{limits as l,data,frame}};
const PLAIN:usize=l::MAX_DATAGRAM-data::BYTES-l::TAG;
#[derive(Clone,Copy,Default)]
struct Message{start:usize,length:usize,text:bool,fragment:u64,index:usize,count:usize}
pub struct Queue{bytes:Vec<u8>,rows:Vec<Message>,head:usize,count:usize,tail:usize,used:usize,next:u64,pub dropped:u64}
impl Default for Queue{fn default()->Self{Self::new()}}
impl Queue{
    pub fn new()->Self{Self{bytes:vec![0;l::MAX_QUEUED_DATAGRAMS*l::MAX_DATAGRAM],rows:vec![Message::default();l::MAX_QUEUED_DATAGRAMS],head:0,count:0,tail:0,used:0,next:0,dropped:0}}
    pub fn is_empty(&self)->bool{self.count==0}
    fn pop(&mut self){if self.count>0{self.used-=self.rows[self.head].length;self.head=(self.head+1)%self.rows.len();self.count-=1;}}
    // Counts and bytes have independent caps. Eviction always removes one
    // complete old message, including any unsent remainder of its fragments.
    pub fn push(&mut self,bytes:&[u8],text:bool)->Result<()>{
        if bytes.len()>l::MAX_FRAGMENTS*l::FRAGMENT_BYTES{return Err(Error("state message too large"));}
        while self.count==self.rows.len()||self.used+bytes.len()>self.bytes.len(){self.pop();self.dropped+=1;}
        let first=bytes.len().min(self.bytes.len()-self.tail);self.bytes[self.tail..self.tail+first].copy_from_slice(&bytes[..first]);self.bytes[..bytes.len()-first].copy_from_slice(&bytes[first..]);
        let slot=(self.head+self.count)%self.rows.len();self.rows[slot]=Message{start:self.tail,length:bytes.len(),text,..Default::default()};
        self.tail=(self.tail+bytes.len())%self.bytes.len();self.used+=bytes.len();self.count+=1;Ok(())
    }
    fn copy(&self,offset:usize,count:usize,out:&mut Writer<'_>)->Result<()>{
        let at=offset%self.bytes.len();let first=count.min(self.bytes.len()-at);out.span(&self.bytes[at..at+first])?;out.span(&self.bytes[..count-first])
    }
    // A fragment keeps its place until all parts have left. ACK-only packets
    // can precede a full-sized part; truncating a part to fit an ACK would
    // change the count without changing its message boundary.
    pub fn frame(&mut self,out:&mut Writer<'_>)->Result<Option<bool>>{
        if self.count==0{return Ok(None);}let mut row=self.rows[self.head];
        if row.count==0&&2+varint_size(row.length as u64)?+row.length<=out.remaining(){
            out.byte(frame::DATAGRAM as u8)?;out.byte(row.text as u8)?;out.varint(row.length as u64)?;self.copy(row.start,row.length,out)?;self.pop();return Ok(Some(true));
        }
        if row.count==0{
            if 2+varint_size(row.length as u64)?+row.length<=PLAIN-48{return Ok(None);}
            row.count=row.length.div_ceil(l::FRAGMENT_BYTES);row.fragment=self.next;
            if self.next>=l::MAX_SAFE_INTEGER as u64{return Err(Error("datagram identifiers exhausted"));}self.next+=1;
        }
        let offset=row.index*l::FRAGMENT_BYTES;let count=(row.length-offset).min(l::FRAGMENT_BYTES);
        if 4+varint_size(row.fragment)?+varint_size(count as u64)?+count>out.remaining(){self.rows[self.head]=row;return Ok(None);}
        out.byte(frame::FRAGMENT as u8)?;out.byte(row.text as u8)?;out.varint(row.fragment)?;out.byte(row.index as u8)?;out.byte(row.count as u8)?;out.varint(count as u64)?;self.copy(row.start+offset,count,out)?;
        row.index+=1;let done=row.index==row.count;if done{self.pop();}else{self.rows[self.head]=row;}Ok(Some(done))
    }
}
struct Assembly{id:Option<u64>,at:f64,count:usize,bits:u32,lengths:[usize;l::MAX_FRAGMENTS],bytes:Vec<u8>}
// Reassembly reserves every permitted part before first use. The final
// contiguous assembly is borrowed only for the synchronous delivery callback.
pub struct Fragments{slots:Vec<Assembly>,whole:Vec<u8>}
impl Default for Fragments{fn default()->Self{Self::new()}}
impl Fragments{
    pub fn new()->Self{Self{slots:(0..l::MAX_PENDING_FRAGMENTS).map(|_|Assembly{id:None,at:0.,count:0,bits:0,lengths:[0;l::MAX_FRAGMENTS],bytes:vec![0;l::MAX_FRAGMENTS*PLAIN]}).collect(),whole:vec![0;l::MAX_FRAGMENTS*PLAIN]}}
    // A changed fragment count or repeated index cannot overwrite the first
    // accepted assembly. Timeout and oldest eviction keep stalled peers bounded.
    pub fn receive(&mut self,id:u64,index:usize,count:usize,text:bool,bytes:&[u8],now:f64,deliver:&mut impl FnMut(bool,&[u8]))->Result<bool>{
        if count==0||count>l::MAX_FRAGMENTS||index>=count||bytes.len()>PLAIN{return Err(Error("invalid fragment"));}
        for row in &mut self.slots{if row.id.is_some()&&now-row.at>l::FRAGMENT_LIFETIME as f64{row.id=None;}}
        let slot=self.slots.iter().position(|row|row.id==Some(id)).or_else(||self.slots.iter().position(|row|row.id.is_none()))
            .unwrap_or_else(||self.slots.iter().enumerate().min_by(|(_,a),(_,b)|a.at.total_cmp(&b.at)).map(|(i,_)|i).unwrap_or(0));
        let row=&mut self.slots[slot];
        if row.id!=Some(id){row.id=Some(id);row.at=now;row.count=count;row.bits=0;row.lengths.fill(0);}
        if row.count!=count||row.bits&(1u32<<index)!=0{return Ok(false);}
        row.bytes[index*PLAIN..index*PLAIN+bytes.len()].copy_from_slice(bytes);row.lengths[index]=bytes.len();row.bits|=1u32<<index;
        let mask=if count==32{u32::MAX}else{(1u32<<count)-1};if row.bits!=mask{return Ok(false);}
        let mut n=0;for i in 0..count{let length=row.lengths[i];self.whole[n..n+length].copy_from_slice(&row.bytes[i*PLAIN..i*PLAIN+length]);n+=length;}
        row.id=None;deliver(text,&self.whole[..n]);Ok(true)
    }
}
