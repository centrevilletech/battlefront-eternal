use super::{Hulls, Receiver, Bridge, Matrix4, n, r, k};
use bfe_render::line_layout::{kind, limit, row, view};
use three::Vector3;

const POINTS: usize = 32;
type TowRow = (u32, u32, u32, [f64; 3], String, f64, f64);

pub(super) struct Cable {
    source: u32,
    serial: f64,
    pub(super) state: Option<State>,
}
pub(super) struct State {
    owner: (u32, u32),
    target: (u32, u32),
    local: [f64; 3],
    angle: f64,
    winding: f64,
}
impl Cable {
    pub(super) fn new(bridge: &mut Bridge, program: u32) -> Result<Self, String> {
        let source = bridge.line_source(program, kind::TOW as u32, POINTS as u32)?
            .ok_or("tow cable line capacity")?;
        Ok(Self { source, serial: 0., state: None })
    }
}

// drawTethers: a fixed anchor, quarter-turn wraps around the legs, then
// sixteen points along the sagging span to the gun. All endpoints use the
// same displayed transforms as the hull meshes.
fn points(state: &State, frame: &Matrix4, bounds: [f64; 6], gun: Vector3, out: &mut [[f32; 3]; POINTS]) -> usize {
    let mut anchor = Vector3::new(state.local[0], state.local[1], state.local[2]);
    anchor.apply_matrix4(frame);
    let put = |out: &mut [[f32; 3]; POINTS], i, p: Vector3| { out[i] = [p.x as f32, p.y as f32, p.z as f32]; };
    put(out, 0, anchor);
    let quarters = ((state.winding.abs() / std::f64::consts::FRAC_PI_2).floor() as usize).min(POINTS - 18);
    let radius = (bounds[3] - bounds[0]).max(bounds[5] - bounds[2]) * 0.5;
    let mut from = anchor;
    for q in 1..=quarters {
        let angle = state.angle - state.winding.signum() * (state.winding.abs() - q as f64 * std::f64::consts::FRAC_PI_2);
        from.set((bounds[0] + bounds[3]) * 0.5, state.local[1], (bounds[2] + bounds[5]) * 0.5).apply_matrix4(frame);
        from.x += angle.sin() * radius;
        from.z += angle.cos() * radius;
        put(out, q, from);
    }
    let sag = (from.distance_to(&gun) * 0.025).min(2.);
    for i in 1..=16 {
        let t = i as f64 / 16.;
        let mut p = Vector3::default();
        p.lerp_vectors(&from, &gun, t);
        p.y -= (t * std::f64::consts::PI).sin() * sag;
        put(out, quarters + i, p);
    }
    quarters + 17
}

impl Hulls {
    pub(super) fn receive_tethers(&mut self, receiver: &Receiver) {
        for field in receiver.publications.iter().filter(|f| f.row[r::KIND] == k::SUMMARY as f64 && f.row[r::SCHEMA] == 8.) {
            let Ok((id, target, generation, local, _, angle, winding)) =
                serde_json::from_slice::<TowRow>(&field.bytes[..field.length]) else { continue; };
            let owner = (id, field.row[r::GENERATION] as u32);
            let Some(&slot) = self.bound.get(&owner) else { continue; };
            let Some(cable) = self.slots[slot].cable.as_mut() else { continue; };
            cable.state = (target != 0).then_some(State { owner, target: (target, generation), local, angle, winding });
        }
    }

    pub fn draw_tethers(&mut self, camera: &[f64], bridge: &mut Bridge) -> Result<(), String> {
        let mut shown = 0;
        for index in 0..self.slots.len() {
            let slot = &self.slots[index];
            let Some(cable) = &slot.cable else { continue; };
            let Some(state) = &cable.state else { continue; };
            if !slot.active || slot.health <= 0. || slot.dying || slot.identity != Some(state.owner) { continue; }
            let Some(&target) = self.bound.get(&state.target) else { continue; };
            let target = &self.slots[target];
            if !target.active || target.health <= 0. || target.dying { continue; }
            let Some(target_nodes) = self.owner.slot_nodes(target.rig, 0) else { continue; };
            let frame = Matrix4 { elements: target_nodes[n::WORLD..n::WORLD + 16].try_into().unwrap() };
            let gun = match self.models[slot.model].tow_node {
                Some(node) => {
                    let Some(nodes) = self.owner.slot_nodes(slot.rig, 0) else { continue; };
                    let at = node * n::STRIDE + n::WORLD + 12;
                    let Some(p) = nodes.get(at..at + 3) else { continue; };
                    Vector3::new(p[0], p[1], p[2])
                }
                None => {
                    let Some(p) = self.aim_point(state.owner) else { continue; };
                    Vector3::new(p[0], p[1], p[2])
                }
            };
            let mut vertices = [[0.; 3]; POINTS];
            let count = points(state, &frame, self.models[target.model].bounds, gun, &mut vertices);
            let object = state.owner.0;
            let cable = self.slots[index].cable.as_mut().unwrap();
            cable.serial += 1.;
            let raw = bridge.bank("line_data")?;
            let data = unsafe { std::slice::from_raw_parts_mut(raw.as_mut_ptr().cast::<f32>(), raw.len() / 4) };
            let at = cable.source as usize * limit::VERTICES * 3;
            data[at..at + count * 3].copy_from_slice(vertices[..count].as_flattened());
            let raw = bridge.bank("line_input")?;
            let input = unsafe { std::slice::from_raw_parts_mut(raw.as_mut_ptr().cast::<f64>(), raw.len() / 8) };
            let r = &mut input[view::LEN + shown * row::LEN..][..row::LEN];
            r.fill(0.);
            r[row::SOURCE] = cable.source as f64;
            r[row::COUNT] = count as f64;
            r[row::SERIAL] = cable.serial;
            r[row::WORLD..row::WORLD + 16].copy_from_slice(&Matrix4::default().elements);
            r[row::OBJECT] = object as f64;
            shown += 1;
        }
        let raw = bridge.bank("line_input")?;
        let input = unsafe { std::slice::from_raw_parts_mut(raw.as_mut_ptr().cast::<f64>(), raw.len() / 8) };
        input[view::PROJECTION..view::PROJECTION + 16].copy_from_slice(&camera[..16]);
        input[view::INVERSE..view::INVERSE + 16].copy_from_slice(&camera[16..32]);
        let mut world = Matrix4 { elements: camera[16..32].try_into().unwrap() };
        world.invert();
        input[view::WORLD..view::WORLD + 16].copy_from_slice(&world.elements);
        input[view::ORTHOGRAPHIC] = 0.;
        input[view::COUNT] = shown as f64;
        bridge.line_frame(2);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cable_keeps_its_anchor_and_gun_while_winding_around_the_walker() {
        let mut state = State { owner: (3, 1), target: (8, 1), local: [0., 1., 0.], angle: 0., winding: 0. };
        let mut frame = Matrix4::default();
        frame.elements[12] = 10.;
        let gun = Vector3::new(50., 1., 0.);
        let bounds = [-2., 0., -2., 2., 8., 2.];
        let mut out = [[0.; 3]; POINTS];
        assert_eq!(points(&state, &frame, bounds, gun, &mut out), 17);
        assert_eq!(out[0], [10., 1., 0.]);
        assert_eq!(out[8], [30., 0., 0.]);
        assert_eq!(out[16], [50., 1., 0.]);
        state.winding = std::f64::consts::FRAC_PI_2;
        assert_eq!(points(&state, &frame, bounds, gun, &mut out), 18);
        assert_eq!(out[1], [10., 1., 2.]);
        assert_eq!(out[17], [50., 1., 0.]);
    }
}
