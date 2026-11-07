//! ShotDirector's geometry from spawn-screen/page/director.js. The screen's
//! HUD clock and cut notification are input facts; every camera pose is
//! calculated here in the battle's one camera phase, even on undrawn frames.
// The battle behind the page spawn screen, through the mission's camera
// shots (its AddCameraShot views, the original's backdrop for the screen):
// each one held for a while, drifting (a slow pan and a push in) so the
// battle is never a still, then a cut to the next. A post picked brings up
// the shot that best sees it, turned partway toward it.
//
// Only turns and a short push along the shot's own view, so the camera
// never leaves the space the mission's author chose.
// shots: [{ position: [x, y, z], quaternion: [x, y, z, w] }].
// Start on a shot (an index, wrapping).
// The shot that best sees a world point: near it, looking its way.
// A post picked at a world point ([x, y, z]): the shot that sees it best,
// turned partway toward it (cutting only if that is another shot).
// Never past straight up or down, and only within a half turn.
// The view this frame, or null without shots: { position, quaternion }
// as arrays, cut true on the first frame of a shot.

use super::math::*;
use serde::Deserialize;
use three::{Quaternion as Q, Vector3 as V};
#[derive(Deserialize)]
pub struct Shot {
    pub position: [f64; 3],
    pub quaternion: [f64; 4],
}
impl Shot {
    pub fn position(&self) -> V {
        V::new(self.position[0], self.position[1], self.position[2])
    }
    pub fn quaternion(&self) -> Q {
        Q::new(
            self.quaternion[0],
            self.quaternion[1],
            self.quaternion[2],
            self.quaternion[3],
        )
    }
    pub fn normalize(&mut self) {
        let q = normalize(self.quaternion());
        self.quaternion = [q.x, q.y, q.z, q.w];
    }
}
#[derive(Clone, Copy)]
struct State {
    index: Option<usize>,
    turn: f64,
    aim: Q,
}
impl Default for State {
    fn default() -> Self {
        Self {
            index: None,
            turn: 1.0,
            aim: Q::default(),
        }
    }
}
pub struct Director {
    state: State,
    revision: u32,
    committed: State,
    committed_revision: u32,
}
impl Default for Director {
    fn default() -> Self {
        Self {
            state: State::default(),
            revision: 0,
            committed: State::default(),
            committed_revision: 0,
        }
    }
}
impl Director {
    // ShotDirector.update commits one HUD paint's shot before the next frame.
    // A later click can choose another shot before that frame consumes it.
    // Keep the committed selection by revision instead of reading new UI state.
    // One pending and one committed state cover every such event burst.
    // Extra clicks replace only the pending choice, with fixed storage.
    fn preserve(&mut self, selected: u32) {
        if self.revision == selected {
            self.committed = self.state;
            self.committed_revision = selected;
        }
    }
    fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }
    pub fn revision(&self) -> u32 {
        self.revision
    }
    fn select(&mut self, index: usize) {
        self.state.index = Some(index);
        self.state.turn = -self.state.turn;
        self.state.aim = Q::default();
    }
    pub fn show(&mut self, index: f64, shots: &[Shot], selected: u32) -> i32 {
        if shots.is_empty() || !index.is_finite() {
            return -1;
        }
        self.preserve(selected);
        let index = (index as i64).rem_euclid(shots.len() as i64) as usize;
        self.select(index);
        self.bump();
        index as i32
    }
    pub fn best(&self, point: V, shots: &[Shot]) -> Option<usize> {
        let (mut best, mut score) = (None, f64::INFINITY);
        for (i, shot) in shots.iter().enumerate() {
            let to = sub(point, shot.position());
            let distance = to.length().max(1.0);
            let forward = rotate(V::new(0.0, 0.0, -1.0), shot.quaternion());
            let facing = forward.dot(&norm(to));
            let value = distance * (2.2 - facing);
            if value < score {
                score = value;
                best = Some(i)
            }
        }
        best
    }
    pub fn follow<M: Math>(&mut self, point: V, shots: &[Shot], selected: u32) -> i32 {
        let Some(index) = self.best(point, shots) else {
            return -1;
        };
        self.preserve(selected);
        if self.state.index != Some(index) {
            self.select(index);
        }
        let shot = &shots[index];
        let forward = rotate(V::new(0.0, 0.0, -1.0), shot.quaternion());
        let to = norm(sub(point, shot.position()));
        // A post behind this authored shot never turns it through the scene.
        // Rejected follows keep the preceding aim, including between frames.
        if forward.dot(&to) >= -0.2 {
            let mut q = Q::default();
            q.set_from_unit_vectors(&forward, &to);
            self.state.aim = slerp::<M>(q, Q::default(), 1.0 - 0.4);
        }
        self.bump();
        index as i32
    }
    // ShotDirector.update originally multiplied pan, aim, then authored turn.
    // Keep that left-associated product before normalizing for the spawn port.
    // Forward is measured from the unnormalized product for the push distance.
    // The UI supplies elapsed HUD time; this function advances no second clock.
    // A missing or stale revision leaves the underlying selected view intact.
    pub fn pose<M: Math>(&mut self, age: f64, revision: u32, shots: &[Shot]) -> Option<(V, Q)> {
        self.preserve(revision);
        if revision != self.committed_revision {
            return None;
        }
        let state = self.committed;
        let shot = shots.get(state.index?)?;
        let t = (age / 11.0).min(1.0);
        let t = t * t * (3.0 - 2.0 * t);
        let pan = axis::<M>(UP, state.turn * (4.5 * DEG) * (t * 2.0 - 1.0));
        let q = mul(mul(pan, state.aim), shot.quaternion());
        let forward = rotate(V::new(0.0, 0.0, -1.0), q);
        let p = plus(shot.position(), forward, 2.5 * t);
        // spawnPort.setViewShot normalized the director's product once more.
        // Keep that final normalization after using its unnormalized forward.
        Some((p, normalize(q)))
    }
}
