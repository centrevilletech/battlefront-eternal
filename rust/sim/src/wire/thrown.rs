//! A Battlefront II hero's thrown saber as a world field: when the throw animation began
//! and, once the blade has left the hand, everything a viewer needs to replay its flight.
//! The payload stays the same from the press to the catch (ticks are sent as the tick they
//! happened at), so the snapshot codec sends a field once per press and once per launch.
//! A viewer steps the same flight rule the battle runs, [`SaberThrowState`], from the
//! launch parameters and the soldier's own replicated position, in Instant Action and
//! online alike. A Battlefront (2004) battle has no heroes with a throw and never writes
//! this schema.
use crate::combat::combo::{SaberThrowDef, SaberThrowPhase, SaberThrowState};

/// World field schema: the 2005 record kind 6 as a shared world field (`0x0200 + kind`),
/// next to the hero combat display (517). Its id is the soldier's own, `ID_BASE` plus the
/// unit slot.
pub const SCHEMA: f64 = 518.;
pub const ID_BASE: f64 = 100000.;
pub const REVISION: u16 = 1;
/// Encoded length: revision, flags, press tick, then the launch block, padded to 8 bytes.
pub const LEN: usize = 72;
const PRESSED: u8 = 1;
const LAUNCHED: u8 = 2;
/// A viewer drops a flight that has run this long without ending (seconds).
const MAX_SECONDS: f64 = 30.;

/// Where the blade started, which way it flew and how long each pass lasts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launch {
    /// Battle tick of the launch.
    pub tick: f64,
    pub origin: [f64; 3],
    pub direction: [f64; 3],
    /// Metres a second.
    pub speed: f64,
    /// Seconds of the outbound pass: `LifeSpan` scaled by the share of the range that was free.
    pub out_time: f64,
    /// Seconds of the return pass: `ReturnDuration` scaled by the same share.
    pub return_time: f64,
    /// Revolutions a second.
    pub rotation_rate: f64,
    /// The release point in the soldier's own frame: where the throw animation held the weapon
    /// mount when the blade left, and where the blade returns to.
    pub release: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Throw {
    /// Battle tick at which the throw animation began.
    pub press: Option<f64>,
    pub launch: Option<Launch>,
}

impl Throw {
    pub const IDLE: Self = Self { press: None, launch: None };

    pub fn encode_into(&self, out: &mut Vec<u8>) {
        out.clear();
        out.extend_from_slice(&REVISION.to_le_bytes());
        out.push(u8::from(self.press.is_some()) * PRESSED | u8::from(self.launch.is_some()) * LAUNCHED);
        out.extend_from_slice(&self.press.unwrap_or(0.).to_le_bytes());
        let l = self.launch.unwrap_or(Launch {
            tick: 0.,
            origin: [0.; 3],
            direction: [0.; 3],
            speed: 0.,
            out_time: 0.,
            return_time: 0.,
            rotation_rate: 0.,
            release: [0.; 3],
        });
        out.extend_from_slice(&l.tick.to_le_bytes());
        for v in l.origin.into_iter().chain(l.direction) {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        for v in [l.speed, l.out_time, l.return_time, l.rotation_rate] {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        for v in l.release {
            out.extend_from_slice(&(v as f32).to_le_bytes());
        }
        out.resize(LEN, 0);
    }

    /// A received throw, or `None` for bytes no producer writes.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != LEN || bytes[..2] != REVISION.to_le_bytes() {
            return None;
        }
        let flags = bytes[2];
        if flags & !(PRESSED | LAUNCHED) != 0 {
            return None;
        }
        let f64_at = |at: usize| f64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
        let f32_at = |at: usize| f64::from(f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()));
        let finite = |v: f64| v.is_finite().then_some(v);
        let press = if flags & PRESSED != 0 { Some(finite(f64_at(3))?) } else { None };
        let launch = if flags & LAUNCHED != 0 {
            let vec = |at: usize| -> Option<[f64; 3]> {
                Some([finite(f32_at(at))?, finite(f32_at(at + 4))?, finite(f32_at(at + 8))?])
            };
            let direction = vec(31)?;
            let length_sq: f64 = direction.iter().map(|v| v * v).sum();
            if (length_sq - 1.).abs() > 1e-3 {
                return None;
            }
            let positive = |at: usize, max: f64| finite(f32_at(at)).filter(|v| (0. ..=max).contains(v));
            Some(Launch {
                tick: finite(f64_at(11))?,
                origin: vec(19)?,
                direction,
                speed: positive(43, 1000.)?,
                out_time: positive(47, MAX_SECONDS)?,
                return_time: positive(51, MAX_SECONDS)?,
                rotation_rate: positive(55, 100.)?,
                release: vec(59)?,
            })
        } else {
            None
        };
        Some(Self { press, launch })
    }
}

/// The soldier-frame release point placed in the world: the soldier's position plus the
/// offset turned by its aim yaw.
pub fn release_point(position: [f64; 3], yaw: f64, offset: [f64; 3]) -> [f64; 3] {
    let (s, c) = v8math::sin_cos(yaw);
    [
        position[0] + c * offset[0] + s * offset[2],
        position[1] + offset[1],
        position[2] - s * offset[0] + c * offset[2],
    ]
}

/// One tick of a replayed flight, as the presenter draws it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Sample {
    /// Battle tick of the launch (the identity of this throw).
    pub launched_tick: u32,
    pub position: [f64; 3],
    pub prev_position: [f64; 3],
    pub direction: [f64; 3],
    pub spin_angle: f64,
    pub prev_spin_angle: f64,
    pub returning: bool,
    /// Where the blade is heading while it returns: the release point at this tick.
    pub target: [f64; 3],
}

/// A viewer's copy of one flight, stepped with the battle's own rule.
#[derive(Clone, Copy, Debug)]
pub struct Replay {
    launch: Launch,
    def: SaberThrowDef,
    state: SaberThrowState,
    /// The last tick stepped.
    tick: f64,
    started: bool,
    finished: bool,
}

impl Replay {
    pub fn new(launch: Launch) -> Self {
        let def = SaberThrowDef {
            velocity: launch.speed,
            life_span: launch.out_time.max(1e-9),
            return_duration: launch.return_time.max(1e-9),
            rotation_rate: launch.rotation_rate,
            ..SaberThrowDef::default()
        };
        Self { launch, def, state: SaberThrowState::new(), tick: launch.tick, started: false, finished: false }
    }

    pub fn launch(&self) -> &Launch {
        &self.launch
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Step the flight to `tick`. The soldier stood at `position` heading `yaw` for every tick
    /// stepped; a tick before the launch has no sample. After the catch there is none either.
    pub fn at(&mut self, tick: f64, position: [f64; 3], yaw: f64) -> Option<Sample> {
        if self.finished || tick < self.launch.tick || tick - self.launch.tick > MAX_SECONDS * 60. {
            return None;
        }
        let target = release_point(position, yaw, self.launch.release);
        if !self.started {
            let l = &self.launch;
            // The launch tick draws the blade at its origin; stepping starts with the next tick.
            // The def carries the already scaled times, so the share is 1.
            self.started = self.state.launch_reaching(&self.def, l.origin, l.direction, &mut FreeEnergy, 1.);
            if !self.started {
                self.finished = true;
                return None;
            }
            self.tick = l.tick;
        }
        while self.tick + 0.5 < tick {
            self.tick += 1.;
            let step = self.state.step(&self.def, target, true, 1. / 60.);
            if step.caught || !self.state.is_active() {
                self.finished = true;
                return None;
            }
        }
        Some(self.sample(target))
    }

    fn sample(&self, target: [f64; 3]) -> Sample {
        Sample {
            launched_tick: self.launch.tick as u32,
            position: self.state.position,
            prev_position: self.state.prev_position,
            direction: self.state.direction,
            spin_angle: self.state.spin_angle,
            prev_spin_angle: self.state.prev_spin_angle,
            returning: self.state.phase == SaberThrowPhase::Returning,
            target,
        }
    }
}

/// A replay never pays: the battle already did.
struct FreeEnergy;
impl crate::combat::combo::CombatEnergy for FreeEnergy {
    fn balance(&self) -> f64 {
        0.
    }
    fn available(&self) -> f64 {
        0.
    }
    fn pay(&mut self, _amount: f64) -> bool {
        true
    }
}

/// Publishes every living, unseated hero's throw with the battle's world fields.
#[derive(Default)]
pub struct Fields {
    payload: Vec<u8>,
}

impl Fields {
    pub(crate) fn take(
        &mut self,
        tick: f64,
        units: &crate::units::owner::Units,
        store: &crate::gen::Store,
        stagger: &mut super::stagger::Stagger,
    ) -> super::codec::Result<()> {
        let Some(infantry) = units.infantry.as_deref() else {
            return Ok(());
        };
        let mut result = Ok(());
        let payload = &mut self.payload;
        infantry.throws(store, tick, |slot, generation, throw| {
            if result.is_ok() {
                throw.encode_into(payload);
                result = offer(stagger, tick, slot, generation, payload);
            }
        });
        result
    }
}

/// Offers one hero's throw. Like the combat display, a throw never takes the last
/// world payloads for a new field: on a nearly full world it waits for room instead
/// of stopping the battle's publication.
fn offer(
    stagger: &mut super::stagger::Stagger,
    tick: f64,
    slot: u32,
    generation: f64,
    payload: &[u8],
) -> super::codec::Result<()> {
    let id = ID_BASE + f64::from(slot);
    if !stagger.has_room(SCHEMA, id, super::melee::RESERVE) {
        return Ok(());
    }
    stagger.offer_native(tick, SCHEMA, id, generation, payload, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flight() -> Launch {
        Launch {
            tick: 640.,
            origin: [31.5, 1.25, -8.75],
            direction: [0.6, 0.0, 0.8],
            speed: 17.5,
            out_time: 0.5,
            return_time: 0.25,
            rotation_rate: 3.0,
            release: [-0.5, 1.5, 0.75],
        }
    }

    #[test]
    fn a_new_throw_field_leaves_the_last_world_payloads_to_the_battle() {
        let mut stagger = super::super::stagger::Stagger::new();
        let mut other = 0.;
        while stagger.has_room(SCHEMA, -1., super::super::melee::RESERVE + 1) {
            stagger.offer_native(0., 9., other, 1., &[1], None).unwrap();
            other += 1.;
        }
        // One payload above the reserve is left: one hero's field takes it.
        offer(&mut stagger, 0., 3, 1., &[7]).unwrap();
        assert!(stagger.entries.iter().any(|e| e.schema == SCHEMA && e.id == ID_BASE + 3.));
        // The next hero's new field waits; the battle can still add its own.
        offer(&mut stagger, 0., 4, 1., &[7]).unwrap();
        assert!(!stagger.entries.iter().any(|e| e.schema == SCHEMA && e.id == ID_BASE + 4.));
        stagger.offer_native(0., 9., other, 1., &[1], None).unwrap();
        // A field already there keeps travelling.
        offer(&mut stagger, 6., 3, 1., &[8]).unwrap();
        let kept = stagger.entries.iter().find(|e| e.schema == SCHEMA && e.id == ID_BASE + 3.).unwrap();
        assert_eq!(kept.field.as_ref().map(|f| f.payload.clone()), Some(vec![8]));
    }

    #[test]
    fn a_throw_survives_the_wire_in_every_state() {
        let mut bytes = Vec::new();
        for throw in [
            Throw::IDLE,
            Throw { press: Some(623.), launch: None },
            Throw { press: Some(623.), launch: Some(flight()) },
        ] {
            throw.encode_into(&mut bytes);
            assert_eq!(bytes.len(), LEN);
            let back = Throw::decode(&bytes).expect("decodes");
            assert_eq!(back.press, throw.press);
            match (back.launch, throw.launch) {
                (Some(a), Some(b)) => {
                    assert_eq!(a.tick, b.tick);
                    for i in 0..3 {
                        assert!((a.origin[i] - b.origin[i]).abs() < 1e-5);
                        assert!((a.direction[i] - b.direction[i]).abs() < 1e-5);
                        assert!((a.release[i] - b.release[i]).abs() < 1e-5);
                    }
                    assert!((a.speed - b.speed).abs() < 1e-5 && (a.out_time - b.out_time).abs() < 1e-5);
                    assert!((a.return_time - b.return_time).abs() < 1e-5);
                    assert!((a.rotation_rate - b.rotation_rate).abs() < 1e-5);
                }
                (None, None) => {}
                other => panic!("launch lost: {other:?}"),
            }
        }
        // Bytes no producer writes are refused: wrong length, revision, flags, direction.
        let mut bad = bytes.clone();
        assert!(Throw::decode(&bad[..LEN - 1]).is_none());
        bad[0] = 9;
        assert!(Throw::decode(&bad).is_none());
        let mut bad = bytes.clone();
        bad[2] = 0x80;
        assert!(Throw::decode(&bad).is_none());
        let mut bad = bytes.clone();
        bad[35..39].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(Throw::decode(&bad).is_none());
        // A direction that is not a unit vector is refused as well.
        let mut bad = bytes.clone();
        bad[39..43].copy_from_slice(&2.0f32.to_le_bytes());
        assert!(Throw::decode(&bad).is_none());
    }

    #[test]
    fn a_pressed_and_idle_throw_never_change_a_constant_payload() {
        let mut a = Vec::new();
        let mut b = Vec::new();
        Throw { press: Some(9.), launch: Some(flight()) }.encode_into(&mut a);
        Throw { press: Some(9.), launch: Some(flight()) }.encode_into(&mut b);
        assert_eq!(a, b);
    }

    #[test]
    fn the_replay_runs_the_battles_flight_rule_to_the_catch() {
        let launch = flight();
        // The battle's own run of the same throw, with the soldier standing still.
        let def = SaberThrowDef {
            velocity: launch.speed,
            life_span: launch.out_time,
            return_duration: launch.return_time,
            rotation_rate: launch.rotation_rate,
            ..SaberThrowDef::default()
        };
        let position = [30., 0., -9.];
        let target = release_point(position, 0.4, launch.release);
        let mut battle = SaberThrowState::new();
        assert!(battle.launch_reaching(&def, launch.origin, launch.direction, &mut FreeEnergy, 1.));
        let mut replay = Replay::new(launch);
        let first = replay.at(640., position, 0.4).expect("the launch tick draws the blade");
        assert_eq!(first.position, launch.origin);
        assert!(!first.returning);
        assert_eq!(first.launched_tick, 640);
        let mut tick = 640.;
        let mut ended = None;
        for _ in 0..200 {
            tick += 1.;
            let step = battle.step(&def, target, true, 1. / 60.);
            let sample = replay.at(tick, position, 0.4);
            if step.caught {
                assert!(sample.is_none(), "the replay ends on the battle's catch tick");
                ended = Some(tick);
                break;
            }
            let s = sample.expect("a sample while it flies");
            assert_eq!(s.position, battle.position);
            assert_eq!(s.spin_angle, battle.spin_angle);
            assert_eq!(s.returning, battle.phase == SaberThrowPhase::Returning);
            assert_eq!(s.target, target);
        }
        // 0.5 s out (30 ticks) + 0.25 s back (15 ticks) after the launch tick.
        assert_eq!(ended, Some(640. + 30. + 15.));
        assert!(replay.is_finished());
        // Nothing before the launch, nothing after the catch.
        let mut early = Replay::new(launch);
        assert!(early.at(639., position, 0.4).is_none());
    }

    #[test]
    fn a_replay_that_catches_up_steps_every_missed_tick() {
        let launch = flight();
        let position = [30., 0., -9.];
        let mut every = Replay::new(launch);
        let mut last = None;
        for t in 640..=660 {
            last = every.at(f64::from(t), position, 0.4);
        }
        // A viewer that first hears of the flight at tick 660 gets the same blade.
        let mut late = Replay::new(launch);
        let jumped = late.at(660., position, 0.4);
        assert_eq!(jumped, last);
        // Asking twice for one tick changes nothing.
        assert_eq!(late.at(660., position, 0.4), jumped);
        // A flight that is too long to be real ends instead of running for ever.
        let mut old = Replay::new(launch);
        assert!(old.at(640. + 60. * 31., position, 0.4).is_none());
    }
}
