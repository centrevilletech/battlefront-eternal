//! Small ordered seat tables. Live controls are bounded by seat count and
//! rider facts by the registry. Retain their storage across release/reuse;
//! a tree node allocation for every newly boarded control is unnecessary.
#[derive(Clone, Debug, PartialEq)]
pub struct Rows<K,V> { entries: Vec<(K,V)> }
impl<K,V> Default for Rows<K,V> { fn default() -> Self { Self { entries: Vec::new() } } }
impl<K: Ord,V> Rows<K,V> {
    pub fn len(&self) -> usize { self.entries.len() }
    pub fn iter(&self) -> impl Iterator<Item=(&K,&V)> { self.entries.iter().map(|(key,value)| (key,value)) }
    /// Hulls.facts and Crews.facts keep default facts after an occupant leaves.
    pub fn get_or_default(&mut self, key: K) -> &mut V where V: Default {
        let i = match self.entries.binary_search_by(|(k,_)| k.cmp(&key)) {
            Ok(i) => i,
            Err(i) => { self.entries.insert(i,(key,V::default())); i }
        };
        &mut self.entries[i].1
    }
    pub fn reserve(&mut self, additional: usize) { self.entries.reserve(additional); }
    pub fn get(&self, key: &K) -> Option<&V> { self.entries.binary_search_by(|(k,_)| k.cmp(key)).ok().map(|i| &self.entries[i].1) }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> { self.entries.binary_search_by(|(k,_)| k.cmp(key)).ok().map(|i| &mut self.entries[i].1) }
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.entries.binary_search_by(|(k,_)| k.cmp(&key)) {
            Ok(i) => Some(std::mem::replace(&mut self.entries[i].1,value)),
            Err(i) => { self.entries.insert(i,(key,value)); None }
        }
    }
    pub fn remove(&mut self, key: &K) -> Option<V> { self.entries.binary_search_by(|(k,_)| k.cmp(key)).ok().map(|i| self.entries.remove(i).1) }
}
impl<K: Ord,V> FromIterator<(K,V)> for Rows<K,V> {
    fn from_iter<I: IntoIterator<Item=(K,V)>>(iter: I) -> Self {
        let iter = iter.into_iter(); let mut rows = Self::default(); rows.reserve(iter.size_hint().0);
        for (k,v) in iter { rows.insert(k,v); } rows
    }
}

#[cfg(test)]
mod tests {
    use super::Rows;
    #[test]
    fn seat_rows_preserve_key_order_across_replacement_and_reuse() {
        let mut rows = Rows::default(); rows.reserve(4);
        rows.insert((7,2),3); rows.insert((2,1),4); rows.insert((7,1),5);
        assert_eq!(rows.insert((7,2),8),Some(3));
        assert_eq!(rows.iter().map(|(k,v)| (*k,*v)).collect::<Vec<_>>(),vec![((2,1),4),((7,1),5),((7,2),8)]);
        assert_eq!(rows.remove(&(7,1)),Some(5));
        *rows.get_or_default((7,3)) = 9;
        assert_eq!(rows.entries.capacity(),4);
        assert_eq!(rows.get(&(7,1)),None);
        assert_eq!(rows.get(&(7,3)),Some(&9));
    }
}
