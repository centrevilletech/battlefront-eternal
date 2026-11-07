//! Reserved result buffers whose scope does not borrow the world. A nested
//! native query takes another buffer; returning an iterator returns its buffer
//! even when the caller stops on the first result.
use std::{
    cell::RefCell,
    ops::{Deref, DerefMut},
    rc::Rc,
};

#[derive(Debug)]
pub struct Pool<T>(Rc<RefCell<Vec<Vec<T>>>>);
impl<T> Clone for Pool<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Pool<T> {
    /// Reserve at world construction, including the maximum nested queries.
    pub fn new(scopes: usize, capacity: usize) -> Self {
        Self(Rc::new(RefCell::new(
            (0..scopes).map(|_| Vec::with_capacity(capacity)).collect(),
        )))
    }
    pub fn take(&self) -> List<T> {
        let values = self
            .0
            .borrow_mut()
            .pop()
            .expect("AI query nesting exceeds reserved scopes");
        List {
            values,
            pool: Some(self.clone()),
        }
    }
}

#[derive(Debug)]
pub struct List<T> {
    values: Vec<T>,
    pool: Option<Pool<T>>,
}
impl<T> Default for List<T> {
    fn default() -> Self {
        Vec::new().into()
    }
}
impl<T> From<Vec<T>> for List<T> {
    fn from(values: Vec<T>) -> Self {
        Self { values, pool: None }
    }
}
impl<T> List<T> {
    /// Captures may grow their lists; native results always retain their reservation.
    pub fn push(&mut self, value: T) {
        if self.pool.is_some() {
            self.push_reserved(value);
        } else {
            self.values.push(value);
        }
    }
    /// Native callers must use this rather than Vec::push so a bad load-time
    /// reservation fails at its source instead of allocating inside a tick.
    pub fn push_reserved(&mut self, value: T) {
        assert!(
            self.values.len() < self.values.capacity(),
            "AI query exceeds reserved capacity"
        );
        self.values.push(value);
    }
}
impl<T> Deref for List<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.values
    }
}
impl<T> DerefMut for List<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.values
    }
}
impl<T> Drop for List<T> {
    fn drop(&mut self) {
        if let Some(pool) = &self.pool {
            self.values.clear();
            pool.0.borrow_mut().push(std::mem::take(&mut self.values));
        }
    }
}
/// Moving iteration preserves source order and keeps the reserved allocation.
pub struct IntoIter<T>(List<T>);
impl<T> IntoIterator for List<T> {
    type Item = T;
    type IntoIter = IntoIter<T>;
    fn into_iter(mut self) -> IntoIter<T> {
        self.values.reverse();
        IntoIter(self)
    }
}
impl<T> Iterator for IntoIter<T> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        self.0.values.pop()
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.0.len();
        (n, Some(n))
    }
}
impl<T> ExactSizeIterator for IntoIter<T> {}
impl<'a, T> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Vec::from_iter(iter).into()
    }
}

/// Source-bounded records stay inline, including when service requests copy
/// them. No allocation or buffer ownership crosses a controller callback.
#[derive(Clone, Copy, Debug)]
pub struct Inline<T: Copy + Default, const N: usize> {
    values: [T; N],
    len: usize,
}
impl<T: Copy + Default, const N: usize> Default for Inline<T, N> {
    fn default() -> Self {
        Self {
            values: [T::default(); N],
            len: 0,
        }
    }
}
impl<T: Copy + Default, const N: usize> Deref for Inline<T, N> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.values[..self.len]
    }
}
impl<T: Copy + Default, const N: usize> DerefMut for Inline<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.values[..self.len]
    }
}
impl<T: Copy + Default + PartialEq, const N: usize> PartialEq for Inline<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self[..] == other[..]
    }
}
impl<T: Copy + Default, const N: usize> Inline<T, N> {
    pub fn push(&mut self, value: T) {
        assert!(self.len < N, "AI inline record capacity");
        self.values[self.len] = value;
        self.len += 1;
    }
    pub fn insert(&mut self, index: usize, value: T) {
        assert!(
            index <= self.len && self.len < N,
            "AI inline insert capacity"
        );
        self.values.copy_within(index..self.len, index + 1);
        self.values[index] = value;
        self.len += 1;
    }
    pub fn remove(&mut self, index: usize) -> T {
        assert!(index < self.len);
        let value = self.values[index];
        self.values.copy_within(index + 1..self.len, index);
        self.len -= 1;
        value
    }
    pub fn truncate(&mut self, len: usize) {
        self.len = self.len.min(len);
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        let mut out = 0;
        for index in 0..self.len {
            if keep(&self.values[index]) {
                self.values[out] = self.values[index];
                out += 1;
            }
        }
        self.len = out;
    }
}
impl<T: Copy + Default, const N: usize> FromIterator<T> for Inline<T, N> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut out = Self::default();
        for value in iter {
            out.push(value);
        }
        out
    }
}
impl<T: Copy + Default, const N: usize> IntoIterator for Inline<T, N> {
    type Item = T;
    type IntoIter = std::iter::Take<std::array::IntoIter<T, N>>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter().take(self.len)
    }
}
impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a Inline<T, N> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a, T: Copy + Default, const N: usize> IntoIterator for &'a mut Inline<T, N> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
