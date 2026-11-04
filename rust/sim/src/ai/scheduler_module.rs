//! The decision scheduler on the shared AI instance. Its callbacks are
//! ordered individually so controller removal, rebinding and reentrant
//! native navigation observe the current scheduler state.

use super::{control::{scheduler::camera_accepts, world::CameraView}, module::state, nav::math::Vec3};
use crate::gen::{import_ai_schedule, AiScheduleArgs, AiScheduleOp as Op, AiScheduleResult};

fn call(op: Op, id: u32, number: f64, phase: f64) -> AiScheduleResult {
    // No AI state reference spans an import. The callback may use other
    // AI exports, and reads the scheduler's already-published fields.
    import_ai_schedule(&AiScheduleArgs { op, id, number, phase })
}

fn controller(id: u32) -> Vec<f64> { call(Op::Controller, id, 0.0, 0.0).values }

fn update(dt: f64) {
    let config = call(Op::Config, 0, 0.0, 0.0).values;
    let (cycle, los_budget) = (config[0], config[1]);
    let wrap = {
        let s = &mut state().scheduler;
        s.counter += 1.0;
        s.parity = (v8math::to_int32(s.parity) ^ 1) as f64;
        s.counter > cycle
    };
    if wrap {
        state().scheduler.counter = 1.0;
        call(Op::Notify, 0, 0.0, 0.0);
        state().scheduler.skips = 0.0;
        for id in call(Op::Humans, 0, 0.0, 0.0).values {
            call(Op::Broadcast, id as u32, 0.0, 0.0);
        }
        for id in call(Op::Controllers, 0, 0.0, 0.0).values {
            call(Op::Processed, id as u32, 0.0, 0.0);
        }
        call(Op::Rebuild, 0, 0.0, 0.0);
    }
    let list = call(Op::Controllers, 0, 0.0, 0.0).values;
    let budget = super::control::scheduler::candidate_budget(list.len() as f64, cycle, state().scheduler.counter);
    {
        let stats = &mut state().scheduler.stats;
        stats.budget = budget;
        stats.considered = 0.0;
        stats.serviced = 0.0;
        stats.skipped = 0.0;
    }
    for id in list {
        if !(state().scheduler.stats.considered < budget) { break; }
        let id = id as u32;
        if controller(id)[0] != 0.0 { continue; }
        if call(Op::LodSkip, id, 0.0, 0.0).flag {
            state().scheduler.stats.skipped += 1.0;
        } else {
            call(Op::Service, id, dt, 0.0);
            state().scheduler.stats.serviced += 1.0;
        }
        call(Op::Processed, id, 1.0, 0.0);
        state().scheduler.stats.considered += 1.0;
    }
    call(Op::Budgets, 0, los_budget, 0.0);
}

fn service(id: u32, dt: f64) {
    if controller(id)[2] != 0.0 { call(Op::Phase, id, dt, 0.0); }
    call(Op::DropLos, id, 0.0, 0.0);
    call(Op::Scan, id, 0.0, 0.0);
    let unit = controller(id)[3] as u32;
    if unit != 0 { call(Op::Broadcast, unit, 0.0, 0.0); }
    if controller(id)[2] != 0.0 { call(Op::Phase, id, dt, 1.0); }
    call(Op::ClearCandidate, id, 0.0, 0.0);
}

fn budgets(budget: f64) {
    let scale = if call(Op::PlannerIdle, 0, 0.0, 0.0).flag { 2.0 }
        else { call(Op::NavService, 0, 1.0, 0.0); 1.0 };
    if !call(Op::LosService, 0, budget * scale, 0.0).flag {
        call(Op::NavService, 0, 1.0, 0.0);
    }
}

fn visible(unit: u32) -> bool {
    if call(Op::LodDisabled, 0, 0.0, 0.0).flag { return true; }
    let velocity = call(Op::Velocity, unit, 0.0, 0.0).values;
    if !velocity.is_empty() && velocity[0]*velocity[0] + velocity[1]*velocity[1] + velocity[2]*velocity[2] >= 100.0 { return true; }
    let cameras = call(Op::Cameras, 0, 0.0, 0.0).values;
    if cameras.len() == 1 { return true; }
    let p = call(Op::Position, unit, 0.0, 0.0).values;
    let point = Vec3::new(p[0],p[1],p[2]);
    let rows = &cameras[1..];
    let near_count = if cameras[0] != 0.0 { rows.len()/22 } else { 1 };
    for row in rows.chunks_exact(22).take(near_count) {
        if row[0] == 0.0 { return true; }
        let e = &row[6..22];
        let dx=point.x-e[12]; let dy=point.y-e[13]; let dz=point.z-e[14];
        if dx*dx+dy*dy+dz*dz <= 2025.0 { return true; }
    }
    for row in rows.chunks_exact(22) {
        if row[0] == 0.0 { continue; }
        let camera = CameraView { zoom: row[1], near: row[2], far: row[3], fov: row[4], aspect: row[5], elements: row[6..22].try_into().unwrap() };
        if camera_accepts(&camera, point) { return true; }
    }
    false
}

fn lod_skip(id: u32) -> bool {
    let c = controller(id);
    if c[1] == 0.0 {
        let unit = c[3] as u32;
        if unit != 0 {
            let identity = call(Op::Identity, unit, 0.0, 0.0).number;
            if ((!v8math::to_int32(identity)) & 1) as f64 == state().scheduler.parity && !call(Op::Visible, unit, 0.0, 0.0).flag {
                call(Op::WriteLod, id, 1.0, 0.0);
                state().scheduler.skips += 1.0;
                return true;
            }
        }
    }
    call(Op::WriteLod, id, 0.0, 0.0);
    false
}

fn inputs(dt: f64) {
    let parity = (v8math::to_int32(state().scheduler.input_parity)^1) as f64;
    state().scheduler.input_parity = parity;
    let mut index = 0;
    loop {
        // Input execution walks the live collection, unlike decisions'
        // snapshot. Read the next slot after each serviced controller.
        let id = call(Op::InputAt, 0, index as f64, 0.0).number as u32;
        if id == 0 { break; }
        if controller(id)[2] != 0.0 { call(Op::ExecuteInput, id, dt, 0.0); }
        index += 1;
    }
}

fn input(id: u32, dt: f64) -> bool {
    let read = call(Op::InputState, id, 0.0, 0.0);
    let unit = read.number as u32;
    if read.values[0] == 0.0 {
        let identity = call(Op::Identity, unit, 0.0, 0.0).number;
        if ((!v8math::to_int32(identity))&1) as f64 == state().scheduler.input_parity
            && !call(Op::Visible, unit, 0.0, 0.0).flag {
            call(Op::WriteInput, id, dt, 1.0);
            call(Op::MarkSkip, id, 1.0, 0.0);
            return false;
        }
    }
    let step = call(Op::InputCarry, id, 0.0, 0.0).number+dt;
    call(Op::WriteInput, id, 0.0, 0.0);
    call(Op::MarkSkip, id, 0.0, 0.0);
    call(Op::Intent, id, 0.0, 0.0);
    call(Op::Consume, id, step, 0.0);
    if call(Op::ExitPending, id, 0.0, 0.0).flag { call(Op::Exit, id, 0.0, 0.0); }
    call(Op::Aim, id, step, 0.0);
    if call(Op::NavControl, id, 0.0, 0.0).flag {
        call(Op::NavInput, id, step, 0.0);
    } else {
        call(Op::Posture, id, 0.0, 0.0);
        let jet = call(Op::Jet, id, 0.0, 0.0).values;
        if jet[0] != 0.0 {
            let direction = jet[1]*jet[1]+jet[2]*jet[2]+jet[3]*jet[3] > 0.0;
            call(Op::ApplyJet, id, direction as u8 as f64, 0.0);
        }
        if !call(Op::Analogs, id, step, 0.0).flag { fallback(id,step); }
    }
    if call(Op::Kind, id, 0.0, 0.0).number == 3.0 { call(Op::Droideka, id, 0.0, 0.0); }
    let dodge = call(Op::Dodge, id, 0.0, 0.0).number;
    if dodge != 0.0 && !dodge.is_nan() {
        call(Op::ApplyDodge, id, if dodge < 0.0 {-1.0} else {1.0}, 0.0);
    }
    true
}

fn fallback(id: u32, dt: f64) {
    use super::control::{controller::MoveRequest, world::{AiUnit, ClassSnap, SoldierSnap}};
    let v = call(Op::Fallback, id, 0.0, 0.0).values;
    let unit = AiUnit {
        position: Vec3::new(v[1],v[2],v[3]),
        soldier: (v[0] != 0.0).then_some(SoldierSnap {
            aim_yaw: Some(v[4]), aim_pitch: Some(v[5]), max_speed: Some(v[6]),
            max_strafe: Some(v[7]), max_turn: Some(v[8]), ..Default::default()
        }),
        class_def: Some(ClassSnap { max_pitch_speed: Some(v[9]), ..Default::default() }),
        ..Default::default()
    };
    let request = MoveRequest { valid: v[10] != 0.0, steering_target: Vec3::new(v[11],v[12],v[13]), ..Default::default() };
    let aim = (v[14] != 0.0).then_some(Vec3::new(v[15],v[16],v[17]));
    let out = super::control::scheduler::fallback_analogs(&unit,&request,aim,dt,v[18] != 0.0);
    state().out.clear(); state().out.extend(out);
    call(Op::ApplyFallback, id, 0.0, 0.0);
}

fn posture(id: u32) {
    let v = call(Op::PostureInfo, id, 0.0, 0.0).values;
    if v[0] == 0.0 || v[1] == 3.0 { return; }
    let want = if v[2] == 1.0 {2.0} else if v[2] == 2.0 {3.0} else {1.0};
    let have = v[3];
    if have == 0.0 || have == want { return; }
    if want == 2.0 || (want == 1.0 && have == 2.0) { call(Op::Crouch, id, 0.0, 0.0); }
    else if want == 3.0 || (want == 1.0 && have == 3.0) { call(Op::Prone, id, 0.0, 0.0); }
}

fn droideka(id: u32) {
    let v = call(Op::DroidekaInfo, id, 0.0, 0.0).values;
    if v[0] != 0.0 && ((v[3] == 1.0 && v[1] == 12.0) || (v[3] == 2.0 && v[2] != 0.0)) {
        call(Op::Use, id, 0.0, 0.0);
    }
}

#[no_mangle]
pub extern "C" fn ai_scheduler_command(op: u32, id: u32, number: f64) -> u32 {
    match op {
        0 => update(number),
        1 => service(id,number),
        2 => budgets(number),
        3 => return lod_skip(id) as u32,
        4 => return visible(id) as u32,
        5 => inputs(number),
        6 => return input(id,number) as u32,
        7 => posture(id),
        8 => droideka(id),
        _ => panic!("invalid scheduler command"),
    }
    0
}

#[no_mangle]
pub extern "C" fn ai_scheduler_read(field: u32) -> f64 {
    let s = &state().scheduler;
    match field {
        0 => s.counter, 1 => s.parity, 2 => s.input_parity, 3 => s.skips,
        4 => s.stats.serviced, 5 => s.stats.skipped, 6 => s.stats.considered, 7 => s.stats.budget,
        _ => panic!("invalid scheduler field"),
    }
}

#[no_mangle]
pub extern "C" fn ai_scheduler_write(field: u32, value: f64) {
    let s = &mut state().scheduler;
    match field {
        0 => s.counter=value, 1 => s.parity=value, 2 => s.input_parity=value, 3 => s.skips=value,
        4 => s.stats.serviced=value, 5 => s.stats.skipped=value, 6 => s.stats.considered=value, 7 => s.stats.budget=value,
        _ => panic!("invalid scheduler field"),
    }
}
