//! The native phase reserves result scopes at world construction. Lists can
//! outlive a world borrow, including while a nested lifecycle callback runs.
use super::scratch::Pool;
use crate::ai::control::world::{AlertState, EntRef, UKey};
pub struct Buffers {
    pub units: Pool<UKey>,
    pub posts: Pool<UKey>,
    pub predicates: Pool<(UKey, bool)>,
    pub indices: Pool<usize>,
    pub weights: Pool<f64>,
    pub objectives: Pool<Option<usize>>,
    pub recruits: Pool<(f64, usize)>,
    pub alerts: Pool<AlertState>,
    pub references: Pool<(usize, EntRef)>,
    /// Each boardable can notify the same thirty Characters. These are
    /// ordered occurrences, not a set limited to the roster's length.
    pub vehicle_notes: Pool<(usize, EntRef)>,
    recruit_sort: jsrt::sort::SortScratch<(f64, usize)>,
}
impl Buffers {
    pub fn new(
        registry: usize,
        posts: usize,
        characters: usize,
        hints: usize,
        scopes: usize,
    ) -> Self {
        let mut recruit_sort = jsrt::sort::SortScratch::default();
        recruit_sort.reserve(registry);
        Self {
            units: Pool::new(scopes, registry),
            posts: Pool::new(scopes, posts),
            predicates: Pool::new(scopes, registry),
            indices: Pool::new(scopes, posts.max(characters).max(hints)),
            weights: Pool::new(scopes, posts),
            objectives: Pool::new(scopes, characters),
            recruits: Pool::new(scopes, registry),
            alerts: Pool::new(scopes, characters),
            references: Pool::new(scopes, characters),
            vehicle_notes: Pool::new(scopes, registry.saturating_mul(characters.min(30))),
            recruit_sort,
        }
    }
    pub fn sort_recruits(&mut self, rows: &mut Vec<(f64, usize)>) {
        jsrt::sort::js_sort_buffered(rows, &mut |a, b| a.0 - b.0, &mut self.recruit_sort);
    }
}
