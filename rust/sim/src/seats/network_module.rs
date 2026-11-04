//! Live seat/input field access. Each transport owns its invocation token;
//! only callback-free argument/result publication touches the module state.
use super::{boarding::{LookAccess, LookField, LookScales, SeatControl}, network::{self, CycleAccess, InputAccess, Input, Value, FlightKeys}};
use crate::{flight::controls::{self, InputReader}, gen::NetworkSeatsOp as Op};

pub trait Transport { fn call(&mut self, op: Op, values: Vec<f64>) -> Result<Vec<f64>, ()>; }
pub struct Remote<T>(pub T);
fn number(v: bool) -> f64 { if v { 1.0 } else { 0.0 } }
fn put(value: Value, words: &mut Vec<f64>) { words.extend([value.kind as f64, value.value, number(value.truth)]); }
fn value(words: &[f64]) -> Value { Value { kind: words[0] as u8, value: words[1], truth: words[2] != 0.0 } }
const NAMES: &[&str] = &["throttle", "brake", "rollRight", "rollLeft", "boost", "rollLock", "takeoff", "trick", "immelmann", "targetLock", "forward", "right", "turn"];
fn name(s: &str) -> f64 { NAMES.iter().position(|n| *n == s).expect("input name") as f64 }

impl<T: Transport> CycleAccess for Remote<T> {
    type Error = ();
    type Lease = u32;
    type Hull = u32;
    fn rider(&mut self, _: u32) -> Result<Option<(u32, u32, f64)>, ()> {
        let v = self.0.call(Op::Rider, vec![])?;
        Ok(if v[0] != 0.0 { Some((v[1] as u32, v[2] as u32, v[3])) } else { None })
    }
    fn seat_count(&mut self, hull: u32) -> Result<usize, ()> { Ok(self.0.call(Op::SeatCount, vec![hull as f64])?[0] as usize) }
    fn direction(&mut self, _: f64) -> Result<f64, ()> { Ok(self.0.call(Op::Direction, vec![])?[0]) }
    fn candidate(&mut self, hull: u32, lease: Option<u32>, to: f64) -> Result<(bool, Option<u32>), ()> {
        let v = self.0.call(Op::Candidate, vec![hull as f64, lease.map_or(-1.0, |r| r as f64), to])?;
        Ok((v[0] != 0.0, if v[1] < 0.0 { None } else { Some(v[1] as u32) }))
    }
    fn copy_aim(&mut self, hull: u32, from: f64, to: f64) -> Result<(), ()> { self.0.call(Op::CopyAim, vec![hull as f64, from, to])?; Ok(()) }
    fn move_lease(&mut self, lease: u32, to: f64) -> Result<(), ()> { self.0.call(Op::Move, vec![lease as f64, to])?; Ok(()) }
    fn aim(&mut self, hull: u32, to: f64) -> Result<(), ()> { self.0.call(Op::Aim, vec![hull as f64, to])?; Ok(()) }
    fn clear_sticks(&mut self, hull: u32) -> Result<(), ()> { self.0.call(Op::ClearSticks, vec![hull as f64])?; Ok(()) }
}
impl<T: Transport> LookAccess for Remote<T> {
    type Error = ();
    fn control(&mut self) -> Result<Option<SeatControl>, ()> {
        Ok(match self.0.call(Op::Control, vec![])?[0] { 1.0 => Some(SeatControl::Flight), 2.0 => Some(SeatControl::Drive), 3.0 => Some(SeatControl::Turret), _ => None })
    }
    fn look_read(&mut self, field: LookField) -> Result<f64, ()> { Ok(self.0.call(Op::LookRead, vec![field as u8 as f64])?[0]) }
    fn look_write(&mut self, field: LookField, v: f64) -> Result<(), ()> { self.0.call(Op::LookWrite, vec![field as u8 as f64, v])?; Ok(()) }
    fn clamp(&mut self) -> Result<(), ()> { self.0.call(Op::Clamp, vec![])?; Ok(()) }
}
impl<T: Transport> InputReader for Remote<T> {
    type Error = ();
    type Binding = u32;
    type Code = Value;
    type Value = Value;
    fn binding(&mut self, s: &str) -> Result<(u32, usize), ()> {
        let v = self.0.call(Op::Binding, vec![name(s)])?; Ok((v[0] as u32, v[1] as usize))
    }
    fn code(&mut self, binding: u32, index: usize) -> Result<Option<Value>, ()> {
        let v = self.0.call(Op::BindingCode, vec![binding as f64, index as f64])?;
        Ok(if v[0] != 0.0 { Some(value(&v[1..])) } else { None })
    }
    fn down(&mut self, code: Value) -> Result<bool, ()> { let mut v = Vec::new(); put(code, &mut v); Ok(self.0.call(Op::Down, v)?[0] != 0.0) }
    fn take(&mut self, s: &str) -> Result<Value, ()> { Ok(value(&self.0.call(Op::Take, vec![name(s)])?)) }
    fn axis(&mut self, i: usize) -> Result<Option<f64>, ()> { Ok(Some(self.0.call(Op::Axis, vec![i as f64])?[0])) }
    fn button(&mut self, i: usize) -> Result<bool, ()> { Ok(self.0.call(Op::Button, vec![i as f64])?[0] != 0.0) }
}
impl<T: Transport> InputAccess for Remote<T> {
    fn scales(&mut self) -> Result<LookScales, ()> {
        let v = self.0.call(Op::Scales, vec![])?; Ok(network::look_scales(v[0], v[1] != 0.0, v[2], v[3] != 0.0))
    }
    fn next_look(&mut self) -> Result<Option<(f64, f64)>, ()> {
        let v = self.0.call(Op::NextLook, vec![])?; Ok(if v[0] != 0.0 { Some((v[1], v[2])) } else { None })
    }
    fn aim_from_look(&mut self) -> Result<(), ()> { self.0.call(Op::CopyLook, vec![])?; Ok(()) }
    fn flight_input(&mut self) -> Result<Option<FlightKeys>, ()> {
        self.0.call(Op::Flight, vec![])?;
        let keys = controls::read_input(self)?;
        let curve = value(&self.0.call(Op::Curve, vec![])?);
        Ok(Some(FlightKeys { keys, curve }))
    }
    fn drive_axis(&mut self, s: &str) -> Result<Value, ()> { Ok(value(&self.0.call(Op::DriveAxis, vec![name(s)])?)) }
    fn space(&mut self) -> Result<Value, ()> { Ok(value(&self.0.call(Op::Space, vec![])?)) }
}

pub fn run<T: Transport>(transport: T, cycle: bool) -> Result<Vec<f64>, ()> {
    let mut world = Remote(transport);
    if cycle { return Ok(vec![number(network::cycle_with(&mut world, 0, 1.0)?)]); }
    let mut words = Vec::new();
    match network::network_input_with(&mut world)? {
        Input::Drive { forward, right, turn, brake } => {
            words.push(0.0); for v in [forward, right, turn, brake] { put(v, &mut words); }
        }
        Input::Turret => { words.push(0.0); for v in [Value::number(0.0), Value::number(0.0), Value::number(0.0), Value::boolean(false)] { put(v, &mut words); } }
        Input::Flight(Some(FlightKeys { keys: k, curve })) => {
            words.extend([1.0, k.forward, k.right, number(k.stick.is_some())]);
            if let Some((x, y)) = k.stick { words.extend([x, y]); }
            words.extend([number(k.boost), number(k.roll_lock)]);
            for v in [k.takeoff, k.trick, k.immelmann, k.target_lock, curve] { put(v, &mut words); }
        }
        Input::Flight(None) => unreachable!("live flight reader supplies keys"),
    }
    Ok(words)
}

#[cfg(target_arch = "wasm32")]
mod abi {
    use super::*;
    use crate::{abi::sim, gen::{import_network_seats_world, NetworkSeatsCommand, NetworkSeatsQuery, NetworkSeatsReply}};
    struct Js { context: u32 }
    impl Transport for Js {
        fn call(&mut self, op: Op, values: Vec<f64>) -> Result<Vec<f64>, ()> {
            let answer = import_network_seats_world(&NetworkSeatsQuery { context: self.context, op, values });
            if answer.ok { Ok(answer.values) } else { Err(()) }
        }
    }
    #[no_mangle]
    pub extern "C" fn network_seats_words(result: u32) -> u32 {
        if result == 0 { NetworkSeatsCommand::WORDS as u32 } else { NetworkSeatsReply::WORDS as u32 }
    }
    #[no_mangle]
    pub extern "C" fn network_seats_run() -> *const f64 {
        let input = { let (row, heap) = sim().calls.args(); NetworkSeatsCommand::take(row, heap) };
        let result = run(Js { context: input.context }, input.cycle);
        let answer = NetworkSeatsReply { ok: result.is_ok(), values: result.unwrap_or_default() };
        // All imports and their owned temporaries have finished before this borrow.
        let calls = &mut sim().calls;
        calls.result(NetworkSeatsReply::WORDS, |row, heap| answer.put(row, heap));
        calls.block.ptr()
    }
}
