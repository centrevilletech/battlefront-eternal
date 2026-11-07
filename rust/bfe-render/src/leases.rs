//! Bounded numeric resource leases. Handles include a generation so a late
//! release or retained host identity cannot name a replacement occupant.
#[derive(Clone,Copy,Default)]
struct Entry {generation:u32,touched:u64,live:bool}
pub struct Leases<const N:usize> {
    entries:[Entry;N],pub handles:[f64;N],clock:u64,pub evicted:u32,
}
impl<const N:usize> Default for Leases<N> {
    fn default()->Self{Self{entries:[Entry::default();N],handles:[-1.;N],clock:0,evicted:0}}
}
impl<const N:usize> Leases<N> {
    // Numeric handles remain exact: a u32 generation times this bounded bank
    // fits below JavaScript's 53-bit integer limit, without BigInt allocation.
    pub fn acquire(&mut self)->f64 {
        if N==0{return -1.;}
        let slot=self.entries.iter().position(|e|!e.live).unwrap_or_else(||{
            self.evicted=self.evicted.saturating_add(1);
            self.entries.iter().enumerate().min_by_key(|(_,e)|e.touched).map(|(i,_)|i).unwrap_or(0)
        });
        self.clock=self.clock.wrapping_add(1);let e=&mut self.entries[slot];
        e.generation=e.generation.wrapping_add(1).max(1);e.live=true;e.touched=self.clock;
        let handle=e.generation as f64*N as f64+slot as f64;self.handles[slot]=handle;handle
    }
    pub fn resolve(&self,handle:f64)->Option<usize>{
        if N==0||!handle.is_finite()||handle<0.||handle.fract()!=0.{return None;}
        let slot=(handle%N as f64) as usize;
        (self.entries[slot].live&&self.handles[slot]==handle).then_some(slot)
    }
    pub fn touch(&mut self,handle:f64)->Option<usize>{
        let slot=self.resolve(handle)?;self.clock=self.clock.wrapping_add(1);self.entries[slot].touched=self.clock;Some(slot)
    }
    pub fn release(&mut self,handle:f64)->bool{
        let Some(slot)=self.resolve(handle)else{return false};
        self.entries[slot].live=false;self.handles[slot]=-1.;true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reuse_rejects_old_generations_and_delayed_releases(){
        let mut leases=Leases::<128>::default();let first=leases.acquire();assert!(leases.release(first));
        let next=leases.acquire();assert_ne!(first,next);assert_eq!(first%128.,next%128.);
        assert!(!leases.release(first));assert_eq!(leases.resolve(next),Some(0));
        for _ in 0..10000 {let h=leases.acquire();assert!(leases.resolve(h).is_some());}
        assert!(leases.evicted>0);assert!(leases.resolve(next).is_none());
        for h in [-1.,f64::NAN,f64::INFINITY,1.5,1e100]{assert!(leases.resolve(h).is_none());}
    }
}
