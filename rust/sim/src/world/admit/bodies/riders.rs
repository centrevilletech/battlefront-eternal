use crate::gen::{F2RiderBirth, F2RiderClip, Turret, Unit, Vehicle};
use crate::store::Ref;
use bfe_content::clips::{Clip, Clips};

/// A seat's authored rider description (vehicles.js seat.view.rider and
/// props.js Turret.rider). A missing clip and a missing rider are distinct.
#[derive(Clone, Debug, Default)]
pub struct Rider {
    pub clip: Option<String>,
    pub anim: Option<String>,
    pub nine_pose: bool,
}

/// F2Riders.ensure is called once per holder, in live hull, prepared hull,
/// stationary turret order. A hull contributes one binding for every seat.
#[derive(Clone, Debug)]
pub enum Holder {
    Hull { hull: Ref<Vehicle>, seats: Vec<Option<Rider>> },
    Turret { turret: Ref<Turret>, rider: Option<Rider> },
}

/// Retain the holder directly from the fleet owner's constructed rig. A
/// missing rider is a real empty seat; no class-name lookup runs at birth.
pub fn hull_holder(hull: Ref<Vehicle>, rig: &bfe_pose::rig::hull::HullRig) -> Holder {
    Holder::Hull { hull, seats: rig.seats.iter().map(|seat| seat.rider.as_ref().map(|rider| Rider {
        clip: Some(rider.clip.clone()), anim: Some(rider.anim.clone()), nine_pose: rider.nine_pose,
    })).collect() }
}

/// F2Riders.ensure's construction row, sharing the body's existing immutable
/// clips. Timed stand-ins deliberately keep the authored ninePose flag.
/// A turret uses Turret.riderSample's minigun fallback even without a rider.
pub fn rider_birth<'a>(unit: Ref<Unit>, life: f64, scale: f64, clips: &'a Clips,
    holder: &Holder, mut clip_id: impl FnMut(&'a Clip) -> u32) -> F2RiderBirth
{
    let bindings = match holder {
        Holder::Hull { hull, seats } => seats.iter().enumerate().map(|(index, rider)| {
            let mut clip = rider.as_ref().and_then(|r| r.clip.as_deref().and_then(|n| clips.get(n))
                .or_else(|| r.anim.as_deref().and_then(|n| clips.get(n))));
            let standin = rider.as_ref().filter(|_| clip.is_none()).and_then(|r| match r.anim.as_deref() {
                Some("man_minigun") => Some(("human_minigun_9pose", 4.0)),
                Some("drivesnowspeeder" | "drivesnowspeedergunner") => Some(("human_drive", 0.0)),
                _ => None,
            });
            if let Some((name, _)) = standin { clip = clips.get(name); }
            F2RiderClip { hull: Some(*hull), turret: None, index: index as u32,
                clip: clip.map_or(-1, |c| clip_id(c) as i32),
                nine: rider.as_ref().is_some_and(|r| r.nine_pose),
                time: standin.and_then(|(_, frame)| clip.map(|c| frame / c.fps)) }
        }).collect(),
        Holder::Turret { turret, rider } => {
            // props.js riderSample only considers rider.clip before its
            // fallback. rider.anim belongs to the hull alias path above.
            let clip = rider.as_ref().and_then(|r| r.clip.as_deref()).and_then(|n| clips.get(n))
                .or_else(|| clips.get("human_minigun_9pose"));
            let time = clip.and_then(|_| rider.as_ref().filter(|r| !r.nine_pose).map(|_| 0.0));
            vec![F2RiderClip { hull: None, turret: Some(*turret), index: 0,
                clip: clip.map_or(-1, |c| clip_id(c) as i32), nine: time.is_none(), time }]
        }
    };
    F2RiderBirth { unit, life, scale, clips: bindings }
}
