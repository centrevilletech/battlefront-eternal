//! Instance-local resource identities for cold admission and synchronous phases.
//! Buffers remain at their cold addresses while the owning slot table grows.
//! A released identity never resolves to a replacement or a different bank.

struct Slot<T> { generation: u16, value: Option<Box<T>> }
pub struct Pool<T> { tag: usize, rows: Vec<Slot<T>>, free: Vec<usize> }
const SLOTS: usize = 0xffff;
const GENERATIONS: u16 = 0x3ff;
impl<T> Pool<T> {
    pub const fn new(tag: usize) -> Self { Self { tag, rows: Vec::new(), free: Vec::new() } }
    fn key(&self, index: usize) -> usize {
        (self.tag << 26) | ((self.rows[index].generation as usize) << 16) | (index + 1)
    }
    fn index(&self, key: usize) -> Option<usize> {
        if key >> 26 != self.tag || key & SLOTS == 0 { return None; }
        let index=(key & SLOTS)-1;let row=self.rows.get(index)?;
        (row.generation as usize==((key >> 16)&GENERATIONS as usize)).then_some(index)
    }
    /// Admission is cold. Exhaustion refuses a resource without consuming an
    /// existing slot; phase lookups and releases never allocate.
    pub fn insert(&mut self, value: T) -> Option<usize> {
        let index=if let Some(index)=self.free.pop() {index} else {
            if self.rows.len()==SLOTS || self.rows.try_reserve(1).is_err() || self.free.try_reserve(self.rows.len()+1).is_err() {return None;}
            let index=self.rows.len();self.rows.push(Slot {generation:0,value:None});index
        };
        self.rows[index].value=Some(Box::new(value));Some(self.key(index))
    }
    pub fn get(&self, key: usize) -> Option<&T> {self.rows.get(self.index(key)?)?.value.as_deref()}
    pub fn get_mut(&mut self, key: usize) -> Option<&mut T> {
        let index=self.index(key)?;self.rows[index].value.as_deref_mut()
    }
    pub fn remove(&mut self, key: usize) -> bool {
        let Some(index)=self.index(key) else{return false};let row=&mut self.rows[index];
        if row.value.take().is_none() {return false;}
        // Never wrap a generation into an old identity, even after repeated
        // page/session rotations. An exhausted slot stays retired.
        if row.generation<GENERATIONS {row.generation+=1;self.free.push(index);}
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identities_are_local_qualified_and_stable_across_admission() {
        let mut a=Pool::new(1);let mut b=Pool::new(1);let mut other=Pool::new(2);
        let first=a.insert(7).unwrap();assert_eq!(b.insert(9),Some(first));assert_eq!(b.get(first),Some(&9));
        let address=a.get(first).unwrap() as *const i32;
        for i in 0..1024 {a.insert(i).unwrap();}
        assert_eq!(a.get(first).unwrap() as *const i32,address);
        assert!(other.get(first).is_none());other.insert(11).unwrap();assert!(other.get(first).is_none());
        assert!(a.remove(first));assert!(!a.remove(first));let replacement=a.insert(13).unwrap();
        assert_ne!(first,replacement);assert!(a.get(first).is_none());assert_eq!(a.get(replacement),Some(&13));
    }
}
