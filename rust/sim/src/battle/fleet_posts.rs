//! MobilePosts reads the fleet's canonical slots, hulls and leases. Prepared
//! class records stay on each pad; refreshing a post changes scalar facts only.
use super::mobile::{self, ClassRecord, Hull, Pad};
use crate::gen::{Body, MobilePost, PsHull, PsPad, Store};
use crate::store::Ref;
use three::{Matrix4, Vector3};

#[derive(Default)]
pub struct Pads { pads: Vec<Option<Pad>> }
fn hull(h: PsHull) -> Hull {
    Hull { id: h.vehicle, command: h.command, name: h.name, label: h.label,
        value_bleed_setting: h.value_bleed,
        record: h.record.map(|r| ClassRecord { atk: r.atk, def: r.def, value_bleed: r.value_bleed, spawn_points: r.points }),
        ..Hull::default() }
}
impl Pads {
    /// Construction supplies immutable command-class facts once. The dynamic
    /// fields in this generated birth record never become a second authority.
    pub fn adopt(&mut self, slot: usize, pad: PsPad) {
        self.pads.resize_with(self.pads.len().max(slot + 1), || None);
        self.pads[slot] = Some(Pad { variants: pad.variants.into_iter().map(hull).collect(), vehicle: pad.vehicle.map(hull) });
    }
    /// The post can change hull life without growing its class/path arrays.
    pub fn reserve_post(&self, slot: usize, post: &mut MobilePost) {
        let pad = self.pads[slot].as_ref().expect("prepared command pad");
        for hull in pad.variants.iter().chain(pad.vehicle.iter()) {
            let r = hull.record.as_ref();
            mobile::reserve_class(post, r.and_then(|r| r.atk.as_ref()).map_or(6, Vec::len),
                r.and_then(|r| r.def.as_ref()).map_or(6, Vec::len), r.map_or(0, |r| r.spawn_points.len()));
        }
    }
    /// Align retained records with the live Map order using only swaps. A
    /// removed variant stays prepared but cannot win the last-live-hull rule.
    /// New variants must register their class facts during cold construction.
    pub fn get(&mut self, s: &Store, slot: usize) -> &Pad {
        let source = &s.vehicles.slots[slot];
        let pad = self.pads[slot].as_mut().expect("prepared command pad");
        for (i, &(_, r)) in source.variants.entries.iter().enumerate() {
            let found = pad.variants[i..].iter().position(|h| h.id == r).map(|j| i + j).expect("prepared pad variant");
            pad.variants.swap(i, found);
        }
        for (i, h) in pad.variants.iter_mut().enumerate() {
            refresh(s, h, i < source.variants.entries.len());
        }
        if let Some(h) = &mut pad.vehicle { refresh(s, h, source.vehicle == Some(h.id)); }
        pad
    }
}
fn refresh(s: &Store, h: &mut Hull, present: bool) {
    let Some(v) = s.vehicle.get(h.id).filter(|_| present) else { h.alive = false; h.seated = false; return };
    h.alive = v.alive; h.generation = v.target_generation; h.team = jsrt::conv::to_int32(v.team.unwrap_or(f64::NAN)) as f64;
    // Vehicle.controllingTeam returns the first lease's team. MobilePost then
    // applies |0, retaining the last occupation after every seat is empty.
    h.seated = false; h.controlling_team = h.team;
    for seat in &v.seats {
        if let Some(lease) = seat.lease {
            h.seated = true;
            h.controlling_team = jsrt::conv::to_int32(s.seat_lease.get(lease).expect("current fleet lease").team) as f64;
            break;
        }
    }
    h.yaw = v.yaw; h.speed = v.speed;
    h.radius = Vector3::new(v.size[0], v.size[1], v.size[2]).length() / 2.0;
}

/// MobilePost.ai.classSpawnWorld into retained caller storage. Static casts
/// are native geometry reads; they cannot enter a foreign gameplay callback.
pub fn spawn_points(s: &Store, post: Ref<MobilePost>, into: &mut Vec<[f64; 4]>,
    mut cast: impl FnMut(Vector3, Vector3, f64, Ref<Body>) -> Option<Vector3>) {
    into.clear();
    let p = s.mobile_post.get(post).expect("mobile spawn post");
    if !p.active { return; }
    let v = s.vehicle.get(p.vehicle.expect("active mobile hull")).expect("current mobile hull");
    let mut matrix = Matrix4::default(); matrix.elements = v.matrix;
    let center = Vector3::new(v.center[0], v.center[1], v.center[2]);
    for &point in &p.ai.points {
        into.push(mobile::command_spawn_point(&matrix, center, v.yaw, point, |from, direction, span| cast(from, direction, span, v.body)));
    }
}

/// CommandPosts.all observes current command hulls before AI and before a
/// foreign post hook. Refresh scalar views without advancing reported events.
pub(crate) fn refresh_posts(s:&mut Store,pads:&mut Pads){
    for index in 0..s.mobile_posts.posts.len(){
        let r=s.mobile_posts.posts[index];let slot=s.mobile_post.get(r).unwrap().index as usize;
        let pad=pads.get(s,slot);mobile::refresh_native(s,r,pad);
    }
}
