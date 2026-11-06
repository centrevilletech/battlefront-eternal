//! Prepared LiveObjects construction (src/props.js, src/traps.js). Generated
//! birth records decode geometry and clips once; ordinary phases retain them.
use super::{animated::{Animated, Grab, Settings, Trigger, Victim}, live_phase};
use crate::gen::*;
use crate::pose::{Clip, Pose};
use crate::seats::turrets::TurretRig;
use three::{Matrix4, Quaternion, Vector3};
fn v3(v: [f64; 3]) -> Vector3 { Vector3::new(v[0], v[1], v[2]) }
fn q4(v: [f64; 4]) -> Quaternion { Quaternion::new(v[0], v[1], v[2], v[3]) }
fn mat(v: [f64; 16]) -> Matrix4 { let mut m = Matrix4::default(); m.from_array(&v, 0); m }
pub(crate) struct Configuration { pub(crate) settings: Settings, root: [f64; 16], parents: Vec<f64>, bind: Vec<(Quaternion, Vector3)> }
fn clip(v: PaClip) -> Clip {
    Clip { frames: v.frames as usize, fps: v.fps, root_motion: v.root_motion,
        tracks: v.tracks.into_iter().map(|n| n as i32).collect(), data: v.data.into_iter().map(|n| n as f32).collect() }
}
pub(crate) fn configuration(v: PaSettings) -> Configuration {
    Configuration { root: v.root, parents: v.parents, bind: v.bind.into_iter().map(|b| (q4(b.q), v3(b.p))).collect(),
        settings: Settings { door: v.door, clips: v.clips.into_iter().map(|v| v.map(clip)).collect(), door_clip: v.door_clip.map(clip),
            door_duration: v.door_duration, idle_delay: v.idle_delay, open_slots: v.open_slots, position: v3(v.position), kill_sound: v.kill_sound,
            triggers: v.triggers.into_iter().map(|t| Trigger { slot: t.slot as i32, point: v3(t.point), radius: t.radius, sound: t.sound, opens: t.opens }).collect(),
            grabs: v.grabs.into_iter().map(|g| Grab { slot: g.slot as i32, window: g.window, radius: g.radius, node: g.node.map(|n| n as usize), clip: g.clip }).collect() } }
}
pub(crate) fn prop(v: PaState, config: &Configuration) -> Animated {
    let bones: Vec<_> = v.bones.iter().enumerate().map(|(i, b)| {
        let parent = config.parents[i]; ((parent >= 0.0).then_some(parent as usize), v3(b.p), q4(b.q), v3(b.s))
    }).collect();
    let worlds: Vec<_> = v.bones.iter().map(|b| b.world).collect();
    let mut rig = TurretRig::new(&config.root, &bones, Some(&worlds));
    rig.tree.node_mut(rig.root).matrix_world = mat(v.root_world);
    let pose = v.pose.map(|v| {
        let mut pose = Pose::new(v.w.len(), v.root as i32);
        pose.q = v.q.into_iter().map(|x| x as f32).collect(); pose.t = v.t.into_iter().map(|x| x as f32).collect(); pose.w = v.w.into_iter().map(|x| x as f32).collect(); pose
    });
    Animated { index: v.index as usize, slot: v.slot as i32, time: v.time, weight: v.weight, looping: v.r#loop, hold: v.hold, started: v.started, start_time: v.start_time,
        victim: v.victim.map(|v| Victim { unit: v.unit as usize, generation: v.generation, grab: v.grab as usize, since: v.since }),
        openness: v.openness, body_enabled: v.body_enabled, stand_q: v.stand_q.into_iter().map(|n| n as f32).collect(), stand_t: v.stand_t.into_iter().map(|n| n as f32).collect(),
        pose, rig, bind: config.bind.clone() }
}

/// A station/turret uses Building's one health frame and prepared settings.
pub fn building(v: F4BuildingBirth) -> live_phase::Building {
    let mut settings: super::props::Settings = v.settings.into();
    settings.has_crew = v.crew; settings.has_damaged = v.damaged;
    live_phase::Building { state: v.state.into(), settings, supply: v.supply.map(Into::into), attacker: v.attacker }
}
pub fn trap(v: F4TrapBirth, hull_capacity: usize) -> live_phase::Trap {
    let mut trap = live_phase::Trap::new(v.state.into(), v.settings.into(), hull_capacity);
    trap.attacker = v.attacker; trap
}
pub fn animated(v: F4AnimatedBirth) -> live_phase::Animated {
    let prepared = configuration(v.settings);
    let state = prop(v.state, &prepared);
    live_phase::Animated { state, settings: prepared.settings }
}
