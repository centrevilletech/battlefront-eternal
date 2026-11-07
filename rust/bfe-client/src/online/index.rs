//! Bounded identities for live resources. A standard HashMap may rehash after
//! enough removals even below its original capacity; this table closes probe
//! clusters in place, so long battles and repeated respawns never reserve.
#[derive(Clone,Copy)]struct Bucket{key:(u32,u32),value:usize,used:bool}
pub struct Index{rows:Vec<Bucket>,count:usize,maximum:usize}
impl Index{
    // Keep a low load factor even at the admitted maximum. Empty buckets
    // guarantee that every live lookup terminates without a sentinel key.
    pub fn new(maximum:usize)->Self{Self{rows:vec![Bucket{key:(0,0),value:0,used:false};(maximum.max(1)*4).next_power_of_two()],count:0,maximum}}
    // Resource retirement changes occupancy only; the backing table survives
    // map resets and arbitrarily many generations of the same wire id.
    pub fn clear(&mut self){for row in &mut self.rows{row.used=false;}self.count=0;}
    fn bucket(&self,key:(u32,u32))->usize{
        let mut hash=(key.0 as u64)<<32|key.1 as u64;hash=(hash^(hash>>30)).wrapping_mul(0xbf58476d1ce4e5b9);hash^=hash>>27;
        let mask=self.rows.len()-1;let mut at=hash as usize&mask;
        while self.rows[at].used&&self.rows[at].key!=key{at=(at+1)&mask;}at
    }
    pub fn get(&self,key:&(u32,u32))->Option<&usize>{let at=self.bucket(*key);self.rows[at].used.then_some(&self.rows[at].value)}
    pub fn insert(&mut self,key:(u32,u32),value:usize)->bool{
        let at=self.bucket(key);if !self.rows[at].used{if self.count==self.maximum{return false;}self.count+=1;}
        self.rows[at]=Bucket{key,value,used:true};true
    }
    pub fn remove(&mut self,key:&(u32,u32))->Option<usize>{
        let at=self.bucket(*key);if !self.rows[at].used{return None;}let value=self.rows[at].value;self.rows[at].used=false;self.count-=1;
        // Removal never leaves a tombstone. Every displaced entry is placed
        // at the first available bucket on its own original probe sequence.
        let mask=self.rows.len()-1;let mut next=(at+1)&mask;
        while self.rows[next].used{let row=self.rows[next];self.rows[next].used=false;let to=self.bucket(row.key);self.rows[to]=row;next=(next+1)&mask;}
        Some(value)
    }
    pub fn values(&self)->impl Iterator<Item=&usize>{self.rows.iter().filter(|r|r.used).map(|r|&r.value)}
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn repeated_slot_reuse_keeps_sparse_identities_and_the_fixed_reservation(){
        let mut index=Index::new(32);let pointer=index.rows.as_ptr();for generation in 1..10000{for id in 0..32{assert!(index.insert((id*65537,generation),id as usize));}assert!(!index.insert((99,generation),99));for id in 0..32{assert_eq!(index.get(&(id*65537,generation)),Some(&(id as usize)));assert_eq!(index.remove(&(id*65537,generation)),Some(id as usize));}assert_eq!(index.rows.as_ptr(),pointer);}
    }
}
