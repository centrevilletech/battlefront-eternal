//! Director's mission-shot selection and slow push behind the preview stage.
use bfe_battle_ui::spawn::CameraShot;
use three::{Matrix4, Quaternion, Vector3 as V};
pub(super) struct Director {
    shots: Vec<CameraShot>,
    index: usize,
    age: f64,
    turn: f64,
    aim: Quaternion,
    active: bool,
    cut: bool,
}
fn position(shot: CameraShot) -> V {
    V::new(shot.position[0], shot.position[1], shot.position[2])
}
fn rotation(shot: CameraShot) -> Quaternion {
    Quaternion::new(
        shot.quaternion[0],
        shot.quaternion[1],
        shot.quaternion[2],
        shot.quaternion[3],
    )
}
fn forward(q: &Quaternion) -> V {
    let mut v = V::new(0., 0., -1.);
    v.apply_quaternion(q);
    v
}
/// Seconds a shot is held (HOLD in the browser's director.js).
const HOLD: f64 = 11.;
impl Director {
    pub fn new(shots: Vec<CameraShot>) -> Self {
        Self {
            shots,
            index: 0,
            age: 0.,
            turn: 1.,
            aim: Quaternion::default(),
            active: false,
            cut: false,
        }
    }
    /// The screen opens on a shot chosen at random, then the picked post's best shot takes over
    /// (view.js open(): director.show(random), then follow(post)).
    pub fn open(&mut self) {
        self.active = !self.shots.is_empty();
        if self.active {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.subsec_nanos() as usize);
            self.show(nanos % self.shots.len());
        }
    }
    /// Starts a shot: its age restarts, the pan reverses and any aim is dropped (ShotDirector.show).
    fn show(&mut self, index: usize) {
        self.index = index % self.shots.len();
        self.age = 0.;
        self.turn = -self.turn;
        self.aim = Quaternion::default();
        self.cut = true;
    }
    /// True once for the first frame of a new shot (the view's `cut` in director.js update()).
    pub fn take_cut(&mut self) -> bool {
        std::mem::take(&mut self.cut)
    }
    /// A post picked: the shot that best sees it, turned part way toward it. Only a different shot
    /// cuts and restarts the hold; the same shot keeps its age (ShotDirector.follow).
    pub fn follow(&mut self, p: [f64; 3]) {
        if self.shots.is_empty() {
            return;
        }
        let point = V::new(p[0], p[1], p[2]);
        let mut best = 0;
        let mut score = f64::INFINITY;
        for (i, shot) in self.shots.iter().copied().enumerate() {
            let mut delta = point;
            delta.sub(&position(shot));
            let distance = delta.length().max(1.);
            delta.normalize();
            let facing = forward(&rotation(shot)).dot(&delta);
            let value = distance * (2.2 - facing);
            if value < score {
                best = i;
                score = value;
            }
        }
        if best != self.index {
            self.show(best);
        }
        let shot = self.shots[self.index];
        let from = forward(&rotation(shot));
        let mut to = point;
        to.sub(&position(shot)).normalize();
        // Never past a half turn: beyond it the aim is left as it was.
        if from.dot(&to) < -0.2 {
            return;
        }
        self.aim.set_from_unit_vectors(&from, &to);
        self.aim.slerp(&Quaternion::default(), 0.6);
    }
    pub fn update(&mut self, dt: f64) -> Option<CameraShot> {
        if !self.active {
            return None;
        }
        self.age += dt;
        if self.age >= HOLD {
            self.show(self.index + 1);
        }
        let shot = self.shots[self.index];
        let t = (self.age / HOLD).clamp(0., 1.);
        let t = t * t * (3. - 2. * t);
        let mut pan = Quaternion::default();
        pan.set_from_axis_angle(
            &V::new(0., 1., 0.),
            self.turn * 4.5f64.to_radians() * (2. * t - 1.),
        );
        pan.multiply(&self.aim).multiply(&rotation(shot));
        let mut p = position(shot);
        let mut push = forward(&pan);
        push.multiply_scalar(2.5 * t);
        p.add(&push);
        Some(CameraShot {
            position: [p.x, p.y, p.z],
            quaternion: [pan.x, pan.y, pan.z, pan.w],
        })
    }
}
pub(super) fn inverse(shot: CameraShot) -> [f64; 16] {
    let mut m = Matrix4::default();
    m.compose(&position(shot), &rotation(shot), &V::new(1., 1., 1.))
        .invert();
    m.elements
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two shots on either side of the origin, both looking down -z (identity orientation).
    fn shots() -> Vec<CameraShot> {
        [-50., 50.]
            .into_iter()
            .map(|x| CameraShot {
                position: [x, 2., 0.],
                quaternion: [0., 0., 0., 1.],
            })
            .collect()
    }

    #[test]
    fn a_post_picks_the_shot_that_sees_it_and_cuts_once() {
        let mut director = Director::new(shots());
        director.open();
        director.index = 0;
        director.take_cut();
        director.follow([55., 0., -30.]);
        assert_eq!(director.index, 1);
        assert!(director.take_cut());
        assert!(!director.take_cut());
    }

    #[test]
    fn following_a_post_in_the_same_shot_keeps_the_hold_running() {
        let mut director = Director::new(shots());
        director.open();
        director.follow([55., 0., -30.]);
        director.update(4.);
        director.follow([60., 0., -40.]);
        assert!(director.age >= 4., "the same shot is not restarted");
    }

    #[test]
    fn a_shot_is_held_for_eleven_seconds_then_the_next_one_cuts_in() {
        let mut director = Director::new(shots());
        director.open();
        director.follow([55., 0., -30.]);
        director.take_cut();
        director.update(10.9);
        assert_eq!(director.index, 1);
        director.update(0.2);
        assert_eq!(director.index, 0);
        assert!(director.take_cut());
    }
}
