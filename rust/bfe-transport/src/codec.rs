//! codec.js::readVarint/writeVarint and RangeSet, with fixed reservations.
//! A packet parser never extends a buffer or turns an integer into an index
//! before checking its full span. Errors belong to the connection boundary.
use crate::{Error, Result};
use crate::layout::limits as lim;

// QUIC selects width from the value, then stores that width in the high
// two bits. Refuse values beyond JS safe integers on both hosts.
// codec.js varintSize rejects values outside the wire integer domain.
// Validation happens before a writer can publish any part of that integer.
pub fn varint_size(value:u64)->Result<usize>{
    if value<64{Ok(1)}else if value<16384{Ok(2)}else if value<1073741824{Ok(4)}
    else if value<=lim::MAX_SAFE_INTEGER as u64{Ok(8)}else{Err(Error("varint too large"))}
}
// Cursor advances only after the complete span is known. A truncated field
// cannot leave its caller with a partially consumed length prefix.
pub struct Reader<'a>{pub bytes:&'a[u8],pub at:usize}
impl<'a> Reader<'a>{
    pub fn new(bytes:&'a[u8])->Self{Self{bytes,at:0}}
    pub fn remaining(&self)->usize{self.bytes.len()-self.at}
    pub fn span(&mut self,n:usize)->Result<&'a[u8]>{
        let end=self.at.checked_add(n).filter(|&end|end<=self.bytes.len()).ok_or(Error("truncated frame"))?;
        let bytes=&self.bytes[self.at..end];self.at=end;Ok(bytes)
    }
    // Keep wire lengths wide until they fit the actual datagram. This also
    // makes the same parser safe on hosts whose usize is narrower than u64.
    // Narrow only after comparing with the available byte prefix. A forged
    // wire length must not wrap when converted to the platform index width.
    pub fn wide_span(&mut self,n:u64)->Result<&'a[u8]>{
        if n>self.remaining()as u64{return Err(Error("truncated frame"));}self.span(n as usize)
    }
    pub fn byte(&mut self)->Result<u8>{Ok(self.span(1)?[0])}
    // The first two bits select QUIC integer width. The remaining bytes
    // must all exist before a malformed datagram can advance this reader.
    pub fn varint(&mut self)->Result<u64>{
        let first=self.byte()?;let length=1usize<<(first>>6);let mut value=(first&63)as u64;
        for &byte in self.span(length-1)?{value=(value<<8)|byte as u64;}
        if value>lim::MAX_SAFE_INTEGER as u64{return Err(Error("varint too large"));}Ok(value)
    }
}
// Packet assembly borrows its destination. No temporary field buffer can
// outlive sealing, and capacity refusal never reallocates the packet.
pub struct Writer<'a>{pub bytes:&'a mut[u8],pub at:usize}
impl<'a> Writer<'a>{
    pub fn new(bytes:&'a mut[u8])->Self{Self{bytes,at:0}}
    pub fn remaining(&self)->usize{self.bytes.len()-self.at}
    pub fn span(&mut self,bytes:&[u8])->Result<()>{
        let end=self.at.checked_add(bytes.len()).filter(|&end|end<=self.bytes.len()).ok_or(Error("packet exceeds bounds"))?;
        self.bytes[self.at..end].copy_from_slice(bytes);self.at=end;Ok(())
    }
    pub fn byte(&mut self,value:u8)->Result<()>{self.span(&[value])}
    // Leading payload bits are known clear from varint_size. Adding the
    // width tag cannot erase any of the integer's significant bits.
    pub fn varint(&mut self,value:u64)->Result<()>{
        let length=varint_size(value)?;let mut bytes=value.to_be_bytes();
        bytes[8-length]|=match length{1=>0,2=>64,4=>128,_=>192};self.span(&bytes[8-length..])
    }
}

// Sorted disjoint half-open intervals retain their storage through reset.
// Received packet numbers may discard old history; reliable byte ranges may
// never discard an acknowledgement, so their callers handle refusal explicitly.
pub struct Ranges{rows:Vec<[u64;2]>,len:usize}
impl Ranges{
    pub fn new(capacity:usize)->Self{Self{rows:vec![[0;2];capacity],len:0}}
    // Closing a battle clears occupancy without returning its reservation.
    // A later peer cannot accidentally inherit old acknowledged ranges.
    pub fn clear(&mut self){self.len=0;}
    pub fn len(&self)->usize{self.len}
    pub fn is_empty(&self)->bool{self.len==0}
    pub fn rows(&self)->&[[u64;2]]{&self.rows[..self.len]}
    pub fn first(&self)->Option<[u64;2]>{self.rows().first().copied()}
    pub fn last(&self)->Option<[u64;2]>{self.rows().last().copied()}
    pub fn contains(&self,n:u64)->bool{self.rows().binary_search_by(|[a,b]|if n<*a{std::cmp::Ordering::Greater}else if n>=*b{std::cmp::Ordering::Less}else{std::cmp::Ordering::Equal}).is_ok()}
    pub fn covers(&self,a:u64,b:u64)->bool{self.rows().iter().any(|r|r[0]<=a&&r[1]>=b)}
    // Touching intervals merge too: stream offsets and packet numbers are
    // half-open, so [a,b) followed by [b,c) is one contiguous run.
    pub fn add(&mut self,mut a:u64,mut b:u64)->bool{
        if b<=a{return true;}
        let mut i=0;while i<self.len&&self.rows[i][1]<a{i+=1;}
        let mut end=i;while end<self.len&&self.rows[end][0]<=b{a=a.min(self.rows[end][0]);b=b.max(self.rows[end][1]);end+=1;}
        if end==i{
            if self.len==self.rows.len(){return false;}
            self.rows.copy_within(i..self.len,i+1);self.len+=1;
        }else{self.rows.copy_within(end..self.len,i+1);self.len-=end-i-1;}
        self.rows[i]=[a,b];true
    }
    // Removing the middle needs one spare row. Check before splitting so
    // refusal preserves every previously acknowledged byte.
    pub fn remove(&mut self,a:u64,b:u64)->bool{
        if b<=a{return true;}let mut i=0;
        while i<self.len{
            let [lo,hi]=self.rows[i];if lo>=b{break;}if hi<=a{i+=1;continue;}
            if lo<a&&hi>b{
                if self.len==self.rows.len(){return false;}
                self.rows.copy_within(i+1..self.len,i+2);self.rows[i]=[lo,a];self.rows[i+1]=[b,hi];self.len+=1;break;
            }else if lo<a{self.rows[i][1]=a;i+=1;}
            else if hi>b{self.rows[i][0]=b;break;}
            else{self.rows.copy_within(i+1..self.len,i);self.len-=1;}
        }true
    }
    // Trimming old history changes only the visible prefix. Hidden slots
    // contain inert numbers and never retain another peer's payload bytes.
    // ACK history may forget old prefixes, but cannot manufacture receipt
    // of a hole. Trim boundaries retain their half-open interval meaning.
    pub fn trim(&mut self,before:u64){
        let count=self.rows().iter().take_while(|r|r[1]<=before).count();
        self.rows.copy_within(count..self.len,0);self.len-=count;
        if self.len>0{self.rows[0][0]=self.rows[0][0].max(before);}
    }
}

// ReplayWindow commits only authenticated packets. A failed AEAD attempt
// cannot advance its horizon or erase a real packet that is still in flight.
#[derive(Clone)]
pub struct Replay{bits:[u64;lim::REPLAY_WINDOW/64],pub largest:Option<u32>}
impl Default for Replay{fn default()->Self{Self{bits:[0;lim::REPLAY_WINDOW/64],largest:None}}}
impl Replay{
    // A future number may alias an old bitmap slot. It is unseen until
    // authentication succeeds and commit clears the skipped horizon.
    pub fn seen(&self,n:u32)->bool{
        if self.largest.is_none_or(|largest|n>largest){return false;}
        if let Some(largest)=self.largest{if n<=largest&&largest-n>=lim::REPLAY_WINDOW as u32{return true;}}
        let slot=n as usize%lim::REPLAY_WINDOW;self.bits[slot/64]&(1u64<<(slot%64))!=0
    }
    // Clear skipped sequence numbers before setting the received bit. A
    // gap larger than the bitmap discards its complete old history at once.
    pub fn commit(&mut self,n:u32){
        if self.largest.is_none_or(|largest|n>largest){
            let start=self.largest.map_or(n,|v|v+1);
            if n-start>=lim::REPLAY_WINDOW as u32{self.bits.fill(0);}
            else{for at in start..=n{let slot=at as usize%lim::REPLAY_WINDOW;self.bits[slot/64]&=!(1u64<<(slot%64));}}
            self.largest=Some(n);
        }
        let slot=n as usize%lim::REPLAY_WINDOW;self.bits[slot/64]|=1u64<<(slot%64);
    }
}

// Boundary fixtures include the exact width transitions and a wrapped
// replay horizon; ordinary in-order traffic cannot expose these edges.
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn varints_pin_boundaries_and_reject_truncation(){
        for n in [0,63,64,16383,16384,1073741823,1073741824,9007199254740991]{
            let mut bytes=[0;8];let mut out=Writer::new(&mut bytes);out.varint(n).unwrap();let len=out.at;
            assert_eq!(Reader::new(&bytes[..len]).varint().unwrap(),n);
            for short in 0..len{assert!(Reader::new(&bytes[..short]).varint().is_err());}
        }
        assert!(varint_size(9007199254740992).is_err());
    }
    #[test]fn ranges_merge_split_and_retain_bounds(){
        let mut r=Ranges::new(4);assert!(r.add(5,9));assert!(r.add(1,3));assert!(r.add(3,5));assert_eq!(r.rows(),&[[1,9]]);
        assert!(r.remove(3,5));assert_eq!(r.rows(),&[[1,3],[5,9]]);assert!(r.contains(5));assert!(!r.contains(4));
        r.trim(6);assert_eq!(r.rows(),&[[6,9]]);assert!(r.add(10,11));assert!(r.add(12,13));assert!(r.add(14,15));assert!(!r.add(16,17));
    }
    #[test]fn replay_keeps_reordering_and_expires_old_numbers(){
        let mut r=Replay::default();r.commit(0);r.commit(4095);assert!(r.seen(0));assert!(!r.seen(1));r.commit(1);assert!(r.seen(1));
        r.commit(4096);assert!(r.seen(0));assert!(!r.seen(4097));r.commit(20000);assert!(r.seen(4096));assert!(!r.seen(19999));
    }
}
