//! Cold seatView records become typed resource facts before admission ends.
//! Cameras and network preview retain all numerical behavior. Frame packing
//! never searches authored JSON or resolves a property name for a live seat.
use bfe_presentation::{cameras::vehicle::Seat, layout::camera_frame as f, network_layout::net_preview_frame as p};
use serde_json::Value;
use three::Vector3;
#[derive(Clone, Copy)]
pub struct View {
    camera: Seat,
    pivot: [f64; 3],
    up: [f64; 3],
    pub turret: bool,
    pitch: [f64; 2],
    yaw: Option<[f64; 2]>,
    wrap: bool,
    turn_rate: f64,
    filter: f64,
}
fn vector(v: &Value) -> [f64; 3] {
    std::array::from_fn(|i| v[i].as_f64().unwrap_or(0.))
}
fn v(v: &Value) -> Vector3 {
    let a = vector(v);
    Vector3::new(a[0], a[1], a[2])
}
impl View {
    // Numerical builders already parsed the authored section and its quirks.
    // This conversion changes representation only, including missing vectors
    // and optional arcs; it does not choose another camera or default policy.
    pub fn new(row: &Value) -> Self {
        Self {
            camera: Seat {
                eye: v(&row["eye"]),
                center: v(&row["center"]),
                offset: v(&row["offset"]),
                tilt: row["tilt"].as_f64().unwrap_or(0.),
                first: row["firstFov"].as_f64().unwrap_or(0.),
                third: row["thirdFov"].as_f64().unwrap_or(0.),
                rates: std::array::from_fn(|i| row["moveTension"][i].as_f64().unwrap_or(0.)),
                aim: row["aimTension"].as_f64().unwrap_or(0.),
            },
            pivot: vector(&row["pivot"]),
            up: vector(&row["up"]),
            turret: !row["turret"].is_null(),
            pitch: [row["pitchLimits"][0].as_f64().unwrap_or(-1.5), row["pitchLimits"][1].as_f64().unwrap_or(1.5)],
            yaw: row["yawLimits"]
                .is_array()
                .then(|| [row["yawLimits"][0].as_f64().unwrap_or(0.), row["yawLimits"][1].as_f64().unwrap_or(0.)]),
            wrap: row["wrapYaw"].as_bool().unwrap_or(false),
            turn_rate: row["turnRate"].as_f64().unwrap_or(2.),
            filter: row["filter"].as_f64().unwrap_or(10.),
        }
    }
    // The common camera phase consumes the same generated seat fields for
    // hulls and turrets. Each caller supplies its own published root and aim
    // frame; these immutable lens/offset facts are copied without integration.
    pub fn seat(&self, x: &mut [f64]) {
        for (at, values) in [
            (f::SEAT_EYE, [self.camera.eye.x, self.camera.eye.y, self.camera.eye.z]),
            (f::SEAT_CENTER, [self.camera.center.x, self.camera.center.y, self.camera.center.z]),
            (f::SEAT_OFFSET, [self.camera.offset.x, self.camera.offset.y, self.camera.offset.z]),
            (f::SEAT_PIVOT, self.pivot),
            (f::SEAT_UP, self.up),
        ] {
            x[at..at + 3].copy_from_slice(&values);
        }
        x[f::SEAT_RATES..f::SEAT_RATES + 6].copy_from_slice(&self.camera.rates);
        x[f::SEAT_TILT] = self.camera.tilt;
        x[f::SEAT_FIRST] = self.camera.first;
        x[f::SEAT_THIRD] = self.camera.third;
        x[f::SEAT_AIM] = self.camera.aim;
    }
    // Preview owns cyclic arc clamps and turret servo filtering. The host
    // publishes only class limits, preserving optional full-circle yaw arcs.
    // A passenger's canonical baseline and a turret's six checkpoint scalars
    // are supplied separately by the currently qualified mounted lease.
    pub fn preview(&self, x: &mut [f64], turret: bool) {
        let (has, low, high, wrap, pitch_low, pitch_high) = if turret {
            (p::HAS_YAW_ARC, p::ARC_LOW, p::ARC_HIGH, p::WRAP_YAW, p::TURRET_PITCH_LOW, p::TURRET_PITCH_HIGH)
        } else {
            (
                p::VEHICLE_HAS_YAW_ARC,
                p::VEHICLE_ARC_LOW,
                p::VEHICLE_ARC_HIGH,
                p::VEHICLE_WRAP_YAW,
                p::VEHICLE_PITCH_LOW,
                p::VEHICLE_PITCH_HIGH,
            )
        };
        x[has] = self.yaw.is_some() as u8 as f64;
        let arc = self.yaw.unwrap_or([0.; 2]);
        x[low] = arc[0];
        x[high] = arc[1];
        x[wrap] = self.wrap as u8 as f64;
        x[pitch_low] = self.pitch[0];
        x[pitch_high] = self.pitch[1];
        if turret {
            x[p::TURN_RATE] = self.turn_rate;
            x[p::FILTER] = self.filter;
        }
    }
}
