//! connection.js's independent SendStream and ReceiveStream owners. Absolute
//! offsets address retained rings; message delivery borrows one assembly bank.
use crate::{Error,Result,codec::{Ranges,Reader,Writer},layout::limits as lim};
// Half-open stream byte span [offset, offset + length) emitted into a STREAM frame.
#[derive(Clone,Copy,Default,Debug)]
pub struct Span{pub offset:u64,pub length:usize,pub retransmit:bool}
// Absolute offsets wrap only at the physical copy. Acknowledgements and
// peer-visible stream offsets retain their full monotonically increasing value.
// Growing `ring.len()` lazily within the pre-reserved `MAX_STREAM_BUFFER` capacity
// avoids faulting unused physical pages into RSS while keeping steady-state writes allocation-free.
fn put_ring(ring:&mut Vec<u8>,offset:u64,bytes:&[u8]){
    let at=offset as usize%lim::MAX_STREAM_BUFFER;let first=bytes.len().min(lim::MAX_STREAM_BUFFER-at);
    let end1=at+first;if ring.len()<end1{ring.resize(end1,0);}
    ring[at..end1].copy_from_slice(&bytes[..first]);
    let second=bytes.len()-first;
    if second>0{if ring.len()<second{ring.resize(second,0);}ring[..second].copy_from_slice(&bytes[first..]);}
}
// Fixed-capacity send ring tracking written, sent, acknowledged, and lost byte ranges.
pub struct SendStream{bytes:Vec<u8>,pub written:u64,pub sent:u64,pub base:u64,acked:Ranges,lost:Ranges,pub lost_bytes:usize}
impl Default for SendStream{fn default()->Self{Self::new()}}
impl SendStream{
    pub fn new()->Self{Self{bytes:Vec::with_capacity(lim::MAX_STREAM_BUFFER),written:0,sent:0,base:0,acked:Ranges::new(lim::MAX_SENT_PACKETS*2),lost:Ranges::new(lim::MAX_SENT_PACKETS*2),lost_bytes:0}}
    pub fn pending(&self)->usize{(self.written-self.sent)as usize+self.lost_bytes}
    // Releases the stream ring once the owning connection has closed and entered draining.
    pub(crate) fn release_closed_buffers(&mut self){self.bytes=Vec::new();self.lost_bytes=0;}
    // Prefix and payload are admitted together. A full stream cannot retain
    // half a message whose remaining bytes could never be acknowledged.
    pub fn write(&mut self,bytes:&[u8],text:bool)->Result<()>{
        let mut header=[0;9];let mut w=Writer::new(&mut header);w.varint(bytes.len()as u64)?;w.byte(text as u8)?;let n=w.at;
        if bytes.len()>lim::MAX_MESSAGE||self.written-self.base+(n+bytes.len())as u64>lim::MAX_STREAM_BUFFER as u64{return Err(Error("reliable data not acknowledged"));}
        if self.written+(n+bytes.len())as u64>lim::MAX_SAFE_INTEGER{return Err(Error("stream offsets exhausted"));}
        put_ring(&mut self.bytes,self.written,&header[..n]);self.written+=n as u64;
        put_ring(&mut self.bytes,self.written,bytes);self.written+=bytes.len()as u64;Ok(())
    }
    // Lost bytes have priority and do not wait for another window opening.
    // A piece only advances bookkeeping after its bounds have been checked.
    pub fn take(&mut self,max:usize,new_max:usize)->Result<Option<Span>>{
        if max==0{return Ok(None);}
        if let Some([a,b])=self.lost.first(){let n=(b-a).min(max as u64)as usize;
            if !self.lost.remove(a,a+n as u64){return Err(Error("stream range capacity exceeded"));}
            self.lost_bytes-=n;return Ok(Some(Span{offset:a,length:n,retransmit:true}));
        }
        let n=(self.written-self.sent).min(max.min(new_max)as u64)as usize;if n==0{return Ok(None);}
        let span=Span{offset:self.sent,length:n,retransmit:false};self.sent+=n as u64;Ok(Some(span))
    }
    // Sent metadata refers only to retained bytes. Copying a retransmission
    // from released space would disclose a newer message under an old offset.
    pub fn copy(&self,span:Span,out:&mut Writer<'_>)->Result<()>{
        if span.offset<self.base||span.offset+span.length as u64>self.written{return Err(Error("invalid stream span"));}
        let at=span.offset as usize%lim::MAX_STREAM_BUFFER;let first=span.length.min(lim::MAX_STREAM_BUFFER-at);
        out.span(&self.bytes[at..at+first])?;out.span(&self.bytes[..span.length-first])
    }
    // A contiguous acknowledged prefix releases physical storage. Sparse
    // acknowledgements remain ranges until the preceding hole is repaired.
    pub fn ack(&mut self,span:Span)->Result<()>{
        let a=span.offset.max(self.base);let b=span.offset+span.length as u64;if b<=a{return Ok(());}
        if !self.acked.add(a,b)||!self.lost.remove(a,b){return Err(Error("stream range capacity exceeded"));}
        if let Some([lo,hi])=self.acked.first(){if lo<=self.base&&hi>self.base{self.base=hi;self.acked.trim(hi);}}
        self.lost_bytes=self.lost.rows().iter().map(|[a,b]|(b-a)as usize).sum();Ok(())
    }
    // Repeated loss reports overlap both ACKed and already-lost spans. Queue
    // only their missing union, so another PTO cannot multiply pending bytes.
    pub fn loss(&mut self,span:Span)->Result<()>{
        let mut cursor=span.offset.max(self.base);let end=span.offset+span.length as u64;
        for &[a,b] in self.acked.rows(){
            if b<=cursor{continue;}
            if a>=end{break;}
            if a>cursor&&!self.lost.add(cursor,a.min(end)){return Err(Error("stream range capacity exceeded"));}cursor=cursor.max(b);
        }
        if cursor<end&&!self.lost.add(cursor,end){return Err(Error("stream range capacity exceeded"));}
        self.lost_bytes=self.lost.rows().iter().map(|[a,b]|(b-a)as usize).sum();Ok(())
    }
}
// Incremental parser for varint-length + kind framed messages on an ordered stream.
struct Message{header:[u8;9],header_len:usize,bytes:Vec<u8>,used:usize,need:Option<usize>,text:bool}
impl Message{
    fn new()->Self{Self{header:[0;9],header_len:0,bytes:Vec::with_capacity(lim::MAX_MESSAGE),used:0,need:None,text:false}}
    // Header bytes may themselves cross packets and ring boundaries. Hold
    // at most nine before a checked message length selects the payload bank.
    fn feed(&mut self,mut bytes:&[u8],deliver:&mut impl FnMut(bool,&[u8]))->Result<()>{
        while !bytes.is_empty(){
            if self.need.is_none(){
                self.header[self.header_len]=bytes[0];self.header_len+=1;bytes=&bytes[1..];
                let header=(1usize<<(self.header[0]>>6))+1;if self.header_len<header{continue;}
                let mut r=Reader::new(&self.header[..self.header_len]);let n=r.varint()?;let kind=r.byte()?;
                if n>lim::MAX_MESSAGE as u64||kind>1{return Err(Error("invalid stream message"));}
                let need=n as usize;if self.bytes.len()<need{self.bytes.resize(need,0);}
                self.need=Some(need);self.text=kind!=0;self.used=0;self.header_len=0;
                if n==0{deliver(self.text,&[]);self.need=None;}continue;
            }
            let Some(need)=self.need else{return Err(Error("stream message length field missing"));};
            let n=bytes.len().min(need-self.used);
            self.bytes[self.used..self.used+n].copy_from_slice(&bytes[..n]);self.used+=n;bytes=&bytes[n..];
            if self.used==need{deliver(self.text,&self.bytes[..need]);self.need=None;self.used=0;}
        }Ok(())
    }
}
// Out-of-order stream reassembly ring with bounded segment and piece reservations.
pub struct ReceiveStream{bytes:Vec<u8>,pub offset:u64,runs:Ranges,parts:Vec<[u64;2]>,part_count:usize,message:Message}
impl Default for ReceiveStream{fn default()->Self{Self::new()}}
impl ReceiveStream{
    pub fn new()->Self{Self{bytes:Vec::with_capacity(lim::MAX_STREAM_BUFFER),offset:0,runs:Ranges::new(lim::MAX_STREAM_SEGMENTS),parts:vec![[0;2];lim::MAX_STREAM_PARTS],part_count:0,message:Message::new()}}
    // Releases the reassembly ring and message assembly bank once closed.
    pub(crate) fn release_closed_buffers(&mut self){self.bytes=Vec::new();self.parts=Vec::new();self.part_count=0;self.message.bytes=Vec::new();}
    // Physical piece count bounds scattered overlap work separately from
    // merged runs. In-order messages never consume this reservation.
    fn store_piece(&mut self,a:u64,b:u64,start:u64,bytes:&[u8])->Result<()>{
        if b<=a{return Ok(());}
        if self.part_count==self.parts.len(){return Err(Error("stream reassembly limit exceeded"));}
        self.parts[self.part_count]=[a,b];self.part_count+=1;
        put_ring(&mut self.bytes,a,&bytes[(a-start)as usize..(b-start)as usize]);Ok(())
    }
    // Bytes below delivery are duplicates. Bytes beyond the fixed horizon
    // are protocol failure, rather than a request to enlarge reassembly.
    pub fn receive(&mut self,mut start:u64,mut bytes:&[u8],deliver:&mut impl FnMut(bool,&[u8]))->Result<()>{
        let end=start.checked_add(bytes.len()as u64).filter(|&n|n<=lim::MAX_SAFE_INTEGER).ok_or(Error("invalid stream offset"))?;
        if end<=self.offset{return Ok(());}
        if end-self.offset>lim::MAX_STREAM_BUFFER as u64{return Err(Error("stream buffer exceeded"));}
        if start<self.offset{bytes=&bytes[(self.offset-start)as usize..];start=self.offset;}
        if start>self.offset{
            // Preserve the first out-of-order copy of overlapping bytes, as
            // ReceiveStream.store did. Runs and physical pieces have separate
            // caps so scattered single-byte writes cannot cause quadratic growth.
            let mut cursor=start;let n=self.runs.len();
            for i in 0..n{
                let [a,b]=self.runs.rows()[i];
                if b<=cursor{continue;}
                if a>=end{break;}
                if a>cursor{self.store_piece(cursor,a.min(end),start,bytes)?;}cursor=cursor.max(b);
            }
            if cursor<end{self.store_piece(cursor,end,start,bytes)?;}
            if !self.runs.add(start,end){return Err(Error("stream reassembly limit exceeded"));}return Ok(());
        }
        // The contiguous arrival wins any overlap at the delivery cursor,
        // matching ReceiveStream.receive before it drains stored runs.
        self.message.feed(bytes,deliver)?;self.offset=end;
        while let Some([a,b])=self.runs.first(){
            if a>self.offset{break;}
            if b>self.offset{let at=self.offset as usize%lim::MAX_STREAM_BUFFER;let count=(b-self.offset)as usize;let first=count.min(lim::MAX_STREAM_BUFFER-at);
                self.message.feed(&self.bytes[at..at+first],deliver)?;self.message.feed(&self.bytes[..count-first],deliver)?;self.offset=b;
            }
            self.runs.trim(self.offset);
        }
        // Release piece accounting as soon as its containing run is drained.
        // A long healthy session must not exhaust the out-of-order budget.
        let mut kept=0;for i in 0..self.part_count{if self.parts[i][1]>self.offset{self.parts[kept]=self.parts[i];kept+=1;}}self.part_count=kept;Ok(())
    }
}
// Verify ring wraparound, out-of-order overlap, selective retransmission, and frame bounds.
#[cfg(test)]mod tests{
    use super::*;
    // Exercise 50,000 messages across multiple ring wraps with overlapping out-of-order slices.
    #[test]fn ordered_messages_survive_wrapping_reordering_and_overlap()->std::result::Result<(),Box<dyn std::error::Error>>{
        let mut tx=SendStream::new();let mut rx=ReceiveStream::new();let mut out=[0;100];let mut count=0;
        for n in 0..50000{
            let payload=[n as u8;90];tx.write(&payload,n%2==0)?;let span=tx.take(100,100)?.ok_or("expected span")?;let mut w=Writer::new(&mut out);tx.copy(span,&mut w)?;let length=w.at;
            let mut deliver=|text:bool,b:&[u8]|{assert_eq!(text,n%2==0);assert_eq!(b,payload);count+=1;};
            rx.receive(span.offset+20,&out[20..length],&mut deliver)?;rx.receive(span.offset+30,&out[30..length],&mut deliver)?;
            rx.receive(span.offset,&out[..25],&mut deliver)?;tx.ack(span)?;
        }
        assert_eq!(count,50000);assert_eq!(tx.base,tx.written);assert_eq!(rx.offset,tx.written);Ok(())
    }
    // Verify that loss declarations skip already-acknowledged prefix spans.
    #[test]fn sender_retransmits_only_unacknowledged_ranges()->std::result::Result<(),Box<dyn std::error::Error>>{
        let mut tx=SendStream::new();tx.write(&[1;100],false)?;let a=tx.take(30,30)?.ok_or("expected span a")?;let b=tx.take(72,72)?.ok_or("expected span b")?;tx.ack(a)?;tx.loss(a)?;tx.loss(b)?;
        assert_eq!(tx.lost_bytes,72);let r=tx.take(20,0)?.ok_or("expected retransmit span")?;assert!(r.retransmit);assert_eq!(r.offset,b.offset);tx.ack(b)?;assert_eq!(tx.lost_bytes,0);Ok(())
    }
    // Ensure offsets beyond MAX_STREAM_BUFFER and lengths above MAX_MESSAGE are rejected.
    #[test]fn malformed_stream_spans_and_messages_are_bounded()->std::result::Result<(),Box<dyn std::error::Error>>{
        let mut rx=ReceiveStream::new();assert!(rx.receive(lim::MAX_STREAM_BUFFER as u64,&[1],&mut|_,_|{}).is_err());
        let mut bytes=[0;10];let mut w=Writer::new(&mut bytes);w.varint(lim::MAX_MESSAGE as u64+1)?;w.byte(0)?;let n=w.at;
        assert!(rx.receive(0,&bytes[..n],&mut|_,_|{}).is_err());assert_eq!(crate::codec::varint_size(0)?,1);Ok(())
    }
}
