//! SnapshotClient's changed facts have bounded native lifetimes. Transport
//! lends a packet's rows; deferred gameplay resources retain their leases in
//! the next coarse exchange. Baseline entries and display samples stay native.
use crate::network_layout::{net_limits as l,net_record as r,net_lease_changes as c};

pub struct Leases {
    pub records:Vec<f64>,pub payloads:Vec<u8>,pub publications:Vec<u32>,pub seats:Vec<u32>,pub changes:Vec<i32>,
    refs:Vec<u32>,used:Vec<bool>,free:Vec<usize>,packet:Vec<usize>,dirty:Vec<usize>,marked:Vec<bool>,
    pub serial:u64,pub refused:u64,poisoned:bool,
}
impl Leases {
    // Encode-only owners have no gameplay publication. Receiver setup makes
    // every row, byte span and retention command before the first datagram.
    pub fn empty()->Self{Self{records:Vec::new(),payloads:Vec::new(),publications:Vec::new(),seats:Vec::new(),changes:Vec::new(),
        refs:Vec::new(),used:Vec::new(),free:Vec::new(),packet:Vec::new(),dirty:Vec::new(),marked:Vec::new(),serial:0,refused:0,poisoned:false}}
    pub fn reserve(&mut self){
        if !self.refs.is_empty(){return;}
        self.records=vec![0.;l::LEASES*r::STRIDE];self.payloads=vec![0;l::LEASES*l::PAYLOAD];
        self.publications=vec![0;l::RECORDS];self.seats=vec![0;l::SLOTS*2];self.changes=vec![0;l::LEASE_CHANGES+c::STRIDE];
        self.refs=vec![0;l::LEASES];self.used=vec![false;l::LEASES];self.free=(0..l::LEASES).rev().collect();
        self.packet=Vec::with_capacity(l::LEASES);self.dirty=Vec::with_capacity(l::LEASES);self.marked=vec![false;l::LEASES];
    }
    // Native references are the sole lifetime state. JS writes only bounded
    // retain/release requests, consumed before the previous packet releases
    // its borrow. A release and retain in one exchange cannot recycle a row.
    fn touch(&mut self,id:usize){if !self.marked[id]{self.marked[id]=true;self.dirty.push(id);}}
    pub fn begin(&mut self){
        if self.refs.is_empty(){return;}
        let count=self.changes[c::COUNT].max(0)as usize;
        if count>l::LEASE_CHANGES{self.refused+=1;self.poisoned=true;}
        for i in 0..count.min(l::LEASE_CHANGES){
            let change=self.changes[i+c::STRIDE];let id=(change.unsigned_abs()as usize).wrapping_sub(1);
            if id>=l::LEASES||!self.used[id]{continue;}
            if change>0{self.refs[id]=self.refs[id].saturating_add(1);}else{self.refs[id]=self.refs[id].saturating_sub(1);}
            self.touch(id);
        }
        self.changes[c::COUNT]=0;
        for i in 0..self.packet.len(){let id=self.packet[i];self.refs[id]=self.refs[id].saturating_sub(1);self.touch(id);}
        self.packet.clear();
        // A foreign retention overflow protects existing views until reset.
        // New publications can be refused while the page and input keep
        // running; uncertain ownership must never expose a recycled record.
        for id in self.dirty.drain(..){
            self.marked[id]=false;
            if self.refs[id]==0&&!self.poisoned{self.used[id]=false;self.free.push(id);}
        }
        self.serial=self.serial.wrapping_add(1);
    }
    pub fn reset(&mut self){
        if self.refs.is_empty(){return;}
        self.refs.fill(0);self.used.fill(false);self.marked.fill(false);self.free.clear();
        self.free.extend((0..l::LEASES).rev());self.packet.clear();self.dirty.clear();self.changes.fill(0);self.poisoned=false;self.refused=0;
        self.serial=self.serial.wrapping_add(1);
    }
    pub fn available(&self)->usize{self.free.len()}
    // Each publication owns one packet reference. Numeric and opaque data
    // share that identity, so a deferred correction cannot acquire the bytes
    // or input ACK of a later datagram that reused its output row.
    pub fn put(&mut self,row:&[f64;r::STRIDE],bytes:&[u8])->Option<usize>{
        if bytes.len()>l::PAYLOAD{self.refused+=1;return None;}
        let Some(id)=self.free.pop()else{self.refused+=1;return None;};
        let target=&mut self.records[id*r::STRIDE..][..r::STRIDE];target.copy_from_slice(row);
        target[r::PAIR] = -1.;target[r::PAYLOAD_OFFSET]=(id*l::PAYLOAD)as f64;target[r::PAYLOAD_LENGTH]=bytes.len()as f64;
        self.payloads[id*l::PAYLOAD..id*l::PAYLOAD+bytes.len()].copy_from_slice(bytes);
        self.used[id]=true;self.refs[id]=1;self.packet.push(id);Some(id)
    }
    // Vehicle and mount facts are published together from the same entry.
    // This direct pair replaces the page's vehicle-by-mount nested search.
    pub fn pair(&mut self,a:usize,b:usize){self.records[a*r::STRIDE+r::PAIR]=b as f64;self.records[b*r::STRIDE+r::PAIR]=a as f64;}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retained_records_survive_rotation_and_release_retain_in_one_exchange(){
        let mut leases=Leases::empty();leases.reserve();let mut row=[0.;r::STRIDE];row[r::ID]=7.;
        let first=leases.put(&row,&[1,2,3]).unwrap();leases.changes[0]=1;leases.changes[1]=(first+1)as i32;leases.begin();
        for i in 0..l::LEASES-1{row[r::ID]=i as f64+100.;leases.put(&row,&[]).unwrap();}
        assert!(leases.put(&row,&[]).is_none());assert_eq!(leases.records[first*r::STRIDE+r::ID],7.);
        leases.changes[0]=2;leases.changes[1] = -((first+1)as i32);leases.changes[2]=(first+1)as i32;leases.begin();
        assert_eq!(leases.records[first*r::STRIDE+r::ID],7.);assert!(leases.used[first]);
        leases.changes[0]=1;leases.changes[1] = -((first+1)as i32);leases.begin();assert_eq!(leases.free.len(),l::LEASES);
        leases.reset();assert_eq!(leases.free.len(),l::LEASES);
    }
}
