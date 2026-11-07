//! Class jobs and combat share navigation's live hint reservations. Ring
//! identities are resolved once; a query never copies the map's hint table.
use super::scratch::{List, Pool};
use crate::ai::{
    combat::world::{HintView, Obj},
    control::world::{HintSnap, SharedRows},
    nav::{math::Vec3, Nav},
};
struct Definition {
    linked: SharedRows<usize>,
    siblings: SharedRows<Obj>,
    post: Option<usize>,
}
pub struct Hints {
    definitions: Vec<Definition>,
    indices: Pool<usize>,
    combat: Pool<HintView>,
}
impl Hints {
    pub fn new(
        nav: &Nav,
        scopes: usize,
        post: impl Fn(u64) -> Option<usize>,
        identity: impl Fn(usize) -> Obj,
    ) -> Self {
        let definitions = nav
            .hints
            .list
            .iter()
            .enumerate()
            .map(|(index, h)| {
                let mut linked = Vec::new();
                let mut next = h.link;
                while let Some(i) = next {
                    if i == index {
                        break;
                    }
                    assert!(
                        linked.len() < nav.hints.list.len(),
                        "hint ring closes at its first member"
                    );
                    linked.push(i);
                    next = nav.hints.list[i].link;
                }
                Definition {
                    linked: linked.into(),
                    siblings: h.siblings.iter().map(|&i| identity(i)).collect(),
                    post: h.command_post.and_then(&post),
                }
            })
            .collect();
        Self {
            definitions,
            indices: Pool::new(scopes, nav.hints.list.len()),
            combat: Pool::new(scopes, 30),
        }
    }
    pub fn read(&self, nav: &Nav, index: usize) -> HintSnap {
        let h = &nav.hints.list[index];
        let d = &self.definitions[index];
        HintSnap {
            bit: h.bit as f64,
            position: h.position,
            forward: h.forward,
            primary_stance: h.primary_stance as f64,
            post: d.post,
            mode: h.mode,
            radius: h.radius,
            reserved_at: Some(h.reserved_at),
            linked: d.linked.clone(),
        }
    }
    pub fn query(
        &self,
        nav: &Nav,
        mask: u32,
        center: Vec3,
        half: f64,
        cap: f64,
        requester: Option<u64>,
        now: f64,
    ) -> List<usize> {
        let mut out = self.indices.take();
        nav.query_hints_into(mask, center, half, cap, requester, now, &mut out);
        out
    }
    pub fn cover(
        &self,
        nav: &Nav,
        center: Vec3,
        half: f64,
        now: f64,
        identity: impl Fn(usize) -> Obj,
    ) -> List<HintView> {
        let mut out = self.combat.take();
        for index in self.query(nav, 4, center, half, 30., None, now) {
            let h = &nav.hints.list[index];
            let p = h.position;
            let f = h.forward;
            out.push_reserved(HintView {
                obj: identity(index),
                position: three::Vector3::new(p.x, p.y, p.z),
                forward: three::Vector3::new(f.x, f.y, f.z),
                primary_mask: Some(h.primary_mask as f64),
                secondary_mask: Some(h.secondary_mask as f64),
                side_mask: Some(h.side_mask as f64),
                siblings: self.definitions[index].siblings.clone(),
            });
        }
        out
    }
}
