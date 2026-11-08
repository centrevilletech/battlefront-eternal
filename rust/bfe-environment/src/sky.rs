//! The sky's flyers (src/sky-flyers.js): the ships of a class kept in the sky
//! all the time, flown as the 2004 game flies them, and, Enhanced, drawn past
//! the far plane and faded smoothly. A frame flies each ship, then lays out
//! the draws far to near from the eye as instance records.

use crate::common::{sort_descending, EngineRandom};
use crate::layout::{sky_class as sc, sky_frame as sf, sky_result as sr};
use jsrt::conv::{js_hypot2, js_hypot3, js_max, js_min};

// Constants of the flight: ships fade in and out over four seconds; a ship is
// retired this far beyond its ring; the Enhanced sky draws far ships scaled
// into the far plane (REACH of it) and hazes the low far ones (up to HAZE).
const FADE_RATE: f64 = 0.25; // of full alpha a second: 4 s in, 4 s out
const KILL_MARGIN: f64 = 10.0; // m past Distance, across the ground
const PRE_ROLL: f64 = 1.0 / FADE_RATE;
const REACH: f64 = 0.98;
const HAZE: f64 = 0.7;
const SOLID: f64 = 0.999;

#[derive(Clone, Copy, Default)]
// A ship: where it is, which way it heads, its velocity in its own axes, its
// age and fade (negative while fading out), the matrix it was drawn with and
// the one before (the upscaler's motion), and its distance from the eye.
struct Ship {
    position: [f64; 3],
    forward: [f64; 3],
    velocity: [f64; 3],
    age: f64,
    fade: f64,
    entered: bool,
    matrix: [f64; 16],
    previous: [f64; 16],
    distance: f64,
}

/// One class: its definition, its ships and the records its draws read.
struct Kind {
    count: usize,
    distance: f64,
    height: [f64; 2],
    in_direction: f64,
    velocity: [[f64; 2]; 3],
    acceleration: [f64; 3],
    life_time: f64,
    radius: f64,
    classic: bool,
    enhanced: bool,
    ships: Vec<Ship>,
    order: Vec<u32>,
    scratch: Vec<u32>,
    keys: Vec<f64>,
    /// solid matrices and looks, fading ones, motion's matrices, previous
    /// matrices and shown flags, in the order the page names them.
    arrays: [Vec<f32>; 7],
    solids: usize,
    fadings: usize,
    on: bool,
}

// A draw in a range, as the engine draws them.
fn span(range: [f64; 2], random: &mut EngineRandom) -> f64 {
    range[0] + random.float() * (range[1] - range[0])
}

fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = js_min(js_max((x - a) / (b - a), 0.0), 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The model's x along right, y up, z along its heading (placement), as
/// matrix elements.
// The model's x along its right, y up, z along its heading: the ship never
// turns or banks, so its matrix is its heading and its place.
fn placement(ship: &Ship) -> [f64; 16] {
    let f = ship.forward;
    let p = ship.position;
    [f[2], 0.0, -f[0], 0.0, 0.0, 1.0, 0.0, 0.0, f[0], 0.0, f[2], 0.0, p[0], p[1], p[2], 1.0]
}

impl Kind {
    /// A ship starts again: the draws in the 2004 game's order (launch).
    // A ship starts again: Distance from the world's origin, in a random
    // direction, at a height in range, heading across that direction (either way,
    // turned inward by a random share), with a velocity drawn in its own axes.
    // The draws are made in the 2004 game's order.
    fn launch_ship(&self, random: &mut EngineRandom, ship: &mut Ship) {
        ship.fade = 0.0;
        // A direction with both parts zero is drawn again.
        let (mut dx, mut dz) = (0.0f64, 0.0f64);
        while dx == 0.0 && dz == 0.0 {
            dz = random.float() - 0.5;
            dx = random.float() - 0.5;
        }
        let length = js_hypot2(dx, dz);
        dx /= length;
        dz /= length;
        let height = span(self.height, random);
        // Where it starts, on the ring.
        ship.position = [dx * self.distance, height, dz * self.distance];
        // Across the direction it starts in (up x direction), either way, then
        // turned inward.
        // Across the direction it starts in (up cross direction), either way.
        let (mut tx, mut tz) = (dz, -dx);
        if random.float() > 0.5 {
            tx = -tx;
            tz = -tz;
        }
        if self.in_direction > 0.0 {
            let inward = random.float() * self.in_direction * 0.5 + self.in_direction * 0.5;
            tx -= dx * inward;
            tz -= dz * inward;
        }
        let heading = js_hypot2(tx, tz);
        ship.forward = [tx / heading, 0.0, tz / heading];
        // Its velocity in its own axes, drawn forward first.
        // Its velocity in its own axes, drawn forward first.
        let vz = span(self.velocity[2], random);
        let vy = span(self.velocity[1], random);
        let vx = span(self.velocity[0], random);
        ship.velocity = [vx, vy, vz];
        ship.age = self.life_time * (random.float() * 0.2 + 0.8);
        ship.entered = true;
    }

    /// Enhanced: the same start, the same draws, then PRE_ROLL seconds back
    /// along its course (its acceleration undone too) (launchEnhanced).
    // Enhanced: the same start, the same draws, then four seconds back along its
    // course (its acceleration undone too), so it fades in on the way and is
    // solid where the original starts it.
    fn launch_enhanced(&self, random: &mut EngineRandom, ship: &mut Ship) {
        self.launch_ship(random, ship);
        let t = PRE_ROLL;
        let (v, a, f) = (ship.velocity, self.acceleration, ship.forward);
        // Undo PRE_ROLL seconds of flight: position back along the heading, speed
        // back by the acceleration.
        let back = [v[0] * t - 0.5 * a[0] * t * t, v[1] * t - 0.5 * a[1] * t * t, v[2] * t - 0.5 * a[2] * t * t];
        ship.position[0] -= f[2] * back[0] + f[0] * back[2];
        ship.position[1] -= back[1];
        ship.position[2] -= -f[0] * back[0] + f[2] * back[2];
        for k in 0..3 {
            ship.velocity[k] -= a[k] * t;
        }
        ship.age -= t;
        ship.entered = false;
    }

    /// Its flight over dt in its own axes: Acceleration first, then the move.
    // Its flight over dt in its own axes: acceleration first, then the move.
    fn advance(&self, ship: &mut Ship, dt: f64) {
        let a = self.acceleration;
        ship.velocity[0] += a[0] * dt;
        ship.velocity[1] += a[1] * dt;
        ship.velocity[2] += a[2] * dt;
        let (v, f) = (ship.velocity, ship.forward);
        ship.position[0] += (f[2] * v[0] + f[0] * v[2]) * dt;
        ship.position[1] += v[1] * dt;
        ship.position[2] += (-f[0] * v[0] + f[2] * v[2]) * dt;
    }

    /// One frame of a ship. False when it has just faded out (fly).
    // One frame of a ship. Past its ring or its lifetime its fade is set to
    // minus one: drawn as its absolute value it jumps to full and fades out,
    // still flying, and then it starts again. False when it has just faded out.
    fn fly(&self, ship: &mut Ship, dt: f64) -> bool {
        if ship.fade >= 0.0 {
            let reach = self.distance + KILL_MARGIN;
            let away = ship.position[0] * ship.position[0] + ship.position[2] * ship.position[2];
            if away > reach * reach || ship.age > self.life_time {
                ship.fade = -1.0;
            }
        }
        ship.age += dt;
        let before = ship.fade;
        ship.fade = js_min(before + dt * FADE_RATE, 1.0);
        if ship.fade >= 0.0 && before < 0.0 {
            return false;
        }
        self.advance(ship, dt);
        true
    }

    /// Enhanced: a ship leaves its ring only once it has been inside it, and
    /// fades out from the alpha it has (flyEnhanced).
    // Enhanced: a ship leaves its ring only once it has been inside it, and fades
    // out from the alpha it has (no jump).
    fn fly_enhanced(&self, ship: &mut Ship, dt: f64) -> bool {
        let reach = self.distance + KILL_MARGIN;
        let away = ship.position[0] * ship.position[0] + ship.position[2] * ship.position[2];
        if away <= reach * reach {
            ship.entered = true;
        }
        if ship.fade >= 0.0 && ((ship.entered && away > reach * reach) || ship.age > self.life_time) {
            ship.fade = -js_max(ship.fade, 1e-6);
        }
        ship.age += dt;
        let before = ship.fade;
        ship.fade = js_min(before + dt * FADE_RATE, 1.0);
        if ship.fade >= 0.0 && before < 0.0 {
            return false;
        }
        self.advance(ship, dt);
        true
    }
}

/// Enhanced: where a ship is drawn so the far plane doesn't cut it: one
/// reaching past REACH x far is scaled about the eye by the share that brings
/// its far side there. Returns the scale and writes the placement drawn
/// (withinReach).
// Enhanced: a ship reaching past REACH of the far plane is scaled about the eye
// by the share that brings its far side there: its image is the same, it lies
// inside the far plane, and the shader puts its depth just inside it.
fn within_reach(matrix: &[f64; 16], eye: [f64; 3], radius: f64, far: f64, out: &mut [f64; 16]) -> f64 {
    let (x, y, z) = (matrix[12] - eye[0], matrix[13] - eye[1], matrix[14] - eye[2]);
    let distance = js_hypot3(x, y, z);
    let reach = far * REACH;
    *out = *matrix;
    if !(distance + radius > reach) || !(reach > 0.0) {
        return 1.0;
    }
    let scale = reach / (distance + radius);
    for i in 0..12 {
        if i % 4 != 3 {
            out[i] *= scale;
        }
    }
    out[12] = eye[0] + x * scale;
    out[13] = eye[1] + y * scale;
    out[14] = eye[2] + z * scale;
    scale
}

/// Enhanced: how much of the sky behind it a ship at `distance`, `rise`
/// metres above the eye, takes on (haze).
// How much of the sky behind it a ship takes on: none overhead or near, up to
// HAZE low on the horizon three times the far plane away.
pub fn haze(distance: f64, rise: f64, far: f64) -> f64 {
    if !(distance > 0.0) || !(far > 0.0) {
        return 0.0;
    }
    HAZE * (1.0 - smooth(0.04, 0.35, rise / distance)) * smooth(0.6, 3.0, distance / far)
}

pub struct Sky {
    random: EngineRandom,
    enhanced: bool,
    kinds: Vec<Kind>,
}

impl Sky {
    /// `config` is the SkyConfig record then a SkyClass record per class.
    // Only the classes flying in the current mode are launched now: one coming in
    // later (a live switch) starts then, so Classic's draws are the original's
    // alone.
    pub fn new(config: &[f64]) -> Sky {
        use crate::layout::sky_config as k;
        // One kind (class) per SkyClass record of the page's configuration.
        let mut kinds = Vec::new();
        for i in 0..config[k::CLASSES] as usize {
            let c = &config[k::STRIDE + i * sc::STRIDE..k::STRIDE + (i + 1) * sc::STRIDE];
            let count = c[sc::COUNT] as usize;
            kinds.push(Kind {
                count,
                distance: c[sc::DISTANCE],
                height: [c[sc::HEIGHT], c[sc::HEIGHT + 1]],
                in_direction: c[sc::IN_DIRECTION],
                velocity: [[c[sc::VELOCITY], c[sc::VELOCITY + 1]], [c[sc::VELOCITY + 2], c[sc::VELOCITY + 3]], [c[sc::VELOCITY + 4], c[sc::VELOCITY + 5]]],
                acceleration: [c[sc::ACCELERATION], c[sc::ACCELERATION + 1], c[sc::ACCELERATION + 2]],
                life_time: c[sc::LIFE_TIME],
                radius: c[sc::RADIUS],
                classic: c[sc::CLASSIC] != 0.0,
                enhanced: c[sc::ENHANCED] != 0.0,
                ships: vec![Ship::default(); count],
                order: vec![0; count],
                scratch: vec![0; count],
                keys: vec![0.0; count],
                arrays: [vec![0.0; count * 16], vec![0.0; count * 4], vec![0.0; count * 16], vec![0.0; count * 4], vec![0.0; count * 16], vec![0.0; count * 16], vec![0.0; count]],
                solids: 0,
                fadings: 0,
                on: false,
            });
        }
        let mut sky = Sky { random: EngineRandom::new(config[k::SEED]), enhanced: config[k::ENHANCED] != 0.0, kinds };
        // Only the classes flying now: one coming in later starts then, so
        // Classic's random draws are the original's alone.
        for i in 0..sky.kinds.len() {
            if sky.active(i) {
                sky.relaunch(i);
            }
        }
        sky
    }

    // SkyFlyers.count is a native metadata query, independent of the eye.
    pub fn enhanced(&self)->bool{self.enhanced}
    pub fn probe_needed(&self)->bool{self.enhanced&&!self.kinds.is_empty()}
    pub fn count(&self)->usize { self.kinds.iter().enumerate().filter(|(i,_)|self.active(*i)).map(|(_,k)|k.count).sum() }
    pub fn active(&self, i: usize) -> bool {
        self.kinds.get(i).is_some_and(|k|if self.enhanced {k.enhanced}else{k.classic})
    }

    fn launch(&mut self, i: usize, s: usize) {
        let kind = &mut self.kinds[i];
        let mut ship = kind.ships[s];
        if self.enhanced {
            kind.launch_enhanced(&mut self.random, &mut ship);
        } else {
            kind.launch_ship(&mut self.random, &mut ship);
        }
        ship.matrix = placement(&ship);
        ship.previous = ship.matrix;
        kind.ships[s] = ship;
    }

    fn relaunch(&mut self, i: usize) {
        for s in 0..self.kinds[i].count {
            self.launch(i, s);
        }
    }

    /// Classic or Enhanced, as the setting changes: classes coming in start
    /// afresh; ships flying on keep their course under the new rules
    /// (setEnhanced).
    // Classic or Enhanced, as the setting changes: classes coming in start
    // afresh; ships flying on keep their course under the new rules (and may
    // leave their ring only once they have been inside it).
    pub fn set_enhanced(&mut self, enhanced: bool) {
        if enhanced == self.enhanced {
            return;
        }
        // Which classes flew before the switch.
        let before=self.enhanced;
        self.enhanced = enhanced;
        for i in 0..self.kinds.len() {
            if !self.active(i) {
                continue;
            }
            let was=if before{self.kinds[i].enhanced}else{self.kinds[i].classic};
            if !was {
                self.relaunch(i);
            } else if enhanced {
                let reach = self.kinds[i].distance + KILL_MARGIN;
                for ship in self.kinds[i].ships.iter_mut() {
                    ship.entered = ship.position[0] * ship.position[0] + ship.position[2] * ship.position[2] <= reach * reach;
                }
            }
        }
    }

    /// A frame of flight, then the draws: far to near from the eye
    /// (update, place). `frame` is a SkyFrame record; `result` takes a
    /// SkyResult per class.
    // A frame of flight, then the draws far to near from the eye.
    pub fn update(&mut self, frame: &[f64], result: &mut [f64]) {
        // Flight happens only while time passes (a paused frame lays out the draws
        // and nothing more).
        let dt = frame[sf::DT];
        if dt > 0.0 {
            for i in 0..self.kinds.len() {
                if !self.active(i) {
                    continue;
                }
                for s in 0..self.kinds[i].count {
                    let kind = &self.kinds[i];
                    let mut ship = kind.ships[s];
                    ship.previous = ship.matrix;
                    let flying = if self.enhanced { kind.fly_enhanced(&mut ship, dt) } else { kind.fly(&mut ship, dt) };
                    if flying {
                        ship.matrix = placement(&ship);
                        self.kinds[i].ships[s] = ship;
                    } else {
                        self.kinds[i].ships[s] = ship;
                        self.launch(i, s);
                    }
                }
            }
        }
        self.place(frame, result);
    }

    // The draws: each class's ships are sorted by distance from the eye (kept in
    // gathering order without one), the solid ones and the Enhanced fading ones
    // are written apart (fading ones write no depth, so they are drawn last),
    // and their motion records are the true placement.
    fn place(&mut self, frame: &[f64], result: &mut [f64]) {
        // With no eye (the camera was missing for a frame) the draws are laid out in
        // gathering order and none is scaled or hazed.
        let has_eye = frame[sf::HAS_EYE] != 0.0;
        let eye = [frame[sf::EYE], frame[sf::EYE + 1], frame[sf::EYE + 2]];
        let far = frame[sf::FAR];
        let enhanced = self.enhanced && has_eye && far > 0.0;
        // The haze needs the sky behind (the page's probe); without it, none.
        // The haze needs the sky behind the ships, which the page's probe supplies;
        // without it there is none.
        let misty = enhanced && frame[sf::MISTY] != 0.0;
        for i in 0..self.kinds.len() {
            let on = self.active(i) && frame[sf::VISIBLE] != 0.0;
            let smooth_fades = self.enhanced;
            let kind = &mut self.kinds[i];
            kind.on = on;
            if !on {
                kind.solids = 0;
                kind.fadings = 0;
                Self::write_result(kind, &mut result[i * crate::layout::sky_result::STRIDE..]);
                continue;
            }
            let n = kind.count;
            for s in 0..n {
                let ship = &mut kind.ships[s];
                ship.distance = if has_eye {
                    let (dx, dy, dz) = (ship.position[0] - eye[0], ship.position[1] - eye[1], ship.position[2] - eye[2]);
                    dx * dx + dy * dy + dz * dz
                } else {
                    0.0
                };
                // (Squared distances: only their order matters.)
                kind.keys[s] = ship.distance;
                kind.order[s] = s as u32;
                // Its motion, at its true placement now and then.
                for k in 0..16 {
                    kind.arrays[4][s * 16 + k] = ship.matrix[k] as f32;
                    kind.arrays[5][s * 16 + k] = ship.previous[k] as f32;
                }
                kind.arrays[6][s] = if ship.fade.abs() >= 0.5 { 1.0 } else { 0.0 };
            }
            if has_eye {
                sort_descending(&kind.keys[..n], &mut kind.order[..n], &mut kind.scratch[..n]);
            }
            let (mut solids, mut fadings) = (0, 0);
            for o in 0..n {
                let ship = kind.ships[kind.order[o] as usize];
                let alpha = ship.fade.abs();
                let (mut scale, mut mist) = (1.0, 0.0);
                let mut drawn = ship.matrix;
                if enhanced {
                    scale = within_reach(&ship.matrix, eye, kind.radius, far, &mut drawn);
                    if misty {
                        mist = haze(ship.distance.sqrt(), ship.position[1] - eye[1], far);
                    }
                }
                // In Enhanced, a ship that has not fully faded in or out is drawn with the
                // fading ones (no depth write).
                let fading = smooth_fades && alpha < SOLID;
                let (m, l, at) = if fading { (2, 3, fadings) } else { (0, 1, solids) };
                if fading { fadings += 1 } else { solids += 1 }
                for k in 0..16 {
                    kind.arrays[m][at * 16 + k] = drawn[k] as f32;
                }
                kind.arrays[l][at * 4] = alpha as f32;
                kind.arrays[l][at * 4 + 1] = mist as f32;
                kind.arrays[l][at * 4 + 2] = if scale < 1.0 { 1.0 } else { 0.0 };
                kind.arrays[l][at * 4 + 3] = scale as f32;
            }
            kind.solids = solids;
            kind.fadings = fadings;
            Self::write_result(kind, &mut result[i * crate::layout::sky_result::STRIDE..]);
        }
    }

    fn write_result(kind: &Kind, r: &mut [f64]) {
        r[sr::SOLIDS] = kind.solids as f64;
        r[sr::FADINGS] = kind.fadings as f64;
        r[sr::ON] = f64::from(kind.on);
    }

    pub fn array(&self, class: usize, which: usize) -> &[f32] {
        &self.kinds[class].arrays[which]
    }
}
