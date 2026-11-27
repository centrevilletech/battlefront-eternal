//! Read the existing published motion and action journal without retaining packet memory.
use crate::online::publications::Field;
use bfe_content_native::animation2::{
    state::{Action, Air, Direction, State},
    Class,
};
use bfe_presentation::network_layout::{net_kind as k, net_record as r};
use bfe_sim::wire::melee::{self, CastPose, Display, MeleePose};
use bfe_sim::{gen::Soldier, movement::soldier::swbf2::RollDirection};
use serde::Deserialize;
use three::Matrix4;

const HARD_LANDING_SECONDS: f64 = 1.5;
const HARD_LANDING_MIN_SECONDS: f64 = 0.5;
const LANDING_CANCEL_SPEED_SQ: f64 = 0.01;
const AIR_RESTART_VY_STEP: f64 = 4.0;
/// A soldier stays alert for 2.5 s after it last fired or took up a weapon.
const ALERT_TICKS: u32 = 150;

/// The player's own soldier while this client predicts him, and after it. His
/// poses are of the prediction's ticks, ahead of all the server has said of
/// him, so nothing he does waits for it, and he stands where the prediction
/// shows him this frame. When the prediction stops (he falls, or takes a seat)
/// the server's ticks go on from the last of those.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Ahead {
    pub epoch: f64,
    pub id: (u32, u32),
    /// From the tick of his newest correction to his newest pose.
    lead: f64,
    /// From the clock every other soldier is shown at to his.
    clock: f64,
    /// Where he is shown; none once he is not predicted.
    pub root: Option<Matrix4>,
    /// Rounds his prediction has fired, and whether it has him reloading.
    pub rounds: f64,
    pub reloading: bool,
}
/// One frame of the prediction: the tick it stands at, the one it is displayed
/// at, where it is displayed, its new rounds and whether it reloads.
pub(super) struct Predicted {
    pub tick: f64,
    pub displayed: f64,
    pub root: Matrix4,
    pub rounds: usize,
    pub reloading: bool,
}
impl Ahead {
    /// The own soldier `id` in `epoch` on a frame whose newest correction is of
    /// `own` and whose display clock shows `clock`.
    pub fn frame(last: Option<Self>, epoch: f64, id: (u32, u32), own: f64, clock: f64, predicted: Option<Predicted>) -> Option<Self> {
        let last = last.filter(|last| last.epoch == epoch && last.id == id);
        let Some(now) = predicted else {
            return last.map(|last| Self { lead: last.lead + f64::from(u8::from(last.root.is_some())), root: None, ..last });
        };
        Some(Self { epoch, id, lead: now.tick - own, clock: now.displayed - clock, root: Some(now.root),
            rounds: last.map_or(0., |last| last.rounds) + now.rounds as f64, reloading: now.reloading })
    }
    /// The tick of his newest pose, and the one he is shown at.
    pub fn newest(&self, own: f64) -> u32 { (own + self.lead).max(0.) as u32 }
    pub fn shown(&self, clock: f64) -> f64 { clock + self.clock }
    /// His record as his prediction has him: the server's of him is of an
    /// older tick. What it cannot foresee (his health) stays the server's.
    pub fn record(&self, row: &[f64; r::STRIDE], body: Option<&Soldier>) -> [f64; r::STRIDE] {
        let mut row = *row;
        row[r::FIRE_COUNTER] = self.rounds;
        if let Some(body) = body {
            row[r::POSITION..r::POSITION + 3].copy_from_slice(&body.position);
            row[r::VELOCITY..r::VELOCITY + 3].copy_from_slice(&body.velocity);
            (row[r::YAW], row[r::PITCH]) = (body.aim_yaw, body.aim_pitch);
            row[r::POSTURE] = f64::from(body.posture as u8);
            row[r::STATE] = body.air.map_or(0., |air| f64::from(air as u8 + 1));
            row[r::DISC] = 0.;
        }
        row
    }
}
/// A 2005 soldier's sprint and roll: the roll `seconds` after its first step,
/// of a window `window` seconds long.
pub(super) fn movement(state: &mut State, sprint: bool, roll: Option<(RollDirection, f64, f64)>) {
    state.sprint = sprint;
    state.roll = roll.map(|(direction, ..)| match direction {
        RollDirection::Forward => Direction::Forward,
        RollDirection::Left => Direction::Left,
        RollDirection::Right => Direction::Right,
    });
    if let Some((_, seconds, window)) = roll {
        (state.phase_time, state.roll_seconds) = (Some(seconds), Some(window));
    }
}

#[derive(Default)]
pub(super) struct Clocks {
    pub last: Option<u32>,
    position: Option<[f64; 3]>,
    seat: Option<(u32, u32, usize)>,
    health: f64,
    fire: f64,
    death: Option<u32>,
    action: Option<(Action, u32)>,
    phase: (u8, u32),
    air_start: Option<u32>,
    landed: Option<u32>,
    hard_landing: bool,
    vertical_speed: f64,
    jump_count: Option<u32>,
    impact: Option<(u32, Option<[f64; 3]>)>,
    /// Tick of the last shot or weapon selection that keeps the soldier alert.
    alert_from: Option<u32>,
    weapon: Option<usize>,
    choked: Option<(u32, u32)>,
    /// The battle's last melee pose for this life (swing, guard, guard reaction).
    pub melee: Option<MeleePose>,
    /// The pose of a power this life casts or holds, as the battle last published it.
    pub cast: Option<CastPose>,
    /// The clip of the choke that holds this life, as the battle last published it.
    pub choke: Option<u32>,
}
pub(super) struct Event {
    pub owner: (u32, u32),
    pub tick: u32,
    pub action: Action,
    pub weapon_id: usize,
    pub channel: u8,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message<'a> {
    kind: &'a str,
    owner_id: u32,
    #[serde(default)]
    owner_generation: u32,
    action: Option<&'a str>,
    #[serde(default)]
    bank: &'a str,
    #[serde(default)]
    mount_id: u32,
    #[serde(default)]
    channel: u8,
    #[serde(default)]
    weapon_id: usize,
}
/// A hero's published throw: who it belongs to and what it says.
pub(super) fn throw(field: &Field) -> Option<((u32, u32), bfe_sim::wire::thrown::Throw)> {
    use bfe_sim::wire::thrown::{ID_BASE, SCHEMA};
    if field.row[r::KIND] != k::SUMMARY as f64 || field.row[r::SCHEMA] != SCHEMA {
        return None;
    }
    let throw = bfe_sim::wire::thrown::Throw::decode(&field.bytes[..field.length])?;
    let id = field.row[r::ID] - ID_BASE;
    (id >= 0. && id < f64::from(u32::MAX)).then_some(((id as u32, field.row[r::GENERATION] as u32), throw))
}
pub(super) fn action_event(field: &Field) -> Option<Event> {
    if field.row[r::KIND] != k::COMBAT as f64 || field.row[r::SCHEMA] != 8. {
        return None;
    }
    let msg: Message = serde_json::from_slice(&field.bytes[..field.length]).ok()?;
    let action = match (msg.kind, msg.action) {
        ("weaponAction", Some("reload")) => Action::Reload,
        ("weaponAction", Some("shoot" | "shoot2" | "shoot3" | "shoot4" | "shoot5")) => {
            if msg.bank.is_empty() {
                Action::FireSecondary
            } else {
                Action::Fire
            }
        }
        ("shot", _) if msg.mount_id == 0 => Action::Fire,
        ("endWeaponAction", _) => Action::None,
        _ => return None,
    };
    Some(Event {
        owner: (msg.owner_id, msg.owner_generation),
        tick: field.row[r::TICK] as u32,
        action,
        weapon_id: msg.weapon_id,
        channel: msg.channel,
    })
}
fn seat(row: &[f64; r::STRIDE]) -> Option<(u32, u32, usize)> {
    (row[r::SEAT_PRESENT] != 0.).then(|| {
        (
            row[r::SEAT_VEHICLE_ID] as u32,
            row[r::SEAT_VEHICLE_GENERATION] as u32,
            row[r::SEAT_INDEX] as usize,
        )
    })
}
fn dead(row: &[f64; r::STRIDE]) -> bool {
    row[r::DEAD] != 0. || matches!(row[r::STATE] as u8, 7 | 8) || row[r::HEALTH] <= 0.
}
impl Clocks {
    pub fn note_jump_count(&mut self, count: u32, tick: u32, phase: u8) {
        if self.jump_count.is_some_and(|prev| prev != count) && phase == 1 {
            self.air_start = Some(tick);
        }
        self.jump_count = Some(count);
    }
    pub fn incoming(&mut self, tick: u32, bearing: Option<[f64; 3]>) {
        if self.impact.is_none_or(|(old, _)| old <= tick) {
            self.impact = Some((tick, bearing));
        }
    }
    fn refresh_alert(&mut self, tick: u32) {
        if self.alert_from.is_none_or(|from| from < tick) {
            self.alert_from = Some(tick);
        }
    }
    pub fn event(&mut self, action: Action, tick: u32) {
        if matches!(
            action,
            Action::Fire | Action::FireSecondary | Action::FireSecondary2
        ) {
            self.refresh_alert(tick);
        }
        if self.action.is_none_or(|(_, t)| t <= tick) {
            self.action = Some((action, tick));
        }
    }
    pub fn choke_elapsed(&mut self, anim: Option<u32>, tick: u32) -> Option<(u32, f64)> {
        let Some(crc) = anim else {
            self.choked = None;
            return None;
        };
        let start = match self.choked {
            Some((prev, start)) if prev == crc => start,
            _ => {
                self.choked = Some((crc, tick));
                tick
            }
        };
        Some((crc, f64::from(tick.saturating_sub(start)) / 60.))
    }
    /// A complete snapshot confirms that any omitted row still has its last value.
    pub fn sample_tick(
        &self,
        row: &[f64; r::STRIDE],
        record_tick: u32,
        snapshot_tick: u32,
    ) -> Option<u32> {
        let tick = snapshot_tick.max(record_tick);
        self.last
            .is_none_or(|last| {
                tick > last
                    || (tick == last
                        && (self.fresh_cut(row)
                            || self.seat != seat(row)
                            || (self.death.is_some() && !dead(row))))
            })
            .then_some(tick)
    }
    fn fresh_cut(&self, row: &[f64; r::STRIDE]) -> bool {
        row[r::DISC] != 0.
            && self
                .position
                .is_some_and(|old| old != row[r::POSITION..r::POSITION + 3])
    }
    pub fn state(
        &mut self,
        row: &[f64; r::STRIDE],
        tick: u32,
        weapon: usize,
        class: &Class,
    ) -> State {
        let discontinuity = self.fresh_cut(row);
        let position = std::array::from_fn(|i| row[r::POSITION + i]);
        self.position = Some(position);
        let velocity = std::array::from_fn(|i| row[r::VELOCITY + i]);
        let phase = row[r::STATE] as u8;
        let age = |from| f64::from(tick.saturating_sub(from)) / 60.;
        if self.last.is_none() || self.phase.0 != phase {
            let previous = self.phase.0;
            self.landed = (phase == 0 && matches!(previous, 1..=4)).then_some(tick);
            if self.landed.is_some() {
                self.hard_landing = matches!(previous, 1 | 2)
                    && self
                        .air_start
                        .is_some_and(|start| age(start) > HARD_LANDING_SECONDS);
                self.air_start = None;
            } else if matches!(phase, 1 | 2) {
                if self.air_start.is_none() || previous == 4 {
                    self.air_start = Some(tick);
                }
            } else {
                self.air_start = None;
            }
            self.phase = (phase, tick);
        } else if phase == 1 && velocity[1] - self.vertical_speed > AIR_RESTART_VY_STEP {
            self.air_start = Some(tick);
        }
        if dead(row) {
            self.death.get_or_insert(tick);
        } else if self.last.is_some() {
            if self.death.take().is_some() {
                self.action = None;
                self.impact = None;
                self.refresh_alert(tick);
            } else if row[r::HEALTH] < self.health {
                self.event(Action::Hit, tick);
            } else if row[r::FIRE_COUNTER] != self.fire {
                self.event(Action::Fire, tick);
            }
        }
        if self.weapon.is_some_and(|previous| previous != weapon) {
            self.refresh_alert(tick);
        }
        self.weapon = Some(weapon);
        self.last = Some(tick);
        self.health = row[r::HEALTH];
        self.fire = row[r::FIRE_COUNTER];
        self.vertical_speed = velocity[1];
        let yaw = row[r::YAW];
        let (action, from) = self
            .death
            .map(|t| (Action::Death, t))
            .or(self.action)
            .unwrap_or((Action::None, tick));
        self.seat = seat(row);
        let mut s = State {
            tick,
            discontinuity,
            position,
            yaw,
            pitch: row[r::PITCH],
            impact: self
                .impact
                .filter(|(t, _)| *t <= tick && t.abs_diff(from) <= 3)
                .and_then(|(_, bearing)| bearing)
                .map(|d| Direction::from_velocity(d, yaw)),
            hard_landing: self.hard_landing,
            seat: self.seat,
            velocity,
            crouch: row[r::POSTURE] == 1.,
            weapon,
            action,
            elapsed: age(from),
            phase_time: Some(age(self.phase.1)),
            air: match phase {
                1 => Air::Jump,
                2 => Air::Fall,
                3 => Air::Thrown,
                4 => Air::Jet,
                0 if self.landed.is_some() => Air::Land,
                _ => Air::Ground,
            },
            roll: matches!(phase, 5 | 6).then(|| Direction::from_velocity(velocity, yaw)),
            alert: self
                .alert_from
                .is_some_and(|from| tick < from.saturating_add(ALERT_TICKS)),
            ..Default::default()
        };
        if let Some(landed) = self.landed {
            let row = class.tables.sample(&class.animation, &s, 0.).motion;
            let duration = [row.base, row.lower, row.upper]
                .into_iter()
                .flatten()
                .map(|i| class.animation.clips[i].duration())
                .fold(0., f64::max);
            let land_age = age(landed);
            let moving =
                velocity[0] * velocity[0] + velocity[2] * velocity[2] >= LANDING_CANCEL_SPEED_SQ;
            let cancelled = moving
                && ((!self.hard_landing && land_age > 0.)
                    || (self.hard_landing && land_age > HARD_LANDING_MIN_SECONDS));
            if land_age >= duration || cancelled {
                self.landed = None;
                s.air = Air::Ground;
            }
        }
        if action != Action::Death && action != Action::None {
            let row = class.tables.sample(&class.animation, &s, 0.).action;
            let duration = row
                .into_iter()
                .flat_map(|r| [r.base, r.lower, r.upper])
                .flatten()
                .map(|i| class.animation.clips[i].duration())
                .fold(0., f64::max);
            if s.elapsed >= duration {
                s.action = Action::None;
            }
        }
        s
    }
}

/// A hero's combat display the battle published for a life, from its world field.
pub(super) fn hero_display(field: &Field) -> Option<((u32, u32), Display)> {
    if field.row[r::KIND] != f64::from(k::SUMMARY) || field.row[r::SCHEMA] != melee::SCHEMA {
        return None;
    }
    let slot = field.row[r::ID] - melee::ID_BASE;
    if !(0. ..f64::from(u16::MAX)).contains(&slot) || slot.fract() != 0. {
        return None;
    }
    let display = Display::decode(field.bytes.get(..field.length)?)?;
    Some(((slot as u32, field.row[r::GENERATION] as u32), display))
}

/// The action a published cast pose plays: the power weapon's fire, second fire or throw clip.
pub(super) fn cast_action(cast: &CastPose) -> Action {
    match cast.fire_anim {
        2 => Action::Throw,
        1 => Action::FireSecondary2,
        _ => Action::FireSecondary,
    }
}

/// Every soldier but the local player shows the melee pose the battle published for its
/// life, at the state's tick: the same table rows, clip clock, blade mask, guard
/// reaction and swing turn as the player's own sample. `elapsed` keeps timing the presenter's own
/// action (a hit flinch, a death).
pub(super) fn published_combo(s: &mut State, pose: &MeleePose) {
    let tick = f64::from(s.tick);
    s.combo = Some(usize::from(pose.state));
    s.anim_time = pose.anim_time(tick);
    s.blades_off = pose.blades_off;
    s.swing_turn = pose.swing_turn;
    if let Some((clip, time)) = pose.reaction_at(tick) {
        s.contact_clip = Some(clip);
        s.anim_time = time;
    }
}

/// The damage/death payload contains the target point and, optionally, the
/// source point. If the sender omits the point, use the same-life attacker.
type Bearing = ((u32, u32), Option<[f64; 3]>);

pub(super) fn impact(
    field: &Field,
    receiver: &crate::online::receiver::Receiver,
) -> Option<Bearing> {
    if field.row[r::KIND] != f64::from(k::COMBAT) || !matches!(field.row[r::SCHEMA] as u32, 3 | 4) {
        return None;
    }
    let b = field.bytes.get(..field.length)?;
    let id = |at| Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?));
    let position = |at| -> Option<[f64; 3]> {
        let mut p = [0.; 3];
        for (i, to) in p.iter_mut().enumerate() {
            let start = at + i * 8;
            *to = f64::from_le_bytes(b.get(start..start + 8)?.try_into().ok()?);
        }
        p.iter().all(|v| v.is_finite()).then_some(p)
    };
    let victim = (id(4)?, id(8)?);
    let target = position(20)?;
    let has_source = u16::from_le_bytes(b.get(52..54)?.try_into().ok()?);
    let source = if has_source != 0 {
        position(54)
    } else {
        let (attacker, generation) = (id(12)?, id(16)?);
        [k::SOLDIER, k::DROIDEKA, k::VEHICLE, k::TURRET]
            .into_iter()
            .find_map(|kind| receiver.record(kind, attacker.into(), generation.into()))
            .map(|row| std::array::from_fn(|i| row[r::POSITION + i]))
    };
    let bearing = source
        .map(|p| std::array::from_fn(|i| p[i] - target[i]))
        .filter(|v| v[0].hypot(v[2]) > 1e-6);
    Some((victim, bearing))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Two frames a tick, a correction every three ticks and fifteen behind: the
    // own soldier's poses are of the prediction's ticks and his clock is the
    // frame's. When he falls the server's ticks go on from there: no pose is of
    // a tick he was already shown at, and his clock neither stops nor jumps.
    #[test]
    fn the_own_soldier_stays_ahead_of_the_server_and_never_steps_back() {
        let id = (7, 1);
        let mut ahead = None;
        let (mut own, mut clock, mut shown, mut newest) = (100., 95.5, f64::NAN, 0);
        for frame in 0..240 {
            let tick = 115. + f64::from(frame / 2);
            if frame % 6 == 0 { own = 100. + f64::from(frame / 2); }
            clock += 0.5;
            let displayed = tick - 1. + f64::from(frame % 2) / 2.;
            let predicted = Predicted { tick, displayed, root: Matrix4::default(), rounds: usize::from(frame == 50 || frame == 51), reloading: frame >= 200 };
            ahead = Ahead::frame(ahead, 1., id, own, clock, Some(predicted));
            let now = ahead.unwrap();
            assert_eq!((now.newest(own), now.shown(clock)), (tick as u32, displayed));
            assert_eq!((now.rounds, now.reloading), (f64::from(u8::from(frame >= 50) + u8::from(frame >= 51)), frame >= 200));
            (newest, shown) = (tick as u32, displayed);
        }
        for frame in 240..300 {
            if frame % 6 == 0 { own += 3.; }
            clock += 0.5;
            ahead = Ahead::frame(ahead, 1., id, own, clock, None);
            let now = ahead.unwrap();
            assert!(now.root.is_none() && now.newest(own) > newest, "frame {frame}");
            assert_eq!(now.shown(clock), shown + 0.5);
            assert!(now.shown(clock) <= f64::from(now.newest(own)) + 3.);
            shown = now.shown(clock);
            if frame == 240 { newest = now.newest(own) - 1; }
        }
        assert_eq!(Ahead::frame(ahead, 1., (7, 2), own, clock, None), None, "the next life starts over");
        assert_eq!(Ahead::frame(ahead, 2., id, own, clock, None), None, "a new epoch starts over even at the same id and life");
        let fresh = Ahead::frame(ahead, 2., id, 10., 8., Some(Predicted { tick: 15., displayed: 14.5, root: Matrix4::default(), rounds: 0, reloading: false })).unwrap();
        assert_eq!((fresh.epoch, fresh.rounds, fresh.reloading), (2., 0., false));
    }

    #[test]
    fn the_own_record_is_his_prediction_s_but_for_what_it_cannot_foresee() {
        let mut row = [0.; r::STRIDE];
        (row[r::HEALTH], row[r::FIRE_COUNTER], row[r::DISC], row[r::STATE], row[r::YAW]) = (180., 9., 1., 1., 2.);
        let body = Soldier { position: [4., 5., 6.], velocity: [1., 0., -1.], aim_yaw: 0.5, aim_pitch: -0.1, posture: bfe_sim::gen::Posture::Crouch, ..Default::default() };
        let now = Predicted { tick: 40., displayed: 39.5, root: Matrix4::default(), rounds: 3, reloading: false };
        let ahead = Ahead::frame(None, 1., (7, 1), 30., 25., Some(now)).unwrap();
        let shown = ahead.record(&row, Some(&body));
        assert_eq!((shown[r::POSITION], shown[r::POSITION + 2], shown[r::VELOCITY], shown[r::YAW], shown[r::PITCH]), (4., 6., 1., 0.5, -0.1));
        assert_eq!((shown[r::POSTURE], shown[r::STATE], shown[r::DISC], shown[r::HEALTH], shown[r::FIRE_COUNTER]), (1., 0., 0., 180., 3.));
        // Once he is not predicted only his rounds stay as he last fired them.
        let after = Ahead::frame(Some(ahead), 1., (7, 1), 31., 25.5, None).unwrap().record(&row, None);
        assert_eq!((after[r::FIRE_COUNTER], after[r::YAW], after[r::DISC]), (3., 2., 1.));
    }

    fn generated_class() -> Class {
        use bfe_content::{actor::build_actor, json, zero_hash};
        use bfe_content_native::animation2::{state::Tables, Archive, Character};
        let mesh = json::parse(r#"{"bones":[{"name":"pivot","parent":-1,"local":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1]}],"parts":[]}"#).unwrap();
        let bone = zero_hash::crc_hex("pivot");
        let mut samples = Vec::new();
        let mut clips = Vec::new();
        for (suffix, heights) in [("stand_idle", [1., 2., 1.]), ("die", [1., -1., -2.])] {
            clips.push(serde_json::json!({"crc":zero_hash::crc_hex(&format!("umber_test_{suffix}")),"flags":0,"offset":samples.len(),"frames":3,"tracks":[bone]}));
            for y in heights {
                samples.extend([0f32, 0., 0., 1., 0., y, 0.]);
            }
        }
        let meta = json::parse(
            &serde_json::json!({"fps":2,"banks":{"umber":{"clips":clips}}}).to_string(),
        )
        .unwrap();
        let bytes: Vec<_> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        let archive = Archive::new(&meta, &bytes).unwrap();
        let def = json::parse(r#"{"weapons":[{"AnimationBank":"test"}]}"#).unwrap();
        let animation = Character::bind(
            "figure",
            &def,
            &["umber".into()],
            &build_actor(&mesh, &[]).unwrap(),
            &archive,
            &archive,
        )
        .unwrap();
        let tables = Tables::new(
            &animation,
            &def,
            &bfe_content::Value::Null,
            &bfe_content::Value::Null,
        )
        .unwrap();
        Class {
            name: String::new(),
            side: String::new(),
            model: String::new(),
            animation,
            tables,
        }
    }
    #[test]
    fn a_published_melee_pose_drives_a_bot_through_the_players_combo_fields() {
        use bfe_sim::combat::combo::LoopMode;
        use bfe_sim::wire::melee::{ID_BASE, SCHEMA};
        let swing = MeleePose { state: 5, origin: 82., loop_mode: LoopMode::None, clip_len: 0.75, blades_off: 0b100, reaction: None, swing_turn: -0.5 };
        let cast = CastPose { weapon: 0x0c0f_fee0, fire_anim: 1, origin: 70., holding: true };
        let display = Display { melee: Some(swing), cast: Some(cast), choked: None };
        let mut bytes = Vec::new();
        display.encode_into(&mut bytes);
        let mut field = Field::default();
        field.row[r::KIND] = f64::from(k::SUMMARY);
        field.row[r::SCHEMA] = SCHEMA;
        field.row[r::ID] = ID_BASE + 4.;
        field.row[r::GENERATION] = 3.;
        field.bytes[..bytes.len()].copy_from_slice(&bytes);
        field.length = bytes.len();
        assert_eq!(hero_display(&field), Some(((4, 3), display)));
        assert_eq!(cast_action(&cast), Action::FireSecondary2);
        assert!((cast.elapsed(100.) - 0.5).abs() < 1e-12);
        field.row[r::SCHEMA] = SCHEMA + 1.;
        assert_eq!(hero_display(&field), None);
        // Eighteen ticks into the state: 0.3 s of its clip.
        let mut s = State { tick: 100, action: Action::Fire, elapsed: 2., ..Default::default() };
        published_combo(&mut s, &swing);
        assert_eq!((s.combo, s.elapsed, s.blades_off, s.contact_clip, s.swing_turn), (Some(5), 2., 0b100, None, -0.5));
        assert!((s.anim_time - 0.3).abs() < 1e-12);
        let guard = MeleePose { reaction: Some((11, 97.5, 0.5)), ..swing };
        published_combo(&mut s, &guard);
        assert_eq!((s.combo, s.contact_clip), (Some(5), Some(11)));
        assert!((s.anim_time - 2.5 / 60.).abs() < 1e-12);
    }

    #[test]
    fn a_combo_state_leaves_the_hit_flinch_on_its_own_clock() {
        use bfe_sim::combat::combo::LoopMode;
        // A bot idles in its combo machine for ten seconds, then a shot lands.
        let mut s = State { tick: 1000, action: Action::Hit, elapsed: 1. / 60., ..Default::default() };
        let idle = MeleePose { state: 1, origin: 400., loop_mode: LoopMode::Wrap, clip_len: 2., blades_off: 0, reaction: None, swing_turn: 0. };
        published_combo(&mut s, &idle);
        assert_eq!((s.action, s.elapsed), (Action::Hit, 1. / 60.));
    }

    #[test]
    fn a_dying_saber_soldier_plays_its_death_clip_from_the_start() {
        use bfe_sim::combat::combo::LoopMode;
        // Idle in its combo machine for ten seconds, dead for a tenth of a second.
        let idle = MeleePose { state: 1, origin: 400., loop_mode: LoopMode::Wrap, clip_len: 2., blades_off: 0, reaction: None, swing_turn: 0. };
        let mut s = State { tick: 1000, action: Action::Death, elapsed: 0.1, ..Default::default() };
        published_combo(&mut s, &idle);
        assert_eq!(s.elapsed, 0.1, "the death clip's clock is the time since death, never the combo state's age");
    }

    #[test]
    fn unchanged_soldier_advances_on_empty_snapshots() {
        use bfe_content_native::animation2::player::{Arena, Player};
        let class = generated_class();
        let mut row = [0.; r::STRIDE];
        row[r::HEALTH] = 1.;
        let mut clocks = Clocks::default();
        let mut arena = Arena::new(1, 1).unwrap();
        let mut pose = arena.slot(0, 1);
        let mut player = Player::default();
        let mut clock = crate::online::clock::Clock::online();
        let mut previous = None;
        let (mut moving, mut resting) = (0, 0);
        for frame in 1..=144 * 13 {
            let now = f64::from(frame) / 144.;
            let tick = (frame * 60 / 144) as u32;
            if tick.is_multiple_of(3) {
                if tick == 600 && row[r::HEALTH] > 0. {
                    row[r::HEALTH] = 0.;
                    row[r::TICK] = 600.;
                }
                if let Some(t) = clocks.sample_tick(&row, row[r::TICK] as u32, tick) {
                    let s = clocks.state(&row, t, 0, &class);
                    player.publish(&class, s, &mut pose);
                    clock.arrive(t.into(), now, 3.).unwrap();
                }
            }
            let shown = clock.advance(1. / 144.).unwrap();
            player.place_at(&class, shown, false, &mut pose);
            let y = pose.world[0].elements[13];
            if shown > 10. && shown < 590. {
                assert!(previous.is_none_or(|p: f64| (p - y).abs() > 1e-10));
                moving += 1;
            }
            if shown > 665. {
                assert!((y + 2.).abs() < 1e-10);
                resting += 1;
            }
            previous = Some(y);
        }
        assert!(moving > 1300 && resting > 200);
        println!(
            "{moving} moving idle frames, death completed, {resting} frames at the final pose"
        );
    }

    #[test]
    fn same_tick_seat_life_and_discontinuity_publications_are_applied_once() {
        let mut clocks = Clocks {
            last: Some(36),
            position: Some([0.; 3]),
            ..Clocks::default()
        };
        let mut row = [0.; r::STRIDE];
        row[r::HEALTH] = 1.;
        row[r::TICK] = 36.;
        assert_eq!(clocks.sample_tick(&row, 36, 33), None);
        row[r::POSITION] = 2.;
        row[r::DISC] = 1.;
        assert_eq!(clocks.sample_tick(&row, 36, 33), Some(36));
        clocks.position = Some([2., 0., 0.]);
        assert_eq!(clocks.sample_tick(&row, 36, 33), None);
        row[r::SEAT_PRESENT] = 1.;
        row[r::SEAT_VEHICLE_ID] = 8.;
        assert_eq!(clocks.sample_tick(&row, 36, 36), Some(36));
        clocks.seat = seat(&row);
        assert_eq!(clocks.sample_tick(&row, 36, 36), None);
        clocks.death = Some(33);
        assert_eq!(clocks.sample_tick(&row, 36, 36), Some(36));
    }

    #[test]
    fn a_retained_position_mark_cuts_once_after_a_teleport_or_a_seat_exit() {
        use bfe_content_native::animation2::player::{Arena, Player};
        use bfe_presentation::network_layout::net_header as h;
        use bfe_sim::{
            gen::{WireOwn, WireRecord, WireRecordKind, WireRecordState},
            wire::{codec, records},
        };
        let class = generated_class();
        for mounted in [false, true] {
            let mut receiver = crate::online::receiver::Receiver::new(serde_json::Value::Null);
            receiver.welcomed = true;
            receiver.registry = true;
            let mut slots = records::SnapshotSlotTable::default();
            let mut previous: Option<bfe_sim::gen::WireSnapshot> = None;
            let mut clocks = Clocks::default();
            let mut player = Player::default();
            let mut arena = Arena::new(1, 1).unwrap();
            let mut pose = arena.slot(0, 1);
            let mut cuts = Vec::new();
            let mut retained = 0;
            for tick in 1..=120u32 {
                let position = [
                    if tick < 30 {
                        0.
                    } else if mounted {
                        1.
                    } else {
                        40.
                    },
                    0.,
                    0.,
                ];
                let mut rows = vec![WireRecord {
                    id: 7.,
                    generation: 2.,
                    kind: WireRecordKind::Soldier,
                    position: Some(position),
                    health: Some(1.),
                    yaw: Some(f64::from(tick) / 60.),
                    pitch: Some(0.),
                    state: WireRecordState::F64(0.),
                    ..Default::default()
                }];
                if mounted {
                    rows.push(WireRecord {
                        id: 9.,
                        generation: 3.,
                        kind: WireRecordKind::Vehicle,
                        state: WireRecordState::F64(0.),
                        quaternion: Some([0., 0., 0., 1.]),
                        health: Some(1.),
                        position: Some([0.; 3]),
                        ..Default::default()
                    });
                    if tick < 30 {
                        rows.push(WireRecord {
                            id: 7.,
                            kind: WireRecordKind::Seat,
                            vehicle_id: Some(9.),
                            seat_index: Some(0.),
                            ..Default::default()
                        });
                    }
                }
                let t = f64::from(tick);
                let snapshot = records::snapshot_from_records(
                    t,
                    1.,
                    &rows,
                    vec![],
                    &mut slots,
                    &bfe_sim::wire::PositionBounds::LEGACY,
                    previous.as_ref(),
                    &bfe_sim::store::Strings::new(),
                )
                .unwrap();
                for packet in codec::encode_entities(&snapshot, previous.as_ref(), 1., bfe_sim::wire::PositionBounds::LEGACY.width()).unwrap() {
                    receiver.message(false, &packet, t * 1000. / 60.);
                }
                let own = codec::own_datagram(&WireOwn {
                    tick: t,
                    epoch: snapshot.epoch,
                    interval: 1.,
                    baseline_tick: previous.as_ref().map_or(0., |s| s.tick),
                    own_parts: 1.,
                    ..Default::default()
                })
                .unwrap();
                receiver.message(false, &own, t * 1000. / 60.);
                assert_eq!(receiver.network.borrow().header[h::ACK_TICK], t);
                let row = receiver.record(k::SOLDIER, 7., 2.).unwrap();
                assert_eq!(seat(row).is_some(), mounted && tick < 30);
                retained += usize::from(tick > 30 && row[r::DISC] != 0.);
                let at = clocks.sample_tick(row, row[r::TICK] as u32, tick).unwrap();
                let state = clocks.state(row, at, 0, &class);
                assert_eq!(state.discontinuity, tick == 30);
                if player.publish(&class, state, &mut pose) {
                    cuts.push(tick);
                    assert_eq!(player.state_at(t - 0.5).position, state.position);
                }
                previous = Some(snapshot);
            }
            assert!(retained > 80);
            assert_eq!(cuts, [30]);
            println!("mounted={mounted} cuts={cuts:?}, later flagged rows={retained}");
        }
    }

    #[test]
    fn damage_uses_the_published_source_point() {
        let receiver = crate::online::receiver::Receiver::new(serde_json::Value::Null);
        let mut field = Field::default();
        field.row[r::KIND] = f64::from(k::COMBAT);
        field.row[r::SCHEMA] = 3.;
        field.bytes[4..8].copy_from_slice(&17u32.to_le_bytes());
        field.bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
        field.bytes[52..54].copy_from_slice(&1u16.to_le_bytes());
        for (at, p) in [(20, [1f64, 2., 3.]), (54, [1., 2., -7.])] {
            for (i, v) in p.into_iter().enumerate() {
                field.bytes[at + i * 8..at + i * 8 + 8].copy_from_slice(&v.to_le_bytes());
            }
        }
        field.length = 78;
        assert_eq!(
            impact(&field, &receiver),
            Some(((17, 2), Some([0., 0., -10.])))
        );
        field.bytes[52..54].fill(0);
        field.length = 54;
        assert_eq!(impact(&field, &receiver), Some(((17, 2), None)));
    }

    #[test]
    fn a_soldier_stays_alert_for_two_and_a_half_seconds_after_its_last_shot_or_weapon_change() {
        let class = generated_class();
        let mut clocks = Clocks::default();
        let mut row = [0.; r::STRIDE];
        row[r::HEALTH] = 1.;
        // First sight of a soldier at rest: not alert.
        assert!(!clocks.state(&row, 10, 0, &class).alert);
        // A shot makes it alert for 150 ticks, counted from the shot.
        clocks.event(Action::Fire, 20);
        assert!(clocks.state(&row, 20, 0, &class).alert);
        assert!(clocks.state(&row, 169, 0, &class).alert);
        assert!(!clocks.state(&row, 170, 0, &class).alert);
        // A later shot extends it; an older event does not shorten it.
        clocks.event(Action::Fire, 200);
        clocks.event(Action::Fire, 150);
        assert!(clocks.state(&row, 349, 0, &class).alert);
        assert!(!clocks.state(&row, 350, 0, &class).alert);
        // Taking up another weapon starts it again.
        assert!(clocks.state(&row, 360, 1, &class).alert);
        assert!(!clocks.state(&row, 510, 1, &class).alert);
    }

    #[test]
    fn offhand_action_and_shot_events_preserve_weapon_and_channel() {
        let make = |body: &str, tick: f64| {
            let mut field = Field::default();
            field.row[r::KIND] = f64::from(k::COMBAT);
            field.row[r::SCHEMA] = 8.;
            field.row[r::TICK] = tick;
            let bytes = body.as_bytes();
            field.bytes[..bytes.len()].copy_from_slice(bytes);
            field.length = bytes.len();
            field
        };
        let offhand = action_event(&make(
            r#"{"kind":"weaponAction","ownerId":4,"ownerGeneration":1,"action":"shoot","bank":"","channel":1,"weaponId":2}"#,
            12.,
        ))
        .unwrap();
        assert_eq!(offhand.owner, (4, 1));
        assert_eq!(offhand.tick, 12);
        assert_eq!(offhand.action, Action::FireSecondary);
        assert_eq!(offhand.channel, 1);
        assert_eq!(offhand.weapon_id, 2);

        let primary = action_event(&make(
            r#"{"kind":"weaponAction","ownerId":4,"ownerGeneration":1,"action":"shoot","bank":"rifle"}"#,
            13.,
        ))
        .unwrap();
        assert_eq!(primary.action, Action::Fire);

        let shot = action_event(&make(
            r#"{"kind":"shot","ownerId":5,"ownerGeneration":2,"weaponId":3,"channel":1,"mountId":0}"#,
            14.,
        ))
        .unwrap();
        assert_eq!(shot.owner, (5, 2));
        assert_eq!(shot.action, Action::Fire);
        assert_eq!(shot.weapon_id, 3);
        assert_eq!(shot.channel, 1);
    }

    #[test]
    fn landing_uses_airtime_threshold_and_cancels_on_movement() {
        let class = generated_class();
        let mut clocks = Clocks::default();
        let mut row = [0.; r::STRIDE];
        row[r::HEALTH] = 1.;
        // Short 1.2s jump landing fast (-12 m/s): must stay soft and cancel once moving.
        row[r::STATE] = 1.;
        row[r::VELOCITY + 1] = 11.;
        let _ = clocks.state(&row, 10, 0, &class);
        row[r::VELOCITY + 1] = -12.;
        let _ = clocks.state(&row, 82, 0, &class);
        row[r::STATE] = 0.;
        row[r::VELOCITY] = 4.;
        row[r::VELOCITY + 1] = 0.;
        let land0 = clocks.state(&row, 83, 0, &class);
        assert_eq!(land0.air, Air::Land);
        assert!(!land0.hard_landing);
        let land1 = clocks.state(&row, 84, 0, &class);
        assert_eq!(land1.air, Air::Ground);

        // Long 1.7s fall triggers hard landing, holds through 0.5s even while moving, then cancels.
        row[r::STATE] = 2.;
        row[r::VELOCITY] = 0.;
        let _ = clocks.state(&row, 100, 0, &class);
        row[r::STATE] = 0.;
        row[r::VELOCITY] = 4.;
        let hard0 = clocks.state(&row, 202, 0, &class);
        assert_eq!(hard0.air, Air::Land);
        assert!(hard0.hard_landing);
        let hard_mid = clocks.state(&row, 220, 0, &class);
        assert_eq!(hard_mid.air, Air::Land);
        let hard_end = clocks.state(&row, 233, 0, &class);
        assert_eq!(hard_end.air, Air::Ground);

        // Long 2.5s jet flight selects Air::Jet and never hard-lands on touchdown.
        row[r::STATE] = 4.;
        let jet = clocks.state(&row, 300, 0, &class);
        assert_eq!(jet.air, Air::Jet);
        row[r::STATE] = 0.;
        row[r::VELOCITY] = 0.;
        let jet_land = clocks.state(&row, 450, 0, &class);
        assert_eq!(jet_land.air, Air::Land);
        assert!(!jet_land.hard_landing);
    }

    #[test]
    fn wire_phase_three_maps_to_air_thrown() {
        let class = generated_class();
        let mut clocks = Clocks::default();
        let mut row = [0.; r::STRIDE];
        row[r::HEALTH] = 1.;
        row[r::STATE] = 3.;
        row[r::VELOCITY + 2] = -8.;
        let s = clocks.state(&row, 10, 0, &class);
        assert_eq!(s.air, Air::Thrown);
    }
}
