//! Own look preview, Gunner.preview, VehicleDriver.preview and correction
//! smoothing join the existing page-network presentation frame. Events are
//! inert generated records; no input or resource callback runs in this phase.
use crate::network_layout::{
    net_limits as lim, net_outbound_command as oc, net_outbound_look as ol, net_preview_air as air, net_preview_command as c,
    net_preview_frame as f, net_preview_look as l, net_preview_output as o, net_preview_posture as posture, net_smooth_command as sc,
    net_smooth_input as si, net_smooth_kind as kind, net_smooth_output as so,
};
use bfe_pose::smoothing::SmootherState;
use v8math::{atan2, cos, max, min, sin};
// Servo previews use the fixed simulation tick even on high refresh displays.
// Fractional display time only unwinds the completed final servo step; using
// frame dt here changes acceleration and makes sensitivity depend on refresh.
// Wrap and clamp retain the original Math operation order and signed zeros.
const TICK: f64 = 1. / 60.;
const TAU: f64 = 2. * std::f64::consts::PI;
fn wrap(a: f64) -> f64 {
    atan2(sin(a), cos(a))
}
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    max(lo, min(hi, x))
}
fn arc(a: f64, wrap_yaw: bool, lo: f64, hi: f64) -> f64 {
    let mut a = wrap(a);
    if wrap_yaw && a < 0. {
        a += TAU;
    }
    clamp(a, lo, hi)
}
#[derive(Default)]
// Smoothing offsets never enter collided coordinates. Each stream keeps its
// own counters and snap threshold while sharing the same frame reservation.
// A queue preserves reset/note/decay order across bunched correction packets.
struct Smooth {
    state: SmootherState,
    snaps: f64,
    last: f64,
}
pub struct Phase {
    pub frame: Vec<f64>,
    pub commands: Vec<f64>,
    pub looks: Vec<f64>,
    pub output: Vec<f64>,
    pub smooth_input: Vec<f64>,
    pub smooth_commands: Vec<f64>,
    pub smooth_output: Vec<f64>,
    smooth: [Smooth; 2],
    turret: Option<[f64; 2]>,
}
impl Default for Phase {
    fn default() -> Self {
        Self::new()
    }
}
impl Phase {
    // Every exchange has a fixed reservation. Mouse bursts keep their bounded
    // prefix; paused correction queues never allocate while being drained.
    pub fn new() -> Self {
        Self {
            frame: vec![0.; f::STRIDE],
            commands: vec![0.; lim::PREVIEW_COMMANDS * c::STRIDE],
            looks: vec![0.; lim::PREVIEW_LOOKS * l::STRIDE],
            output: vec![0.; o::STRIDE],
            smooth_input: vec![0.; 2 * si::STRIDE],
            smooth_commands: vec![0.; 2 * lim::SMOOTH_COMMANDS * sc::STRIDE],
            smooth_output: vec![0.; 2 * so::STRIDE],
            smooth: Default::default(),
            turret: None,
        }
    }
    // A pooled connection keeps its reservations but cannot retain an old
    // battle's visual offset, lease identity or mouse preview. Reset occurs
    // at connection lifetime boundaries, outside the normal frame loop.
    pub fn reset(&mut self) {
        self.frame.fill(0.);
        self.output.fill(0.);
        self.smooth_input.fill(0.);
        self.smooth_output.fill(0.);
        self.smooth = Default::default();
        self.turret = None;
    }
    // CorrectionSmoother used observable reset methods between operations.
    // Here reset is numerical state, so apply it immediately after its note
    // or decay while retaining accumulated count, worst distance and snaps.
    // A newly bound resource seeds its public metrics exactly once.
    fn smooth(&mut self) {
        for n in 0..2 {
            let input = &mut self.smooth_input[n * si::STRIDE..][..si::STRIDE];
            let s = &mut self.smooth[n];
            // Migration imports facts once; subsequent frames carry events.
            // Seeding again would hide prior queued corrections or reset the
            // exponential tail every time a packet reused the same resource.
            if input[si::SEED] != 0. {
                s.state.offset = [input[si::X], input[si::Y], input[si::Z]];
                s.state.count = input[si::COUNT];
                s.state.worst = input[si::WORST];
                s.snaps = input[si::SNAPS];
                input[si::SEED] = 0.;
            }
            // Commands use generated fixed strides and an explicit count.
            // Clamp on the Rust side as well as in the host; an oversized or
            // resumed producer cannot index past either stream's reservation.
            let count = (input[si::COMMANDS] as usize).min(lim::SMOOTH_COMMANDS);
            for at in 0..count {
                let at = (n * lim::SMOOTH_COMMANDS + at) * sc::STRIDE;
                let row = &self.smooth_commands[at..at + sc::STRIDE];
                match row[sc::KIND] as u32 {
                    kind::RESET => s.state.reset(),
                    kind::DECAY => {
                        if s.state.decay(input[si::TAU], row[sc::VALUE]) {
                            s.state.reset();
                        }
                    }
                    kind::NOTE => {
                        let note = s.state.note(
                            [row[sc::OLD], row[sc::OLD + 1], row[sc::OLD + 2]],
                            [row[sc::NEW], row[sc::NEW + 1], row[sc::NEW + 2]],
                            input[si::SNAP],
                        );
                        s.last = note.size;
                        if note.reset {
                            s.state.reset();
                            s.snaps += 1.;
                        }
                    }
                    _ => {}
                }
            }
            // Consuming a queue is independent of display enablement. Paused
            // or hidden bodies can receive corrections and must not replay the
            // same decay or note again when their next frame becomes visible.
            input[si::COMMANDS] = 0.;
            let output = &mut self.smooth_output[n * so::STRIDE..][..so::STRIDE];
            output[so::X] = s.state.offset[0];
            output[so::Y] = s.state.offset[1];
            output[so::Z] = s.state.offset[2];
            output[so::LENGTH] = s.state.length();
            output[so::COUNT] = s.state.count;
            output[so::SNAPS] = s.snaps;
            output[so::WORST] = s.state.worst;
            output[so::LAST_SIZE] = s.last;
        }
    }
    // A command can have no look and still advances a turret's servo filter.
    // The optional loose-look group advances it once more, even when empty.
    fn steer(a: &[f64], state: &mut [f64; 4], looks: &[f64], scale: f64, invert: bool) {
        let mut x = 0.;
        let mut y = 0.;
        for look in looks.chunks_exact(l::STRIDE) {
            x += look[l::X] * scale;
            y += (if invert { look[l::Y] } else { -look[l::Y] }) * scale;
        }
        // Apply displacement sums before conversion to a bounded turn rate.
        // Summing per-event rates, or clamping before sensitivity, changes
        // reversals and the filtered velocity at authored turret arc limits.
        let turn = -clamp(x / (2048. * TICK), -16., 16.);
        let pitch = clamp(y / (2048. * TICK), -16., 16.);
        let k = min(1., a[f::FILTER] * TICK);
        let rate = a[f::TURN_RATE];
        state[2] += (turn * rate - state[2]) * k;
        state[3] += (pitch * rate - state[3]) * k;
        let yaw = state[0] + state[2] * TICK;
        state[0] = if a[f::HAS_YAW_ARC] != 0. { arc(yaw, a[f::WRAP_YAW] != 0., a[f::ARC_LOW], a[f::ARC_HIGH]) } else { wrap(yaw) };
        if a[f::HAS_YAW_ARC] != 0. && wrap(yaw - state[0]).abs() > 1e-9 {
            state[2] = 0.;
        }
        state[1] = clamp(state[1] + state[3] * TICK, a[f::TURRET_PITCH_LOW], a[f::TURRET_PITCH_HIGH]);
    }
    // Soldier.lookRaw applies posture turn and pitch clamps per sample. Mouse
    // displacement divides by zoom before sensitivity, preserving IEEE order.
    fn own_look(a: &[f64], yaw: &mut f64, pitch: &mut f64, looks: &[f64]) {
        if a[f::DEAD] != 0. || a[f::WALKER] != 0. || a[f::LOCK] > 0. {
            return;
        }
        // Jet uses ordinary standing turn speed; other airborne modes halve
        // it. A grounded dive has its own factor and prone has a narrow pitch
        // interval. These distinctions are independent of the displayed clip.
        let mode = if a[f::AIR] == air::JET as f64 {
            1.
        } else if a[f::AIR] != air::NONE as f64 {
            0.5
        } else if a[f::DIVE] != 0. {
            0.6
        } else if a[f::POSTURE] == posture::PRONE as f64 {
            0.5
        } else {
            1.
        };
        let (lo, hi) = if a[f::POSTURE] == posture::PRONE as f64 && a[f::AIR] == air::NONE as f64 {
            (-0.4, 0.4)
        } else {
            (a[f::PITCH_LOW], a[f::PITCH_HIGH])
        };
        let rate = a[f::MAX_TURN] / 2048.;
        let scale = a[f::SCALE];
        let zoom = a[f::ZOOM];
        for look in looks.chunks_exact(l::STRIDE) {
            *yaw -= (look[l::X] / zoom) * scale * rate * mode;
            let dy = look[l::Y] / zoom;
            *pitch = clamp(*pitch + (if a[f::INVERT] != 0. { dy } else { -dy }) * scale * rate, lo, hi);
        }
    }
    // VehicleDriver.preview clamps each individual event. A reversal must
    // start from the previously clamped pitch and hull-relative arc, rather
    // than from a final aggregate displacement. Authority angles stay intact.
    fn vehicle_look(a: &[f64], state: &mut [f64; 2], looks: &[f64], scale: f64, invert: bool) {
        let [yaw, pitch] = state;
        for look in looks.chunks_exact(l::STRIDE) {
            *yaw -= look[l::X] * scale / 2048.;
            *pitch = clamp(
                *pitch + (if invert { look[l::Y] } else { -look[l::Y] }) * scale / 2048.,
                a[f::VEHICLE_PITCH_LOW],
                a[f::VEHICLE_PITCH_HIGH],
            );
            if a[f::VEHICLE_HAS_YAW_ARC] != 0. {
                *yaw = a[f::HULL_YAW]
                    + arc(*yaw - a[f::HULL_YAW], a[f::VEHICLE_WRAP_YAW] != 0., a[f::VEHICLE_ARC_LOW], a[f::VEHICLE_ARC_HIGH]);
            }
        }
    }
    // commandBodyAim ran inside the fire transport callback. Its counts already
    // cross with OutboundCommand, so derive the body angles there without a
    // second call or a temporary Soldier facade. Encoding consumes this result.
    pub fn command_aim(&self, words: &[f64], numbers: &mut [f64]) {
        use crate::network_layout::net_outbound as out;
        // This small stack copy permits the command's own sensitivity and
        // dead-body semantics without mutating the display frame's facts.
        // Its pointer samples are already in the transport exchange and are
        // consumed individually, preserving pitch clamps across reversals.
        let mut facts = [0.; f::STRIDE];
        facts.copy_from_slice(&self.frame);
        numbers[out::NEEDS_REACH]=0.;
        if facts[f::ENABLED] == 0. {
            return;
        }
        let mut yaw = facts[f::OWN_YAW];
        let mut pitch = facts[f::OWN_PITCH];
        facts[f::DEAD] = 0.;
        facts[f::SCALE] = facts[f::COMMAND_SCALE];
        facts[f::INVERT] = facts[f::COMMAND_INVERT];
        // The variable held-name prefix precedes the raw look list. Both
        // prefix layouts are generated; the transport reader has validated
        // the list, and this consumer independently caps its available span.
        let held = words.get(oc::HELD_COUNT).copied().unwrap_or(0.) as usize;
        let start = oc::STRIDE.saturating_add(held).saturating_add(ol::STRIDE).min(words.len());
        let count = words.get(start.saturating_sub(ol::STRIDE) + ol::COUNT).copied().unwrap_or(0.) as usize;
        let count = count.min((words.len() - start) / l::STRIDE);
        Self::own_look(&facts, &mut yaw, &mut pitch, &words[start..start + count * l::STRIDE]);
        // Mounted fire reads the previously shown reticle, using the latest
        // lease's canonical seat when no preview belongs to that identity.
        // Keep its direction-to-angle round trip, including signed zeros.
        if facts[f::VEHICLE_ACTIVE] != 0. || facts[f::TURRET_ACTIVE] != 0. {
            if facts[f::VEHICLE_ACTIVE] != 0. {
                yaw = if facts[f::INPUT_VEHICLE_PREVIEW] != 0. { facts[f::INPUT_VEHICLE_YAW] } else { facts[f::SEAT_YAW] };
                pitch = if facts[f::INPUT_VEHICLE_PREVIEW] != 0. { facts[f::INPUT_VEHICLE_PITCH] } else { facts[f::SEAT_PITCH] };
            } else {
                yaw = facts[f::FACING] + facts[f::TURRET_YAW] + facts[f::INPUT_TURRET_YAW];
                pitch = facts[f::TURRET_PITCH] + facts[f::INPUT_TURRET_PITCH];
            }
            yaw = atan2(sin(yaw) * cos(pitch), cos(yaw) * cos(pitch));
            pitch = v8math::asin(clamp(sin(pitch), -1., 1.));
        }
        if facts[f::VEHICLE_ACTIVE]==0.&&facts[f::TURRET_ACTIVE]==0.&&numbers[out::FIRE_COUNT]!=0.{
            use crate::{aim::{Basis,View},cameras::math::V8};use three::Vector3 as V;
            let position=V::new(facts[f::POSITION],facts[f::POSITION+1],facts[f::POSITION+2]);
            let posture=facts[f::POSTURE]as usize;let air=facts[f::AIR]!=air::NONE as f64;
            let source=Basis::new::<V8>(position,facts[f::OWN_YAW],facts[f::OWN_PITCH],posture,air);
            let aim=View{position,feet:V::new(facts[f::FEET],facts[f::FEET+1],facts[f::FEET+2]),yaw,pitch,posture,air,
                walker:facts[f::WALKER]!=0.,rolled:facts[f::ROLLED]!=0.,eye_height:facts[f::EYE_HEIGHT],zoom:facts[f::ZOOM],
                camera:(facts[f::HAS_CAMERA]!=0.).then(||V::new(facts[f::CAMERA],facts[f::CAMERA+1],facts[f::CAMERA+2])),
                uses_eye:facts[f::USES_EYE]!=0.,track_offsets:facts[f::TRACK_OFFSETS..f::TRACK_OFFSETS+3].try_into().unwrap()}.sample::<V8>();
            numbers[out::SOURCE_BASIS..out::SOURCE_BASIS+15].copy_from_slice(&source.words());
            numbers[out::AIM_ORIGIN..out::AIM_ORIGIN+3].copy_from_slice(&[aim.basis.origin.x,aim.basis.origin.y,aim.basis.origin.z]);
            numbers[out::AIM_DIRECTION..out::AIM_DIRECTION+3].copy_from_slice(&[aim.basis.direction.x,aim.basis.direction.y,aim.basis.direction.z]);
            numbers[out::AIM_FROM..out::AIM_FROM+3].copy_from_slice(&[aim.from.x,aim.from.y,aim.from.z]);
            numbers[out::NEEDS_REACH]=aim.needs_reach as u8 as f64;
        }
        numbers[out::BODY_YAW] = yaw;
        numbers[out::BODY_PITCH] = pitch;
    }
    // The host answers every command ray before one convergence resume.
    // This is the same command basis used by page_body's replay-shot batch.
    pub fn resolve_command(row:&mut[f64]){
        use crate::{aim::{Basis,CommandAim},cameras::math::V8};use crate::network_layout::net_outbound as o;use three::Vector3 as V;
        if row[o::NEEDS_REACH]==0.{return;}
        let mut aim=CommandAim{basis:Basis{origin:V::new(row[o::AIM_ORIGIN],row[o::AIM_ORIGIN+1],row[o::AIM_ORIGIN+2]),
            direction:V::new(row[o::AIM_DIRECTION],row[o::AIM_DIRECTION+1],row[o::AIM_DIRECTION+2]),..Default::default()},
            from:V::new(row[o::AIM_FROM],row[o::AIM_FROM+1],row[o::AIM_FROM+2]),needs_reach:true,..Default::default()};
        aim.resolve::<V8>(row[o::REACH]);row[o::BODY_YAW]=aim.yaw;row[o::BODY_PITCH]=aim.pitch;
    }
    // Scratch angles begin at the current authoritative pose each frame.
    // Acknowledgements and lease changes therefore cannot double-apply look
    // or leave a previous seat's preview on its replacement. No frame result
    // is a mutable continuation of a soldier or vehicle controller.
    pub fn present(&mut self) {
        self.smooth();
        let a = &self.frame;
        if a[f::ENABLED] == 0. {
            return;
        }
        let mut yaw = a[f::OWN_YAW];
        let mut pitch = a[f::OWN_PITCH];
        let mut turret = [a[f::TURRET_YAW], a[f::TURRET_PITCH], a[f::YAW_VEL], a[f::PITCH_VEL]];
        let mut vehicle = [
            if a[f::HAS_LOOK_YAW] != 0. { a[f::LOOK_YAW] } else { a[f::SEAT_YAW] },
            if a[f::HAS_LOOK_PITCH] != 0. { a[f::LOOK_PITCH] } else { a[f::SEAT_PITCH] },
        ];
        // Sent-only own look has already been applied by ClientPrediction.
        // Unsent samples still belong to the view; turret and seat previews
        // replay every command because their canonical baseline is separate.
        // Frozen frames preserve the prior turret display, whereas normal
        // frames rebuild all scratch angles. Lockstep has no outstanding
        // commands; it only interpolates the current canonical servo state.
        let count = if a[f::FROZEN] != 0. || a[f::LOCKSTEP] != 0. { 0 } else { (a[f::COMMANDS] as usize).min(lim::PREVIEW_COMMANDS) };
        for row in self.commands[..count * c::STRIDE].chunks_exact(c::STRIDE) {
            let start = (row[c::LOOK_AT] as usize).min(lim::PREVIEW_LOOKS);
            let n = (row[c::LOOK_COUNT] as usize).min(lim::PREVIEW_LOOKS - start);
            let looks = &self.looks[start * l::STRIDE..(start + n) * l::STRIDE];
            let scale = if row[c::HAS_SCALE] != 0. { row[c::SCALE] } else { a[f::SCALE] };
            let invert = if row[c::HAS_INVERT] != 0. { row[c::INVERT] != 0. } else { a[f::INVERT] != 0. };
            if a[f::PREDICTED] == 0. || row[c::LOOK_SENT] == 0. {
                Self::own_look(a, &mut yaw, &mut pitch, looks);
            }
            if a[f::TURRET_ACTIVE] != 0. {
                Self::steer(a, &mut turret, looks, scale, invert);
            }
            if a[f::VEHICLE_ACTIVE] != 0. {
                Self::vehicle_look(a, &mut vehicle, looks, scale, invert);
            }
        }
        let count = if a[f::FROZEN] != 0. { 0 } else { (a[f::LOOKS] as usize).min(lim::PREVIEW_LOOKS) };
        let looks = &self.looks[(lim::PREVIEW_LOOKS - count) * l::STRIDE..];
        if a[f::LOCKSTEP] == 0. {
            Self::own_look(a, &mut yaw, &mut pitch, looks);
        }
        if a[f::HAS_LOOSE] != 0. {
            if a[f::TURRET_ACTIVE] != 0. {
                Self::steer(a, &mut turret, looks, a[f::SCALE], a[f::INVERT] != 0.);
            }
            if a[f::VEHICLE_ACTIVE] != 0. {
                Self::vehicle_look(a, &mut vehicle, looks, a[f::SCALE], a[f::INVERT] != 0.);
            }
        }
        // Body aim undoes the unfinished part of its latest simulation turn.
        // HUD heading omits that interpolation, matching the original page.
        // Reticle previews remain separate from authoritative weapon rays.
        let output = &mut self.output;
        output[o::PREVIEW_YAW] = yaw - a[f::OWN_YAW];
        output[o::PREVIEW_PITCH] = pitch - a[f::OWN_PITCH];
        output[o::HUD_YAW] = a[f::OWN_YAW] + output[o::PREVIEW_YAW];
        output[o::RENDERED_YAW] = a[f::OWN_YAW] - a[f::TICK_YAW] * (1. - a[f::ALPHA]) + output[o::PREVIEW_YAW];
        output[o::RENDERED_PITCH] = a[f::OWN_PITCH] + output[o::PREVIEW_PITCH];
        output[o::VEHICLE_VALID] = if a[f::VEHICLE_ACTIVE] != 0. && !(a[f::FLYING] != 0. && a[f::SEAT_INDEX] == 0.) { 1. } else { 0. };
        output[o::VEHICLE_YAW] = vehicle[0];
        output[o::VEHICLE_PITCH] = vehicle[1];
        output[o::TURRET_VALID] = a[f::TURRET_ACTIVE];
        // A frozen display retains the turret preview already shown. Lockstep
        // explicitly samples the servo at its fractional world clock instead.
        let identity = [a[f::TURRET_ID], a[f::TURRET_GENERATION]];
        if a[f::TURRET_ACTIVE] != 0. && (a[f::FROZEN] == 0. || self.turret != Some(identity)) {
            // Pending commands use the input clock's fractional tick. With
            // no outstanding input, interpolation follows the playout clock.
            // Pitch clamps after unwinding velocity, preserving its endpoint.
            let alpha = if a[f::LOCKSTEP] != 0. {
                a[f::ALPHA]
            } else if a[f::COMMANDS] > 0. {
                a[f::INPUT_ALPHA]
            } else {
                a[f::PRESENT_ALPHA]
            };
            let back = TICK * (1. - clamp(alpha, 0., 1.));
            let shown = turret[0] - turret[2] * back;
            output[o::TURRET_PREVIEW_YAW] = wrap(
                (if a[f::HAS_YAW_ARC] != 0. { arc(shown, a[f::WRAP_YAW] != 0., a[f::ARC_LOW], a[f::ARC_HIGH]) } else { shown })
                    - a[f::TURRET_YAW],
            );
            output[o::TURRET_PREVIEW_PITCH] =
                clamp(turret[1] - turret[3] * back, a[f::TURRET_PITCH_LOW], a[f::TURRET_PITCH_HIGH]) - a[f::TURRET_PITCH];
            output[o::TURRET_YAW] = turret[0];
            output[o::TURRET_PITCH] = turret[1];
            output[o::TURRET_YAW_VEL] = turret[2];
            output[o::TURRET_PITCH_VEL] = turret[3];
            self.turret = Some(identity);
        }
        // Releasing a turret clears its displayed offsets immediately. The
        // identity guard above also prevents a new lease from inheriting a
        // frozen predecessor's preview while the world is paused.
        if a[f::TURRET_ACTIVE] == 0. {
            output[o::TURRET_PREVIEW_YAW] = 0.;
            output[o::TURRET_PREVIEW_PITCH] = 0.;
            self.turret = None;
        }
        output[o::SHOWN_YAW] = a[f::FACING] + a[f::TURRET_YAW] + output[o::TURRET_PREVIEW_YAW];
        output[o::SHOWN_PITCH] = a[f::TURRET_PITCH] + output[o::TURRET_PREVIEW_PITCH];
    }
}
