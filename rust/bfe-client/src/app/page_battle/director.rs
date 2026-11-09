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
impl Director {
    pub fn new(shots: Vec<CameraShot>) -> Self {
        Self {
            shots,
            index: 0,
            age: 0.,
            turn: 1.,
            aim: Quaternion::default(),
            active: false,
        }
    }
    pub fn open(&mut self) {
        self.active = !self.shots.is_empty();
        self.age = 0.;
        self.aim = Quaternion::default();
    }
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
            let distance = delta.length();
            delta.normalize();
            let facing = forward(&rotation(shot)).dot(&delta);
            let value = distance * (2.2 - facing);
            if value < score {
                best = i;
                score = value;
            }
        }
        if best != self.index {
            self.turn = -self.turn;
        }
        self.index = best;
        self.age = 0.;
        let shot = self.shots[best];
        let from = forward(&rotation(shot));
        let mut to = point;
        to.sub(&position(shot)).normalize();
        self.aim = Quaternion::default();
        if from.dot(&to) >= -0.2 {
            self.aim.set_from_unit_vectors(&from, &to);
            self.aim.slerp(&Quaternion::default(), 0.6);
        }
    }
    pub fn update(&mut self, dt: f64) -> Option<CameraShot> {
        if !self.active {
            return None;
        }
        self.age += dt;
        if self.age >= 11. {
            self.age = 0.;
            self.index = (self.index + 1) % self.shots.len();
            self.turn = -self.turn;
            self.aim = Quaternion::default();
        }
        let shot = self.shots[self.index];
        let t = (self.age / 11.).clamp(0., 1.);
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
