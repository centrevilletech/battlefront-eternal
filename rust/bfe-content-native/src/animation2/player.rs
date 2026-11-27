//! Sample the poses bracketing the display clock, independently of packet and draw rates.
use super::{
    state::{Action, Air, Direction, Row, State, TumblePhase},
    Class, Local, Mask,
};
use three::{Matrix4, Quaternion, Vector3};

// The playout clock allows ten ticks of delay. Sixteen consecutive publications
// cover that delay and both endpoints, including a three-tick server interval.
const HISTORY: usize = 16;
/// Maximum torso aim pitch, matching the authored 0.7 * pi/2 vertical limit.
const MAX_AIM_PITCH: f64 = 0.7 * std::f64::consts::FRAC_PI_2;

/// All character leases share storage allocated at admission.
pub struct Arena {
    stride: usize,
    history: Vec<Local>,
    next: Vec<Local>,
    transition: Vec<Local>,
    overlay: Vec<Local>,
    model_space: Vec<Matrix4>,
    world: Vec<Matrix4>,
    palette: Vec<f32>,
}
pub struct Pose<'a> {
    history: &'a mut [Local],
    next: &'a mut [Local],
    transition: &'a mut [Local],
    overlay: &'a mut [Local],
    pub model_space: &'a mut [Matrix4],
    pub world: &'a mut [Matrix4],
    pub palette: &'a mut [f32],
}
impl Arena {
    pub fn new(slots: usize, joints: usize) -> Result<Self, String> {
        super::check_joints(joints)?;
        let n = slots
            .checked_mul(joints)
            .ok_or("character pose capacity overflow")?;
        let palette = n
            .checked_mul(16)
            .ok_or("character palette capacity overflow")?;
        Ok(Self {
            stride: joints,
            history: vec![
                Local::default();
                n.checked_mul(HISTORY)
                    .ok_or("pose history capacity overflow")?
            ],
            next: vec![Local::default(); n],
            transition: vec![Local::default(); n],
            overlay: vec![Local::default(); n],
            model_space: vec![Matrix4::default(); n],
            world: vec![Matrix4::default(); n],
            palette: vec![0.; palette],
        })
    }
    pub fn slot(&mut self, slot: usize, joints: usize) -> Pose<'_> {
        assert!(joints <= self.stride);
        let offset = slot * self.stride;
        let r = offset..offset + joints;
        Pose {
            history: &mut self.history[offset * HISTORY..offset * HISTORY + joints * HISTORY],
            next: &mut self.next[r.clone()],
            transition: &mut self.transition[r.clone()],
            overlay: &mut self.overlay[r.clone()],
            model_space: &mut self.model_space[r.clone()],
            world: &mut self.world[r],
            palette: &mut self.palette[offset * 16..(offset + joints) * 16],
        }
    }
}
#[derive(Clone, Copy, Default)]
struct ClipSample {
    clip: Option<usize>,
    action: Option<usize>,
    time: f64,
    rate: f64,
    blend: f64,
    blend_progress: f64,
    repeats: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct ClipFrame<'a> {
    pub name: &'a str,
    pub action: &'a str,
    pub crc: u32,
    pub time: f64,
    pub duration: f64,
    pub rate: f64,
    pub blend: f64,
    pub blend_progress: f64,
    pub shown: f64,
}

#[derive(Default)]
pub struct Player {
    newest: usize,
    count: usize,
    states: [State; HISTORY],
    clips: [ClipSample; HISTORY],
    placed: Option<(f64, Matrix4)>,
    key: Option<(usize, Row, Option<Row>)>,
    changed: u32,
    blend: f64,
    upper_transition: bool,
    upper_action: bool,
    last_action_time: f64,
    was_tumble_spin: bool,
    last_untumbled_root: Quaternion,
    phase: f64,
    backward: bool,
    jump_dir: Option<Option<Direction>>,
    jump_count: u32,
    jump_started_tick: u32,
    jump_last_vy: f64,
    steering: [f64; HISTORY],
    body_turn: [f64; HISTORY],
    aim_weights: [[f64; 2]; HISTORY],
    /// The body's clip changes counted up to each publication (see [`Player::changes_at`]).
    changes: [u32; HISTORY],
    change_count: u32,
    aim_from: [f64; 2],
    pub trail: bool,
    fallback: Option<Direction>,
    /// Number of lives this player presented using the generic fall.
    pub fallback_deaths: usize,
}
impl Player {
    pub fn reset(&mut self) {
        self.count = 0;
        self.key = None;
        self.placed = None;
        self.last_action_time = 0.;
        self.was_tumble_spin = false;
        self.last_untumbled_root = Quaternion::default();
        self.phase = 0.;
        self.backward = false;
        self.jump_dir = None;
        self.jump_count = 0;
        self.jump_started_tick = 0;
        self.jump_last_vy = 0.;
        self.aim_from = [0.; 2];
        self.trail = false;
        self.fallback = None;
    }
    pub fn publish(&mut self, c: &Class, s: State, pose: &mut Pose) -> bool {
        let cut = self.count != 0 && {
            let old = self.states[self.newest];
            s.tick >= old.tick
                && ((s.discontinuity && s.position != old.position)
                    || old.seat != s.seat
                    || (old.action == Action::Death && s.action != Action::Death))
        };
        let continuing_fall = self.fallback;
        if cut {
            self.reset();
            if s.action == Action::Death {
                self.fallback = continuing_fall;
            }
        }
        let previous = (self.count != 0).then(|| self.states[self.newest].tick);
        if previous.is_some_and(|t| s.tick <= t) {
            return false;
        }
        self.placed = None;
        if previous.is_none() || self.states[self.newest].tumble.map(|t| t.phase) != s.tumble.map(|t| t.phase) {
            self.change_count = self.change_count.wrapping_add(1);
        }
        let advance = previous.map_or(0., |t| f64::from(s.tick - t) / 60.);
        let mut jump_restarted = false;
        let jump_elapsed = if s.air == super::state::Air::Jump && s.roll.is_none() {
            let speed_sq = s.velocity[0] * s.velocity[0] + s.velocity[2] * s.velocity[2];
            let pick_dir = || {
                (c.tables.directional_jumps() && speed_sq >= 4.0)
                    .then(|| Direction::from_velocity(s.velocity, s.yaw))
            };
            if self.jump_dir.is_none() {
                self.jump_dir = Some(pick_dir());
                self.jump_count = s.jump_count;
                self.jump_started_tick = s.tick;
            } else {
                let restarted_jump = (s.jump_count != self.jump_count)
                    || (s.jump_count == 0 && s.velocity[1] - self.jump_last_vy > 4.0);
                if restarted_jump {
                    let elapsed = f64::from(s.tick.saturating_sub(self.jump_started_tick)) / 60.;
                    let next_dir = pick_dir();
                    let old_dir = self.jump_dir.flatten();
                    if next_dir != old_dir
                        || elapsed >= 0.5
                        || matches!(next_dir, None | Some(Direction::Forward))
                    {
                        self.jump_dir = Some(next_dir);
                        self.jump_started_tick = s.tick;
                        jump_restarted = true;
                    }
                    self.jump_count = s.jump_count;
                }
            }
            self.jump_last_vy = s.velocity[1];
            Some(f64::from(s.tick.saturating_sub(self.jump_started_tick)) / 60.)
        } else {
            self.jump_dir = None;
            self.jump_count = s.jump_count;
            self.jump_last_vy = s.velocity[1];
            None
        };
        let mut sample = c.tables.sample_after(
            &c.animation,
            &s,
            self.phase,
            self.backward,
            self.jump_dir,
            jump_elapsed,
        );
        self.phase += advance * sample.rate;
        if sample.gait_clock {
            sample.motion_time = self.phase;
        }
        self.backward = sample.backward;
        let rig = &c.animation.rig;
        let speed = s.velocity[0].hypot(s.velocity[2]);
        let upper_action = sample.action.is_some_and(|action| {
            if sample.combo_action {
                !action.full_body && (speed > 0.3 || s.air != super::state::Air::Ground)
            } else {
                matches!(
                    s.action,
                    Action::Fire
                        | Action::FireSecondary
                        | Action::FireSecondary2
                        | Action::Throw
                        | Action::Reload
                ) && (speed > 1e-4 || s.air != super::state::Air::Ground)
            }
        });
        // Gates every pitch of the body; the torso also needs a gun or an action that aims.
        let aims_gate = s.seat.is_none()
            && s.choke_clip.is_none()
            && s.tumble.is_none()
            && s.air != Air::Thrown
            && s.action != Action::Death
            && !sample.combo_action;
        let torso_gate = c.tables.aims(s.weapon) || sample.action.is_some_and(|a| a.aims);
        let tumble_spin_q = if s.action != Action::Death {
            s.tumble
                .filter(|t| t.phase == TumblePhase::Thrown)
                .map(|t| t.angle)
                .filter(|a| a.is_finite() && a.abs() > 1e-6)
                .map(|a| {
                    let (sy, cy) = s.yaw.sin_cos();
                    let fwd = (s.velocity[0] * sy + s.velocity[2] * cy).abs();
                    let right = (s.velocity[2] * sy - s.velocity[0] * cy).abs();
                    let len = fwd.hypot(right);
                    let (ax, az) = if len > 1e-4 {
                        (fwd / len, right / len)
                    } else {
                        (1., 0.)
                    };
                    let (sa, ca) = (a * 0.5).sin_cos();
                    Quaternion::new(sa * ax, 0., sa * az, ca)
                })
        } else {
            None
        };
        let aim_weights;
        let mut locks_body_yaw = false;
        if s.action == Action::Death && sample.action.is_none() && sample.tumble_turn.is_none() {
            aim_weights = [0.; 2];
            if self.fallback.is_none() {
                if previous.is_some() {
                    pose.transition.copy_from_slice(pose.next);
                } else {
                    for (local, joint) in pose.transition.iter_mut().zip(&rig.joints) {
                        *local = joint.bind;
                    }
                }
                self.fallback = Some(s.impact.unwrap_or(Direction::Forward));
                self.fallback_deaths += 1;
                self.change_count = self.change_count.wrapping_add(1);
            }
            pose.next.copy_from_slice(pose.transition);
            // Rotate the held pose around its ground origin, away from impact.
            let angle = s.elapsed.clamp(0., 0.6) / 0.6 * std::f64::consts::FRAC_PI_4;
            let (v, w) = angle.sin_cos();
            let rotation = match self.fallback.unwrap() {
                Direction::Forward => Quaternion::new(-v, 0., 0., w),
                Direction::Backward => Quaternion::new(v, 0., 0., w),
                Direction::Left => Quaternion::new(0., 0., v, w),
                Direction::Right => Quaternion::new(0., 0., -v, w),
            };
            for (local, joint) in pose.next.iter_mut().zip(&rig.joints) {
                if joint.parent.is_none() {
                    local.position.apply_quaternion(&rotation);
                    local.rotation.premultiply(&rotation);
                }
            }
        } else {
            self.fallback = None;
            let key = (s.weapon, sample.motion, sample.action);
            let primary = sample.action.unwrap_or(sample.motion);
            locks_body_yaw = primary.aim_type.locks_body_yaw();
            let aim_target = if aims_gate {
                let weights = primary.aim_type.weights();
                [if torso_gate { weights[0] } else { 0. }, weights[1]]
            } else {
                [0.; 2]
            };
            let action_restarted = self.key == Some(key)
                && sample.action.is_some_and(|r| !r.repeats)
                && sample.action_time + 1e-6 < self.last_action_time;
            if self.key != Some(key)
                || self.upper_action != upper_action
                || jump_restarted
                || action_restarted
            {
                self.aim_from = if previous.is_some() {
                    self.aim_weights[self.newest]
                } else {
                    aim_target
                };
                self.upper_transition = !jump_restarted
                    && self.key.is_some_and(|(weapon, gait, old)| {
                        weapon == s.weapon
                            && gait == sample.motion
                            && (old.is_none() || self.upper_action)
                            && (sample.action.is_none() || upper_action)
                    });
                self.upper_action = upper_action;
                self.change_count = self.change_count.wrapping_add(1);
                // The interval ending at this publication already elapsed.
                self.changed = previous.unwrap_or(s.tick);
                self.blend = if action_restarted {
                    primary.blend_in
                } else {
                    self.key.map_or(0., |(weapon, base, action)| {
                        let old = action.unwrap_or(base);
                        if weapon == s.weapon {
                            c.tables.blend_time(weapon, old, primary)
                        } else {
                            primary.blend_in.min(old.blend_out)
                        }
                    })
                };
                pose.transition.copy_from_slice(pose.next);
                // The game writes the somersault onto the drawn pose's root only, after its clips are
                // blended: no blend ever starts from it, so the root is back on its clip the moment the
                // spin ends, whatever the next clip's blend time.
                if self.was_tumble_spin {
                    if let Some(root) = rig.root {
                        pose.transition[root].rotation = self.last_untumbled_root;
                    }
                }
                // A fixed tick can leave the roll between two publications.
                // Recover from the dive's endpoint when its window actually expired.
                let old = self.states[self.newest];
                if old.roll.is_some()
                    && s.roll.is_none()
                    && sample.action.is_none()
                    && old.roll_seconds.is_some_and(|window| {
                        old.phase_time.unwrap_or(old.elapsed) + advance + 1e-9 >= window
                    })
                {
                    if let Some((_, Row { base: Some(id), .. }, None)) = self.key {
                        let clip = &c.animation.clips[id];
                        clip.apply(rig, clip.duration(), false, Mask::All, pose.transition);
                        self.changed = s.tick;
                    }
                }
                self.key = Some(key);
            }
            self.last_action_time = if sample.action.is_some() {
                sample.action_time
            } else {
                0.
            };
            for (v, j) in pose.next.iter_mut().zip(&rig.joints) {
                *v = j.bind;
            }
            let mut apply = |row: Row, time: f64, upper: bool| {
                let clips = &c.animation.clips;
                let mut layer = |id: Option<usize>, mask| {
                    if let Some(i) = id {
                        clips[i].apply(rig, time, row.repeats, mask, pose.next);
                    }
                };
                if upper {
                    if rig.torso.is_some() {
                        layer(row.base, Mask::Upper(&rig.upper));
                        layer(row.upper, Mask::Upper(&rig.upper));
                    } else {
                        // Without an anatomical split, only an explicitly upper clip owns this layer.
                        if let Some(lower) = row.lower {
                            layer(row.upper, Mask::Over(&clips[lower]));
                        } else {
                            layer(row.upper, Mask::All);
                        }
                    }
                    return;
                }
                layer(row.base, Mask::All);
                if row.lower != row.upper {
                    layer(row.lower, Mask::All);
                }
                let mask = match row.lower.filter(|id| Some(*id) != row.upper) {
                    Some(_) if row.inherited_gait && rig.torso.is_some() => Mask::Upper(&rig.upper),
                    Some(i) => Mask::Over(&clips[i]),
                    None => Mask::All,
                };
                layer(row.upper, mask);
            };
            apply(sample.motion, sample.motion_time, false);
            if let Some(r) = sample.action {
                apply(r, sample.action_time, upper_action);
            }
            let progress = if self.blend > 0. {
                f64::from(s.tick - self.changed) / (60. * self.blend)
            } else {
                1.
            };
            let t = progress.clamp(0., 1.);
            aim_weights = [
                self.aim_from[0] + (aim_target[0] - self.aim_from[0]) * t,
                self.aim_from[1] + (aim_target[1] - self.aim_from[1]) * t,
            ];
            if progress < 1. {
                for (i, (dst, from)) in pose.next.iter_mut().zip(pose.transition.iter()).enumerate()
                {
                    if !self.upper_transition || rig.upper[i] {
                        *dst = between(*from, *dst, progress);
                    }
                }
            }
            if let Some((row, time, weight)) = sample.overlay {
                pose.overlay.copy_from_slice(pose.next);
                let clips = &c.animation.clips;
                for id in [row.base, row.lower, row.upper].into_iter().flatten() {
                    clips[id].apply(rig, time, row.repeats, Mask::All, pose.overlay);
                }
                // A full-body swing keeps its legs; the hit then shows above the hips only.
                let legs = !sample.action.is_some_and(|a| a.full_body) || rig.torso.is_none();
                for (i, (dst, hit)) in pose.next.iter_mut().zip(pose.overlay.iter()).enumerate() {
                    if legs || rig.upper[i] {
                        *dst = between(*dst, *hit, weight);
                    }
                }
            }
            if let (Some(root), Some(tq)) = (rig.root, tumble_spin_q) {
                self.last_untumbled_root = pose.next[root].rotation;
                pose.next[root].rotation = tq;
                self.was_tumble_spin = true;
            } else {
                self.was_tumble_spin = false;
            }
        }
        // A directional dive turns the whole body. Its turn and the gait's
        // travel angle are alternative targets for the same lower-body yaw.
        let dive = if sample.action.is_none() {
            sample.motion.turn
        } else {
            0.
        };
        let travel = if !locks_body_yaw
            && self.fallback.is_none()
            && (sample.action.is_none() || upper_action)
        {
            sample.travel_yaw + dive
        } else {
            0.
        };
        let share = 1. - 0.75_f64.powf(30. * advance);
        let toward = |old: f64, target: f64| {
            if previous.is_some() {
                old + ((target - old) * share).clamp(-8. * advance, 8. * advance)
            } else {
                target
            }
        };
        let (yaw, body) = if let Some(turn) = sample.tumble_turn {
            (turn, turn)
        } else {
            (
                toward(self.steering[self.newest], travel),
                toward(self.body_turn[self.newest], dive),
            )
        };
        self.newest = (self.newest + 1) % HISTORY;
        self.count = (self.count + 1).min(HISTORY);
        self.states[self.newest] = s;
        let clip_sample = if s.action == Action::Death && sample.action.is_none() {
            ClipSample::default()
        } else {
            let (row, raw_time) = match sample.action {
                Some(action) if !upper_action => (action, sample.action_time),
                _ => (sample.motion, sample.motion_time),
            };
            let clip_id = row.base.or(row.lower).or(row.upper);
            let time = clip_id.map_or(0., |id| {
                let dur = c.animation.clips[id].duration();
                if row.repeats && dur > 0. {
                    raw_time.rem_euclid(dur)
                } else {
                    raw_time.clamp(0., dur)
                }
            });
            let progress = if self.blend > 0. {
                (f64::from(s.tick - self.changed) / (60. * self.blend)).min(1.)
            } else {
                1.
            };
            let action_id = if upper_action {
                sample.action.and_then(|a| a.upper.or(a.base).or(a.lower))
            } else {
                None
            };
            ClipSample {
                clip: clip_id,
                action: action_id,
                time,
                rate: if sample.gait_clock { sample.rate } else { 1. },
                blend: self.blend,
                blend_progress: progress,
                repeats: row.repeats,
            }
        };
        self.clips[self.newest] = clip_sample;
        self.changes[self.newest] = self.change_count;
        self.steering[self.newest] = yaw;
        self.body_turn[self.newest] = body;
        self.aim_weights[self.newest] = aim_weights;
        let start = self.newest * pose.next.len();
        pose.history[start..start + pose.next.len()].copy_from_slice(pose.next);
        self.trail = sample.action.is_some_and(|a| a.attack);
        cut
    }

    fn bracket(&self, tick: f64) -> (usize, usize, f64) {
        let mut later = self.newest;
        for _ in 0..self.count.saturating_sub(1) {
            let earlier = (later + HISTORY - 1) % HISTORY;
            let from = f64::from(self.states[earlier].tick);
            if tick >= from {
                let span = f64::from(self.states[later].tick) - from;
                return (earlier, later, ((tick - from) / span).min(1.));
            }
            later = earlier;
        }
        (later, later, 0.)
    }

    /// Discrete fields take effect at their tick. The root follows the same
    /// two publications as the joints, including across the yaw wrap.
    pub fn state_at(&self, tick: f64) -> State {
        let (left, right, a) = self.bracket(tick);
        let p = self.states[left];
        let q = self.states[right];
        let mut state = if a >= 1. { q } else { p };
        state.position =
            std::array::from_fn(|i| p.position[i] + (q.position[i] - p.position[i]) * a);
        let turn = q.yaw - p.yaw;
        state.yaw = p.yaw + turn.sin().atan2(turn.cos()) * a;
        state.pitch = p.pitch + (q.pitch - p.pitch) * a;
        state
    }

    /// How many clip changes (a new clip or action, a restarted jump or action, a new tumble phase, the start of
    /// the generic fall) the body had made by each of the two publications that bracket display clock `tick`:
    /// the pose drawn at `tick` crosses a change when they differ.
    pub fn changes_at(&self, tick: f64) -> (u32, u32) {
        let (left, right, _) = self.bracket(tick);
        (self.changes[left], self.changes[right])
    }

    /// Interpolated primary clip and transition timing at display clock `tick`,
    /// using the same history bracket as [`Player::place_on`].
    pub fn clip_at<'a>(&self, c: &'a Class, tick: f64) -> Option<ClipFrame<'a>> {
        if self.count == 0 {
            return None;
        }
        let (left, right, a) = self.bracket(tick);
        let p = self.clips[left];
        let q = self.clips[right];
        let (id, time, rate, blend, blend_progress) = match (p.clip, q.clip) {
            (Some(l_id), Some(r_id)) if l_id == r_id => {
                let dur = c.animation.clips[l_id].duration();
                let t = if p.repeats && dur > 0. && q.time < p.time {
                    (p.time + a * (q.time + dur - p.time)).rem_euclid(dur)
                } else {
                    p.time + a * (q.time - p.time)
                };
                let bp = if q.blend == p.blend && q.blend_progress >= p.blend_progress {
                    p.blend_progress + a * (q.blend_progress - p.blend_progress)
                } else if a >= 1. {
                    q.blend_progress
                } else {
                    p.blend_progress
                };
                (l_id, t, p.rate + a * (q.rate - p.rate), p.blend, bp)
            }
            _ => {
                let s = if a >= 1. { q } else { p };
                (s.clip?, s.time, s.rate, s.blend, s.blend_progress)
            }
        };
        let action_id = if a >= 1. { q.action } else { p.action };
        let clip = &c.animation.clips[id];
        Some(ClipFrame {
            name: c.tables.clip_name(id),
            action: action_id.map_or("", |aid| c.tables.clip_name(aid)),
            crc: clip.crc,
            time,
            duration: clip.duration(),
            rate,
            blend,
            blend_progress,
            shown: tick,
        })
    }

    pub fn place_at(&mut self, c: &Class, tick: f64, _distant: bool, pose: &mut Pose) -> Matrix4 {
        let state = self.state_at(tick);
        let [x, y, z] = state.position;
        let position = Vector3::new(x, y, z);
        let yaw = state.yaw;
        let mut root = Matrix4::default();
        let rotation = Quaternion::new(0., (yaw * 0.5).sin(), 0., (yaw * 0.5).cos());
        root.compose(&position, &rotation, &Vector3::new(1., 1., 1.));
        self.place_on(c, tick, root, pose)
    }

    /// A seat supplies its complete displayed frame, including vehicle roll and pitch.
    pub fn place_on(&mut self, c: &Class, tick: f64, root: Matrix4, pose: &mut Pose) -> Matrix4 {
        if self.placed == Some((tick, root)) {
            return root;
        }
        let (left, right, a) = self.bracket(tick);
        let state = self.state_at(tick);
        let rig = &c.animation.rig;
        let joints = rig.joints.len();
        let yaw = self.steering[left];
        let delta = (self.steering[right] - yaw).sin().atan2((self.steering[right] - yaw).cos());
        let travel = yaw + a * delta;
        let b0 = self.body_turn[left];
        let bdelta = (self.body_turn[right] - b0).sin().atan2((self.body_turn[right] - b0).cos());
        // A combo state's turn toward the stick turns the body above the hips with the swing.
        let w0 = self.states[left].swing_turn;
        let wdelta = (self.states[right].swing_turn - w0).sin().atan2((self.states[right].swing_turn - w0).cos());
        let body = b0 + a * bdelta + w0 + a * wdelta;
        let hip_limit = 72_f64.to_radians();
        let hips = if rig.thighs.iter().all(Option::is_some) {
            body + (travel - body).clamp(-hip_limit, hip_limit)
        } else {
            travel
        };
        let turn = |angle| {
            let mut m = Matrix4::default();
            m.make_rotation_y(angle);
            m
        };
        let lower = turn(hips);
        let upper = turn(body - hips);
        let foot = turn(travel - hips);
        let torso_weight = self.aim_weights[left][0]
            + a * (self.aim_weights[right][0] - self.aim_weights[left][0]);
        let head_weight = self.aim_weights[left][1]
            + a * (self.aim_weights[right][1] - self.aim_weights[left][1]);
        // The weights already carry the published gates (no seat, choke, tumble or combo
        // state; a gun, or an action that aims) and the animation set's AimType.
        let allows_aim = state.pitch != 0. && state.seat.is_none() && state.action != Action::Death;
        let clamped_pitch = state.pitch.clamp(-MAX_AIM_PITCH, MAX_AIM_PITCH);
        let torso_pitch = if allows_aim && torso_weight > 0. {
            clamped_pitch * torso_weight
        } else {
            0.
        };
        let head_pitch = if allows_aim && head_weight > 0. {
            clamped_pitch * head_weight
        } else {
            0.
        };
        for &i in &rig.order {
            let j = &rig.joints[i];
            let b = between(
                pose.history[left * joints + i],
                pose.history[right * joints + i],
                a,
            );
            let mut m = Matrix4::default();
            m.compose(&b.position, &b.rotation, &j.scale);
            pose.model_space[i] = if let Some(p) = j.parent {
                let mut parent = pose.model_space[p];
                parent.multiply(&m);
                parent
            } else {
                m
            };
            if travel != 0. || body != 0. {
                let model = &mut pose.model_space[i];
                if j.parent.is_none() {
                    model.premultiply(&lower);
                }
                let steering = if Some(i) == rig.torso {
                    Some(&upper)
                } else if rig.thighs.contains(&Some(i)) {
                    Some(&foot)
                } else {
                    None
                };
                if let Some(turn) = steering {
                    let e = &model.elements;
                    let pivot = Vector3::new(e[12], e[13], e[14]);
                    model.premultiply(turn).set_position_v3(&pivot);
                }
            }
            if torso_pitch != 0. || head_pitch != 0. {
                let joint_pitch = torso_pitch * rig.aim_share(i)
                    + head_pitch * rig.head_aim_share(i);
                if joint_pitch != 0. {
                    let model = &mut pose.model_space[i];
                    let e = &model.elements;
                    let pivot = Vector3::new(e[12], e[13], e[14]);
                    let mut tilt = Matrix4::default();
                    tilt.make_rotation_x(-joint_pitch);
                    model.premultiply(&tilt).set_position_v3(&pivot);
                }
            }
            pose.world[i].multiply_matrices(&root, &pose.model_space[i]);
            if i < rig.mesh_joints {
                let mut skin = pose.world[i];
                skin.multiply(&j.inverse);
                for (dst, v) in pose.palette[i * 16..][..16].iter_mut().zip(skin.elements) {
                    *dst = v as f32;
                }
            }
        }
        self.placed = Some((tick, root));
        root
    }
}
fn between(mut a: Local, b: Local, t: f64) -> Local {
    a.position.lerp(&b.position, t);
    a.rotation.slerp(&b.rotation, t);
    a
}

#[cfg(test)]
mod offhand_tests {
    use super::*;
    use crate::animation2::{
        state::{Action, State, Tables},
        Archive, Character, Class,
    };
    use bfe_content::{actor::build_actor, json, zero_hash};
    use serde_json::json as j;

    #[test]
    fn a_hit_over_a_swing_blends_half_way_and_is_gone_after_an_eighth_of_a_second() {
        let (animation, tables) = crate::animation2::state::blend_tests::melee_fixture();
        let class = Class { name: String::new(), side: String::new(), model: String::new(), animation, tables };
        let joints = class.animation.rig.joints.len();
        let torso = class.animation.rig.joint("torso").unwrap();
        let mut arena = Arena::new(1, joints).unwrap();
        let mut pose = arena.slot(0, joints);
        let mut player = Player::default();
        let struck = State { tick: 1, weapon: 0, combo: Some(1), anim_time: 0., action: Action::Hit, elapsed: 0., ..Default::default() };
        player.publish(&class, struck, &mut pose);
        // Swing frame 0 holds the torso at 1, the hit's at 5: half weight lands at 3.
        assert!((pose.next[torso].position.y - 3.).abs() < 1e-9, "{}", pose.next[torso].position.y);
        player.publish(&class, State { tick: 9, elapsed: 8. / 60., ..struck }, &mut pose);
        assert!((pose.next[torso].position.y - 1.).abs() < 1e-9, "{}", pose.next[torso].position.y);
    }

    #[test]
    fn offhand_hold_clamps_final_frame_layers_over_gait_and_aims_torso() {
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_l_thigh","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,-0.5,0,1]},
                {"name":"bone_r_thigh","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,-0.5,0,1]},
                {"name":"bone_a_spine","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.2,0,1]},
                {"name":"bone_ribcage","parent":4,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"bone_head","parent":5,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"hp_weapons","parent":5,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]},
                {"name":"bone_l_upperarm","parent":5,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_l_hand","parent":8,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.4,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let arm_bone = zero_hash::crc_hex("bone_l_upperarm");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str, root_ys: [f32; 3], arm_angles: [f32; 3]| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": 3,
                "tracks": [root_bone, arm_bone]
            }));
            for y in root_ys {
                samples.extend([0f32, 0., 0., 1., 0., y, 0.]);
            }
            for a in arm_angles {
                let (s, c) = (a * 0.5).sin_cos();
                samples.extend([s, 0., 0., c, 0., 0., 0.]);
            }
        };
        add("oak_sabre_stand_idle", [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);
        add("oak_sabre_stand_walkforward", [1.2, 1.4, 1.2], [0.05, 0.05, 0.05]);
        add("oak_sabre_stand_cast_full", [0.6, 0.6, 0.6], [-0.2, -0.6, -1.0]);
        let meta = json::parse(
            &j!({"fps": 10, "banks": {"oak": {"clips": clips}, "oak_sabre": {"clips": []}}})
                .to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(
            &j!({
                "MaxSpeed": 5.0,
                "weapons": [{
                    "AnimationBank": "sabre",
                    "comboBindings": [{
                        "combo": "1234abcd",
                        "bank": "oak_sabre",
                        "mode": "melee",
                        "stateAnimations": {"IDLE": []},
                        "clipTiming": {
                            "stand_cast": {"bank": "oak", "crc": zero_hash::crc_hex("oak_sabre_stand_cast_full"), "frames": 3, "fps": 10}
                        }
                    }]
                }]
            })
            .to_string(),
        )
        .unwrap();
        let machines = json::parse(
            &j!({
                "1234abcd": {
                    "states": [{"values": ["IDLE"], "children": []}],
                    "animations": [],
                    "offhandAnimations": [{
                        "values": ["stand_cast", "FIRE"],
                        "children": [
                            {"key": "Loop", "values": ["FinalFrame"]},
                            {"key": "AimType", "values": ["Torso"]},
                            {"key": "BlendInTime", "values": [0.0]},
                            {"key": "BlendOutTime", "values": [0.0]}
                        ]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("hero", &def, &["oak".into()], &actor, &archive, &archive).unwrap();
        assert!(animation.rig.torso.is_some() && animation.rig.aim.is_some());
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &machines).unwrap();
        let class = Class {
            side: String::new(),
            name: "hero".into(),
            model: String::new(),
            animation,
            tables,
        };
        let root_i = class.animation.rig.joint("bone_root").unwrap();
        let hand_i = class.animation.rig.joint("bone_l_hand").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // Stationary hold past clip duration (0.2 s) keeps final frame (-1.0 rad lift) and full-body root (y = 0.6).
        player.publish(
            &class,
            State {
                tick: 1,
                combo: Some(0),
                action: Action::FireSecondary,
                elapsed: 0.5,
                pitch: 0.0,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        assert!((pose.world[root_i].elements[13] - 0.6).abs() < 1e-6);
        let hand_offset = 0.2 + 0.3 + 0.4 * 1.0_f64.sin();
        let level_hand_y = pose.world[hand_i].elements[13];
        assert!((level_hand_y - (0.6 + hand_offset)).abs() < 1e-5);
        // AimType=Torso pitches the ribcage/hand even on a saber weapon with idle combo Some(0).
        player.publish(
            &class,
            State {
                tick: 2,
                combo: Some(0),
                action: Action::FireSecondary,
                elapsed: 0.5,
                pitch: 0.4,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 2., false, &mut pose);
        assert!((pose.world[hand_i].elements[13] - level_hand_y).abs() > 0.05);
        // Moving while casting masks the cast clip onto upper body and preserves walk gait on bone_root.
        player.publish(
            &class,
            State {
                tick: 3,
                combo: Some(0),
                action: Action::FireSecondary,
                elapsed: 0.5,
                velocity: [0., 0., 1.0],
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 3., false, &mut pose);
        let moving_root_y = pose.world[root_i].elements[13];
        assert!(
            moving_root_y > 1.1,
            "root follows walk gait ({moving_root_y}) rather than 0.6 cast stance"
        );
        assert!(
            (pose.world[hand_i].elements[13] - (moving_root_y + hand_offset)).abs() < 1e-5,
            "hand still holds final-frame cast pose relative to moving torso"
        );
        // Airborne cast with zero horizontal speed also masks onto upper body so jump/fall legs remain intact.
        player.publish(
            &class,
            State {
                tick: 4,
                combo: Some(0),
                air: Air::Jump,
                action: Action::FireSecondary,
                elapsed: 0.5,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 4., false, &mut pose);
        assert!(
            pose.world[root_i].elements[13] > 0.9,
            "airborne cast keeps jump/idle root height ({}) rather than full-body stand root 0.6",
            pose.world[root_i].elements[13]
        );
    }

    #[test]
    fn rapid_recast_of_same_offhand_clip_blends_instead_of_snapping() {
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_a_spine","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.2,0,1]},
                {"name":"bone_ribcage","parent":2,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"bone_head","parent":3,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"hp_weapons","parent":3,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]},
                {"name":"bone_l_upperarm","parent":3,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_l_hand","parent":6,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.4,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let arm_bone = zero_hash::crc_hex("bone_l_upperarm");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str, arm_angles: [f32; 3]| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": 3,
                "tracks": [root_bone, arm_bone]
            }));
            for _ in 0..3 {
                samples.extend([0f32, 0., 0., 1., 0., 1.0, 0.]);
            }
            for a in arm_angles {
                let (s, c) = (a * 0.5).sin_cos();
                samples.extend([s, 0., 0., c, 0., 0., 0.]);
            }
        };
        add("pine_sabre_stand_idle", [0.0, 0.0, 0.0]);
        add("pine_sabre_stand_cast_full", [0.0, -0.6, -1.2]);
        let meta = json::parse(
            &j!({"fps": 10, "banks": {"pine": {"clips": clips}, "pine_sabre": {"clips": []}}})
                .to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(
            &j!({
                "MaxSpeed": 5.0,
                "weapons": [{
                    "AnimationBank": "sabre",
                    "comboBindings": [{
                        "combo": "feedbeef",
                        "bank": "pine_sabre",
                        "mode": "melee",
                        "stateAnimations": {"IDLE": []},
                        "clipTiming": {
                            "stand_cast": {"bank": "pine", "crc": zero_hash::crc_hex("pine_sabre_stand_cast_full"), "frames": 3, "fps": 10}
                        }
                    }]
                }]
            })
            .to_string(),
        )
        .unwrap();
        let machines = json::parse(
            &j!({
                "feedbeef": {
                    "states": [{"values": ["IDLE"], "children": []}],
                    "animations": [],
                    "offhandAnimations": [{
                        "values": ["stand_cast", "FIRE"],
                        "children": [
                            {"key": "BlendInTime", "values": [0.15]},
                            {"key": "BlendOutTime", "values": [0.15]}
                        ]
                    }]
                }
            })
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("caster", &def, &["pine".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &machines).unwrap();
        let class = Class { side: String::new(), name: "caster".into(), model: String::new(), animation, tables };
        let hand_i = class.animation.rig.joint("bone_l_hand").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // End of first cast (elapsed=0.20s -> arm lifted at -1.2 rad).
        player.publish(
            &class,
            State { tick: 10, combo: Some(0), action: Action::FireSecondary, elapsed: 0.2, ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 10., false, &mut pose);
        let lifted_y = pose.world[hand_i].elements[13];
        // Immediate re-cast on tick 11 (elapsed resets to 0.0 -> frame 0 is arm 0.0 rad).
        // With BlendInTime=0.15s (9 ticks), tick 11 is only 1/9 through the blend so hand stays near lifted_y.
        player.publish(
            &class,
            State { tick: 11, combo: Some(0), action: Action::FireSecondary, elapsed: 0.0, ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 11., false, &mut pose);
        let recast_tick1_y = pose.world[hand_i].elements[13];
        // Unblended frame 0 hand_y is 1.5; lifted_y is ~1.87. Blended tick 1 must stay well above unblended 1.5!
        assert!(
            recast_tick1_y > 1.75 && recast_tick1_y < lifted_y,
            "rapid re-cast should blend smoothly instead of snapping to frame 0: lifted={lifted_y}, tick1={recast_tick1_y}"
        );
    }

    #[test]
    fn choke_clip_lifts_victim_loops_and_disables_torso_aim() {
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_a_spine","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.2,0,1]},
                {"name":"bone_ribcage","parent":2,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"bone_head","parent":3,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"hp_weapons","parent":3,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str, ys: &[f32]| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": ys.len(),
                "tracks": [root_bone]
            }));
            for &y in ys {
                samples.extend([0f32, 0., 0., 1., 0., y, 0.]);
            }
        };
        add("oak_rifle_stand_idle", &[0.9, 0.9]);
        add("oak_lifted", &[1.3, 1.5, 1.3]);
        add("oak_rifle_die", &[0.2, 0.1]);
        let meta = json::parse(&j!({"fps": 10, "banks": {"oak": {"clips": clips}}}).to_string())
            .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(&j!({"MaxSpeed": 5.0, "weapons": [{"AnimationBank": "rifle"}]}).to_string()).unwrap();
        let animation = Character::bind("target", &def, &["oak".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &bfe_content::Value::Null).unwrap();
        let class = Class { side: String::new(), name: "target".into(), model: String::new(), animation, tables };
        let choke = class.animation.named_clip("oak_lifted").unwrap();
        let root_i = class.animation.rig.joint("bone_root").unwrap();
        let head_i = class.animation.rig.joint("bone_head").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // Enter lifted clip on tick 1 (no prior blend), sample mid-loop (t = 0.1 s -> y = 1.5), verify pitch aim disabled.
        player.publish(
            &class,
            State { tick: 1, choke_clip: Some(choke), choke_time: 0.1, pitch: 0.6, ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        assert!((pose.world[root_i].elements[13] - 1.5).abs() < 1e-5);
        let lifted_head_z = pose.world[head_i].elements[14];
        assert!(lifted_head_z.abs() < 1e-6, "choked victim disables torso pitch aim ({lifted_head_z})");
        // Looping past clip duration wraps back into the lifted cycle (0.3 s wraps to 0.1 s -> y = 1.5).
        player.publish(
            &class,
            State { tick: 13, choke_clip: Some(choke), choke_time: 0.3, pitch: 0.6, ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 13., false, &mut pose);
        assert!((pose.world[root_i].elements[13] - 1.5).abs() < 1e-5);
        // Death overrides choke lift cleanly.
        player.publish(
            &class,
            State { tick: 30, choke_clip: Some(choke), choke_time: 0.4, action: Action::Death, elapsed: 1.0, ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 30., false, &mut pose);
        assert!(pose.world[root_i].elements[13] < 0.3);
    }

    #[test]
    fn moving_guard_combo_layers_upper_body_over_walk_and_keeps_animated_move_full_body() {
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_l_thigh","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,-0.5,0,1]},
                {"name":"bone_r_thigh","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,-0.5,0,1]},
                {"name":"bone_a_spine","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.2,0,1]},
                {"name":"bone_ribcage","parent":4,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"bone_l_upperarm","parent":5,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_l_hand","parent":6,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.4,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let thigh_bone = zero_hash::crc_hex("bone_l_thigh");
        let arm_bone = zero_hash::crc_hex("bone_l_upperarm");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str, root_ys: [f32; 3], leg_angles: [f32; 3], arm_angles: [f32; 3]| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": 3,
                "tracks": [root_bone, thigh_bone, arm_bone]
            }));
            for y in root_ys {
                samples.extend([0f32, 0., 0., 1., 0., y, 0.]);
            }
            for a in leg_angles {
                let (s, c) = (a * 0.5).sin_cos();
                samples.extend([s, 0., 0., c, 0., -0.5, 0.]);
            }
            for a in arm_angles {
                let (s, c) = (a * 0.5).sin_cos();
                samples.extend([s, 0., 0., c, 0., 0., 0.]);
            }
        };
        add("pine_sabre_stand_idle", [1.0, 1.0, 1.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]);
        add("pine_sabre_stand_walkforward", [1.2, 1.4, 1.2], [0.1, 0.6, 0.1], [0.05, 0.05, 0.05]);
        add("pine_sabre_guard_full", [0.65, 0.65, 0.65], [0.0, 0.0, 0.0], [-0.9, -0.9, -0.9]);
        add("pine_sabre_slash_full", [0.55, 0.55, 0.55], [-0.4, -0.4, -0.4], [-1.1, -1.1, -1.1]);
        let meta = json::parse(
            &j!({"fps": 10, "banks": {"pine": {"clips": clips}, "pine_sabre": {"clips": []}}})
                .to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(
            &j!({
                "MaxSpeed": 6.0,
                "weapons": [{
                    "AnimationBank": "sabre",
                    "comboBindings": [{
                        "combo": "abcd5678",
                        "bank": "pine_sabre",
                        "mode": "melee",
                        "stateAnimations": {
                            "IDLE": [],
                            "GUARD": [{"key": "Animation", "name": "guard", "playback": "loop"}],
                            "SLASH": [{"key": "Animation", "name": "slash", "playback": "once"}]
                        },
                        "clipTiming": {
                            "guard": {"bank": "pine", "crc": zero_hash::crc_hex("pine_sabre_guard_full"), "frames": 3, "fps": 10},
                            "slash": {"bank": "pine", "crc": zero_hash::crc_hex("pine_sabre_slash_full"), "frames": 3, "fps": 10}
                        }
                    }]
                }]
            })
            .to_string(),
        )
        .unwrap();
        let machines = json::parse(
            &j!({
                "abcd5678": {
                    "states": [
                        {"values": ["IDLE"], "children": []},
                        {"values": ["GUARD"], "children": []},
                        {"values": ["SLASH"], "children": [{"key": "AnimatedMove"}, {"key": "Attack"}]}
                    ],
                    "animations": [
                        {"values": ["guard"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                        {"values": ["slash"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]}
                    ]
                }
            })
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("guarder", &def, &["pine".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &machines).unwrap();
        let class = Class { side: String::new(), name: "guarder".into(), model: String::new(), animation, tables };
        let root_i = class.animation.rig.joint("bone_root").unwrap();
        let thigh_i = class.animation.rig.joint("bone_l_thigh").unwrap();
        let hand_i = class.animation.rig.joint("bone_l_hand").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();

        // 1. Stationary GUARD plays full-body guard stance (root y = 0.65).
        player.publish(&class, State { tick: 1, combo: Some(1), ..Default::default() }, &mut pose);
        player.place_at(&class, 1., false, &mut pose);
        assert!((pose.world[root_i].elements[13] - 0.65).abs() < 1e-5);
        let guard_arm_offset = 0.2 + 0.3 + 0.4 * 0.9_f64.sin();
        assert!((pose.world[hand_i].elements[13] - (0.65 + guard_arm_offset)).abs() < 1e-5);

        // 2. Walking forward while holding GUARD animates lower-body walk cycle & smoothly interpolates sub-ticks,
        //    while keeping upper body in the guard pose.
        for tick in 2..=8 {
            player.publish(
                &class,
                State { tick, combo: Some(1), velocity: [0., 0., 2.5], ..Default::default() },
                &mut pose,
            );
        }
        player.place_at(&class, 2., false, &mut pose);
        let root_t2 = pose.world[root_i].elements[13];
        let thigh_t2 = pose.world[thigh_i].elements[9];
        player.place_at(&class, 5.5, false, &mut pose);
        let root_t55 = pose.world[root_i].elements[13];
        let thigh_t55 = pose.world[thigh_i].elements[9];
        player.place_at(&class, 6.0, false, &mut pose);
        let root_t6 = pose.world[root_i].elements[13];
        assert!(root_t2 > 1.1 && root_t55 > 1.1, "moving guard follows walk gait ({root_t2}, {root_t55})");
        assert!((thigh_t55 - thigh_t2).abs() > 0.05, "legs advance through walk cycle while guarding");
        assert!((root_t6 - root_t55).abs() > 1e-3, "sub-tick pose interpolates smoothly between ticks");
        assert!(
            (pose.world[hand_i].elements[13] - (root_t6 + guard_arm_offset)).abs() < 1e-5,
            "upper body stays in guard pose relative to moving root"
        );

        // 3. Full-body AnimatedMove combo attack (SLASH) still drives root & legs even while moving.
        player.publish(
            &class,
            State { tick: 9, combo: Some(2), velocity: [0., 0., 2.5], ..Default::default() },
            &mut pose,
        );
        player.place_at(&class, 9., false, &mut pose);
        assert!(
            (pose.world[root_i].elements[13] - 0.55).abs() < 1e-5,
            "AnimatedMove attack stays full-body, got root y={}",
            pose.world[root_i].elements[13]
        );

        // 4. A swing turned toward the stick turns the body above the hips by the
        //    published turn, interpolated between ticks; the legs keep their heading.
        let torso_i = class.animation.rig.torso.unwrap();
        let heading = |pose: &Pose| {
            let (h, t) = (&pose.world[hand_i].elements, &pose.world[torso_i].elements);
            (h[12] - t[12]).atan2(h[14] - t[14])
        };
        for (tick, turn) in [(10, 0.0), (11, -0.6), (12, -0.6)] {
            player.publish(&class, State { tick, combo: Some(2), swing_turn: turn, ..Default::default() }, &mut pose);
        }
        player.place_at(&class, 10., false, &mut pose);
        let straight = heading(&pose);
        let thigh = pose.world[thigh_i].elements[12..15].to_vec();
        player.place_at(&class, 10.5, false, &mut pose);
        assert!((heading(&pose) - straight + 0.3).abs() < 1e-9, "half way between ticks");
        player.place_at(&class, 12., false, &mut pose);
        assert!((heading(&pose) - straight + 0.6).abs() < 1e-9, "turned by the published swing");
        assert_eq!(pose.world[thigh_i].elements[12..15].to_vec(), thigh, "legs keep their heading");
    }
}

#[cfg(test)]
mod jump_tests {
    use super::*;
    use serde_json::json as j;
    use crate::animation2::{
        state::{Air, State, Tables},
        Archive, Character,
    };
    use bfe_content::{actor::build_actor, json, zero_hash};

    fn jump_fixture(directional: bool) -> Class {
        let mesh = json::parse(
            r#"{"bones":[{"name":"pivot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#,
        )
        .unwrap();
        let bone = zero_hash::crc_hex("pivot");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        for (suffix, ys) in [
            ("stand_idle", vec![0., 0.]),
            ("jump", vec![1., 2., 3.]),
            ("jump_left", vec![10., 11., 12.]),
            ("jump_right", vec![20., 21., 22.]),
            ("fall", vec![30., 30.]),
            ("jetpack_hover", vec![50., 52., 50.]),
            ("landsoft", vec![40., 40.]),
        ] {
            clips.push(serde_json::json!({
                "crc": zero_hash::crc_hex(&format!("quartz_blade_{suffix}")),
                "flags": 0,
                "offset": samples.len(),
                "frames": ys.len(),
                "tracks": [bone]
            }));
            for y in ys {
                samples.extend([0f32, 0., 0., 1., 0., y as f32, 0.]);
            }
        }
        let meta = json::parse(
            &serde_json::json!({"fps": 4, "banks": {"quartz": {"clips": clips}}}).to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(
            &serde_json::json!({
                "UseDirectionalJumps": if directional { "1" } else { "0" },
                "weapons": [{"AnimationBank": "blade"}]
            })
            .to_string(),
        )
        .unwrap();
        let sets = json::parse(
            r#"{"set":[
                {"values":["jump_left"],"children":[{"key":"BlendInTime","values":[0.01]},{"key":"BlendOutTime","values":[1.0]}]},
                {"values":["jump_right"],"children":[{"key":"BlendInTime","values":[0.01]},{"key":"BlendOutTime","values":[1.0]}]},
                {"values":["fall"],"children":[{"key":"BlendInTime","values":[1.0]},{"key":"BlendOutTime","values":[0.01]}]},
                {"values":["landsoft"],"children":[{"key":"BlendInTime","values":[0.01]},{"key":"BlendOutTime","values":[0.15]}]}
            ]}"#,
        )
        .unwrap();
        let animation = Character::bind(
            "jumper",
            &def,
            &["quartz".into()],
            &build_actor(&mesh, &[]).unwrap(),
            &archive,
            &archive,
        )
        .unwrap();
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        Class {
            side: String::new(),
            name: "jumper".into(),
            model: String::new(),
            animation,
            tables,
        }
    }

    #[test]
    fn directional_jump_latches_on_takeoff_rolls_to_fall_and_blends_crisply_to_landing() {
        let class = jump_fixture(true);
        let mut arena = Arena::new(1, 1).unwrap();
        let mut pose = arena.slot(0, 1);
        let mut player = Player::default();

        // Take off moving left (+X at yaw=0) above the 2.0 m/s threshold.
        player.publish(
            &class,
            State {
                tick: 1,
                air: Air::Jump,
                velocity: [5., 8., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        assert!((pose.world[0].elements[13] - 10.).abs() < 1e-6);

        // Reverse velocity mid-air to right (-X): must stay on jump_left, not flip to jump_right.
        player.publish(
            &class,
            State {
                tick: 16,
                air: Air::Jump,
                velocity: [-5., 3., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 16., false, &mut pose);
        assert!((pose.world[0].elements[13] - 11.).abs() < 1e-6);

        // After clip duration (0.5 s at 4 fps = 30 ticks), rolls over to fall (blend=min(1.0, 1.0)=1.0).
        player.publish(
            &class,
            State {
                tick: 32,
                air: Air::Jump,
                velocity: [-5., -4., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        assert!((player.blend - 1.0).abs() < 1e-9);

        // Touch down directly from jump_left -> landsoft uses min(1.0, 0.01) = 0.01s, not 1.0s.
        let mut quick = Player::default();
        let mut quick_pose = arena.slot(0, 1);
        quick.publish(
            &class,
            State {
                tick: 1,
                air: Air::Jump,
                velocity: [5., 8., 0.],
                ..Default::default()
            },
            &mut quick_pose,
        );
        quick.publish(
            &class,
            State {
                tick: 10,
                air: Air::Land,
                velocity: [0., 0., 0.],
                ..Default::default()
            },
            &mut quick_pose,
        );
        assert!((quick.blend - 0.01).abs() < 1e-9);
    }

    #[test]
    fn disabled_directional_jumps_always_select_neutral_jump_and_double_jump_restarts() {
        let disabled = jump_fixture(false);
        let mut arena = Arena::new(1, 1).unwrap();
        let mut pose = arena.slot(0, 1);
        let mut player = Player::default();
        player.publish(
            &disabled,
            State {
                tick: 1,
                air: Air::Jump,
                velocity: [6., 8., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&disabled, 1., false, &mut pose);
        assert!((pose.world[0].elements[13] - 1.).abs() < 1e-6);

        // With directional jumps enabled, a jump_count bump while moving right switches to jump_right.
        let enabled = jump_fixture(true);
        let mut p = Player::default();
        p.publish(
            &enabled,
            State {
                tick: 1,
                air: Air::Jump,
                jump_count: 1,
                velocity: [5., 8., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        p.publish(
            &enabled,
            State {
                tick: 12,
                air: Air::Jump,
                jump_count: 2,
                velocity: [-5., 12., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        p.place_at(&enabled, 12., false, &mut pose);
        assert!((pose.world[0].elements[13] - 20.).abs() < 1e-6);
    }

    #[test]
    fn jet_air_phase_selects_looping_hover_clip_instead_of_fall() {
        let class = jump_fixture(true);
        let mut arena = Arena::new(1, 1).unwrap();
        let mut pose = arena.slot(0, 1);
        let mut player = Player::default();
        player.publish(
            &class,
            State {
                tick: 1,
                air: Air::Jet,
                phase_time: Some(0.0),
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        assert!((pose.world[0].elements[13] - 50.).abs() < 1e-6);

        // Looping wraps past clip duration (0.5s at 4 fps, 3 frames) back through the midpoint (52).
        player.publish(
            &class,
            State {
                tick: 46,
                air: Air::Jet,
                phase_time: Some(0.75),
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 46., false, &mut pose);
        assert!((pose.world[0].elements[13] - 52.).abs() < 1e-6);
    }

    #[test]
    fn tumble_phases_select_clips_rotate_root_smoothly_and_rest_dead() {
        use crate::animation2::state::{TumblePhase, TumblePose};
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_a_spine","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.3,0,1]},
                {"name":"bone_head","parent":2,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.4,0,1]},
                {"name":"hp_weapons","parent":2,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str, ys: &[f32]| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": ys.len(),
                "tracks": [root_bone]
            }));
            for &y in ys {
                samples.extend([0f32, 0., 0., 1., 0., y, 0.]);
            }
        };
        add("ash_rifle_stand_idle", &[1.0, 1.0]);
        add("ash_rifle_thrown_flyingback", &[1.6]);
        add("ash_rifle_thrown_flail", &[1.3, 1.35, 1.3]);
        add("ash_rifle_thrown_tumbleback", &[0.45, 0.50, 0.45]);
        add("ash_rifle_thrown_bouncebacksoft", &[0.35]);
        add("ash_rifle_thrown_landbacksoft", &[0.12]);
        add("ash_rifle_stand_getupback", &[0.12, 0.55, 1.0]);
        add("ash_rifle_thrown_restbacksoft", &[0.11]);
        let meta = json::parse(&j!({"fps": 10, "banks": {"ash": {"clips": clips}}}).to_string())
            .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(&j!({"MaxSpeed": 5.0, "weapons": [{"AnimationBank": "rifle"}]}).to_string()).unwrap();
        let sets = json::parse(
            &j!({"s": [
                {"values": ["thrown_flyingback"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["thrown_flail"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}, {"key": "Loop", "values": [1.0]}]},
                {"values": ["thrown_tumbleback"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}, {"key": "Loop", "values": [1.0]}]},
                {"values": ["thrown_bouncebacksoft"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["thrown_landbacksoft"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["stand_getupback"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["thrown_restbacksoft"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]}
            ]})
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("victim", &def, &["ash".into()], &actor, &archive, &archive).unwrap();
        assert!(animation.rig.root.is_some());
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        let class = Class { side: String::new(), name: "victim".into(), model: String::new(), animation, tables };
        let root_i = class.animation.rig.root.unwrap();
        let head_i = class.animation.rig.joint("bone_head").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // 1. Thrown (flyingback <= 0.3s) with horizontal-axis somersault angle advancing across ticks.
        let pose_at = |phase, front, bounce, elapsed, angle| Some(TumblePose {
            phase,
            front,
            bounce,
            fly_dir: 1,
            elapsed,
            angle,
            heading: 0.,
        });
        player.publish(
            &class,
            State {
                tick: 1,
                velocity: [0., 3., -10.],
                tumble: pose_at(TumblePhase::Thrown, false, false, 0.1, 0.0),
                ..Default::default()
            },
            &mut pose,
        );
        player.publish(
            &class,
            State {
                tick: 2,
                velocity: [0., 2.7, -9.9],
                tumble: pose_at(TumblePhase::Thrown, false, false, 0.12, 1.0),
                ..Default::default()
            },
            &mut pose,
        );
        // Sample at fractional tick 1.5 (120+ Hz display) -> head rotates smoothly partway around pelvis!
        player.place_at(&class, 1.0, false, &mut pose);
        let head_y0 = pose.world[head_i].elements[13];
        player.place_at(&class, 1.5, false, &mut pose);
        let head_y_mid = pose.world[head_i].elements[13];
        player.place_at(&class, 2.0, false, &mut pose);
        let head_y1 = pose.world[head_i].elements[13];
        assert!(
            head_y0 > head_y_mid && head_y_mid > head_y1,
            "bone_root somersault interpolates smoothly at fractional display ticks: {head_y0} -> {head_y_mid} -> {head_y1}"
        );
        // 2. Thrown > 0.3s switches to flail (root_y = 1.3 vs flyingback 1.6).
        player.publish(
            &class,
            State {
                tick: 3,
                velocity: [0., 0., -9.],
                tumble: pose_at(TumblePhase::Thrown, false, false, 0.4, 0.0),
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 3., false, &mut pose);
        assert!((pose.world[root_i].elements[13] - 1.3).abs() < 1e-4);
        // 3. Tumble -> Landed (bounce vs land) -> GetUp -> RestDead.
        for (tick, phase, bounce, elapsed, expect_y) in [
            (4, TumblePhase::Tumble, false, 0.0, 0.45),
            (5, TumblePhase::Landed, true, 0.0, 0.35),
            (6, TumblePhase::Landed, false, 0.0, 0.12),
            (7, TumblePhase::GetUp, false, 0.2, 1.0),
            (8, TumblePhase::RestDead, false, 0.1, 0.11),
        ] {
            let action = if phase == TumblePhase::RestDead { Action::Death } else { Action::None };
            player.publish(
                &class,
                State {
                    tick,
                    action,
                    tumble: pose_at(phase, false, bounce, elapsed, 0.0),
                    ..Default::default()
                },
                &mut pose,
            );
            player.place_at(&class, tick as f64, false, &mut pose);
            assert!(
                (pose.world[root_i].elements[13] - expect_y).abs() < 0.06,
                "phase {phase:?} bounce={bounce} expected root_y~{expect_y}, got {}",
                pose.world[root_i].elements[13]
            );
        }
        assert_eq!(player.fallback_deaths, 0, "RestDead uses authored thrown_rest clip, not generic fallback");
    }

    #[test]
    fn clip_at_tracks_active_clip_interpolated_time_and_blend_duration() {
        let class = jump_fixture(true);
        let mut arena = Arena::new(1, 1).unwrap();
        let mut pose = arena.slot(0, 1);
        let mut player = Player::default();
        player.publish(
            &class,
            State {
                tick: 1,
                air: Air::Jump,
                jump_count: 1,
                velocity: [5., 8., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        player.publish(
            &class,
            State {
                tick: 2,
                air: Air::Jump,
                jump_count: 1,
                velocity: [5., 7., 0.],
                ..Default::default()
            },
            &mut pose,
        );
        let mid = player.clip_at(&class, 1.5).unwrap();
        assert_eq!(mid.name, "jump_left");
        assert!((mid.time - (0.5 / 60.)).abs() < 1e-9);
        assert_eq!(mid.rate, 1.0);

        // Landing from jump_left uses min(1.0, 0.01) = 0.01s blend, never 1.0s.
        player.publish(
            &class,
            State {
                tick: 3,
                air: Air::Land,
                elapsed: 0.0,
                ..Default::default()
            },
            &mut pose,
        );
        let land = player.clip_at(&class, 3.0).unwrap();
        assert_eq!(land.name, "landsoft");
        assert!((land.blend - 0.01).abs() < 1e-9);
    }

    fn spine_aim_fixture() -> Class {
        let bone = |name: &str, parent: i32, y: f64, z: f64| {
            serde_json::json!({
                "name": name,
                "parent": parent,
                "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,y,z,1]
            })
        };
        let mesh = json::parse(
            &serde_json::json!({
                "attach": "hp_weapons",
                "bones": [
                    bone("DummyRoot", -1, 0.0, 0.0),
                    bone("bone_pelvis", 0, 1.0, 0.0),
                    bone("bone_a_spine", 1, 0.1, 0.0),
                    bone("bone_b_spine", 2, 0.15, 0.0),
                    bone("bone_ribcage", 3, 0.2, 0.0),
                    bone("bone_head", 4, 0.35, 0.0),
                    bone("hp_weapons", 4, -0.05, 0.45)
                ],
                "parts": []
            })
            .to_string(),
        )
        .unwrap();
        let rib = zero_hash::crc_hex("bone_ribcage");
        let wep = zero_hash::crc_hex("hp_weapons");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        // Level forward weapon grip for idle, hover, shoot; overhead grip for fall/jump.
        for (suffix, wep_y) in [
            ("stand_idle_full", -0.05f32),
            ("jetpack_hover", -0.05),
            ("stand_shoot_full", 0.05),
            ("jump", 0.55),
            ("fall", 0.55),
        ] {
            clips.push(serde_json::json!({
                "crc": zero_hash::crc_hex(&format!("quartz_carbine_{suffix}")),
                "flags": 0,
                "offset": samples.len(),
                "frames": 2,
                "tracks": [rib, wep]
            }));
            for _ in 0..2 {
                samples.extend([0f32, 0., 0., 1., 0., 0.2, 0.]);
            }
            for _ in 0..2 {
                samples.extend([0f32, 0., 0., 1., 0., wep_y, 0.45]);
            }
        }
        let meta = json::parse(
            &serde_json::json!({"fps": 30, "banks": {"quartz": {"clips": clips}}}).to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(r#"{"weapons":[{"AnimationBank":"carbine"}]}"#).unwrap();
        let sets = json::parse(
            r#"{"set":[
                {"values":["jetpack_hover"],"children":[
                    {"key":"Loop","values":[1.0]},
                    {"key":"AimType","values":["Torso"]},
                    {"key":"BlendInTime","values":[0.15]},
                    {"key":"BlendOutTime","values":[0.25]}
                ]},
                {"values":["stand_shoot_upper"],"children":[
                    {"key":"AimType","values":["Torso"]},
                    {"key":"BlendInTime","values":[0.02]},
                    {"key":"BlendOutTime","values":[0.02]}
                ]},
                {"values":["stand_shoot_lower"],"children":[
                    {"key":"BlendInTime","values":[0.5]},
                    {"key":"BlendOutTime","values":[0.5]}
                ]},
                {"values":["jump"],"children":[
                    {"key":"BlendInTime","values":[0.01]},
                    {"key":"BlendOutTime","values":[1.0]}
                ]},
                {"values":["fall"],"children":[
                    {"key":"Loop","values":[1.0]},
                    {"key":"BlendInTime","values":[1.0]},
                    {"key":"BlendOutTime","values":[0.01]}
                ]}
            ]}"#,
        )
        .unwrap();
        let animation = Character::bind(
            "hoverer",
            &def,
            &["quartz".into()],
            &build_actor(&mesh, &[]).unwrap(),
            &archive,
            &archive,
        )
        .unwrap();
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        Class {
            side: String::new(),
            name: "hoverer".into(),
            model: String::new(),
            animation,
            tables,
        }
    }

    #[test]
    fn hover_and_fire_distribute_torso_pitch_across_three_spine_bones_and_clamp() {
        let class = spine_aim_fixture();
        let rig = &class.animation.rig;
        assert_eq!(rig.spine, [Some(2), Some(3), Some(4)]);
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();

        // Hovering with pitch=+0.4 rad splits 1/8, 3/8, 4/8 across a_spine, b_spine, ribcage.
        let pitch = 0.4;
        player.publish(
            &class,
            State {
                tick: 1,
                air: Air::Jet,
                pitch,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        let elev = |pose: &Pose, joint: usize| {
            let e = &pose.world[joint].elements;
            e[9].atan2(e[10])
        };
        assert!((elev(&pose, 2) - pitch * 0.125).abs() < 1e-6);
        assert!((elev(&pose, 3) - pitch * 0.500).abs() < 1e-6);
        assert!((elev(&pose, 4) - pitch).abs() < 1e-6);
        assert!((elev(&pose, 6) - pitch).abs() < 1e-6);
        assert!(pose.world[6].elements[13] < pose.world[5].elements[13]);

        // Firing while hovering layers stand_shoot on upper body and preserves exact aim alignment.
        player.publish(
            &class,
            State {
                tick: 2,
                air: Air::Jet,
                action: Action::Fire,
                pitch: -0.35,
                elapsed: 0.0,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 2., false, &mut pose);
        let frame = player.clip_at(&class, 2.).unwrap();
        assert_eq!(frame.name, "jetpack_hover");
        assert_eq!(frame.action, "stand_shoot");
        assert!((elev(&pose, 6) - (-0.35)).abs() < 1e-6);

        // Extreme pitch clamps at +/-0.7 * pi/2.
        player.publish(
            &class,
            State {
                tick: 3,
                air: Air::Jet,
                pitch: 1.5,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 3., false, &mut pose);
        assert!((elev(&pose, 6) - MAX_AIM_PITCH).abs() < 1e-6);
    }

    #[test]
    fn jump_and_fall_without_aim_type_suppress_torso_pitch_and_blend_into_hover() {
        let class = spine_aim_fixture();
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();
        let elev = |pose: &Pose| {
            let e = &pose.world[6].elements;
            e[9].atan2(e[10])
        };

        // Fall and jump have no AimType in the animation set -> zero torso tilt.
        player.publish(
            &class,
            State {
                tick: 1,
                air: Air::Fall,
                pitch: 0.6,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        assert!(elev(&pose).abs() < 1e-6);

        player.publish(
            &class,
            State {
                tick: 2,
                air: Air::Jump,
                pitch: 0.6,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 2., false, &mut pose);
        assert!(elev(&pose).abs() < 1e-6);

        // Entering jetpack_hover from jump blends aim_weight smoothly over 0.15s (9 ticks).
        player.publish(
            &class,
            State {
                tick: 3,
                air: Air::Jet,
                pitch: 0.6,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 3., false, &mut pose);
        let first_step = elev(&pose);
        assert!(first_step > 0.0 && first_step < 0.2, "got {first_step}");

        // After 0.15s (tick 2 + 9 = 11), full torso pitch is active.
        player.publish(
            &class,
            State {
                tick: 12,
                air: Air::Jet,
                pitch: 0.6,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 12., false, &mut pose);
        assert!((elev(&pose) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn tumble_overwrites_root_rotation_and_airborne_death_rests_without_spin() {
        use crate::animation2::state::{TumblePhase, TumblePose};
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"bone_head","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0.5,0,1]},
                {"name":"hp_weapons","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        // Give thrown_flyingback a non-identity 90 deg Y root rotation so premultiply != overwrite.
        let s45 = std::f32::consts::FRAC_1_SQRT_2;
        let mut add = |name: &str, q: [f32; 4], y: f32| {
            clips.push(j!({
                "crc": zero_hash::crc_hex(name),
                "flags": 0,
                "offset": samples.len(),
                "frames": 1,
                "tracks": [root_bone]
            }));
            samples.extend([q[0], q[1], q[2], q[3], 0., y, 0.]);
        };
        add("birch_rifle_stand_idle", [0., 0., 0., 1.], 1.0);
        add("birch_rifle_thrown_flyingback", [0., s45, 0., s45], 1.5);
        add("birch_rifle_thrown_flail", [0., 0., 0., 1.], 1.3);
        add("birch_rifle_thrown_restbacksoft", [0., 0., 0., 1.], 0.12);
        let meta = json::parse(&j!({"fps": 10, "banks": {"birch": {"clips": clips}}}).to_string())
            .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(&j!({"MaxSpeed": 5.0, "weapons": [{"AnimationBank": "rifle"}]}).to_string()).unwrap();
        let sets = json::parse(
            &j!({"s": [
                {"values": ["thrown_flyingback"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["thrown_flail"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]},
                {"values": ["thrown_restbacksoft"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.0]}]}
            ]})
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("victim", &def, &["birch".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        let class = Class { side: String::new(), name: "victim".into(), model: String::new(), animation, tables };
        let root_i = class.animation.rig.root.unwrap();
        let head_i = class.animation.rig.joint("bone_head").unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // 1. Overwrite (not premultiply): velocity along -Z => pure X rotation by +pi/2.
        // If premultiplied with clip's 90 deg Y, root world column 0 (X axis) would point along -Z instead of +X!
        player.publish(
            &class,
            State {
                tick: 1,
                velocity: [0., 2., -10.],
                tumble: Some(TumblePose {
                    phase: TumblePhase::Thrown,
                    front: false,
                    bounce: false,
                    fly_dir: 1,
                    elapsed: 0.1,
                    angle: std::f64::consts::FRAC_PI_2,
                    heading: 0.,
                }),
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        // Pure X rotation leaves X-axis basis vector (1, 0, 0) intact; premultiply with 90 deg Y would yield (0, 0, -1).
        let rx = pose.world[root_i].elements[0];
        let rz = pose.world[root_i].elements[2];
        assert!(
            (rx - 1.0).abs() < 1e-5 && rz.abs() < 1e-5,
            "tumble spin must overwrite bone_root local rotation, not premultiply: col0=({rx}, {}, {rz})",
            pose.world[root_i].elements[1]
        );
        // 2. Death while Thrown selects thrown_restbacksoft (root_y = 0.12) with ZERO spin.
        player.publish(
            &class,
            State {
                tick: 2,
                action: Action::Death,
                velocity: [0., 2., -10.],
                tumble: Some(TumblePose {
                    phase: TumblePhase::Thrown,
                    front: false,
                    bounce: false,
                    fly_dir: 1,
                    elapsed: 0.1,
                    angle: std::f64::consts::FRAC_PI_2,
                    heading: 0.,
                }),
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 2., false, &mut pose);
        assert!(
            (pose.world[root_i].elements[13] - 0.12).abs() < 1e-4,
            "airborne death must select thrown_rest clip, got root_y={}",
            pose.world[root_i].elements[13]
        );
        assert!(
            (pose.world[head_i].elements[13] - 0.62).abs() < 1e-4,
            "airborne death must apply zero somersault spin, got head_y={}",
            pose.world[head_i].elements[13]
        );
        // 3. Without TumblePose (Air::Thrown fallback) or in Recover, no invented fallback spin is applied.
        let mut p2 = Player::default();
        p2.publish(
            &class,
            State {
                tick: 1,
                air: Air::Thrown,
                elapsed: 0.25,
                velocity: [0., 2., -10.],
                tumble: None,
                ..Default::default()
            },
            &mut pose,
        );
        p2.place_at(&class, 1., false, &mut pose);
        assert!(
            (pose.world[head_i].elements[13] - 2.0).abs() < 1e-4,
            "no fallback spin without TumblePose: head_y={}",
            pose.world[head_i].elements[13]
        );
    }

    #[test]
    fn the_somersault_ends_at_once_when_the_throw_leaves_its_spin() {
        let class = thrown_fixture();
        let root_i = class.animation.rig.root.unwrap();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        // Thrown, turned a quarter over by the somersault; every clip's root is upright.
        player.publish(&class, State { tick: 1, velocity: [0., 2., -10.], tumble: Some(tumble(TumblePhase::Thrown, std::f64::consts::FRAC_PI_2)), ..Default::default() }, &mut pose);
        player.place_at(&class, 1., false, &mut pose);
        let up = pose.world[root_i].elements[5];
        assert!(up.abs() < 1e-4, "thrown: the root is turned a quarter over (its up axis level), up.y={up}");
        // The next tick the throw is a ground tumble: no spin. The root is upright at once, not blended back over 0.5 s.
        player.publish(&class, State { tick: 2, velocity: [0., 0., -6.], tumble: Some(tumble(TumblePhase::Tumble, 0.)), ..Default::default() }, &mut pose);
        player.place_at(&class, 2., false, &mut pose);
        let up = pose.world[root_i].elements[5];
        assert!((up - 1.).abs() < 1e-4, "the tumble's first frame starts upright: up.y={up}");
    }

    #[test]
    fn a_pose_crosses_a_clip_change_only_between_the_publications_that_change_the_clip() {
        let class = thrown_fixture();
        let mut arena = Arena::new(1, class.animation.rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, class.animation.rig.joints.len());
        let mut player = Player::default();
        let thrown = |tick| State { tick, velocity: [0., 2., -10.], tumble: Some(tumble(TumblePhase::Thrown, 0.)), ..Default::default() };
        for tick in 1..=3 {
            player.publish(&class, thrown(tick), &mut pose);
        }
        let (a, b) = player.changes_at(2.5);
        assert_eq!(a, b, "the same clip on: no change between ticks 2 and 3");
        // A new tumble phase with a new clip.
        player.publish(&class, State { tick: 4, velocity: [0., 0., -6.], tumble: Some(tumble(TumblePhase::Tumble, 0.)), ..Default::default() }, &mut pose);
        let (c, d) = player.changes_at(3.5);
        assert_eq!(c, b);
        assert_ne!(c, d, "the pose between ticks 3 and 4 crosses the change");
        let (e, f) = player.changes_at(4.);
        assert_ne!(e, f, "at the changing tick the pose still crosses it (from tick 3)");
        player.publish(&class, State { tick: 5, velocity: [0., 0., -5.], tumble: Some(tumble(TumblePhase::Tumble, 0.)), ..Default::default() }, &mut pose);
        let (g, h) = player.changes_at(4.5);
        assert_eq!((g, h), (d, d), "inside the new clip: no change");
    }

    fn tumble(phase: crate::animation2::state::TumblePhase, angle: f64) -> crate::animation2::state::TumblePose {
        crate::animation2::state::TumblePose { phase, front: false, bounce: false, fly_dir: 1, elapsed: 0.1, angle, heading: 0. }
    }

    /// A body with a root and a gun hand and the thrown clips of one bank, with long blends between them.
    fn thrown_fixture() -> Class {
        let mesh = json::parse(
            r#"{"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"bone_root","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]},
                {"name":"hp_weapons","parent":1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0.2,1]}
            ],"parts":[]}"#,
        )
        .unwrap();
        let actor = build_actor(&mesh, &[]).unwrap();
        let root_bone = zero_hash::crc_hex("bone_root");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        let mut add = |name: &str| {
            clips.push(j!({"crc": zero_hash::crc_hex(name), "flags": 0, "offset": samples.len(), "frames": 1, "tracks": [root_bone]}));
            samples.extend([0f32, 0., 0., 1., 0., 1., 0.]);
        };
        for name in ["oak_rifle_stand_idle", "oak_rifle_thrown_flyingback", "oak_rifle_thrown_flail", "oak_rifle_thrown_tumbleback"] {
            add(name);
        }
        let meta = json::parse(&j!({"fps": 10, "banks": {"oak": {"clips": clips}}}).to_string()).unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(&j!({"MaxSpeed": 5.0, "weapons": [{"AnimationBank": "rifle"}]}).to_string()).unwrap();
        // A long blend between the throw's clips: whatever the root shows on the tumble's first frame is the blend's start.
        let sets = json::parse(
            &j!({"s": [
                {"values": ["thrown_flyingback"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.5]}]},
                {"values": ["thrown_flail"], "children": [{"key": "BlendInTime", "values": [0.0]}, {"key": "BlendOutTime", "values": [0.5]}]},
                {"values": ["thrown_tumbleback"], "children": [{"key": "BlendInTime", "values": [0.5]}, {"key": "BlendOutTime", "values": [0.5]}]}
            ]})
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind("thrown", &def, &["oak".into()], &actor, &archive, &archive).unwrap();
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        Class { side: String::new(), name: "thrown".into(), model: String::new(), animation, tables }
    }

    fn aim_modes_fixture(with_neck: bool, abdomen_fallback: bool) -> Class {
        let bone = |name: &str, parent: i32, y: f64, z: f64| {
            serde_json::json!({
                "name": name,
                "parent": parent,
                "local": [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,y,z,1]
            })
        };
        let mut bones = vec![
            bone("DummyRoot", -1, 0.0, 0.0),
            bone("bone_pelvis", 0, 1.0, 0.0),
        ];
        let rib_idx = if abdomen_fallback {
            bones.push(bone("bone_abdomen", 1, 0.2, 0.0));
            bones.push(bone("bone_ribcage", 2, 0.25, 0.0));
            3i32
        } else {
            bones.push(bone("bone_a_spine", 1, 0.1, 0.0));
            bones.push(bone("bone_b_spine", 2, 0.15, 0.0));
            bones.push(bone("bone_ribcage", 3, 0.2, 0.0));
            4i32
        };
        if with_neck {
            let neck_idx = bones.len() as i32;
            bones.push(bone("bone_neck", rib_idx, 0.2, 0.0));
            bones.push(bone("bone_head", neck_idx, 0.15, 0.0));
        } else {
            bones.push(bone("bone_head", rib_idx, 0.35, 0.0));
        }
        bones.push(bone("hp_weapons", rib_idx, -0.05, 0.45));
        bones.push(bone("bone_l_thigh", 1, -0.1, 0.0));
        bones.push(bone("bone_r_thigh", 1, -0.1, 0.0));
        let mesh = json::parse(
            &serde_json::json!({
                "attach": "hp_weapons",
                "bones": bones,
                "parts": []
            })
            .to_string(),
        )
        .unwrap();
        let rib = zero_hash::crc_hex("bone_ribcage");
        let wep = zero_hash::crc_hex("hp_weapons");
        let thigh = zero_hash::crc_hex("bone_l_thigh");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        for suffix in [
            "stand_idle_full",
            "stand_walkforward_full",
            "stand_shoot_full",
            "fall",
            "standalert_idle_full",
        ] {
            let tracks = if suffix == "stand_walkforward_full" {
                vec![rib.clone(), wep.clone(), thigh.clone()]
            } else {
                vec![rib.clone(), wep.clone()]
            };
            clips.push(serde_json::json!({
                "crc": zero_hash::crc_hex(&format!("quartz_carbine_{suffix}")),
                "flags": 0,
                "offset": samples.len(),
                "frames": 2,
                "tracks": tracks
            }));
            for _ in 0..2 {
                samples.extend([0f32, 0., 0., 1., 0., 0.2, 0.]);
            }
            for _ in 0..2 {
                samples.extend([0f32, 0., 0., 1., 0., -0.05, 0.45]);
            }
            if suffix == "stand_walkforward_full" {
                // Moving leg track so gait admission treats stand_walkforward as a stride clip.
                samples.extend([0f32, 0., 0., 1., 0., -0.1, 0.0]);
                samples.extend([0f32, 0., 0., 1., 0., -0.1, 0.05]);
            }
        }
        let meta = json::parse(
            &serde_json::json!({"fps": 30, "banks": {"quartz": {"clips": clips}}}).to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(r#"{"MaxSpeed":"6","weapons":[{"AnimationBank":"carbine"}]}"#).unwrap();
        let sets = json::parse(
            r#"{"set":[
                {"values":["stand_idle_lower"],"children":[
                    {"key":"Loop","values":[1.0]},
                    {"key":"BlendInTime","values":[0.1]},
                    {"key":"BlendOutTime","values":[0.1]}
                ]},
                {"values":["stand_idle_upper"],"children":[
                    {"key":"AimType","values":["Head"]}
                ]},
                {"values":["stand_walkforward_lower"],"children":[
                    {"key":"Loop","values":[1.0]},
                    {"key":"BlendInTime","values":[0.0]},
                    {"key":"BlendOutTime","values":[0.0]}
                ]},
                {"values":["stand_walkforward_upper"],"children":[
                    {"key":"AimType","values":["FullBody"]}
                ]},
                {"values":["stand_shoot_upper"],"children":[
                    {"key":"AimType","values":["Torso"]},
                    {"key":"BlendInTime","values":[0.1]},
                    {"key":"BlendOutTime","values":[0.1]}
                ]},
                {"values":["standalert_idle_upper"],"children":[
                    {"key":"AimType","values":["Torso"]},
                    {"key":"BlendInTime","values":[0.1]},
                    {"key":"BlendOutTime","values":[0.1]}
                ]},
                {"values":["fall"],"children":[
                    {"key":"Loop","values":[1.0]},
                    {"key":"AimType","values":["None"]},
                    {"key":"BlendInTime","values":[0.1]},
                    {"key":"BlendOutTime","values":[0.1]}
                ]}
            ]}"#,
        )
        .unwrap();
        let animation = Character::bind(
            "aim_tester",
            &def,
            &["quartz".into()],
            &build_actor(&mesh, &[]).unwrap(),
            &archive,
            &archive,
        )
        .unwrap();
        let tables = Tables::new(&animation, &def, &sets, &bfe_content::Value::Null).unwrap();
        Class {
            side: String::new(),
            name: "aim_tester".into(),
            model: String::new(),
            animation,
            tables,
        }
    }

    fn joint_elev(pose: &Pose, rig: &crate::animation2::Rig, name: &str) -> f64 {
        let j = rig.joint(name).unwrap();
        let e = &pose.world[j].elements;
        e[9].atan2(e[10])
    }

    #[test]
    fn aim_type_none_suppresses_spine_neck_head_and_blends_from_head() {
        let class = aim_modes_fixture(true, false);
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();
        let pitch = 0.4;

        // Begin in Head state (stand_idle): only neck/head pitch, spine stays level.
        player.publish(&class, State { tick: 1, pitch, ..Default::default() }, &mut pose);
        player.place_at(&class, 1., false, &mut pose);
        assert!(joint_elev(&pose, rig, "bone_ribcage").abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_head") - pitch * 0.625).abs() < 1e-6);

        // Transition to explicit AimType=None (fall, blend 0.1s = 6 ticks): relaxes to zero on all bones.
        player.publish(&class, State { tick: 7, air: Air::Fall, pitch, ..Default::default() }, &mut pose);
        player.place_at(&class, 7., false, &mut pose);
        for bone in ["bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head", "hp_weapons"] {
            assert!(joint_elev(&pose, rig, bone).abs() < 1e-6, "{bone} should not pitch in None");
        }
    }

    #[test]
    fn aim_type_head_turns_only_neck_and_head_with_neckless_fallback() {
        let class = aim_modes_fixture(true, false);
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();
        let pitch = 0.4;

        player.publish(&class, State { tick: 1, pitch, ..Default::default() }, &mut pose);
        player.place_at(&class, 1., false, &mut pose);
        for bone in ["bone_a_spine", "bone_b_spine", "bone_ribcage", "hp_weapons"] {
            assert!(joint_elev(&pose, rig, bone).abs() < 1e-6, "{bone} pitched in Head");
        }
        assert!((joint_elev(&pose, rig, "bone_neck") - pitch * 0.25).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_head") - pitch * 0.625).abs() < 1e-6);

        // Clamps at +/-0.7 * pi/2.
        player.publish(&class, State { tick: 2, pitch: 1.5, ..Default::default() }, &mut pose);
        player.place_at(&class, 2., false, &mut pose);
        assert!((joint_elev(&pose, rig, "bone_neck") - MAX_AIM_PITCH * 0.25).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_head") - MAX_AIM_PITCH * 0.625).abs() < 1e-6);

        // Without bone_neck, bone_head carries the full 0.625 share alone.
        let neckless = aim_modes_fixture(false, false);
        let nrig = &neckless.animation.rig;
        let mut narena = Arena::new(1, nrig.joints.len()).unwrap();
        let mut npose = narena.slot(0, nrig.joints.len());
        let mut nplayer = Player::default();
        nplayer.publish(&neckless, State { tick: 1, pitch, ..Default::default() }, &mut npose);
        nplayer.place_at(&neckless, 1., false, &mut npose);
        assert!(joint_elev(&npose, nrig, "bone_ribcage").abs() < 1e-6);
        assert!(joint_elev(&npose, nrig, "hp_weapons").abs() < 1e-6);
        assert!((joint_elev(&npose, nrig, "bone_head") - pitch * 0.625).abs() < 1e-6);
    }

    #[test]
    fn aim_type_torso_uses_spine_or_abdomen_chain_and_crossfades_from_head() {
        let class = aim_modes_fixture(true, false);
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();
        let pitch = 0.4;

        // Start in Head (stand_idle), then fire (Torso, 0.1s = 6 tick crossfade).
        player.publish(&class, State { tick: 1, pitch, ..Default::default() }, &mut pose);
        player.publish(&class, State { tick: 4, action: Action::Fire, pitch, ..Default::default() }, &mut pose);
        player.place_at(&class, 4., false, &mut pose);
        // At 50% crossfade: torso_weight = 0.5, head_weight = 0.5 -> ribcage = 0.5*p, head = (0.5 + 0.5*0.625)*p.
        assert!((joint_elev(&pose, rig, "bone_ribcage") - pitch * 0.5).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_head") - pitch * (0.5 + 0.5 * 0.625)).abs() < 1e-6);

        // After crossfade completes: full spine 0.125/0.375/0.5, no extra neck/head pitch.
        player.publish(&class, State { tick: 8, action: Action::Fire, pitch, ..Default::default() }, &mut pose);
        player.place_at(&class, 8., false, &mut pose);
        assert!((joint_elev(&pose, rig, "bone_a_spine") - pitch * 0.125).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_b_spine") - pitch * 0.5).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_ribcage") - pitch).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "hp_weapons") - pitch).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_neck") - pitch).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "bone_head") - pitch).abs() < 1e-6);

        // Two-bone abdomen + ribcage fallback splits 0.5 / 0.5.
        let fallback = aim_modes_fixture(true, true);
        let frig = &fallback.animation.rig;
        let mut farena = Arena::new(1, frig.joints.len()).unwrap();
        let mut fpose = farena.slot(0, frig.joints.len());
        let mut fplayer = Player::default();
        fplayer.publish(&fallback, State { tick: 1, action: Action::Fire, pitch, ..Default::default() }, &mut fpose);
        fplayer.place_at(&fallback, 1., false, &mut fpose);
        assert!((joint_elev(&fpose, frig, "bone_abdomen") - pitch * 0.5).abs() < 1e-6);
        assert!((joint_elev(&fpose, frig, "bone_ribcage") - pitch).abs() < 1e-6);
        assert!((joint_elev(&fpose, frig, "hp_weapons") - pitch).abs() < 1e-6);
    }

    #[test]
    fn aim_type_full_body_aligns_body_yaw_without_spine_or_head_pitch() {
        let class = aim_modes_fixture(true, false);
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let mut player = Player::default();

        // Oblique walk velocity (+45 deg from facing +Z) with AimType=FullBody and pitch=0.4:
        // zero procedural pitch on spine/neck/head AND zero hip yaw twist.
        player.publish(
            &class,
            State {
                tick: 1,
                velocity: [2., 0., 2.],
                pitch: 0.4,
                ..Default::default()
            },
            &mut pose,
        );
        player.place_at(&class, 1., false, &mut pose);
        for bone in ["bone_a_spine", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head", "hp_weapons"] {
            assert!(joint_elev(&pose, rig, bone).abs() < 1e-6, "{bone} pitched in FullBody");
        }
        let pelvis = rig.joint("bone_pelvis").unwrap();
        let pe = &pose.world[pelvis].elements;
        let pelvis_yaw = pe[8].atan2(pe[10]);
        assert!(pelvis_yaw.abs() < 1e-6, "pelvis yaw twisted {pelvis_yaw} in FullBody");
    }

    #[test]
    fn alert_rows_hold_the_aim_where_the_stance_has_them_and_fall_back_where_it_has_not() {
        let class = aim_modes_fixture(true, false);
        let rig = &class.animation.rig;
        let mut arena = Arena::new(1, rig.joints.len()).unwrap();
        let mut pose = arena.slot(0, rig.joints.len());
        let pitch = 0.4;
        let idle = |alert| State { tick: 1, pitch, alert, ..Default::default() };

        // Plain idle is a Head row: the spine stays level.
        let mut plain = Player::default();
        plain.publish(&class, idle(false), &mut pose);
        plain.place_at(&class, 1., false, &mut pose);
        assert!(joint_elev(&pose, rig, "bone_ribcage").abs() < 1e-6);
        assert!(joint_elev(&pose, rig, "hp_weapons").abs() < 1e-6);

        // Alert idle is a Torso row: the weapon follows the aim at once.
        let mut alert = Player::default();
        alert.publish(&class, idle(true), &mut pose);
        alert.place_at(&class, 1., false, &mut pose);
        assert!((joint_elev(&pose, rig, "bone_ribcage") - pitch).abs() < 1e-6);
        assert!((joint_elev(&pose, rig, "hp_weapons") - pitch).abs() < 1e-6);

        // Walking has no alert rows in this stance: the plain walk plays.
        let walk = |alert| State {
            tick: 1,
            pitch,
            alert,
            velocity: [0., 0., 6.],
            ..Default::default()
        };
        let plain_walk = class.tables.sample(&class.animation, &walk(false), 0.).motion;
        let alert_walk = class.tables.sample(&class.animation, &walk(true), 0.).motion;
        assert!(plain_walk == alert_walk);

        // A state that is not idle, walk or run ignores the flag.
        let falling = |alert| State { air: Air::Fall, alert, ..Default::default() };
        assert!(
            class.tables.sample(&class.animation, &falling(false), 0.).motion
                == class.tables.sample(&class.animation, &falling(true), 0.).motion
        );
    }

    #[test]
    fn a_combo_state_takes_its_aim_type_from_the_animation_block_only() {
        let mesh = json::parse(
            &serde_json::json!({"attach":"hp_weapons","bones":[
                {"name":"DummyRoot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]},
                {"name":"hp_weapons","parent":0,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,1,0,1]}
            ],"parts":[]})
            .to_string(),
        )
        .unwrap();
        let wep = zero_hash::crc_hex("hp_weapons");
        let clips: Vec<_> = ["quartz_carbine_stand_idle_full", "quartz_flourish"]
            .iter()
            .enumerate()
            .map(|(i, name)| {
                serde_json::json!({"crc": zero_hash::crc_hex(name), "flags": 0, "offset": i * 14,
                    "frames": 2, "tracks": [wep.clone()]})
            })
            .collect();
        let meta = json::parse(
            &serde_json::json!({"fps": 30, "banks": {"quartz": {"clips": clips}}}).to_string(),
        )
        .unwrap();
        let samples: Vec<f32> = (0..4).flat_map(|_| [0., 0., 0., 1., 0., 1., 0.]).collect();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let binding = serde_json::json!({
            "combo": "ember",
            "clipTiming": {"flourish": {"bank": "quartz",
                "crc": zero_hash::crc_hex("quartz_flourish"), "frames": 2, "fps": 30.0}},
            "stateAnimations": {"open": [{"key": "Animation", "name": "flourish", "playback": "loop"}]}
        });
        let def = json::parse(
            &serde_json::json!({"weapons": [{"AnimationBank": "carbine", "comboBindings": [binding]}]})
                .to_string(),
        )
        .unwrap();
        let machines = json::parse(
            &serde_json::json!({"ember": {
                "states": [{"values": ["open"], "children": [
                    {"key": "Animation", "values": ["flourish"], "children": [
                        {"key": "AimType", "values": ["FullBody"]}]}]}],
                "animations": [{"values": ["flourish"], "children": [
                    {"key": "AimType", "values": ["Torso"]}]}]
            }})
            .to_string(),
        )
        .unwrap();
        let animation = Character::bind(
            "flourisher",
            &def,
            &["quartz".into()],
            &build_actor(&mesh, &[]).unwrap(),
            &archive,
            &archive,
        )
        .unwrap();
        let tables = Tables::new(&animation, &def, &bfe_content::Value::Null, &machines).unwrap();
        let state = State { combo: Some(0), ..Default::default() };
        let row = tables.sample(&animation, &state, 0.).action.unwrap();
        // The animation block says Torso; the state's own child (FullBody) is not read.
        assert_eq!(row.aim_type, crate::animation2::state::AimType::Torso);
        assert!(!row.aim_type.locks_body_yaw());
    }
}
