//! Core controller numbers increase after detach/reattach. Retain only live
//! bindings, ordered by Core number, so storage follows population capacity
//! rather than the number of joins during a session.
use crate::store::Ref;
pub struct Index<T> {rows:Vec<(usize,Ref<T>)>}
impl<T> Index<T> {
    pub fn new(capacity:usize)->Self {Self {rows:Vec::with_capacity(capacity)}}
    #[inline]
    pub fn get(&self,key:usize)->Option<Ref<T>> {
        // Ordinary rosters keep a dense interval of Core numbers. Check its
        // exact key before the sparse search; a departed controller leaves a
        // hole, and a long-lived neighbor can outlast many later joins.
        if let Some((first,_))=self.rows.first() {
            if let Some(at)=key.checked_sub(*first) {
                if let Some((found,value))=self.rows.get(at) {if *found==key {return Some(*value);}}
            }
        }
        self.rows.binary_search_by_key(&key,|r|r.0).ok().map(|at|self.rows[at].1)
    }
    pub fn insert(&mut self,key:usize,value:Ref<T>) {
        match self.rows.binary_search_by_key(&key,|r|r.0) {
            Ok(at)=>self.rows[at].1=value,
            Err(at)=>{assert!(self.rows.len()<self.rows.capacity(),"prepared native controller capacity");self.rows.insert(at,(key,value));},
        }
    }
    pub fn remove(&mut self,key:usize) {
        if let Ok(at)=self.rows.binary_search_by_key(&key,|r|r.0) {self.rows.remove(at);}
    }
    pub fn values(&self)->impl Iterator<Item=Ref<T>>+'_ {self.rows.iter().map(|r|r.1)}
}

#[cfg(test)]
mod tests {

#[test]
fn native_controller_index_keeps_sparse_keys_and_generations_after_dense_joins() {
    use super::Index;
    use crate::{store::Ref,gen::Unit};
    let mut index=Index::<Unit>::new(4);
    for key in 7..11 {index.insert(key,Ref::new(key as u32,3));}
    assert_eq!(index.get(9),Some(Ref::new(9,3)));
    index.remove(8);assert_eq!(index.get(9),Some(Ref::new(9,3)));
    index.insert(usize::MAX,Ref::new(99,7));assert_eq!(index.get(usize::MAX),Some(Ref::new(99,7)));
    index.remove(7);index.insert(9,Ref::new(9,4));
    assert_eq!(index.get(9),Some(Ref::new(9,4)));assert_eq!(index.get(8),None);
    assert_eq!(index.get(0),None);
}

}
